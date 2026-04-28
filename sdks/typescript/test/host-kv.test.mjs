// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Tests for Host.kvGetJson, Host.kvPutJson, Host.incrCounter, Host.incrCounters.
// Uses inline mock objects to simulate host KV and metrics functions without
// requiring the WASM host imports.

import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

// ---------------------------------------------------------------------------
// Inline KV store simulation (mirrors host.kvGet / host.kvPut logic)
// ---------------------------------------------------------------------------

function makeKvStore() {
  const store = new Map();
  return {
    kvGet(key) {
      return store.get(key) ?? '';
    },
    kvPut(key, value) {
      store.set(key, value);
      return '';
    },
    clear() { store.clear(); },
  };
}

// kvGetJson: parse stored JSON string; return null when missing or corrupt
function kvGetJson(kv, key) {
  const raw = kv.kvGet(key);
  if (!raw || raw.startsWith('ERROR:')) return null;
  try { return JSON.parse(raw); } catch { return null; }
}

// kvPutJson: serialize to JSON and store; throw on write failure
function kvPutJson(kv, key, value) {
  const serialized = JSON.stringify(value);
  const result = kv.kvPut(key, serialized);
  if (typeof result === 'string' && result.startsWith('ERROR:')) {
    throw new Error(`kvPutJson(${key}): ${result}`);
  }
}

// ---------------------------------------------------------------------------
// EventLog simulation (mirrors EventLog class logic)
// ---------------------------------------------------------------------------

function makeEventLog(watermark = 0) {
  return { watermark };
}

function eventLogAppend(kv, log, prefix, entry) {
  log.watermark++;
  const key = `${prefix}seq:${log.watermark}`;
  const result = kvPut(kv, key, entry);
  if (typeof result === 'string' && result.startsWith('ERROR:')) {
    log.watermark--;
    throw new Error(`EventLog.append: ${result}`);
  }
  return log.watermark;
}

function kvPut(kv, key, value) { return kv.kvPut(key, JSON.stringify(value)); }

function eventLogPoll(kv, log, prefix, consumerId, limit = 100) {
  const cursorKey = `${prefix}cursor:${consumerId}`;
  const rawCursor = kv.kvGet(cursorKey);
  const cursor = rawCursor ? (parseInt(rawCursor, 10) || 0) : 0;
  const events = [];
  let newCursor = cursor;
  for (let seq = cursor + 1; seq <= log.watermark && events.length < limit; seq++) {
    const raw = kv.kvGet(`${prefix}seq:${seq}`);
    if (raw) { events.push(JSON.parse(raw)); newCursor = seq; }
  }
  if (newCursor !== cursor) kv.kvPut(cursorKey, String(newCursor));
  return [events, newCursor];
}

// ---------------------------------------------------------------------------
// EventLog tests
// ---------------------------------------------------------------------------

describe('EventLog', () => {
  it('append increments watermark and poll returns all events', () => {
    const kv = makeKvStore();
    const log = makeEventLog();
    eventLogAppend(kv, log, 'audit:', { action: 'login' });
    eventLogAppend(kv, log, 'audit:', { action: 'logout' });
    assert.equal(log.watermark, 2);
    const [events, cursor] = eventLogPoll(kv, log, 'audit:', 'c1');
    assert.equal(events.length, 2);
    assert.equal(cursor, 2);
  });

  it('poll is idempotent — second call returns nothing new', () => {
    const kv = makeKvStore();
    const log = makeEventLog();
    eventLogAppend(kv, log, 'ev:', { x: 1 });
    const [e1, c1] = eventLogPoll(kv, log, 'ev:', 'c1');
    assert.equal(e1.length, 1); assert.equal(c1, 1);
    const [e2, c2] = eventLogPoll(kv, log, 'ev:', 'c1');
    assert.equal(e2.length, 0); assert.equal(c2, 1);
  });

  it('two consumers advance their cursors independently', () => {
    const kv = makeKvStore();
    const log = makeEventLog();
    for (let i = 0; i < 3; i++) eventLogAppend(kv, log, 'ev:', { i });
    const [evA, curA] = eventLogPoll(kv, log, 'ev:', 'consumer-A');
    const [evB, curB] = eventLogPoll(kv, log, 'ev:', 'consumer-B', 2);
    assert.equal(evA.length, 3); assert.equal(curA, 3);
    assert.equal(evB.length, 2); assert.equal(curB, 2);
  });

  it('append rolls back watermark on kvPut failure', () => {
    const failingKv = { kvGet: () => '', kvPut: () => 'ERROR: disk full' };
    const log = makeEventLog();
    assert.throws(() => eventLogAppend(failingKv, log, 'ev:', { x: 1 }), /ERROR/);
    assert.equal(log.watermark, 0);
  });
});

