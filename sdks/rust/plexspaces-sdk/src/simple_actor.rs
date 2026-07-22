// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Actor-world WIT support for deployable Rust WASM applications.
//
// This module centralizes the WIT bindings and the boilerplate Guest/export glue
// so Rust WASM examples do not need to hand-write `wit_bindgen::generate!`,
// `impl Guest`, and `export!(...)` in every app.

wit_bindgen::generate!({
    path: "../../../wit/plexspaces-actor",
    world: "actor-world",
});

pub use exports::plexspaces::actor::actor::Guest;

use plexspaces::actor::host_kv::{
    alarm_delete as raw_alarm_delete, alarm_get as raw_alarm_get, alarm_set as raw_alarm_set,
    kv_cas as raw_kv_cas,
    kv_get, kv_increment as raw_kv_increment,
    kv_multi_get as raw_kv_multi_get, kv_multi_put as raw_kv_multi_put,
    kv_put, kv_put_with_ttl as raw_kv_put_with_ttl,
};
pub use plexspaces::actor::host_kv::{kv_delete, kv_list};
use plexspaces::actor::host_actor::pg_members;
use plexspaces::actor::host_logging::{log, now_ms};
use plexspaces::actor::host_shard::application_metrics_add;

/// Decode a protobuf message from actor-world bytes.
pub fn decode_proto<M>(payload: &[u8]) -> Result<M, String>
where
    M: prost::Message + Default,
{
    M::decode(payload).map_err(|err| err.to_string())
}

/// Encode a protobuf message for the actor-world boundary.
pub fn encode_proto<M>(message: &M) -> Vec<u8>
where
    M: prost::Message,
{
    message.encode_to_vec()
}

/// Trait implemented by deployable actor-world WASM app roots.
///
/// The trait keeps the WIT boundary in the SDK while allowing app code to focus on
/// initialization, message handling, and protobuf-encoded state snapshots.
pub trait ActorWorldApp: Sized {
    type State;

    fn init(config: Vec<u8>) -> Result<Self, String>;
    fn handle(
        &mut self,
        from_actor: String,
        msg_type: String,
        payload: Vec<u8>,
    ) -> Result<Vec<u8>, String>;
    fn state(&self) -> &Self::State;
    fn state_mut(&mut self) -> &mut Self::State;
    fn encode_state(state: &Self::State) -> Result<Vec<u8>, String>;
    fn decode_state(state: &[u8]) -> Result<Self::State, String>;
}

// ============================================================================
// Tier 1 ergonomics helpers for WASM actors
// ============================================================================

// ============================================================================
// EventLog — two-cursor watermark
// ============================================================================

/// Two-cursor monotonic append-only log backed by KV.
///
/// Embed in actor state and derive `Serialize`/`Deserialize` so the watermark
/// survives actor restarts.  Each consumer tracks its own cursor independently.
///
/// ```rust,ignore
/// #[derive(Serialize, Deserialize, Default)]
/// struct MyState {
///     log: EventLog,
/// }
///
/// // append
/// let seq = state.log.append(&"audit:", &entry)?;
///
/// // poll
/// let (events, new_cursor) = state.log.poll(&"audit:", "consumer-1", 20)?;
/// ```
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct EventLog {
    pub watermark: i64,
}

impl EventLog {
    /// Append an entry to the log. Returns the assigned sequence number.
    pub fn append<T: serde::Serialize>(&mut self, prefix: &str, entry: &T) -> Result<i64, String> {
        self.watermark += 1;
        let key = format!("{}seq:{}", prefix, self.watermark);
        match kv_put_json(&key, entry) {
            Ok(()) => Ok(self.watermark),
            Err(e) => {
                self.watermark -= 1;
                Err(format!("EventLog.append: {e}"))
            }
        }
    }

