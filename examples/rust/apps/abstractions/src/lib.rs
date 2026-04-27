// SPDX-License-Identifier: AGPL-3.0-or-later

#[cfg(target_arch = "wasm32")]
mod wasm_app {
    use plexspaces_proto::tuplespace::v1::{
        tuple_field::Value as ProtoTupleValue, ReadRequest, ReadResponse, Tuple,
        TupleField as ProtoTupleField, WriteRequest,
    };
    use prost::Message;
    use serde::{Deserialize, Serialize};
    use serde_json::Value;
    use std::sync::{Mutex, OnceLock};

    wit_bindgen::generate!({
        path: "../../../../wit/plexspaces-actor",
        world: "actor-world",
    });

    use exports::plexspaces::actor::actor::Guest;
    use plexspaces::actor::host;
    use plexspaces_sdk::simple_actor::ActorWorldHandlers;
    use plexspaces_sdk::{gen_server_actor, json, plexspaces_handlers};

    const DEFAULT_GROUP: &str = "abstractions-group";

    #[derive(Debug, Clone, Default, Serialize, Deserialize)]
    struct AbstractionsState {
        application_id: String,
        actor_id: String,
        role: String,
        count: i64,
        workflow_status: String,
        workflow_signals: Vec<String>,
        received: Vec<String>,
        timer_ticks: u64,
        reminder_ticks: u64,
        last_timer_id: String,
        last_reminder_id: String,
        joined_group: String,
        last_spawned_id: String,
    }

