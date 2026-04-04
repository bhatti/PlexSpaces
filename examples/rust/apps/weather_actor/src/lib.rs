// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Weather Actor — Service Link + KV Cache Example (Rust WASM)
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
//
// Build with:
//   cargo build --target wasm32-wasip1 --release

fn parse_weather_body(body_str: &str) -> serde_json::Value {
    if body_str.is_empty() {
        return serde_json::json!({});
    }
    if let Ok(decoded) = serde_json::from_str::<serde_json::Value>(body_str) {
        return decoded;
    }
    decode_base64_standard(body_str)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .unwrap_or_else(|| serde_json::json!({}))
}

fn decode_base64_standard(input: &str) -> Result<Vec<u8>, String> {
    let mut values = [255u8; 256];
    for (idx, byte) in b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
        .iter()
        .enumerate()
    {
        values[*byte as usize] = idx as u8;
    }

    let mut clean = Vec::with_capacity(input.len());
    for byte in input.bytes() {
        if byte.is_ascii_whitespace() {
            continue;
        }
        if byte == b'=' || values[byte as usize] != 255 {
            clean.push(byte);
            continue;
        }
        return Err(format!("invalid base64 byte: {byte}"));
    }
    if clean.len() % 4 != 0 {
        return Err("invalid base64 length".to_string());
    }

    let mut out = Vec::with_capacity(clean.len() / 4 * 3);
    for chunk in clean.chunks(4) {
        let pad = chunk.iter().rev().take_while(|&&b| b == b'=').count();
        if pad > 2 || chunk[..4 - pad].iter().any(|&b| b == b'=') {
            return Err("invalid base64 padding".to_string());
        }

        let v0 = values[chunk[0] as usize] as u32;
        let v1 = values[chunk[1] as usize] as u32;
        let v2 = if chunk[2] == b'=' {
            0
        } else {
            values[chunk[2] as usize] as u32
        };
        let v3 = if chunk[3] == b'=' {
            0
        } else {
            values[chunk[3] as usize] as u32
        };
        let n = (v0 << 18) | (v1 << 12) | (v2 << 6) | v3;

        out.push(((n >> 16) & 0xff) as u8);
        if pad < 2 {
            out.push(((n >> 8) & 0xff) as u8);
        }
        if pad < 1 {
            out.push((n & 0xff) as u8);
        }
    }
    Ok(out)
}

/// WASM build: full actor implementation
#[cfg(target_arch = "wasm32")]
mod wasm_app {
    use serde::{Deserialize, Serialize};
    use serde_json::json;
    use std::sync::{Mutex, OnceLock};

    wit_bindgen::generate!({
        path: "../../../../wit/plexspaces-simple-actor",
        world: "actor-world",
    });

    use exports::plexspaces::simple_actor::actor::Guest;
    use plexspaces::simple_actor::host;

    const LINK_NAME: &str = "weather-api";
    const CACHE_TTL_MS: u64 = 5 * 60 * 1000; // 5 minutes

    #[derive(Debug, Clone, Default, Serialize, Deserialize)]
    struct WeatherState {
        actor_id: String,
        cache_hits: u64,
        cache_misses: u64,
    }