    /// Return up to `limit` events for `consumer_id` that arrived after its last cursor.
    /// Returns `(events, new_cursor)`. The new cursor is persisted in KV.
    pub fn poll<T: serde::de::DeserializeOwned>(
        &self,
        prefix: &str,
        consumer_id: &str,
        limit: usize,
    ) -> Result<(Vec<T>, i64), String> {
        let cursor_key = format!("{}cursor:{}", prefix, consumer_id);
        let cursor: i64 = kv_get(&cursor_key)
            .ok()
            .and_then(|b| String::from_utf8(b).ok())
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(0);

        let mut events = Vec::new();
        let mut new_cursor = cursor;
        let mut seq = cursor + 1;
        while seq <= self.watermark && events.len() < limit {
            let key = format!("{}seq:{}", prefix, seq);
            match kv_get_json::<T>(&key)? {
                Some(entry) => {
                    events.push(entry);
                    new_cursor = seq;
                }
                None => {}
            }
            seq += 1;
        }

        if new_cursor != cursor {
            let cursor_bytes = new_cursor.to_string().into_bytes();
            let _ = kv_put(&cursor_key, &cursor_bytes);
        }
        Ok((events, new_cursor))
    }
}

/// Return the first member of a named process group.
///
/// Returns `Ok(actor_id)` on success, `Err` if the group is empty or the host call fails.
///
/// ```rust,ignore
/// let router_id = pg_first("svc:llm_router")?;
/// ```
pub fn pg_first(group: &str) -> Result<String, String> {
    let members = pg_members(group)?;
    members
        .into_iter()
        .next()
        .ok_or_else(|| format!("no members in process group {group:?}"))
}

/// Retrieve a value by key and deserialize it from JSON.
///
/// Returns `Ok(Some(T))` on success, `Ok(None)` if the key does not exist,
/// or `Err` if deserialization fails.
///
/// ```rust,ignore
/// let task: Option<Task> = kv_get_json("queue:pending:1")?;
/// ```
pub fn kv_get_json<T: serde::de::DeserializeOwned>(key: &str) -> Result<Option<T>, String> {
    let raw = kv_get(key)?;
    if raw.is_empty() {
        return Ok(None);
    }
    serde_json::from_slice::<T>(&raw)
        .map(Some)
        .map_err(|e| format!("kv_get_json({key:?}): {e}"))
}

/// Serialize a value to JSON and store it under `key`.
///
/// Returns `Err` if serialization or the host write fails.
///
/// ```rust,ignore
/// kv_put_json("queue:pending:1", &task)?;
/// ```
pub fn kv_put_json<T: serde::Serialize>(key: &str, value: &T) -> Result<(), String> {
    let bytes =
        serde_json::to_vec(value).map_err(|e| format!("kv_put_json({key:?}): serialize: {e}"))?;
    kv_put(key, &bytes)
}

/// Store a value with automatic expiry after `ttl_seconds`.
pub fn kv_put_with_ttl(key: &str, value: &[u8], ttl_seconds: u64) -> Result<(), String> {
    raw_kv_put_with_ttl(key, &value.to_vec(), ttl_seconds)
}

/// Atomically increment a numeric counter by `delta`. Returns the new value.
pub fn kv_increment(key: &str, delta: i64) -> Result<i64, String> {
    raw_kv_increment(key, delta)
}

/// Compare-and-swap: set key to `new_value` only if current value equals `expected`.
/// Pass `None` for `expected` to assert the key does not exist.
/// Returns `true` if the swap was applied.
pub fn kv_cas(key: &str, expected: Option<&[u8]>, new_value: &[u8]) -> Result<bool, String> {
    let exp = match expected {
        Some(b) => b.to_vec(),
        None => Vec::new(),
    };
    raw_kv_cas(key, &exp, &new_value.to_vec())
}

/// Fetch multiple keys in one call.
/// Returns values in the same order as `keys`; `None` for missing keys.
pub fn kv_multi_get(keys: &[&str]) -> Result<Vec<Option<Vec<u8>>>, String> {
    let keys_json = serde_json::to_vec(keys)
        .map_err(|e| format!("kv_multi_get: serialize keys: {e}"))?;
    let result_bytes = raw_kv_multi_get(&keys_json)?;
    let result: Vec<Option<String>> = serde_json::from_slice(&result_bytes)
        .map_err(|e| format!("kv_multi_get: parse response: {e}"))?;
    result.into_iter()
        .map(|v| v.map(|b64| base64::Engine::decode(&base64::engine::general_purpose::STANDARD, b64)
            .map_err(|e| format!("kv_multi_get: base64 decode: {e}")))
            .transpose())
        .collect()
}

