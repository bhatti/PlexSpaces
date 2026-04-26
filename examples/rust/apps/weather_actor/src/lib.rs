// SPDX-License-Identifier: LGPL-2.1-or-later
//
// Weather actor (Rust WASM).
//
// The example keeps the actor-world boundary simple and deterministic:
// - tests use an offline mode driven by app-config args so they do not depend on public internet
// - live deployments can switch offline_mode=false to exercise the service-link HTTP path
// - state is framework-owned protobuf bytes; ask replies stay JSON for example ergonomics

use prost::Message;
#[cfg(target_arch = "wasm32")]
use std::collections::HashMap;
use serde_json::{json, Value};

const CACHE_TTL_MS: u64 = 5 * 60 * 1000;

#[derive(Clone, PartialEq, Message)]
pub struct WeatherConfig {
    #[prost(string, tag = "1")]
    pub actor_id: String,
    #[prost(bool, tag = "2")]
    pub offline_mode: bool,
}

#[derive(Clone, PartialEq, Message)]
pub struct WeatherRequest {
    #[prost(string, tag = "1")]
    pub city: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct WeatherState {
    #[prost(string, tag = "1")]
    pub actor_id: String,
    #[prost(bool, tag = "2")]
    pub offline_mode: bool,
    #[prost(uint64, tag = "3")]
    pub cache_hits: u64,
    #[prost(uint64, tag = "4")]
    pub cache_misses: u64,
    #[prost(message, repeated, tag = "5")]
    pub entries: Vec<CacheEntry>,
}

#[derive(Clone, PartialEq, Message)]
pub struct CacheEntry {
    #[prost(string, tag = "1")]
    pub city: String,
    #[prost(double, tag = "2")]
    pub temp_c: f64,
    #[prost(double, tag = "3")]
    pub wind_kph: f64,
    #[prost(uint64, tag = "4")]
    pub fetched_at_ms: u64,
}

trait WeatherHost {
    fn now_ms(&self) -> u64;
    fn http_fetch(&mut self, path_and_query: &str) -> Result<Vec<u8>, String>;
}

#[derive(Default)]
struct WeatherActor {
    state: WeatherState,
}

impl WeatherActor {
    fn from_config(config: WeatherConfig) -> Self {
        Self {
            state: WeatherState {
                actor_id: config.actor_id,
                offline_mode: config.offline_mode,
                ..WeatherState::default()
            },
        }
    }

    fn handle<H: WeatherHost>(
        &mut self,
        host: &mut H,
        msg_type: &str,
        payload: &[u8],
    ) -> Result<Vec<u8>, String> {
        let op = parse_op(msg_type, payload)?;
        match op.as_str() {
            "get_weather" => self.handle_get_weather(host, payload),
            "cache_stats" => Ok(json!({
                "hits": self.state.cache_hits,
                "misses": self.state.cache_misses,
            })
            .to_string()
            .into_bytes()),
            "clear_cache" => {
                self.state.cache_hits = 0;
                self.state.cache_misses = 0;
                self.state.entries.clear();
                Ok(json!({ "cleared": true }).to_string().into_bytes())
            }
            other => Ok(json!({ "error": format!("unknown message type: {other}") })
                .to_string()
                .into_bytes()),
        }
    }

    fn handle_get_weather<H: WeatherHost>(
        &mut self,
        host: &mut H,
        payload: &[u8],
    ) -> Result<Vec<u8>, String> {
        let request = decode_weather_request(payload)?;
        let city = normalize_city(&request.city);
        let now_ms = host.now_ms();

        if let Some(entry) = self.find_cache_entry(&city, now_ms) {
            self.state.cache_hits += 1;
            return Ok(json!({
                "city": city,
                "temp_c": entry.temp_c,
                "wind_kph": entry.wind_kph,
                "fetched_at_ms": entry.fetched_at_ms,
                "source": "cache",
                "error": "",
            })
            .to_string()
            .into_bytes());
        }

        self.state.cache_misses += 1;
        let (temp_c, wind_kph, source) = if self.state.offline_mode {
            let (temp_c, wind_kph) = deterministic_weather(&city);
            (temp_c, wind_kph, "api")
        } else {
            let (temp_c, wind_kph) = fetch_live_weather(host, &city)?;
            (temp_c, wind_kph, "api")
        };

        let entry = CacheEntry {
            city: city.clone(),
            temp_c,
            wind_kph,
            fetched_at_ms: now_ms,
        };
        self.upsert_cache_entry(entry.clone());

        Ok(json!({
            "city": city,
            "temp_c": entry.temp_c,
            "wind_kph": entry.wind_kph,
            "fetched_at_ms": entry.fetched_at_ms,
            "source": source,
            "error": "",
        })
        .to_string()
        .into_bytes())
    }