    fn state_cell() -> &'static Mutex<AbstractionsState> {
        static STATE: OnceLock<Mutex<AbstractionsState>> = OnceLock::new();
        STATE.get_or_init(|| Mutex::new(AbstractionsState::default()))
    }

    fn with_state<T>(f: impl FnOnce(&mut AbstractionsState) -> T) -> T {
        let mut guard = state_cell()
            .lock()
            .expect("abstractions state lock poisoned");
        f(&mut guard)
    }

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

    fn canonical_actor_target(target: &str) -> String {
        target.to_string()
    }

    fn actor_type_from_actor_id(actor_id: &str) -> Option<String> {
        actor_id
            .split_once("//")
            .and_then(|(_, suffix)| suffix.split_once('@').map(|(qualified, _)| qualified))
            .and_then(|qualified| qualified.split_once("::").map(|(actor_type, _)| actor_type))
            .map(str::to_string)
    }

    fn parse_op(msg_type: &str, payload: &[u8]) -> Result<String, String> {
        let payload = parse_payload(payload)?;
        if let Some(op) = payload
            .get("op")
            .and_then(|value| value.as_str())
            .map(str::to_string)
        {
            Ok(op)
        } else if msg_type == "call" || msg_type == "cast" {
            Err("missing op".to_string())
        } else {
            Ok(msg_type.to_string())
        }
    }

    fn parse_payload(payload: &[u8]) -> Result<Value, String> {
        if payload.is_empty() {
            return Ok(json!({}));
        }
        serde_json::from_slice(payload).map_err(|e| format!("invalid payload: {}", e))
    }

    fn host_result<T>(response: Result<T, String>, context: &str) -> Result<T, String> {
        response.map_err(|err| format!("{context}: {err}"))
    }

    fn json_bytes(value: Value) -> Vec<u8> {
        value.to_string().into_bytes()
    }

    fn json_error(err: impl Into<String>) -> Vec<u8> {
        json_bytes(json!({ "error": err.into() }))
    }

    fn json_string(bytes: Vec<u8>) -> String {
        String::from_utf8(bytes).unwrap_or_default()
    }

    fn json_value_to_proto_tuple_field(value: &Value, allow_wildcard_string: bool) -> Result<ProtoTupleField, String> {
        let field = match value {
            Value::Null => ProtoTupleField {
                value: Some(ProtoTupleValue::Wildcard(true)),
            },
            Value::String(text) if allow_wildcard_string && text == "*" => ProtoTupleField {
                value: Some(ProtoTupleValue::Wildcard(true)),
            },
            Value::Bool(boolean) => ProtoTupleField {
                value: Some(ProtoTupleValue::Boolean(*boolean)),
            },
            Value::Number(number) => {
                if let Some(integer) = number.as_i64() {
                    ProtoTupleField {
                        value: Some(ProtoTupleValue::Integer(integer)),
                    }
                } else if let Some(float) = number.as_f64() {
                    ProtoTupleField {
                        value: Some(ProtoTupleValue::Float(float)),
                    }
                } else {
                    return Err("unsupported numeric tuple field".to_string());
                }
            }
            Value::String(text) => ProtoTupleField {
                value: Some(ProtoTupleValue::String(text.clone())),
            },
            Value::Array(_) | Value::Object(_) => {
                return Err("tuplespace tuple fields must be scalar JSON values".to_string());
            }
        };
        Ok(field)
    }

    fn json_array_to_proto_tuple(values: &[Value], allow_wildcard_string: bool) -> Result<Tuple, String> {
        let mut fields = Vec::with_capacity(values.len());
        for value in values {
            fields.push(json_value_to_proto_tuple_field(value, allow_wildcard_string)?);
        }
        Ok(Tuple {
            id: String::new(),
            fields,
            timestamp: None,
            lease: None,
            metadata: Default::default(),
            location: None,
        })
    }

    fn proto_tuple_field_to_json(field: &ProtoTupleField) -> Value {
        match field.value.as_ref() {
            Some(ProtoTupleValue::Integer(value)) => json!(value),
            Some(ProtoTupleValue::Float(value)) => json!(value),
            Some(ProtoTupleValue::String(value)) => json!(value),
            Some(ProtoTupleValue::Boolean(value)) => json!(value),
            Some(ProtoTupleValue::Binary(value)) => json!(String::from_utf8_lossy(value)),
            Some(ProtoTupleValue::Null(_)) | Some(ProtoTupleValue::Wildcard(_)) | None => Value::Null,
        }
    }

    fn proto_tuple_to_json(tuple: &Tuple) -> Value {
        Value::Array(tuple.fields.iter().map(proto_tuple_field_to_json).collect())
    }

    fn encode_write_request(tuple_value: &Value) -> Result<Vec<u8>, String> {
        let values = tuple_value
            .as_array()
            .ok_or_else(|| "tuple must be a JSON array".to_string())?;
        let request = WriteRequest {
            tuples: vec![json_array_to_proto_tuple(values, false)?],
            transaction_id: String::new(),
        };
        Ok(request.encode_to_vec())
    }

    fn encode_read_request(pattern_value: &Value, take: bool, max_results: i32) -> Result<Vec<u8>, String> {
        let values = pattern_value
            .as_array()
            .ok_or_else(|| "pattern must be a JSON array".to_string())?;
        let request = ReadRequest {
            template: Some(json_array_to_proto_tuple(values, true)?),
            timeout: None,
            blocking: false,
            take,
            max_results,
            transaction_id: String::new(),
            spatial_filter: None,
        };
        Ok(request.encode_to_vec())
    }

    fn decode_read_response(bytes: &[u8]) -> Result<Vec<Value>, String> {
        let response = ReadResponse::decode(bytes)
            .map_err(|err| format!("failed to decode tuplespace response: {}", err))?;
        Ok(response.tuples.iter().map(proto_tuple_to_json).collect())
    }

    fn config_string(value: &serde_json::Value, key: &str) -> Option<String> {
        value
            .get(key)
            .and_then(|item| item.as_str())
            .map(str::to_string)
            .or_else(|| {
                value
                    .get("args")
                    .and_then(|args| args.get(key))
                    .and_then(|item| item.as_str())
                    .map(str::to_string)
            })
    }

    fn init_state(config: &[u8]) -> Result<(), String> {
        let value: Value = if config.is_empty() {
            json!({})
        } else {
            serde_json::from_slice(config).map_err(|e| format!("invalid init JSON: {}", e))?
        };
        let role = config_string(&value, "role").unwrap_or_else(|| "abstractions".to_string());
        let group = config_string(&value, "group").unwrap_or_else(|| DEFAULT_GROUP.to_string());
        let initial_count = value
            .get("initial_count")
            .and_then(|item| item.as_i64())
            .or_else(|| {
                value
                    .get("args")
                    .and_then(|args| args.get("initial_count"))
                    .and_then(|item| item.as_str())
                    .and_then(|item| item.parse::<i64>().ok())
            })
            .unwrap_or(0);
        let self_id = host::self_id();
        with_state(|state| {
            // Reset the guest-local state for every activation. Durable state, when present,
            // is restored afterward through the framework checkpoint path.
            *state = AbstractionsState::default();
            state.actor_id = self_id.clone();
            state.application_id = actor_application_id(&self_id);
            state.role = role.clone();
            state.count = initial_count;
            if role == "channel" && state.joined_group.is_empty() {
                state.joined_group = group.clone();
            }
        });
        if role == "channel" {
            host_result(host::pg_join(&group), "join group")?;
        }
        Ok(())
    }

    fn status_json() -> Vec<u8> {
        with_state(|state| {
            json_bytes(json!({
                "actor_id": state.actor_id,
                "application_id": state.application_id,
                "role": state.role,
                "count": state.count,
                "workflow_status": state.workflow_status,
                "workflow_signals": state.workflow_signals,
                "received": state.received,
                "timer_ticks": state.timer_ticks,
                "reminder_ticks": state.reminder_ticks,
                "joined_group": state.joined_group,
                "last_spawned_id": state.last_spawned_id,
                "self_id": host::self_id(),
            }))
        })
    }

    fn handle_increment(payload: &Value) -> Vec<u8> {
        let amount = payload
            .get("amount")
            .and_then(|value| value.as_i64())
            .unwrap_or(1);
        with_state(|state| {
            state.count += amount;
            json_bytes(json!({
                "actor_id": state.actor_id,
                "count": state.count,
            }))
        })
    }

    fn schedule_self_message(delay_ms: u64, msg_type: &str, payload: Value) -> Vec<u8> {
        let payload_bytes = payload.to_string().into_bytes();
        match host_result(
            host::send_after(delay_ms, msg_type, &payload_bytes),
            msg_type,
        ) {
            Ok(timer_id) => timer_id,
            Err(err) => return json_error(err),
        }
        .into_bytes()
    }

    fn handle_kv_put(payload: &Value) -> Vec<u8> {
        let key = payload
            .get("key")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let value = payload
            .get("value")
            .and_then(|item| item.as_str())
            .unwrap_or("");
        match host_result(host::kv_put(key, value.as_bytes()), "kv_put") {
            Ok(_) => json_bytes(json!({ "ok": true, "key": key, "value": value })),
            Err(err) => json_error(err),
        }
    }

    fn handle_kv_get(payload: &Value) -> Vec<u8> {
        let key = payload
            .get("key")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        match host_result(host::kv_get(key), "kv_get") {
            Ok(value) => json_bytes(json!({ "key": key, "value": json_string(value) })),
            Err(err) => json_error(err),
        }
    }

    fn handle_kv_list(payload: &Value) -> Vec<u8> {
        let prefix = payload
            .get("prefix")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        match host_result(host::kv_list(prefix), "kv_list") {
            Ok(keys) => json_bytes(json!({ "keys": keys })),
            Err(err) => json_error(err),
        }
    }

    fn handle_kv_delete(payload: &Value) -> Vec<u8> {
        let key = payload
            .get("key")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        match host_result(host::kv_delete(key), "kv_delete") {
            Ok(_) => json_bytes(json!({ "ok": true, "key": key })),
            Err(err) => json_error(err),
        }
    }

    fn handle_ts_write(payload: &Value) -> Vec<u8> {
        let tuple = payload.get("tuple").cloned().unwrap_or_else(|| json!([]));
        let tuple_bytes = match encode_write_request(&tuple) {
            Ok(bytes) => bytes,
            Err(err) => return json_error(format!("ts_write: {}", err)),
        };
        match host_result(host::ts_write(&tuple_bytes), "ts_write") {
            Ok(_) => json_bytes(json!({ "ok": true, "tuple": tuple })),
            Err(err) => json_error(err),
        }
    }

    fn tuplespace_result(response: Result<Vec<u8>, String>, field: &str, context: &str) -> Vec<u8> {
        match host_result(response, context) {
            Ok(value) => {
                let tuples = match decode_read_response(&value) {
                    Ok(tuples) => tuples,
                    Err(err) => return json_error(format!("{context}: {err}")),
                };
                let parsed = if field == "tuple" {
                    tuples.into_iter().next().unwrap_or(Value::Null)
                } else {
                    Value::Array(tuples)
                };
                let mut object = serde_json::Map::new();
                object.insert(field.to_string(), parsed);
                json_bytes(Value::Object(object))
            }
            Err(err) => json_error(err),
        }
    }

    fn handle_blob_upload(payload: &Value) -> Vec<u8> {
        let blob_id = payload
            .get("blob_id")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let data = payload
            .get("data")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let content_type = payload
            .get("content_type")
            .and_then(|value| value.as_str())
            .unwrap_or("text/plain");
        match host_result(
            host::blob_upload(blob_id, data.as_bytes(), content_type),
            "blob_upload",
        ) {
            Ok(stored_blob_id) => json_bytes(json!({ "ok": true, "blob_id": stored_blob_id })),
            Err(err) => json_error(err),
        }
    }

    fn handle_blob_download(payload: &Value) -> Vec<u8> {
        let blob_id = payload
            .get("blob_id")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        match host_result(host::blob_download(blob_id), "blob_download") {
            Ok(data) => json_bytes(json!({ "blob_id": blob_id, "data": json_string(data) })),
            Err(err) => json_error(err),
        }
    }

    fn handle_blob_list(payload: &Value) -> Vec<u8> {
        let prefix = payload
            .get("prefix")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        match host_result(host::blob_list(prefix), "blob_list") {
            Ok(ids) => json_bytes(json!({ "blob_ids": ids })),
            Err(err) => json_error(err),
        }
    }

    fn handle_blob_delete(payload: &Value) -> Vec<u8> {
        let blob_id = payload
            .get("blob_id")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        match host_result(host::blob_delete(blob_id), "blob_delete") {
            Ok(_) => json_bytes(json!({ "ok": true, "blob_id": blob_id })),
            Err(err) => json_error(err),
        }
    }

    fn handle_send_event(payload: &Value) -> Vec<u8> {
        let target = payload
            .get("target")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let resolved_target = canonical_actor_target(target);
        let body = payload
            .get("body")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let channel = payload
            .get("channel")
            .and_then(|value| value.as_str())
            .unwrap_or("alerts");
        let event = json!({
            "op": "publish",
            "channel": channel,
            "body": body,
        });
        let event_bytes = event.to_string().into_bytes();
        match host_result(host::send(&resolved_target, "cast", &event_bytes), "send") {
            Ok(_) => json_bytes(json!({ "ok": true, "target": resolved_target })),
            Err(err) => json_error(err),
        }
    }

    fn handle_broadcast_event(payload: &Value) -> Vec<u8> {
        let group = payload
            .get("group")
            .and_then(|value| value.as_str())
            .unwrap_or(DEFAULT_GROUP);
        let body = payload
            .get("body")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let channel = payload
            .get("channel")
            .and_then(|value| value.as_str())
            .unwrap_or("alerts");
        let event = json!({
            "op": "publish",
            "channel": channel,
            "body": body,
        });
        let event_bytes = event.to_string().into_bytes();
        match host_result(
            host::pg_broadcast(group, "cast", &event_bytes),
            "pg_broadcast",
        ) {
            Ok(_) => json_bytes(json!({ "ok": true, "group": group })),
            Err(err) => json_error(err),
        }
    }

    fn handle_join_group(payload: &Value) -> Vec<u8> {
        let group = payload
            .get("group")
            .and_then(|value| value.as_str())
            .unwrap_or(DEFAULT_GROUP);
        match host_result(host::pg_join(group), "pg_join") {
            Ok(_) => {
                with_state(|state| {
                    state.joined_group = group.to_string();
                });
                json_bytes(json!({ "ok": true, "group": group }))
            }
            Err(err) => json_error(err),
        }
    }

    fn handle_group_members(payload: &Value) -> Vec<u8> {
        let group = payload
            .get("group")
            .and_then(|value| value.as_str())
            .unwrap_or(DEFAULT_GROUP);
        match host_result(host::pg_members(group), "pg_members") {
            Ok(members) => json_bytes(json!({ "members": members })),
            Err(err) => json_error(err),
        }
    }

    fn handle_channel_publish(payload: &Value) -> Vec<u8> {
        let channel = payload
            .get("channel")
            .and_then(|value| value.as_str())
            .unwrap_or("alerts");
        let body = payload
            .get("body")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        with_state(|state| {
            state.received.push(format!("{channel}:{body}"));
        });
        json_bytes(json!({}))
    }

    fn handle_workflow_run(payload: &Value) -> Vec<u8> {
        let order_id = payload
            .get("order_id")
            .and_then(|value| value.as_str())
            .unwrap_or("order");
        with_state(|state| {
            state.workflow_status = format!("running:{order_id}");
            state.workflow_signals.clear();
            json_bytes(json!({ "status": state.workflow_status }))
        })
    }

    fn handle_workflow_signal(payload: &Value) -> Vec<u8> {
        let reason = payload
            .get("reason")
            .and_then(|value| value.as_str())
            .unwrap_or("user");
        with_state(|state| {
            state.workflow_signals.push(reason.to_string());
            state.workflow_status = "cancelled".to_string();
        });
        json_bytes(json!({}))
    }

    fn handle_workflow_query() -> Vec<u8> {
        with_state(|state| {
            json_bytes(json!({
                "status": state.workflow_status,
                "signals": state.workflow_signals,
            }))
        })
    }

    fn handle_stop_target(payload: &Value) -> Vec<u8> {
        let actor_id = payload
            .get("actor_id")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        match host_result(host::stop(actor_id), "stop") {
            Ok(_) => json_bytes(json!({ "ok": true, "actor_id": actor_id })),
            Err(err) => json_error(err),
        }
    }

    fn handle_spawn_actor(payload: &Value) -> Vec<u8> {
        let module_ref = payload
            .get("module_ref")
            .and_then(|value| value.as_str())
            .map(str::to_string)
            .filter(|value| !value.is_empty())
            .or_else(|| with_state(|state| actor_type_from_actor_id(&state.actor_id)))
            .unwrap_or_else(|| "abstractions_wasm".to_string());
        let actor_id = payload
            .get("actor_id")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let init_config = payload.get("config").cloned().unwrap_or_else(|| json!({}));
        let init_bytes = init_config.to_string().into_bytes();
        match host_result(host::spawn(&module_ref, actor_id, &init_bytes), "spawn") {
            Ok(spawned_id) => {
                with_state(|state| {
                    state.last_spawned_id = spawned_id.clone();
                });
                json_bytes(json!({ "actor_id": spawned_id }))
            }
            Err(err) => json_error(err),
        }
    }

    #[gen_server_actor(wasm)]
    #[derive(Default)]
    struct AbstractionsWasmActor;

    #[plexspaces_handlers(wasm)]
    impl AbstractionsWasmActor {
        #[init_handler]
        fn configure(&mut self, config: &[u8]) -> Result<(), String> {
            init_state(config)
        }

        #[handler("increment")]
        fn increment(&mut self, _from_actor: &str, payload: &[u8]) -> Result<Vec<u8>, String> {
            let payload = parse_payload(payload)?;
            Ok(handle_increment(&payload))
        }

        #[handler("status")]
        fn status(&mut self, _from_actor: &str, _payload: &[u8]) -> Result<Vec<u8>, String> {
            Ok(status_json())
        }

        #[handler("schedule_timer")]
        fn schedule_timer(&mut self, _from_actor: &str, payload: &[u8]) -> Result<Vec<u8>, String> {
            let payload = parse_payload(payload)?;
            let delay_ms = payload
                .get("delay_ms")
                .and_then(|value| value.as_u64())
                .unwrap_or(100);
            let timer_id =
                schedule_self_message(delay_ms, "timer_tick", json!({ "kind": "timer" }));
            if timer_id.first() == Some(&b'{') {
                return Ok(timer_id);
            }
            with_state(|state| {
                state.last_timer_id = String::from_utf8(timer_id.clone()).unwrap_or_default();
            });
            Ok(json_bytes(
                json!({ "timer_id": String::from_utf8(timer_id).unwrap_or_default() }),
            ))
        }

        #[handler("schedule_reminder")]
        fn schedule_reminder(
            &mut self,
            _from_actor: &str,
            payload: &[u8],
        ) -> Result<Vec<u8>, String> {
            let payload = parse_payload(payload)?;
            let delay_ms = payload
                .get("delay_ms")
                .and_then(|value| value.as_u64())
                .unwrap_or(120);
            let timer_id =
                schedule_self_message(delay_ms, "reminder_tick", json!({ "kind": "reminder" }));
            if timer_id.first() == Some(&b'{') {
                return Ok(timer_id);
            }
            with_state(|state| {
                state.last_reminder_id = String::from_utf8(timer_id.clone()).unwrap_or_default();
            });
            Ok(json_bytes(
                json!({ "reminder_id": String::from_utf8(timer_id).unwrap_or_default() }),
            ))
        }

        #[handler("timer_tick")]
        fn timer_tick(&mut self, _from_actor: &str, _payload: &[u8]) -> Result<Vec<u8>, String> {
            Ok(with_state(|state| {
                state.timer_ticks += 1;
                json_bytes(json!({ "timer_ticks": state.timer_ticks }))
            }))
        }

        #[handler("reminder_tick")]
        fn reminder_tick(&mut self, _from_actor: &str, _payload: &[u8]) -> Result<Vec<u8>, String> {
            Ok(with_state(|state| {
                state.reminder_ticks += 1;
                json_bytes(json!({ "reminder_ticks": state.reminder_ticks }))
            }))
        }

        #[handler("kv_put")]
        fn kv_put(&mut self, _from_actor: &str, payload: &[u8]) -> Result<Vec<u8>, String> {
            let payload = parse_payload(payload)?;
            Ok(handle_kv_put(&payload))
        }

        #[handler("kv_get")]
        fn kv_get(&mut self, _from_actor: &str, payload: &[u8]) -> Result<Vec<u8>, String> {
            let payload = parse_payload(payload)?;
            Ok(handle_kv_get(&payload))
        }

        #[handler("kv_list")]
        fn kv_list(&mut self, _from_actor: &str, payload: &[u8]) -> Result<Vec<u8>, String> {
            let payload = parse_payload(payload)?;
            Ok(handle_kv_list(&payload))
        }

        #[handler("kv_delete")]
        fn kv_delete(&mut self, _from_actor: &str, payload: &[u8]) -> Result<Vec<u8>, String> {
            let payload = parse_payload(payload)?;
            Ok(handle_kv_delete(&payload))
        }

        #[handler("ts_write")]
        fn ts_write(&mut self, _from_actor: &str, payload: &[u8]) -> Result<Vec<u8>, String> {
            let payload = parse_payload(payload)?;
            Ok(handle_ts_write(&payload))
        }

        #[handler("ts_read")]
        fn ts_read(&mut self, _from_actor: &str, payload: &[u8]) -> Result<Vec<u8>, String> {
            let payload = parse_payload(payload)?;
            let pattern = payload.get("pattern").cloned().unwrap_or_else(|| json!([]));
            let pattern_bytes = encode_read_request(&pattern, false, 1)?;
            Ok(tuplespace_result(
                host::ts_read(&pattern_bytes),
                "tuple",
                "ts_read",
            ))
        }

        #[handler("ts_take")]
        fn ts_take(&mut self, _from_actor: &str, payload: &[u8]) -> Result<Vec<u8>, String> {
            let payload = parse_payload(payload)?;
            let pattern = payload.get("pattern").cloned().unwrap_or_else(|| json!([]));
            let pattern_bytes = encode_read_request(&pattern, true, 1)?;
            Ok(tuplespace_result(
                host::ts_take(&pattern_bytes),
                "tuple",
                "ts_take",
            ))
        }

        #[handler("ts_read_all")]
        fn ts_read_all(&mut self, _from_actor: &str, payload: &[u8]) -> Result<Vec<u8>, String> {
            let payload = parse_payload(payload)?;
            let pattern = payload.get("pattern").cloned().unwrap_or_else(|| json!([]));
            let pattern_bytes = encode_read_request(&pattern, false, 1024)?;
            Ok(tuplespace_result(
                host::ts_read_all(&pattern_bytes),
                "tuples",
                "ts_read_all",
            ))
        }

        #[handler("blob_upload")]
        fn blob_upload(&mut self, _from_actor: &str, payload: &[u8]) -> Result<Vec<u8>, String> {
            let payload = parse_payload(payload)?;
            Ok(handle_blob_upload(&payload))
        }

        #[handler("blob_download")]
        fn blob_download(&mut self, _from_actor: &str, payload: &[u8]) -> Result<Vec<u8>, String> {
            let payload = parse_payload(payload)?;
            Ok(handle_blob_download(&payload))
        }

        #[handler("blob_list")]
        fn blob_list(&mut self, _from_actor: &str, payload: &[u8]) -> Result<Vec<u8>, String> {
            let payload = parse_payload(payload)?;
            Ok(handle_blob_list(&payload))
        }

        #[handler("blob_delete")]
        fn blob_delete(&mut self, _from_actor: &str, payload: &[u8]) -> Result<Vec<u8>, String> {
            let payload = parse_payload(payload)?;
            Ok(handle_blob_delete(&payload))
        }

        #[handler("send_event")]
        fn send_event(&mut self, _from_actor: &str, payload: &[u8]) -> Result<Vec<u8>, String> {
            let payload = parse_payload(payload)?;
            Ok(handle_send_event(&payload))
        }

        #[handler("broadcast_event")]
        fn broadcast_event(
            &mut self,
            _from_actor: &str,
            payload: &[u8],
        ) -> Result<Vec<u8>, String> {
            let payload = parse_payload(payload)?;
            Ok(handle_broadcast_event(&payload))
        }

        #[handler("join_group")]
        fn join_group(&mut self, _from_actor: &str, payload: &[u8]) -> Result<Vec<u8>, String> {
            let payload = parse_payload(payload)?;
            Ok(handle_join_group(&payload))
        }

        #[handler("group_members")]
        fn group_members(&mut self, _from_actor: &str, payload: &[u8]) -> Result<Vec<u8>, String> {
            let payload = parse_payload(payload)?;
            Ok(handle_group_members(&payload))
        }

        #[handler("publish")]
        fn publish(&mut self, _from_actor: &str, payload: &[u8]) -> Result<Vec<u8>, String> {
            let payload = parse_payload(payload)?;
            Ok(handle_channel_publish(&payload))
        }

        #[handler("workflow_run")]
        fn workflow_run(&mut self, _from_actor: &str, payload: &[u8]) -> Result<Vec<u8>, String> {
            let payload = parse_payload(payload)?;
            Ok(handle_workflow_run(&payload))
        }

        #[handler("workflow_signal:cancel")]
        fn workflow_signal_cancel(
            &mut self,
            _from_actor: &str,
            payload: &[u8],
        ) -> Result<Vec<u8>, String> {
            let payload = parse_payload(payload)?;
            Ok(handle_workflow_signal(&payload))
        }

        #[handler("workflow_query:status")]
        fn workflow_query_status(
            &mut self,
            _from_actor: &str,
            _payload: &[u8],
        ) -> Result<Vec<u8>, String> {
            Ok(handle_workflow_query())
        }

        #[handler("stop_actor")]
        fn stop_actor(&mut self, _from_actor: &str, payload: &[u8]) -> Result<Vec<u8>, String> {
            let payload = parse_payload(payload)?;
            Ok(handle_stop_target(&payload))
        }

        #[handler("spawn_actor")]
        fn spawn_actor(&mut self, _from_actor: &str, payload: &[u8]) -> Result<Vec<u8>, String> {
            let payload = parse_payload(payload)?;
            Ok(handle_spawn_actor(&payload))
        }
    }

    struct AbstractionsBridge;

    impl Guest for AbstractionsBridge {
        fn init(config: Vec<u8>) -> Result<(), String> {
            let mut actor = AbstractionsWasmActor::default();
            ActorWorldHandlers::init(&mut actor, &config)
        }

        fn handle(
            from_actor: String,
            msg_type: String,
            payload: Vec<u8>,
        ) -> Result<Vec<u8>, String> {
            let op = match parse_op(&msg_type, &payload) {
                Ok(op) => op,
                Err(err) => return Ok(json_error(err)),
            };
            let mut actor = AbstractionsWasmActor::default();
            Ok(actor
                .handle_operation(&from_actor, &op, &payload)
                .unwrap_or_else(json_error))
        }

        fn get_state() -> Result<Vec<u8>, String> {
            with_state(|state| {
                serde_json::to_vec(state).map_err(|err| format!("state encode failed: {err}"))
            })
        }

        fn set_state(state: Vec<u8>) -> Result<(), String> {
            if state.is_empty() {
                return Ok(());
            }
            match serde_json::from_slice::<AbstractionsState>(&state) {
                Ok(next) => {
                    let mut guard = state_cell().lock().expect("set_state lock");
                    *guard = next;
                    Ok(())
                }
                Err(err) => Err(format!("invalid state JSON: {}", err)),
            }
        }
    }

    export!(AbstractionsBridge);
}

