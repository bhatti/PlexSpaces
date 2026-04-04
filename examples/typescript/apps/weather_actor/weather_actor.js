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
function parseWeatherBody(bodyStr) {
    if (!bodyStr)
        return {};
    try {
        return JSON.parse(bodyStr);
    }
    catch {
        try {
            const decoded = decodeBase64Standard(bodyStr);
            return JSON.parse(decoded);
        }
        catch {
            return {};
        }
    }
}
function decodeBase64Standard(input) {
    const alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    const cleaned = input.replace(/\s+/g, "").replace(/=+$/, "");
    const bytes = [];
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
class WeatherActor extends PlexSpacesActor {
    getDefaultState() {
        return { actor_id: "", cache_hits: 0, cache_misses: 0 };
    }
    init(configJSON) {
        try {
            const cfg = JSON.parse(configJSON);
            if (cfg.actor_id)
                this.state.actor_id = cfg.actor_id;
        }
        catch {
            // ignore parse errors
        }
        host.log("info", `WeatherActor initialized: ${this.state.actor_id}`);
        return "";
    }
    handle(_from, msgType, payloadJSON) {
        switch (msgType) {
            case "get_weather":
                return this.handleGetWeather(payloadJSON);
            case "cache_stats":
                return JSON.stringify({ hits: this.state.cache_hits, misses: this.state.cache_misses });
            case "clear_cache":
                this.state.cache_hits = 0;
                this.state.cache_misses = 0;
                return JSON.stringify({ cleared: true });
            default:
                return JSON.stringify({ error: `unknown message type: ${msgType}` });
        }
    }
    handleGetWeather(payloadJSON) {
        let city = "London";
        try {
            const req = JSON.parse(payloadJSON);
            if (req.city)
                city = req.city;
        }
        catch {
            // use default
        }
        const cacheKey = `weather:${city}`;
        const cached = host.kvGet(cacheKey);
        if (cached.startsWith("ERROR:")) {
            host.log("warn", `Cache read failed for ${city}: ${cached}`);
        }
        else if (cached) {
            try {
                const data = JSON.parse(cached);
                const fetchedAt = data["fetched_at_ms"] ?? 0;
                if (host.nowMs() - fetchedAt < CACHE_TTL_MS) {
                    this.state.cache_hits++;
                    host.log("debug", `Cache HIT for ${city}`);
                    return JSON.stringify({ ...data, city, source: "cache" });
                }
            }
            catch {
                // stale or invalid cache — fall through
            }
        }
        this.state.cache_misses++;
        host.log("info", `Cache MISS for ${city} — calling ${LINK_NAME}`);
        try {
            const http = new ServiceHttpClient(LINK_NAME);
            const resp = http.get(`/v1/forecast?latitude=51.5&longitude=-0.12&current=temperature_2m,wind_speed_10m&city=${encodeURIComponent(city)}`);
            const bodyStr = resp.body ?? "";
            const weatherData = parseWeatherBody(bodyStr);
            const current = weatherData["current"] ?? {};
            const result = {
                temp_c: current["temperature_2m"] ?? 0,
                wind_kph: current["wind_speed_10m"] ?? 0,
                fetched_at_ms: host.nowMs(),
            };
            const cacheWrite = host.kvPut(cacheKey, JSON.stringify(result));
            if (cacheWrite.startsWith("ERROR:")) {
                host.log("warn", `Cache write failed for ${city}: ${cacheWrite}`);
            }
            return JSON.stringify({ ...result, city, source: "api" });
        }
        catch (err) {
            host.log("error", `Weather API call failed: ${err}`);
            return JSON.stringify({ city, error: String(err), source: "api" });
        }
    }
}
const router = new ActorRouter({
    weather: () => new WeatherActor(),
});
export const actor = {
    init: (configJson) => router.init(configJson),
    handle: (from, msgType, payloadJson) => router.handle(from, msgType, payloadJson),
    getState: () => router.getState(),
    setState: (stateJson) => router.setState(stateJson),
};