// ---------------------------------------------------------------------------
// incrCounter / incrCounters simulation
// ---------------------------------------------------------------------------

const warnLogs = [];

function incrCounters(applicationMetricsAdd, applicationId, counters) {
  try {
    applicationMetricsAdd(applicationId, {
      message_count: Object.keys(counters).length,
      counter_metrics: counters,
    });
  } catch (e) {
    warnLogs.push(`incrCounters: metrics update failed: ${e}`);
  }
}

function incrCounter(applicationMetricsAdd, applicationId, name) {
  incrCounters(applicationMetricsAdd, applicationId, { [name]: 1 });
}

// ---------------------------------------------------------------------------
// kvGetJson / kvPutJson tests
// ---------------------------------------------------------------------------

describe('Host.kvPutJson / kvGetJson', () => {
  it('round-trips a JSON object', () => {
    const kv = makeKvStore();
    const data = { seq: 42, task_type: 'summarize' };
    kvPutJson(kv, 'test:task:42', data);
    const result = kvGetJson(kv, 'test:task:42');
    assert.deepEqual(result, data);
  });

  it('returns null for a missing key', () => {
    const kv = makeKvStore();
    assert.equal(kvGetJson(kv, 'nonexistent:key'), null);
  });

  it('returns null for corrupt JSON', () => {
    const kv = makeKvStore();
    kv.kvPut('bad:json', 'not-json{');
    assert.equal(kvGetJson(kv, 'bad:json'), null);
  });

  it('throws when kvPut returns an ERROR response', () => {
    const failingKv = {
      kvGet: () => '',
      kvPut: () => 'ERROR: disk full',
    };
    assert.throws(
      () => kvPutJson(failingKv, 'key', { x: 1 }),
      /ERROR: disk full/,
    );
  });

  it('round-trips an array value', () => {
    const kv = makeKvStore();
    const arr = [1, 'two', { three: 3 }];
    kvPutJson(kv, 'arr:key', arr);
    assert.deepEqual(kvGetJson(kv, 'arr:key'), arr);
  });
});

// ---------------------------------------------------------------------------
// incrCounter / incrCounters tests
// ---------------------------------------------------------------------------

describe('Host.incrCounter / incrCounters', () => {
  it('incrCounter calls applicationMetricsAdd with count 1', () => {
    const calls = [];
    const add = (appId, metrics) => calls.push({ appId, metrics });
    incrCounter(add, 'myapp', 'my_op');
    assert.equal(calls.length, 1);
    assert.equal(calls[0].metrics.message_count, 1);
    assert.equal(calls[0].metrics.counter_metrics['my_op'], 1);
  });

  it('incrCounters passes all counters and correct message_count', () => {
    const calls = [];
    const add = (appId, metrics) => calls.push({ appId, metrics });
    incrCounters(add, 'myapp', { cache_hits: 5, cache_misses: 2 });
    assert.equal(calls.length, 1);
    assert.equal(calls[0].metrics.message_count, 2);
    assert.equal(calls[0].metrics.counter_metrics['cache_hits'], 5);
    assert.equal(calls[0].metrics.counter_metrics['cache_misses'], 2);
  });

  it('incrCounters swallows errors from applicationMetricsAdd', () => {
    const add = () => { throw new Error('metrics unavailable'); };
    // must not throw
    incrCounters(add, 'myapp', { op: 1 });
  });

  it('incrCounter swallows errors from applicationMetricsAdd', () => {
    const add = () => { throw new Error('metrics unavailable'); };
    incrCounter(add, 'myapp', 'my_op');
  });
});