    fn state_cell() -> &'static Mutex<WeatherState> {
        static STATE: OnceLock<Mutex<WeatherState>> = OnceLock::new();
        STATE.get_or_init(|| Mutex::new(WeatherState::default()))
    }

    fn with_state<T>(f: impl FnOnce(&mut WeatherState) -> T) -> T {
        f(&mut state_cell().lock().expect("state lock poisoned"))
    }

    struct WeatherActorGuest;

    impl Guest for WeatherActorGuest {
        fn init(config_json: String) -> String {
            let value: serde_json::Value = match serde_json::from_str(&config_json) {
                Ok(v) => v,
                Err(e) => return format!("ERROR: invalid init JSON: {}", e),
            };
            let actor_id = value
                .get("actor_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            with_state(|state| {
                *state = WeatherState::default();
                state.actor_id = actor_id.clone();
            });
            host::log("info", &format!("WeatherActor initialized: {actor_id}"));
            String::new()
        }

        fn handle(_from: String, msg_type: String, payload_json: String) -> String {
            match msg_type.as_str() {
                "get_weather" => handle_get_weather(&payload_json),
                "cache_stats" => {
                    let (hits, misses) = with_state(|s| (s.cache_hits, s.cache_misses));
                    json!({ "hits": hits, "misses": misses }).to_string()
                }
                "clear_cache" => {
                    with_state(|s| {
                        s.cache_hits = 0;
                        s.cache_misses = 0;
                    });
                    json!({ "cleared": true }).to_string()
                }
                other => json!({ "error": format!("unknown message type: {other}") }).to_string(),
            }
        }

        fn get_state() -> String {
            with_state(|s| serde_json::to_string(s).unwrap_or_default())
        }

        fn set_state(state_json: String) -> String {
            match serde_json::from_str::<WeatherState>(&state_json) {
                Ok(s) => {
                    with_state(|state| *state = s);
                    String::new()
                }
                Err(e) => format!("ERROR: {e}"),
            }
        }
    }

    fn handle_get_weather(payload_json: &str) -> String {
        let city: String = serde_json::from_str::<serde_json::Value>(payload_json)
            .ok()
            .and_then(|v| v.get("city").and_then(|c| c.as_str()).map(str::to_string))
            .unwrap_or_else(|| "London".to_string());

        let cache_key = format!("weather:{city}");

        // Try KV cache first
        let cached = host::kv_get(&cache_key);
        if cached.starts_with("ERROR:") {
            host::log("warn", &format!("Cache read failed for {city}: {cached}"));
        } else if !cached.is_empty() {
            if let Ok(data) = serde_json::from_str::<serde_json::Value>(&cached) {
                let fetched_at = data
                    .get("fetched_at_ms")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let now_ms = host::now_ms();
                if now_ms - fetched_at < CACHE_TTL_MS {
                    with_state(|s| s.cache_hits += 1);
                    host::log("debug", &format!("Cache HIT for {city}"));
                    let mut resp = data.clone();
                    resp["city"] = json!(city);
                    resp["source"] = json!("cache");
                    return resp.to_string();
                }
            }
        }

        with_state(|s| s.cache_misses += 1);
        host::log(
            "info",
            &format!("Cache MISS for {city} — calling {LINK_NAME}"),
        );

        // Call the service link via host http_fetch
        let path = format!(
            "/v1/forecast?latitude=51.5&longitude=-0.12&current=temperature_2m,wind_speed_10m&city={city}"
        );
        let raw = host::http_fetch(LINK_NAME, "GET", &path, "{}", "");
        if raw.starts_with("ERROR:") {
            host::log("error", &format!("Weather API call failed: {raw}"));
            return json!({ "city": city, "error": raw, "source": "api" }).to_string();
        }

        // Parse response: { "status": 200, "headers": {}, "body": "..." }
        let resp_val: serde_json::Value = match serde_json::from_str(&raw) {
            Ok(v) => v,
            Err(e) => {
                return json!({ "city": city, "error": format!("parse error: {e}"), "source": "api" }).to_string();
            }
        };
        let body_str = resp_val.get("body").and_then(|b| b.as_str()).unwrap_or("");
        let weather_val = super::parse_weather_body(body_str);
        let current = weather_val.get("current").cloned().unwrap_or(json!({}));
        let temp_c = current
            .get("temperature_2m")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let wind_kph = current
            .get("wind_speed_10m")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let now_ms = host::now_ms();

        let result = json!({
            "temp_c": temp_c,
            "wind_kph": wind_kph,
            "fetched_at_ms": now_ms,
        });
        let cache_write = host::kv_put(&cache_key, &result.to_string());
        if cache_write.starts_with("ERROR:") {
            host::log(
                "warn",
                &format!("Cache write failed for {city}: {cache_write}"),
            );
        }

        json!({
            "city": city,
            "temp_c": temp_c,
            "wind_kph": wind_kph,
            "fetched_at_ms": now_ms,
            "source": "api",
        })
        .to_string()
    }

    export!(WeatherActorGuest);
}

/// Native build: contract tests (no WASM compilation needed)
#[cfg(not(target_arch = "wasm32"))]
pub mod contract_tests {
    use serde_json::{json, Value};