#[cfg(not(target_arch = "wasm32"))]
use plexspaces_sdk::{
    create_facets_with_storage, event_actor, gen_server_actor, handler, json, new_message,
    plexspaces_handlers, query_handler, run_handler, signal_handler, workflow_actor, Actor,
    ActorContext, BehaviorError, DeclaredFacets, Message, Value,
};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;

#[cfg(not(target_arch = "wasm32"))]
#[gen_server_actor(facets = ["virtual_actor", "durability", "timer", "reminder"])]
pub struct AbstractionsActor {
    pub actor_id: String,
    pub count: i64,
}

#[cfg(not(target_arch = "wasm32"))]
#[gen_server_actor(facets = ["virtual_actor"])]
pub struct EphemeralActor {
    pub count: i64,
}

#[cfg(not(target_arch = "wasm32"))]
#[plexspaces_handlers]
impl AbstractionsActor {
    #[handler("increment")]
    async fn increment(
        &mut self,
        _ctx: &ActorContext,
        _msg: &Message,
    ) -> Result<Value, BehaviorError> {
        self.count += 1;
        Ok(json!({ "count": self.count }))
    }

    #[handler("status")]
    async fn status(
        &mut self,
        _ctx: &ActorContext,
        _msg: &Message,
    ) -> Result<Value, BehaviorError> {
        Ok(json!({ "actor_id": self.actor_id, "count": self.count }))
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[plexspaces_handlers]
impl EphemeralActor {
    #[handler("increment")]
    async fn increment(
        &mut self,
        _ctx: &ActorContext,
        _msg: &Message,
    ) -> Result<Value, BehaviorError> {
        self.count += 1;
        Ok(json!({ "count": self.count }))
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[workflow_actor(facets = ["virtual_actor", "durability"])]
pub struct AbstractionsWorkflow {
    pub status: String,
    pub signals: Vec<String>,
}

#[cfg(not(target_arch = "wasm32"))]
#[event_actor(facets = ["process_group"])]
pub struct AbstractionsChannel {
    pub received: Vec<String>,
}

#[cfg(not(target_arch = "wasm32"))]
#[plexspaces_handlers(workflow)]
impl AbstractionsWorkflow {
    #[run_handler]
    async fn run(
        &mut self,
        _ctx: &ActorContext,
        _input: Message,
    ) -> Result<Message, BehaviorError> {
        self.status = "running:o-1".to_string();
        Ok(Message {
            payload: serde_json::to_vec(&json!({ "status": self.status })).unwrap(),
            ..Default::default()
        })
    }

    #[signal_handler("cancel")]
    async fn cancel(&mut self, _ctx: &ActorContext, _input: Message) -> Result<(), BehaviorError> {
        self.signals.push("user".to_string());
        self.status = "cancelled".to_string();
        Ok(())
    }

    #[query_handler("status")]
    async fn status(&self, _ctx: &ActorContext, _input: Message) -> Result<Message, BehaviorError> {
        Ok(Message {
            payload: serde_json::to_vec(&json!({
                "status": self.status,
                "signals": self.signals
            }))
            .unwrap(),
            ..Default::default()
        })
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[plexspaces_handlers(event)]
impl AbstractionsChannel {
    #[handler("publish", cast)]
    async fn publish(&mut self, _ctx: &ActorContext, msg: &Message) -> Result<(), BehaviorError> {
        let payload: serde_json::Value =
            serde_json::from_slice(&msg.payload).expect("channel payload");
        let channel = payload
            .get("channel")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        let body = payload
            .get("body")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        self.received.push(format!("{channel}:{body}"));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plexspaces_actor::TestServiceLocatorStub;
    use plexspaces_core::ServiceLocator;
    use plexspaces_journaling::SqliteJournalStorage;

    #[test]
    fn actor_and_workflow_declare_aligned_facets() {
        assert_eq!(
            AbstractionsActor::declared_facets(),
            &["virtual_actor", "durability", "timer", "reminder"]
        );
        assert_eq!(EphemeralActor::declared_facets(), &["virtual_actor"]);
        assert_eq!(AbstractionsChannel::declared_facets(), &["process_group"]);
        assert_eq!(
            AbstractionsWorkflow::declared_facets(),
            &["virtual_actor", "durability"]
        );
    }

    #[test]
    fn workflow_uses_workflow_behavior() {
        let workflow = AbstractionsWorkflow {
            status: "pending".to_string(),
            signals: vec![],
        };
        assert_eq!(
            workflow.behavior_type(),
            plexspaces_core::BehaviorType::Workflow
        );
    }

    #[tokio::test]
    async fn facets_cover_virtual_actor_timer_reminder_and_durability() {
        let service_locator: Arc<dyn ServiceLocator> = Arc::new(TestServiceLocatorStub::new());
        let storage = Arc::new(
            SqliteJournalStorage::new(":memory:")
                .await
                .expect("sqlite journal storage"),
        );

        let facets = create_facets_with_storage(
            AbstractionsActor::declared_facets(),
            &json!({
                "timer": { "interval_ms": 500 },
                "reminder": { "tick_interval_ms": 2500 },
                "durability": { "checkpoint_interval": 5 }
            }),
            Some(storage),
            service_locator,
        )
        .expect("facets");

        let facet_types: Vec<&str> = facets.iter().map(|facet| facet.facet_type()).collect();
        assert!(facet_types.contains(&"timer"));
        assert!(facet_types.contains(&"reminder"));
        assert!(facet_types.contains(&"durability"));
        assert!(facet_types.contains(&"virtual_actor"));
    }

    #[tokio::test]
    async fn non_durable_virtual_actor_facets_do_not_include_durability() {
        let service_locator: Arc<dyn ServiceLocator> = Arc::new(TestServiceLocatorStub::new());
        let storage = Arc::new(
            SqliteJournalStorage::new(":memory:")
                .await
                .expect("sqlite journal storage"),
        );

        let facets = create_facets_with_storage(
            EphemeralActor::declared_facets(),
            &json!({}),
            Some(storage),
            service_locator,
        )
        .expect("facets");

        let facet_types: Vec<&str> = facets.iter().map(|facet| facet.facet_type()).collect();
        assert_eq!(facet_types, vec!["virtual_actor"]);
    }

    #[tokio::test]
    async fn event_actor_models_channel_delivery() {
        let service_locator: Arc<dyn ServiceLocator> = Arc::new(TestServiceLocatorStub::new());
        let ctx = ActorContext::new(
            "test-node".to_string(),
            String::new(),
            "default".to_string(),
            service_locator,
            None,
        );
        let mut channel = AbstractionsChannel { received: vec![] };
        let message = new_message(
            "cast",
            json!({
                "op": "publish",
                "channel": "alerts",
                "body": "hello"
            }),
        );

        channel
            .handle_message(&ctx, message)
            .await
            .expect("publish event");
        assert_eq!(channel.received, vec!["alerts:hello".to_string()]);
    }
}