    fn find_cache_entry(&self, city: &str, now_ms: u64) -> Option<CacheEntry> {
        self.state.entries.iter().find_map(|entry| {
            if entry.city.eq_ignore_ascii_case(city)
                && now_ms.saturating_sub(entry.fetched_at_ms) < CACHE_TTL_MS
            {
                Some(entry.clone())
            } else {
                None
            }
        })
    }

    fn upsert_cache_entry(&mut self, entry: CacheEntry) {
        if let Some(existing) = self
            .state
            .entries
            .iter_mut()
            .find(|candidate| candidate.city.eq_ignore_ascii_case(&entry.city))
        {
            *existing = entry;
        } else {
            self.state.entries.push(entry);
        }
    }
}

fn normalize_city(city: &str) -> String {
    let trimmed = city.trim();
    if trimmed.is_empty() {
        "London".to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(target_arch = "wasm32")]
fn actor_application_id(actor_id: &str) -> String {
    if let Some(namespace) = actor_id
        .split_once("//")
        .and_then(|(_, suffix)| suffix.split_once('@').map(|(qualified, _)| qualified))
        .and_then(|qualified| qualified.rsplit_once("::").map(|(_, namespace)| namespace))
    {
        return namespace.to_string();
    }

    actor_id
        .split_once(':')
        .and_then(|(_, suffix)| suffix.split_once('@').map(|(namespace, _)| namespace))
        .map(str::to_string)
        .unwrap_or_default()
}

#[cfg(target_arch = "wasm32")]
fn actor_node_id(actor_id: &str) -> String {
    actor_id
        .rsplit_once('@')
        .map(|(_, node_id)| node_id.to_string())
        .unwrap_or_else(|| "local".to_string())
}

#[cfg(target_arch = "wasm32")]
fn json_object_to_metric_map(value: Option<&Value>) -> HashMap<String, u64> {
    value.and_then(|value| value.as_object()).map(|entries| {
        entries.iter().filter_map(|(key, value)| value.as_u64().map(|parsed| (key.clone(), parsed))).collect()
    }).unwrap_or_default()
}

#[cfg(target_arch = "wasm32")]
fn application_metrics_from_json(metrics: Value) -> plexspaces_proto::application::v1::ApplicationMetrics {
    plexspaces_proto::application::v1::ApplicationMetrics {
        actor_counts: json_object_to_metric_map(metrics.get("actor_counts")),
        supervisor_count: metrics.get("supervisor_count").and_then(|value| value.as_u64()).unwrap_or(0) as u32,
        uptime_seconds: metrics.get("uptime_seconds").and_then(|value| value.as_u64()).unwrap_or(0),
        message_count: metrics.get("message_count").and_then(|value| value.as_u64()).unwrap_or(0),
        error_count: metrics.get("error_count").and_then(|value| value.as_u64()).unwrap_or(0),
        counter_metrics: json_object_to_metric_map(metrics.get("counter_metrics")),
        latency_totals_ms: json_object_to_metric_map(metrics.get("latency_totals_ms")),
        latency_max_ms: json_object_to_metric_map(metrics.get("latency_max_ms")),
        latency_samples: json_object_to_metric_map(metrics.get("latency_samples")),
    }
}

#[cfg(target_arch = "wasm32")]
fn application_metrics_to_json(metrics: &plexspaces_proto::application::v1::ApplicationMetrics) -> Value {
    json!({
        "actor_counts": metrics.actor_counts,
        "supervisor_count": metrics.supervisor_count,
        "uptime_seconds": metrics.uptime_seconds,
        "message_count": metrics.message_count,
        "error_count": metrics.error_count,
        "counter_metrics": metrics.counter_metrics,
        "latency_totals_ms": metrics.latency_totals_ms,
        "latency_max_ms": metrics.latency_max_ms,
        "latency_samples": metrics.latency_samples,
    })
}

fn encode_message<M: Message>(message: &M) -> Vec<u8> {
    message.encode_to_vec()
}

fn decode_message<M>(payload: &[u8]) -> Result<M, String>
where
    M: Message + Default,
{
    M::decode(payload).map_err(|err| err.to_string())
}

fn parse_payload(payload: &[u8]) -> Result<Value, String> {
    if payload.is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_slice(payload).map_err(|err| format!("invalid payload: {err}"))
}

fn parse_op(msg_type: &str, payload: &[u8]) -> Result<String, String> {
    let payload = parse_payload(payload)?;
    if let Some(op) = payload
        .get("op")
        .or_else(|| payload.get("message_type"))
        .or_else(|| payload.get("msg_type"))
        .and_then(|value| value.as_str())
    {
        Ok(op.to_string())
    } else if msg_type == "call" || msg_type == "cast" {
        Err("missing op".to_string())
    } else {
        Ok(msg_type.to_string())
    }
}

fn decode_weather_request(payload: &[u8]) -> Result<WeatherRequest, String> {
    if let Ok(request) = decode_message::<WeatherRequest>(payload) {
        if !request.city.is_empty() {
            return Ok(request);
        }
    }

    let value = parse_payload(payload)?;
    Ok(WeatherRequest {
        city: value
            .get("city")
            .and_then(|entry| entry.as_str())
            .unwrap_or("London")
            .to_string(),
    })
}

fn decode_weather_config(payload: &[u8], default_actor_id: &str) -> WeatherConfig {
    if let Ok(config) = decode_message::<WeatherConfig>(payload) {
        if !config.actor_id.is_empty() {
            return config;
        }
    }

    let value = serde_json::from_slice::<Value>(payload).unwrap_or_else(|_| json!({}));
    let actor_id = value
        .get("actor_id")
        .and_then(|entry| entry.as_str())
        .filter(|entry| !entry.is_empty())
        .unwrap_or(default_actor_id)
        .to_string();
    let offline_mode = parse_bool_like(
        value.pointer("/args/offline_mode"),
        true,
    );

    WeatherConfig {
        actor_id,
        offline_mode,
    }
}

fn parse_bool_like(value: Option<&Value>, default: bool) -> bool {
    match value {
        Some(Value::Bool(value)) => *value,
        Some(Value::String(value)) => match value.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => true,
            "0" | "false" | "no" | "off" => false,
            _ => default,
        },
        _ => default,
    }
}

fn deterministic_weather(city: &str) -> (f64, f64) {
    match city.to_ascii_lowercase().as_str() {
        "london" => (15.2, 11.0),
        "sydney" => (24.8, 16.5),
        "paris" => (18.4, 9.2),
        "berlin" => (17.1, 10.4),
        "tokyo" => (22.3, 13.7),
        _ => {
            let hash = city
                .bytes()
                .fold(0u32, |acc, byte| acc.wrapping_mul(31).wrapping_add(byte as u32));
            let temp_c = 10.0 + f64::from(hash % 180) / 10.0;
            let wind_kph = 5.0 + f64::from((hash / 7) % 120) / 10.0;
            (temp_c, wind_kph)
        }
    }
}

fn fetch_live_weather<H: WeatherHost>(host: &mut H, city: &str) -> Result<(f64, f64), String> {
    let path = format!(
        "/v1/forecast?latitude=51.5&longitude=-0.12&current=temperature_2m,wind_speed_10m&city={city}"
    );
    let response =
        decode_message::<plexspaces_proto::wasm::v1::HttpFetchResponse>(&host.http_fetch(&path)?)?;
    let body = parse_weather_body(&response.body);
    let current = body.get("current").cloned().unwrap_or_else(|| json!({}));
    Ok((
        current
            .get("temperature_2m")
            .and_then(|value| value.as_f64())
            .unwrap_or(0.0),
        current
            .get("wind_speed_10m")
            .and_then(|value| value.as_f64())
            .unwrap_or(0.0),
    ))
}

fn parse_weather_body(body: &[u8]) -> Value {
    if body.is_empty() {
        return json!({});
    }

    serde_json::from_slice(body)
        .ok()
        .or_else(|| {
            std::str::from_utf8(body)
                .ok()
                .and_then(|value| decode_base64_standard(value).ok())
                .and_then(|decoded| serde_json::from_slice(&decoded).ok())
        })
        .unwrap_or_else(|| json!({}))
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

#[cfg(target_arch = "wasm32")]
mod wasm_app {
    use super::*;
    use plexspaces_proto::wasm::v1::HttpFetchRequest;
    use std::sync::{Mutex, OnceLock};

    const LINK_NAME: &str = "weather-api";

    wit_bindgen::generate!({
        path: "../../../../wit/plexspaces-actor",
        world: "actor-world",
    });

    use exports::plexspaces::actor::actor::Guest;
    use plexspaces::actor::host;

    struct WasmHost;

    struct WeatherBridge;

    fn state_cell() -> &'static Mutex<WeatherState> {
        static STATE: OnceLock<Mutex<WeatherState>> = OnceLock::new();
        STATE.get_or_init(|| Mutex::new(WeatherState::default()))
    }

    fn current_actor_id() -> String {
        state_cell()
            .lock()
            .expect("weather state lock poisoned")
            .actor_id
            .clone()
    }

    fn current_application_id() -> String {
        actor_application_id(&current_actor_id())
    }

    fn current_node_id() -> String {
        actor_node_id(&host::self_id())
    }

    fn merge_application_metrics(metrics: Value, context: &str) -> Result<(), String> {
        let metrics_bytes = application_metrics_from_json(metrics).encode_to_vec();
        host::application_metrics_add(&current_application_id(), &metrics_bytes)
            .map(|_| ())
            .map_err(|err| format!("{context}: {err}"))
    }

    fn record_metrics(op: &str, payload: &[u8]) -> Result<(), String> {
        let value = serde_json::from_slice::<Value>(payload).unwrap_or_else(|_| json!({}));
        let has_error = value
            .get("error")
            .and_then(|entry| entry.as_str())
            .map(|entry| !entry.is_empty())
            .unwrap_or(false);

        let mut counter_metrics = serde_json::Map::new();
        if op == "get_weather" {
            if let Some(source) = value.get("source").and_then(|entry| entry.as_str()) {
                match source {
                    "api" => {
                        counter_metrics.insert(
                            "weather_api_requests".to_string(),
                            Value::from(1_u64),
                        );
                    }
                    "cache" => {
                        counter_metrics.insert(
                            "weather_cache_hits".to_string(),
                            Value::from(1_u64),
                        );
                    }
                    _ => {}
                }
            }
        }
        if op == "clear_cache" {
            counter_metrics.insert("cache_clears".to_string(), Value::from(1_u64));
        }

        merge_application_metrics(
            json!({
                "message_count": 1,
                "error_count": u64::from(has_error),
                "counter_metrics": counter_metrics,
            }),
            "weather metrics update",
        )
    }

    fn handle_get_metrics() -> Result<Vec<u8>, String> {
        let response = host::application_get_metrics(&current_application_id(), &current_node_id())?;
        let metrics = plexspaces_proto::application::v1::ApplicationMetrics::decode(response.as_slice())
            .map_err(|err| format!("invalid ApplicationMetrics protobuf: {err}"))?;
        Ok(application_metrics_to_json(&metrics).to_string().into_bytes())
    }

    impl WeatherHost for WasmHost {
        fn now_ms(&self) -> u64 {
            host::now_ms()
        }

        fn http_fetch(&mut self, path_and_query: &str) -> Result<Vec<u8>, String> {
            let request = HttpFetchRequest {
                headers: Default::default(),
                body: Vec::new(),
            };
            host::http_fetch(LINK_NAME, "GET", path_and_query, &encode_message(&request))
        }
    }

    impl Guest for WeatherBridge {
        fn init(config: Vec<u8>) -> Result<(), String> {
            let config = decode_weather_config(&config, &host::self_id());
            let actor = WeatherActor::from_config(config);
            let mut guard = state_cell().lock().expect("weather state lock poisoned");
            *guard = actor.state;
            Ok(())
        }

        fn handle(
            _from_actor: String,
            msg_type: String,
            payload: Vec<u8>,
        ) -> Result<Vec<u8>, String> {
            let op = parse_op(&msg_type, &payload)?;
            if op == "get_metrics" || op == "metrics" {
                return handle_get_metrics();
            }

            let current_state = state_cell()
                .lock()
                .expect("weather state lock poisoned")
                .clone();
            let mut actor = WeatherActor {
                state: current_state,
            };
            let mut host_adapter = WasmHost;
            let result = actor.handle(&mut host_adapter, &msg_type, &payload);
            {
                let mut guard = state_cell().lock().expect("weather state lock poisoned");
                *guard = actor.state;
            }
            if let Ok(ref response_payload) = result {
                let _ = record_metrics(&op, response_payload);
            }
            result
        }

        fn get_state() -> Result<Vec<u8>, String> {
            let guard = state_cell().lock().expect("weather state lock poisoned");
            Ok(encode_message(&*guard))
        }

        fn set_state(state: Vec<u8>) -> Result<(), String> {
            let next_state = if state.is_empty() {
                WeatherState::default()
            } else {
                decode_message(&state)?
            };
            let mut guard = state_cell().lock().expect("weather state lock poisoned");
            *guard = next_state;
            Ok(())
        }
    }

    export!(WeatherBridge);
}

#[cfg(not(target_arch = "wasm32"))]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct MockHost {
        now_ms: u64,
        http_response: Vec<u8>,
    }

    impl MockHost {
        fn new() -> Self {
            Self {
                now_ms: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|duration| duration.as_millis() as u64)
                    .unwrap_or(0),
                http_response: encode_message(&plexspaces_proto::wasm::v1::HttpFetchResponse {
                    status: 200,
                    headers: Default::default(),
                    body: b"{}".to_vec(),
                }),
            }
        }

        fn with_live_weather(mut self, temp_c: f64, wind_kph: f64) -> Self {
            let body =
                json!({ "current": { "temperature_2m": temp_c, "wind_speed_10m": wind_kph } });
            self.http_response = encode_message(&plexspaces_proto::wasm::v1::HttpFetchResponse {
                status: 200,
                headers: Default::default(),
                body: body.to_string().into_bytes(),
            });
            self
        }
    }

    impl WeatherHost for MockHost {
        fn now_ms(&self) -> u64 {
            self.now_ms
        }

        fn http_fetch(&mut self, _path_and_query: &str) -> Result<Vec<u8>, String> {
            Ok(self.http_response.clone())
        }
    }

    fn decode_json(bytes: &[u8]) -> Value {
        serde_json::from_slice(bytes).expect("reply should decode as json")
    }

    #[test]
    fn decode_weather_config_reads_json_args() {
        let payload = br#"{"actor_id":"weather:test","args":{"offline_mode":"true"}}"#;
        let config = decode_weather_config(payload, "weather:default");
        assert_eq!(config.actor_id, "weather:test");
        assert!(config.offline_mode);
    }

    #[test]
    fn offline_mode_is_deterministic_and_cached() {
        let mut host = MockHost::new();
        let mut actor = WeatherActor::from_config(WeatherConfig {
            actor_id: "weather:test".to_string(),
            offline_mode: true,
        });

        let first = decode_json(
            &actor
                .handle(
                    &mut host,
                    "get_weather",
                    br#"{"op":"get_weather","city":"London"}"#,
                )
                .expect("first request should succeed"),
        );
        let second = decode_json(
            &actor
                .handle(
                    &mut host,
                    "get_weather",
                    br#"{"op":"get_weather","city":"London"}"#,
                )
                .expect("second request should succeed"),
        );

        assert_eq!(first["source"], "api");
        assert_eq!(second["source"], "cache");
        assert_eq!(actor.state.cache_hits, 1);
        assert_eq!(actor.state.cache_misses, 1);
    }

    #[test]
    fn clear_cache_resets_entries_and_counters() {
        let mut host = MockHost::new();
        let mut actor = WeatherActor::from_config(WeatherConfig {
            actor_id: "weather:test".to_string(),
            offline_mode: true,
        });

        let _ = actor.handle(
            &mut host,
            "get_weather",
            br#"{"op":"get_weather","city":"Paris"}"#,
        );
        let cleared = decode_json(
            &actor
                .handle(&mut host, "clear_cache", br#"{"op":"clear_cache"}"#)
                .expect("clear cache should succeed"),
        );

        assert_eq!(cleared["cleared"], true);
        assert!(actor.state.entries.is_empty());
        assert_eq!(actor.state.cache_hits, 0);
        assert_eq!(actor.state.cache_misses, 0);
    }

    #[test]
    fn live_mode_decodes_http_response() {
        let mut host = MockHost::new().with_live_weather(17.5, 8.2);
        let mut actor = WeatherActor::from_config(WeatherConfig {
            actor_id: "weather:test".to_string(),
            offline_mode: false,
        });

        let response = decode_json(
            &actor
                .handle(
                    &mut host,
                    "get_weather",
                    br#"{"op":"get_weather","city":"Berlin"}"#,
                )
                .expect("live request should succeed"),
        );

        assert_eq!(response["city"], "Berlin");
        assert_eq!(response["temp_c"], 17.5);
        assert_eq!(response["wind_kph"], 8.2);
        assert_eq!(response["source"], "api");
    }
}