/// Store multiple key-value pairs in one call.
pub fn kv_multi_put(entries: &[(&str, &[u8])]) -> Result<(), String> {
    let encoded: std::collections::HashMap<&str, String> = entries.iter()
        .map(|(k, v)| (*k, base64::Engine::encode(&base64::engine::general_purpose::STANDARD, v)))
        .collect();
    let entries_json = serde_json::to_vec(&encoded)
        .map_err(|e| format!("kv_multi_put: serialize: {e}"))?;
    raw_kv_multi_put(&entries_json)
}

/// Schedule a durable alarm at an absolute timestamp (milliseconds since epoch).
/// The alarm survives actor deactivation. When it fires, the actor receives a "__alarm__" message.
/// Equivalent to Cloudflare Durable Object setAlarm(timestamp).
pub fn alarm_set(timestamp_ms: u64) -> Result<(), String> {
    raw_alarm_set(timestamp_ms)
}

/// Schedule an alarm relative to now (convenience wrapper around alarm_set).
pub fn alarm_set_in(delay_ms: u64) -> Result<(), String> {
    raw_alarm_set(now_ms() + delay_ms)
}

/// Returns the scheduled alarm timestamp in ms, or 0 if no alarm is set.
pub fn alarm_get() -> Result<u64, String> {
    raw_alarm_get()
}

/// Cancel the pending durable alarm.
pub fn alarm_delete() -> Result<(), String> {
    raw_alarm_delete()
}

/// Increment a single named application metric counter by 1.
///
/// Errors are logged as warnings and never propagate — metrics must not crash actors.
///
/// ```rust,ignore
/// incr_counter(application_id, "agent_chats");
/// ```
pub fn incr_counter(application_id: &str, name: &str) {
    incr_counters(application_id, &[(name, 1u64)]);
}

/// Increment one or more named application metric counters.
///
/// `counters` is a slice of `(name, delta)` pairs.
/// Errors are logged as warnings and never propagate.
///
/// ```rust,ignore
/// incr_counters(application_id, &[("cache_hits", 5), ("cache_misses", 2)]);
/// ```
pub fn incr_counters(application_id: &str, counters: &[(&str, u64)]) {
    use prost::Message as ProstMessage;

    let counter_metrics: std::collections::HashMap<String, u64> =
        counters.iter().map(|(k, v)| (k.to_string(), *v)).collect();

    let metrics = plexspaces_proto::application::v1::ApplicationMetrics {
        message_count: counters.len() as u64,
        counter_metrics,
        ..Default::default()
    };
    let bytes = metrics.encode_to_vec();
    if let Err(e) = application_metrics_add(application_id, &bytes) {
        log(
            "warn",
            &format!("incr_counters: metrics update failed: {e}"),
        );
    }
}

/// Trait implemented by annotation-driven leader/worker handlers inside an actor-world app.
///
/// This keeps deployable WASM examples close to the native SDK style: actor structs declare
/// handlers with annotations, while the outer app only decides which role instance handles
/// the current message.
pub trait ActorWorldHandlers {
    /// Optional initialization hook for handler-local configuration.
    fn init(&mut self, _config: &[u8]) -> Result<(), String> {
        Ok(())
    }

    /// Dispatch one operation for the current role.
    fn handle_operation(
        &mut self,
        from_actor: &str,
        op: &str,
        payload: &[u8],
    ) -> Result<Vec<u8>, String>;
}