    // ----------- Minimal actor logic (mirrors wasm_app above) ---------------

    const CACHE_TTL_MS: u64 = 5 * 60 * 1000;

    #[derive(Debug, Clone, Default)]
    pub struct WeatherState {
        pub actor_id: String,
        pub cache_hits: u64,
        pub cache_misses: u64,
    }

    pub struct MockHost {
        pub kv: std::collections::HashMap<String, String>,
        pub now_ms: u64,
        pub http_response: Option<String>,
    }

    impl MockHost {
        pub fn new() -> Self {
            Self {
                kv: Default::default(),
                now_ms: 1_000_000,
                http_response: None,
            }
        }
        pub fn with_weather(mut self, temp_c: f64, wind_kph: f64) -> Self {
            let body =
                json!({ "current": { "temperature_2m": temp_c, "wind_speed_10m": wind_kph } });
            let resp = json!({ "status": 200, "headers": {}, "body": body.to_string() });
            self.http_response = Some(resp.to_string());
            self
        }
        pub fn with_weather_base64(mut self, temp_c: f64, wind_kph: f64) -> Self {
            let body =
                json!({ "current": { "temperature_2m": temp_c, "wind_speed_10m": wind_kph } });
            let resp = json!({
                "status": 200,
                "headers": {},
                "body": encode_base64_standard(body.to_string().as_bytes())
            });
            self.http_response = Some(resp.to_string());
            self
        }
        pub fn kv_get(&self, key: &str) -> String {
            self.kv.get(key).cloned().unwrap_or_default()
        }
        pub fn kv_put(&mut self, key: &str, value: &str) {
            self.kv.insert(key.to_string(), value.to_string());
        }
        pub fn http_fetch(&self) -> String {
            self.http_response.clone().unwrap_or_else(|| {
                json!({ "status": 200, "headers": {}, "body": "{}" }).to_string()
            })
        }
    }

    pub struct WeatherActor {
        pub state: WeatherState,
    }

    impl WeatherActor {
        pub fn new() -> Self {
            Self {
                state: WeatherState::default(),
            }
        }
        pub fn init(&mut self, config_json: &str) -> String {
            if let Ok(v) = serde_json::from_str::<Value>(config_json) {
                if let Some(id) = v.get("actor_id").and_then(|v| v.as_str()) {
                    self.state.actor_id = id.to_string();
                }
            }
            String::new()
        }
        pub fn get_weather(&mut self, host: &mut MockHost, city: &str) -> Value {
            let cache_key = format!("weather:{city}");
            let cached = host.kv_get(&cache_key);
            if !cached.is_empty() {
                if let Ok(data) = serde_json::from_str::<Value>(&cached) {
                    let fetched_at = data
                        .get("fetched_at_ms")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    if host.now_ms - fetched_at < CACHE_TTL_MS {
                        self.state.cache_hits += 1;
                        let mut resp = data.clone();
                        resp["city"] = json!(city);
                        resp["source"] = json!("cache");
                        return resp;
                    }
                }
            }
            self.state.cache_misses += 1;
            let raw = host.http_fetch();
            let resp_val: Value = serde_json::from_str(&raw).unwrap_or(json!({}));
            let body_str = resp_val.get("body").and_then(|b| b.as_str()).unwrap_or("");
            let weather_val = super::parse_weather_body(body_str);
            let current = weather_val.get("current").cloned().unwrap_or(json!({}));
            let temp_c = current
                .get("temperature_2m")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            let wind_kph = current
                .get("wind_speed_10m")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            let result =
                json!({ "temp_c": temp_c, "wind_kph": wind_kph, "fetched_at_ms": host.now_ms });
            host.kv_put(&cache_key, &result.to_string());
            json!({ "city": city, "temp_c": temp_c, "wind_kph": wind_kph, "fetched_at_ms": host.now_ms, "source": "api" })
        }
        pub fn cache_stats(&self) -> Value {
            json!({ "hits": self.state.cache_hits, "misses": self.state.cache_misses })
        }
        pub fn clear_cache(&mut self) -> Value {
            self.state.cache_hits = 0;
            self.state.cache_misses = 0;
            json!({ "cleared": true })
        }
    }

