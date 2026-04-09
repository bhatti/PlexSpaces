// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Weather Actor — Service Link + KV Cache Example (TypeScript WASM)
//
// Demonstrates outbound HTTP via a named service link ("weather-api") combined
// with KV-based caching.  The host handles retries, circuit breaking, and
// auth injection transparently.
//
// Service Link Configuration
// ---------------------------
// The "weather-api" link must exist in RuntimeConfig.service_links (release.toml):
//
//   [[runtime.service_links]]
//   name     = "weather-api"
//   base_url = "https://api.open-meteo.com"
//   transport = "HTTP"

import { ActorRouter, PlexSpacesActor, ServiceHttpClient, host } from "@plexspaces/sdk";

const LINK_NAME = "weather-api";
const CACHE_TTL_MS = 5 * 60 * 1000; // 5 minutes

function parseWeatherBody(bodyStr: string): Record<string, unknown> {
  if (!bodyStr) return {};
  try {
    return JSON.parse(bodyStr) as Record<string, unknown>;
  } catch {
    try {
      const decoded = decodeBase64Standard(bodyStr);
      return JSON.parse(decoded) as Record<string, unknown>;
    } catch {
      return {};
    }
  }
}

function decodeBase64Standard(input: string): string {
  const alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
  const cleaned = input.replace(/\s+/g, "").replace(/=+$/, "");
  const bytes: number[] = [];
  let buffer = 0;
  let bits = 0;

  for (let index = 0; index < cleaned.length; index++) {
    const value = alphabet.indexOf(cleaned.charAt(index));
    if (value < 0) {
      throw new Error("invalid base64 payload");
    }
    buffer = (buffer << 6) | value;
    bits += 6;
    if (bits >= 8) {
      bits -= 8;
      bytes.push((buffer >> bits) & 0xff);
    }
  }

  return new TextDecoder().decode(new Uint8Array(bytes));
}

type WeatherState = {
  actor_id: string;
  cache_hits: number;
  cache_misses: number;
};

class WeatherActor extends PlexSpacesActor<WeatherState> {
  getDefaultState(): WeatherState {
    return { actor_id: "", cache_hits: 0, cache_misses: 0 };
  }

  protected override onInit(config: Record<string, unknown>): void {
    if (typeof config.actor_id === "string" && config.actor_id) {
      this.state.actor_id = config.actor_id;
    }
    host.log("info", `WeatherActor initialized: ${this.state.actor_id}`);
  }

  protected onGet_weather(payload: Record<string, unknown>): Record<string, unknown> {
    let city = "London";
    if (typeof payload.city === "string" && payload.city) {
      city = payload.city;
    }

    const cacheKey = `weather:${city}`;
    const cached = host.kvGet(cacheKey);
    if (cached.startsWith("ERROR:")) {
      host.log("warn", `Cache read failed for ${city}: ${cached}`);
    } else if (cached) {
      try {
        const data = JSON.parse(cached) as Record<string, unknown>;
        const fetchedAt = (data["fetched_at_ms"] as number) ?? 0;
        if (host.nowMs() - fetchedAt < CACHE_TTL_MS) {
          this.state.cache_hits++;
          host.log("debug", `Cache HIT for ${city}`);
          return { ...data, city, source: "cache" };
        }
      } catch {
        // stale or invalid cache — fall through
      }
    }

    this.state.cache_misses++;
    host.log("info", `Cache MISS for ${city} — calling ${LINK_NAME}`);

    try {
      const http = new ServiceHttpClient(LINK_NAME);
      const resp = http.get(
        `/v1/forecast?latitude=51.5&longitude=-0.12&current=temperature_2m,wind_speed_10m&city=${encodeURIComponent(city)}`
      );
      const bodyStr = (resp.body as string) ?? "";
      const weatherData = parseWeatherBody(bodyStr);
      const current = (weatherData["current"] as Record<string, number>) ?? {};
      const result = {
        temp_c: current["temperature_2m"] ?? 0,
        wind_kph: current["wind_speed_10m"] ?? 0,
        fetched_at_ms: host.nowMs(),
      };
      const cacheWrite = host.kvPut(cacheKey, JSON.stringify(result));
      if (cacheWrite.startsWith("ERROR:")) {
        host.log("warn", `Cache write failed for ${city}: ${cacheWrite}`);
      }
      return { ...result, city, source: "api" };
    } catch (err) {
      host.log("error", `Weather API call failed: ${err}`);
      return { city, error: String(err), source: "api" };
    }
  }

  protected onCache_stats(_payload: Record<string, unknown>): Record<string, unknown> {
    return { hits: this.state.cache_hits, misses: this.state.cache_misses };
  }

  protected onClear_cache(_payload: Record<string, unknown>): Record<string, unknown> {
    this.state.cache_hits = 0;
    this.state.cache_misses = 0;
    return { cleared: true };
  }
}

const router = new ActorRouter({
  weather: () => new WeatherActor(),
});

export const actor = {
  init: (configJson: string | Uint8Array | ArrayBuffer | ArrayBufferView) => router.init(configJson),
  handle: (
    from: string,
    msgType: string,
    payloadJson: string | Uint8Array | ArrayBuffer | ArrayBufferView
  ) => router.handle(from, msgType, payloadJson),
  getState: () => router.getState(),
  setState: (stateJson: string | Uint8Array | ArrayBuffer | ArrayBufferView) => router.setState(stateJson),
};