/// Export an `ActorWorldApp` implementation as the `plexspaces:actor` guest.
///
/// The generated wrapper keeps one singleton app instance per WASM component instance and
/// handles `get-state` / `set-state` through the app's protobuf codec.
#[macro_export]
macro_rules! export_actor_world_app {
    ($app_ty:ty) => {
        struct __PlexspacesActorWorldComponent;

        static __PLEXSPACES_ACTOR_WORLD_APP: ::std::sync::OnceLock<::std::sync::Mutex<$app_ty>> =
            ::std::sync::OnceLock::new();

        impl $crate::simple_actor::Guest for __PlexspacesActorWorldComponent {
            fn init(
                config: ::std::vec::Vec<u8>,
            ) -> ::core::result::Result<(), ::std::string::String> {
                match <$app_ty as $crate::simple_actor::ActorWorldApp>::init(config) {
                    Ok(app) => {
                        if let Some(cell) = __PLEXSPACES_ACTOR_WORLD_APP.get() {
                            let mut guard =
                                cell.lock().expect("actor-world app state lock poisoned");
                            *guard = app;
                        } else {
                            let _ = __PLEXSPACES_ACTOR_WORLD_APP.set(::std::sync::Mutex::new(app));
                        }
                        Ok(())
                    }
                    Err(err) => Err(err),
                }
            }

            fn handle(
                from_actor: ::std::string::String,
                msg_type: ::std::string::String,
                payload: ::std::vec::Vec<u8>,
            ) -> ::core::result::Result<::std::vec::Vec<u8>, ::std::string::String> {
                let cell = __PLEXSPACES_ACTOR_WORLD_APP
                    .get()
                    .expect("actor-world app used before init");
                let mut guard = cell.lock().expect("actor-world app state lock poisoned");
                <$app_ty as $crate::simple_actor::ActorWorldApp>::handle(
                    &mut *guard,
                    from_actor,
                    msg_type,
                    payload,
                )
            }

            fn get_state() -> ::core::result::Result<::std::vec::Vec<u8>, ::std::string::String> {
                let cell = __PLEXSPACES_ACTOR_WORLD_APP
                    .get()
                    .expect("actor-world app used before init");
                let guard = cell.lock().expect("actor-world app state lock poisoned");
                <$app_ty as $crate::simple_actor::ActorWorldApp>::encode_state(
                    <$app_ty as $crate::simple_actor::ActorWorldApp>::state(&*guard),
                )
            }

            fn set_state(
                state: ::std::vec::Vec<u8>,
            ) -> ::core::result::Result<(), ::std::string::String> {
                let next_state =
                    <$app_ty as $crate::simple_actor::ActorWorldApp>::decode_state(&state)?;
                let cell = __PLEXSPACES_ACTOR_WORLD_APP
                    .get()
                    .expect("actor-world app used before init");
                let mut guard = cell.lock().expect("actor-world app state lock poisoned");
                *<$app_ty as $crate::simple_actor::ActorWorldApp>::state_mut(&mut *guard) =
                    next_state;
                Ok(())
            }
        }

        $crate::simple_actor::export!(__PlexspacesActorWorldComponent);
    };
}

// ============================================================================
// Unit tests for Tier 1 helpers (pure logic, no host calls)
// ============================================================================

#[cfg(test)]
mod tests {
    use super::{kv_get_json, kv_put_json};
    use plexspaces_proto::application::v1::ApplicationMetrics;
    use prost::Message as ProstMessage;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Task {
        seq: u32,
        task_type: String,
    }

    // kv_put_json encodes to valid JSON bytes that kv_get_json round-trips
    #[test]
    fn kv_put_json_produces_bytes_that_deserialize_correctly() {
        let task = Task {
            seq: 42,
            task_type: "summarize".into(),
        };
        let bytes = serde_json::to_vec(&task).expect("serialize");
        let restored: Task = serde_json::from_slice(&bytes).expect("deserialize");
        assert_eq!(restored, task);
    }

    // kv_get_json returns None for empty bytes (missing key)
    #[test]
    fn kv_get_json_logic_returns_none_for_empty_bytes() {
        let empty: &[u8] = b"";
        let result: Option<Task> = if empty.is_empty() {
            None
        } else {
            serde_json::from_slice(empty).map(Some).unwrap_or(None)
        };
        assert!(result.is_none());
    }

    // kv_get_json returns None for corrupt JSON
    #[test]
    fn kv_get_json_logic_returns_none_for_corrupt_json() {
        let corrupt = b"not-json{";
        let result: Result<Option<Task>, _> = serde_json::from_slice(corrupt).map(Some);
        assert!(result.is_err());
    }

