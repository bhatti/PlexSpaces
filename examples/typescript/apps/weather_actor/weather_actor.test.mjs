// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Contract tests for WeatherActor (TypeScript) — no running node required.
// Uses inline actor logic with a mock host to verify behaviour.

import { describe, it, beforeEach } from 'node:test';
import assert from 'node:assert/strict';

// ─── Mock Host ─────────────────────────────────────────────────────────────

function createMockHost(weatherBodyOverride = null) {
  const kv = new Map();
  const logs = [];
  let currentTimeMs = 1000000; // fixed base time for deterministic tests
  return {
    kv, logs,
    kvGet(key) { return kv.get(key) ?? ''; },
    kvPut(key, value) { kv.set(key, value); return ''; },
    kvDelete(key) { kv.delete(key); return ''; },
    kvList(prefix) { return JSON.stringify([...kv.keys()].filter(k => k.startsWith(prefix))); },
    log(level, msg) { logs.push({ level, msg }); },
    nowMs() { return currentTimeMs; },
    advanceTimeMs(ms) { currentTimeMs += ms; },
    setTimeMs(ms) { currentTimeMs = ms; },
    // http_fetch stub: returns JSON-wrapped weather body
    _weatherBody: weatherBodyOverride ?? JSON.stringify({
      current: { temperature_2m: 15.3, wind_speed_10m: 12.0 }
    }),
    httpFetch(linkName, method, pathAndQuery, _headers, _body) {
      return { status: 200, headers: {}, body: this._weatherBody };
    },
  };
}

// ─── Inline WeatherActor logic (mirrors TypeScript implementation) ──────────

const CACHE_TTL_MS = 5 * 60 * 1000;

class WeatherActor {
  constructor(mockHost) {
    this._host = mockHost;
    this.state = { actor_id: '', cache_hits: 0, cache_misses: 0 };
  }

  init(configJSON) {
    try {
      const cfg = JSON.parse(configJSON);
      if (cfg.actor_id) this.state.actor_id = cfg.actor_id;
    } catch {}
    return '';
  }

  _cacheKeyPrefix() { return `${this.state.actor_id}:weather:`; }

  handle(_from, msgType, payloadJSON) {
    switch (msgType) {
      case 'get_weather': return this._getWeather(payloadJSON);
      case 'cache_stats': return JSON.stringify({ hits: this.state.cache_hits, misses: this.state.cache_misses });
      case 'clear_cache': {
        this.state.cache_hits = 0;
        this.state.cache_misses = 0;
        const keysJson = this._host.kvList(this._cacheKeyPrefix());
        try { for (const key of JSON.parse(keysJson)) this._host.kvDelete(key); } catch {}
        return JSON.stringify({ cleared: true });
      }
      default: return JSON.stringify({ error: `unknown message type: ${msgType}` });
    }
  }

  _getWeather(payloadJSON) {
    let city = 'London';
    try { const req = JSON.parse(payloadJSON); if (req.city) city = req.city; } catch {}

    const cacheKey = `${this._cacheKeyPrefix()}${city}`;
    const cached = this._host.kvGet(cacheKey);
    if (cached) {
      try {
        const data = JSON.parse(cached);
        if (this._host.nowMs() - (data.fetched_at_ms ?? 0) < CACHE_TTL_MS) {
          this.state.cache_hits++;
          return JSON.stringify({ ...data, city, source: 'cache' });
        }
      } catch {}
    }

    this.state.cache_misses++;
    const resp = this._host.httpFetch('weather-api', 'GET', `/v1/forecast?city=${city}`, {}, '');
    const bodyStr = resp.body ?? '';
    const weatherData = bodyStr ? JSON.parse(bodyStr) : {};
    const current = weatherData.current ?? {};
    const result = {
      temp_c: current.temperature_2m ?? 0,
      wind_kph: current.wind_speed_10m ?? 0,
      fetched_at_ms: this._host.nowMs(),
    };
    this._host.kvPut(cacheKey, JSON.stringify(result));
    return JSON.stringify({ ...result, city, source: 'api' });
  }
}

// ─── Tests ─────────────────────────────────────────────────────────────────

describe('WeatherActor', () => {
  let mockHost;
  let actor;

  beforeEach(() => {
    mockHost = createMockHost();
    actor = new WeatherActor(mockHost);
    actor.init('{"actor_id":"weather:test@node"}');
  });

  it('first call hits API (cache MISS)', () => {
    const result = JSON.parse(actor.handle('caller', 'get_weather', '{"city":"London"}'));
    assert.equal(result.city, 'London');
    assert.equal(result.temp_c, 15.3);
    assert.equal(result.source, 'api');
    const stats = JSON.parse(actor.handle('caller', 'cache_stats', '{}'));
    assert.equal(stats.misses, 1);
    assert.equal(stats.hits, 0);
  });

  it('second call for same city returns cache HIT', () => {
    actor.handle('caller', 'get_weather', '{"city":"Paris"}');
    const second = JSON.parse(actor.handle('caller', 'get_weather', '{"city":"Paris"}'));
    assert.equal(second.source, 'cache');
    const stats = JSON.parse(actor.handle('caller', 'cache_stats', '{}'));
    assert.equal(stats.misses, 1);
    assert.equal(stats.hits, 1);
  });

  it('clear_cache resets counters', () => {
    actor.handle('caller', 'get_weather', '{"city":"Berlin"}');
    actor.handle('caller', 'get_weather', '{"city":"Berlin"}');
    actor.handle('caller', 'clear_cache', '{}');
    const stats = JSON.parse(actor.handle('caller', 'cache_stats', '{}'));
    assert.equal(stats.hits, 0);
    assert.equal(stats.misses, 0);
  });

  it('different cities cached independently', () => {
    actor.handle('caller', 'get_weather', '{"city":"Tokyo"}');
    actor.handle('caller', 'get_weather', '{"city":"Sydney"}');
    actor.handle('caller', 'get_weather', '{"city":"Tokyo"}'); // cached
    const stats = JSON.parse(actor.handle('caller', 'cache_stats', '{}'));
    assert.equal(stats.misses, 2);
    assert.equal(stats.hits, 1);
  });

  it('unknown message returns error', () => {
    const result = JSON.parse(actor.handle('caller', 'unknown_op', '{}'));
    assert.ok(result.error.includes('unknown message type'));
  });

  it('default city is London when not specified', () => {
    const result = JSON.parse(actor.handle('caller', 'get_weather', '{}'));
    assert.equal(result.city, 'London');
  });

  it('cache entry expires after TTL and re-fetches from API', () => {
    // First call: cache miss, stores entry at t=1000000
    const first = JSON.parse(actor.handle('caller', 'get_weather', '{"city":"Rome"}'));
    assert.equal(first.source, 'api');

    // Advance time by 4 minutes (within TTL) — should hit cache
    mockHost.advanceTimeMs(4 * 60 * 1000);
    const second = JSON.parse(actor.handle('caller', 'get_weather', '{"city":"Rome"}'));
    assert.equal(second.source, 'cache');

    // Advance past 5-minute TTL — stale, should re-fetch from API
    mockHost.advanceTimeMs(2 * 60 * 1000);
    const third = JSON.parse(actor.handle('caller', 'get_weather', '{"city":"Rome"}'));
    assert.equal(third.source, 'api');

    const stats = JSON.parse(actor.handle('caller', 'cache_stats', '{}'));
    assert.equal(stats.misses, 2);
    assert.equal(stats.hits, 1);
  });
});