    fn encode_base64_standard(input: &[u8]) -> String {
        const TABLE: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
        for chunk in input.chunks(3) {
            let b0 = chunk[0];
            let b1 = *chunk.get(1).unwrap_or(&0);
            let b2 = *chunk.get(2).unwrap_or(&0);

            out.push(TABLE[(b0 >> 2) as usize] as char);
            out.push(TABLE[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize] as char);
            if chunk.len() > 1 {
                out.push(TABLE[(((b1 & 0x0f) << 2) | (b2 >> 6)) as usize] as char);
            } else {
                out.push('=');
            }
            if chunk.len() > 2 {
                out.push(TABLE[(b2 & 0x3f) as usize] as char);
            } else {
                out.push('=');
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::contract_tests::*;
    use serde_json::json;

    #[test]
    fn test_get_weather_cache_miss() {
        let mut host = MockHost::new().with_weather(15.3, 12.0);
        let mut actor = WeatherActor::new();
        actor.init(r#"{"actor_id":"weather:test@node"}"#);

        let result = actor.get_weather(&mut host, "London");
        assert_eq!(result["city"], json!("London"));
        assert_eq!(result["temp_c"], json!(15.3));
        assert_eq!(result["source"], json!("api"));
        assert_eq!(actor.state.cache_misses, 1);
        assert_eq!(actor.state.cache_hits, 0);
    }

    #[test]
    fn test_get_weather_parses_base64_response_body() {
        let mut host = MockHost::new().with_weather_base64(14.5, 9.0);
        let mut actor = WeatherActor::new();

        let result = actor.get_weather(&mut host, "Madrid");

        assert_eq!(result["city"], json!("Madrid"));
        assert_eq!(result["temp_c"], json!(14.5));
        assert_eq!(result["wind_kph"], json!(9.0));
        assert_eq!(result["source"], json!("api"));
    }

    #[test]
    fn test_get_weather_cache_hit() {
        let mut host = MockHost::new().with_weather(18.0, 8.5);
        let mut actor = WeatherActor::new();
        actor.init(r#"{"actor_id":"weather:test@node"}"#);

        actor.get_weather(&mut host, "Paris"); // miss
        let second = actor.get_weather(&mut host, "Paris"); // hit
        assert_eq!(second["source"], json!("cache"));
        assert_eq!(actor.state.cache_misses, 1);
        assert_eq!(actor.state.cache_hits, 1);
    }

    #[test]
    fn test_clear_cache() {
        let mut host = MockHost::new().with_weather(10.0, 5.0);
        let mut actor = WeatherActor::new();
        actor.init(r#"{"actor_id":"weather:test@node"}"#);

        actor.get_weather(&mut host, "Berlin");
        actor.get_weather(&mut host, "Berlin");
        let result = actor.clear_cache();
        assert_eq!(result["cleared"], json!(true));
        assert_eq!(actor.state.cache_hits, 0);
        assert_eq!(actor.state.cache_misses, 0);
    }

    #[test]
    fn test_different_cities_cached_independently() {
        let mut host = MockHost::new().with_weather(22.0, 15.0);
        let mut actor = WeatherActor::new();
        actor.init(r#"{"actor_id":"weather:test@node"}"#);

        actor.get_weather(&mut host, "Tokyo");
        actor.get_weather(&mut host, "Sydney");
        actor.get_weather(&mut host, "Tokyo"); // cached
        assert_eq!(actor.state.cache_misses, 2);
        assert_eq!(actor.state.cache_hits, 1);
    }

    #[test]
    fn test_cache_ttl_expiry() {
        let mut host = MockHost::new().with_weather(5.0, 3.0);
        let mut actor = WeatherActor::new();

        actor.get_weather(&mut host, "Oslo"); // miss, store with now_ms=1_000_000
                                              // Advance clock past TTL (5 min = 300_000ms)
        host.now_ms = 1_000_000 + 300_001;
        actor.get_weather(&mut host, "Oslo"); // miss again (TTL expired)
        assert_eq!(actor.state.cache_misses, 2);
        assert_eq!(actor.state.cache_hits, 0);
    }
}