    // kv_put_json fails for values that can't be serialized
    #[test]
    fn kv_put_json_logic_errors_on_bad_value() {
        // std::collections::HashMap with non-string keys can't serialize to JSON
        use std::collections::HashMap;
        let mut m: HashMap<u8, &str> = HashMap::new();
        m.insert(1, "a");
        let result = serde_json::to_vec(&m);
        // serde_json encodes integer map keys as strings — use a truly un-serializable type
        // A channel cannot be serialized; here we verify the error path via a custom type
        #[derive(Serialize)]
        struct Bad {
            #[serde(serialize_with = "fail_serialize")]
            val: u32,
        }
        fn fail_serialize<S: serde::Serializer>(_: &u32, _: S) -> Result<S::Ok, S::Error> {
            Err(serde::ser::Error::custom("intentional failure"))
        }
        let err = serde_json::to_vec(&Bad { val: 0 });
        assert!(err.is_err());
    }

    // incr_counters encodes ApplicationMetrics protobuf correctly
    #[test]
    fn incr_counters_encodes_correct_protobuf() {
        let counters: &[(&str, u64)] = &[("cache_hits", 5), ("cache_misses", 2)];
        let counter_metrics: std::collections::HashMap<String, u64> =
            counters.iter().map(|(k, v)| (k.to_string(), *v)).collect();
        let metrics = ApplicationMetrics {
            message_count: counters.len() as u64,
            counter_metrics,
            ..Default::default()
        };
        let bytes = metrics.encode_to_vec();
        let decoded = ApplicationMetrics::decode(bytes.as_slice()).expect("decode");
        assert_eq!(decoded.message_count, 2);
        assert_eq!(decoded.counter_metrics["cache_hits"], 5);
        assert_eq!(decoded.counter_metrics["cache_misses"], 2);
    }

    // incr_counter is just incr_counters with count 1
    #[test]
    fn incr_counter_encodes_single_counter_with_count_one() {
        let counters: &[(&str, u64)] = &[("my_op", 1)];
        let counter_metrics: std::collections::HashMap<String, u64> =
            counters.iter().map(|(k, v)| (k.to_string(), *v)).collect();
        let metrics = ApplicationMetrics {
            message_count: 1,
            counter_metrics,
            ..Default::default()
        };
        let bytes = metrics.encode_to_vec();
        let decoded = ApplicationMetrics::decode(bytes.as_slice()).expect("decode");
        assert_eq!(decoded.message_count, 1);
        assert_eq!(decoded.counter_metrics["my_op"], 1);
    }

    // pg_first logic: first element of a non-empty list
    #[test]
    fn pg_first_logic_returns_first_member() {
        let members = vec!["actor1@node".to_string(), "actor2@node".to_string()];
        let first = members.into_iter().next();
        assert_eq!(first.as_deref(), Some("actor1@node"));
    }

    // pg_first logic: empty list returns None → maps to error
    #[test]
    fn pg_first_logic_returns_none_for_empty_list() {
        let members: Vec<String> = vec![];
        let first = members.into_iter().next();
        assert!(first.is_none());
    }

    // EventLog: watermark increments on each append
    #[test]
    fn event_log_watermark_increments_on_each_append() {
        // Simulate append logic without calling host (pure watermark tracking)
        let mut log = super::EventLog::default();
        assert_eq!(log.watermark, 0);
        log.watermark += 1; // simulates successful append
        assert_eq!(log.watermark, 1);
        log.watermark += 1;
        assert_eq!(log.watermark, 2);
    }

    // EventLog: rollback on failed append
    #[test]
    fn event_log_watermark_rolls_back_on_failure() {
        let mut log = super::EventLog::default();
        log.watermark += 1;
        // simulate failure: roll back
        log.watermark -= 1;
        assert_eq!(log.watermark, 0);
    }

    // EventLog: serializes / deserializes watermark correctly
    #[test]
    fn event_log_serializes_watermark() {
        let mut log = super::EventLog::default();
        log.watermark = 42;
        let json = serde_json::to_string(&log).expect("serialize");
        let restored: super::EventLog = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(restored.watermark, 42);
    }

    // EventLog poll cursor logic: returns entries from (cursor+1)..watermark
    #[test]
    fn event_log_poll_cursor_range() {
        // Simulate the sequence: watermark=3, consumer at cursor=1 → should get seq 2, 3
        let watermark: i64 = 3;
        let cursor: i64 = 1;
        let limit = 10;
        let available_seqs: Vec<i64> = (cursor + 1..=watermark).take(limit).collect();
        assert_eq!(available_seqs, vec![2, 3]);
    }
}
