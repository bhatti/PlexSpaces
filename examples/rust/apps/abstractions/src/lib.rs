// SPDX-License-Identifier: LGPL-2.1-or-later

#[cfg(target_arch = "wasm32")]
mod wasm_app {
    use serde::{Deserialize, Serialize};
    use std::sync::{Mutex, OnceLock};

    wit_bindgen::generate!({
        path: "../../../../wit/plexspaces-simple-actor",
        world: "actor-world",
    });

    use exports::plexspaces::simple_actor::actor::Guest;
    use plexspaces::simple_actor::host;
    use plexspaces_sdk::simple_actor::SimpleActorHandlers;
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
        if target.contains('@') {
            return target.to_string();
        }
        if let Some((actor_type, actor_name)) = target.split_once(':') {
            let self_id = host::self_id();
            let namespace = actor_application_id(&self_id);
            let node_id = self_id
                .rsplit_once('@')
                .map(|(_, node_id)| node_id.to_string())
                .unwrap_or_default();
            if !actor_name.is_empty() && !actor_type.is_empty() && !namespace.is_empty() && !node_id.is_empty() {
                return format!("{actor_name}//{actor_type}::{namespace}@{node_id}");
            }
        }
        target.to_string()
    }

    fn parse_op(msg_type: &str, payload_json: &str) -> Result<String, String> {
        let payload: serde_json::Value =
            serde_json::from_str(payload_json).map_err(|e| format!("invalid payload: {}", e))?;
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

    fn parse_payload(payload_json: &str) -> Result<serde_json::Value, String> {
        serde_json::from_str(payload_json).map_err(|e| format!("invalid payload: {}", e))
    }

    fn host_ok(response: String, context: &str) -> Result<String, String> {
        if response.starts_with("ERROR:") {
            Err(format!("{}: {}", context, response))
        } else {
            Ok(response)
        }
    }

    fn config_string(value: &serde_json::Value, key: &str) -> Option<String> {
        value.get(key)
            .and_then(|item| item.as_str())
            .map(str::to_string)
            .or_else(|| {
                value.get("args")
                    .and_then(|args| args.get(key))
                    .and_then(|item| item.as_str())
                    .map(str::to_string)
            })
    }

    fn init_state(config_json: &str) -> Result<(), String> {
        let value: serde_json::Value =
            serde_json::from_str(config_json).map_err(|e| format!("invalid init JSON: {}", e))?;
        let role = config_string(&value, "role").unwrap_or_else(|| "abstractions".to_string());
        let group =
            config_string(&value, "group").unwrap_or_else(|| DEFAULT_GROUP.to_string());
        let initial_count = value
            .get("initial_count")
            .and_then(|item| item.as_i64())
            .or_else(|| {
                value.get("args")
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
            host_ok(host::pg_join(&group), "join group")?;
        }
        Ok(())
    }

    fn status_json() -> String {
        with_state(|state| {
            json!({
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
            })
            .to_string()
        })
    }

    fn handle_increment(payload: &serde_json::Value) -> String {
        let amount = payload.get("amount").and_then(|value| value.as_i64()).unwrap_or(1);
        with_state(|state| {
            state.count += amount;
            json!({
                "actor_id": state.actor_id,
                "count": state.count,
            })
            .to_string()
        })
    }

    fn schedule_self_message(delay_ms: u64, msg_type: &str, payload: serde_json::Value) -> String {
        match host_ok(
            host::send_after(delay_ms, msg_type, &payload.to_string()),
            msg_type,
        ) {
            Ok(timer_id) => timer_id,
            Err(err) => json!({ "error": err }).to_string(),
        }
    }

    fn handle_kv_put(payload: &serde_json::Value) -> String {
        let key = payload.get("key").and_then(|value| value.as_str()).unwrap_or("");
        let value = payload
            .get("value")
            .and_then(|item| item.as_str())
            .unwrap_or("");
        match host_ok(host::kv_put(key, value), "kv_put") {
            Ok(_) => json!({ "ok": true, "key": key, "value": value }).to_string(),
            Err(err) => json!({ "error": err }).to_string(),
        }
    }

    fn handle_kv_get(payload: &serde_json::Value) -> String {
        let key = payload.get("key").and_then(|value| value.as_str()).unwrap_or("");
        match host_ok(host::kv_get(key), "kv_get") {
            Ok(value) => json!({ "key": key, "value": value }).to_string(),
            Err(err) => json!({ "error": err }).to_string(),
        }
    }

    fn handle_kv_list(payload: &serde_json::Value) -> String {
        let prefix = payload
            .get("prefix")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        match host_ok(host::kv_list(prefix), "kv_list") {
            Ok(keys_json) => json!({ "keys": serde_json::from_str::<serde_json::Value>(&keys_json).unwrap_or(json!([])) }).to_string(),
            Err(err) => json!({ "error": err }).to_string(),
        }
    }

    fn handle_kv_delete(payload: &serde_json::Value) -> String {
        let key = payload.get("key").and_then(|value| value.as_str()).unwrap_or("");
        match host_ok(host::kv_delete(key), "kv_delete") {
            Ok(_) => json!({ "ok": true, "key": key }).to_string(),
            Err(err) => json!({ "error": err }).to_string(),
        }
    }

    fn handle_ts_write(payload: &serde_json::Value) -> String {
        let tuple = payload.get("tuple").cloned().unwrap_or_else(|| json!([]));
        match host_ok(host::ts_write(&tuple.to_string()), "ts_write") {
            Ok(_) => json!({ "ok": true, "tuple": tuple }).to_string(),
            Err(err) => json!({ "error": err }).to_string(),
        }
    }

    fn tuplespace_result(response: String, field: &str, context: &str) -> String {
        match host_ok(response, context) {
            Ok(value) => {
                let parsed = if value.is_empty() {
                    json!(null)
                } else {
                    serde_json::from_str::<serde_json::Value>(&value).unwrap_or_else(|_| json!(value))
                };
                let mut object = serde_json::Map::new();
                object.insert(field.to_string(), parsed);
                serde_json::Value::Object(object).to_string()
            }
            Err(err) => json!({ "error": err }).to_string(),
        }
    }

    fn handle_blob_upload(payload: &serde_json::Value) -> String {
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
        match host_ok(
            host::blob_upload(blob_id, data, content_type),
            "blob_upload",
        ) {
            Ok(_) => json!({ "ok": true, "blob_id": blob_id }).to_string(),
            Err(err) => json!({ "error": err }).to_string(),
        }
    }

    fn handle_blob_download(payload: &serde_json::Value) -> String {
        let blob_id = payload
            .get("blob_id")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        match host_ok(host::blob_download(blob_id), "blob_download") {
            Ok(data) => json!({ "blob_id": blob_id, "data": data }).to_string(),
            Err(err) => json!({ "error": err }).to_string(),
        }
    }

    fn handle_blob_list(payload: &serde_json::Value) -> String {
        let prefix = payload
            .get("prefix")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        match host_ok(host::blob_list(prefix), "blob_list") {
            Ok(ids_json) => json!({ "blob_ids": serde_json::from_str::<serde_json::Value>(&ids_json).unwrap_or(json!([])) }).to_string(),
            Err(err) => json!({ "error": err }).to_string(),
        }
    }

    fn handle_blob_delete(payload: &serde_json::Value) -> String {
        let blob_id = payload
            .get("blob_id")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        match host_ok(host::blob_delete(blob_id), "blob_delete") {
            Ok(_) => json!({ "ok": true, "blob_id": blob_id }).to_string(),
            Err(err) => json!({ "error": err }).to_string(),
        }
    }

    fn handle_send_event(payload: &serde_json::Value) -> String {
        let target = payload
            .get("target")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let resolved_target = canonical_actor_target(target);
        let body = payload.get("body").and_then(|value| value.as_str()).unwrap_or("");
        let channel = payload
            .get("channel")
            .and_then(|value| value.as_str())
            .unwrap_or("alerts");
        let event = json!({
            "op": "publish",
            "channel": channel,
            "body": body,
        });
        match host_ok(host::send(&resolved_target, "cast", &event.to_string()), "send") {
            Ok(_) => json!({ "ok": true, "target": resolved_target }).to_string(),
            Err(err) => json!({ "error": err }).to_string(),
        }
    }

    fn handle_broadcast_event(payload: &serde_json::Value) -> String {
        let group = payload
            .get("group")
            .and_then(|value| value.as_str())
            .unwrap_or(DEFAULT_GROUP);
        let body = payload.get("body").and_then(|value| value.as_str()).unwrap_or("");
        let channel = payload
            .get("channel")
            .and_then(|value| value.as_str())
            .unwrap_or("alerts");
        let event = json!({
            "op": "publish",
            "channel": channel,
            "body": body,
        });
        match host_ok(
            host::pg_broadcast(group, "cast", &event.to_string()),
            "pg_broadcast",
        ) {
            Ok(_) => json!({ "ok": true, "group": group }).to_string(),
            Err(err) => json!({ "error": err }).to_string(),
        }
    }

    fn handle_join_group(payload: &serde_json::Value) -> String {
        let group = payload
            .get("group")
            .and_then(|value| value.as_str())
            .unwrap_or(DEFAULT_GROUP);
        match host_ok(host::pg_join(group), "pg_join") {
            Ok(_) => {
                with_state(|state| {
                    state.joined_group = group.to_string();
                });
                json!({ "ok": true, "group": group }).to_string()
            }
            Err(err) => json!({ "error": err }).to_string(),
        }
    }

    fn handle_group_members(payload: &serde_json::Value) -> String {
        let group = payload
            .get("group")
            .and_then(|value| value.as_str())
            .unwrap_or(DEFAULT_GROUP);
        match host_ok(host::pg_members(group), "pg_members") {
            Ok(members_json) => json!({ "members": serde_json::from_str::<serde_json::Value>(&members_json).unwrap_or(json!([])) }).to_string(),
            Err(err) => json!({ "error": err }).to_string(),
        }
    }

    fn handle_channel_publish(payload: &serde_json::Value) -> String {
        let channel = payload
            .get("channel")
            .and_then(|value| value.as_str())
            .unwrap_or("alerts");
        let body = payload.get("body").and_then(|value| value.as_str()).unwrap_or("");
        with_state(|state| {
            state.received.push(format!("{channel}:{body}"));
        });
        json!({}).to_string()
    }

    fn handle_workflow_run(payload: &serde_json::Value) -> String {
        let order_id = payload
            .get("order_id")
            .and_then(|value| value.as_str())
            .unwrap_or("order");
        with_state(|state| {
            state.workflow_status = format!("running:{order_id}");
            state.workflow_signals.clear();
            json!({ "status": state.workflow_status }).to_string()
        })
    }

    fn handle_workflow_signal(payload: &serde_json::Value) -> String {
        let reason = payload
            .get("reason")
            .and_then(|value| value.as_str())
            .unwrap_or("user");
        with_state(|state| {
            state.workflow_signals.push(reason.to_string());
            state.workflow_status = "cancelled".to_string();
        });
        json!({}).to_string()
    }

    fn handle_workflow_query() -> String {
        with_state(|state| {
            json!({
                "status": state.workflow_status,
                "signals": state.workflow_signals,
            })
            .to_string()
        })
    }

    fn handle_stop_target(payload: &serde_json::Value) -> String {
        let actor_id = payload
            .get("actor_id")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        match host_ok(host::stop(actor_id), "stop") {
            Ok(_) => json!({ "ok": true, "actor_id": actor_id }).to_string(),
            Err(err) => json!({ "error": err }).to_string(),
        }
    }

    fn handle_spawn_actor(payload: &serde_json::Value) -> String {
        let module_ref = payload
            .get("module_ref")
            .and_then(|value| value.as_str())
            .unwrap_or("abstractions");
        let actor_id = payload
            .get("actor_id")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let init_config = payload.get("config").cloned().unwrap_or_else(|| json!({}));
        match host_ok(
            host::spawn(module_ref, actor_id, &init_config.to_string()),
            "spawn",
        ) {
            Ok(spawned_id) => {
                with_state(|state| {
                    state.last_spawned_id = spawned_id.clone();
                });
                json!({ "actor_id": spawned_id }).to_string()
            }
            Err(err) => json!({ "error": err }).to_string(),
        }
    }

    #[gen_server_actor(wasm)]
    #[derive(Default)]
    struct AbstractionsWasmActor;

    #[plexspaces_handlers(wasm)]
    impl AbstractionsWasmActor {
        #[init_handler]
        fn configure(&mut self, config_json: &str) -> Result<(), String> {
            init_state(config_json)
        }

        #[handler("increment")]
        fn increment(&mut self, _from_actor: &str, payload_json: &str) -> Result<String, String> {
            let payload = parse_payload(payload_json)?;
            Ok(handle_increment(&payload))
        }

        #[handler("status")]
        fn status(&mut self, _from_actor: &str, _payload_json: &str) -> Result<String, String> {
            Ok(status_json())
        }

        #[handler("schedule_timer")]
        fn schedule_timer(
            &mut self,
            _from_actor: &str,
            payload_json: &str,
        ) -> Result<String, String> {
            let payload = parse_payload(payload_json)?;
            let delay_ms = payload
                .get("delay_ms")
                .and_then(|value| value.as_u64())
                .unwrap_or(100);
            let timer_id = schedule_self_message(delay_ms, "timer_tick", json!({ "kind": "timer" }));
            if timer_id.starts_with('{') {
                return Ok(timer_id);
            }
            with_state(|state| {
                state.last_timer_id = timer_id.clone();
            });
            Ok(json!({ "timer_id": timer_id }).to_string())
        }

        #[handler("schedule_reminder")]
        fn schedule_reminder(
            &mut self,
            _from_actor: &str,
            payload_json: &str,
        ) -> Result<String, String> {
            let payload = parse_payload(payload_json)?;
            let delay_ms = payload
                .get("delay_ms")
                .and_then(|value| value.as_u64())
                .unwrap_or(120);
            let timer_id = schedule_self_message(
                delay_ms,
                "reminder_tick",
                json!({ "kind": "reminder" }),
            );
            if timer_id.starts_with('{') {
                return Ok(timer_id);
            }
            with_state(|state| {
                state.last_reminder_id = timer_id.clone();
            });
            Ok(json!({ "reminder_id": timer_id }).to_string())
        }

        #[handler("timer_tick")]
        fn timer_tick(
            &mut self,
            _from_actor: &str,
            _payload_json: &str,
        ) -> Result<String, String> {
            Ok(with_state(|state| {
                state.timer_ticks += 1;
                json!({ "timer_ticks": state.timer_ticks }).to_string()
            }))
        }

        #[handler("reminder_tick")]
        fn reminder_tick(
            &mut self,
            _from_actor: &str,
            _payload_json: &str,
        ) -> Result<String, String> {
            Ok(with_state(|state| {
                state.reminder_ticks += 1;
                json!({ "reminder_ticks": state.reminder_ticks }).to_string()
            }))
        }

        #[handler("kv_put")]
        fn kv_put(&mut self, _from_actor: &str, payload_json: &str) -> Result<String, String> {
            let payload = parse_payload(payload_json)?;
            Ok(handle_kv_put(&payload))
        }

        #[handler("kv_get")]
        fn kv_get(&mut self, _from_actor: &str, payload_json: &str) -> Result<String, String> {
            let payload = parse_payload(payload_json)?;
            Ok(handle_kv_get(&payload))
        }

        #[handler("kv_list")]
        fn kv_list(&mut self, _from_actor: &str, payload_json: &str) -> Result<String, String> {
            let payload = parse_payload(payload_json)?;
            Ok(handle_kv_list(&payload))
        }

        #[handler("kv_delete")]
        fn kv_delete(&mut self, _from_actor: &str, payload_json: &str) -> Result<String, String> {
            let payload = parse_payload(payload_json)?;
            Ok(handle_kv_delete(&payload))
        }

        #[handler("ts_write")]
        fn ts_write(&mut self, _from_actor: &str, payload_json: &str) -> Result<String, String> {
            let payload = parse_payload(payload_json)?;
            Ok(handle_ts_write(&payload))
        }

        #[handler("ts_read")]
        fn ts_read(&mut self, _from_actor: &str, payload_json: &str) -> Result<String, String> {
            let payload = parse_payload(payload_json)?;
            let pattern = payload.get("pattern").cloned().unwrap_or_else(|| json!([]));
            Ok(tuplespace_result(host::ts_read(&pattern.to_string()), "tuple", "ts_read"))
        }

        #[handler("ts_take")]
        fn ts_take(&mut self, _from_actor: &str, payload_json: &str) -> Result<String, String> {
            let payload = parse_payload(payload_json)?;
            let pattern = payload.get("pattern").cloned().unwrap_or_else(|| json!([]));
            Ok(tuplespace_result(host::ts_take(&pattern.to_string()), "tuple", "ts_take"))
        }

        #[handler("ts_read_all")]
        fn ts_read_all(
            &mut self,
            _from_actor: &str,
            payload_json: &str,
        ) -> Result<String, String> {
            let payload = parse_payload(payload_json)?;
            let pattern = payload.get("pattern").cloned().unwrap_or_else(|| json!([]));
            Ok(tuplespace_result(
                host::ts_read_all(&pattern.to_string()),
                "tuples",
                "ts_read_all",
            ))
        }

        #[handler("blob_upload")]
        fn blob_upload(
            &mut self,
            _from_actor: &str,
            payload_json: &str,
        ) -> Result<String, String> {
            let payload = parse_payload(payload_json)?;
            Ok(handle_blob_upload(&payload))
        }

        #[handler("blob_download")]
        fn blob_download(
            &mut self,
            _from_actor: &str,
            payload_json: &str,
        ) -> Result<String, String> {
            let payload = parse_payload(payload_json)?;
            Ok(handle_blob_download(&payload))
        }

        #[handler("blob_list")]
        fn blob_list(
            &mut self,
            _from_actor: &str,
            payload_json: &str,
        ) -> Result<String, String> {
            let payload = parse_payload(payload_json)?;
            Ok(handle_blob_list(&payload))
        }

        #[handler("blob_delete")]
        fn blob_delete(
            &mut self,
            _from_actor: &str,
            payload_json: &str,
        ) -> Result<String, String> {
            let payload = parse_payload(payload_json)?;
            Ok(handle_blob_delete(&payload))
        }

        #[handler("send_event")]
        fn send_event(
            &mut self,
            _from_actor: &str,
            payload_json: &str,
        ) -> Result<String, String> {
            let payload = parse_payload(payload_json)?;
            Ok(handle_send_event(&payload))
        }

        #[handler("broadcast_event")]
        fn broadcast_event(
            &mut self,
            _from_actor: &str,
            payload_json: &str,
        ) -> Result<String, String> {
            let payload = parse_payload(payload_json)?;
            Ok(handle_broadcast_event(&payload))
        }

        #[handler("join_group")]
        fn join_group(
            &mut self,
            _from_actor: &str,
            payload_json: &str,
        ) -> Result<String, String> {
            let payload = parse_payload(payload_json)?;
            Ok(handle_join_group(&payload))
        }

        #[handler("group_members")]
        fn group_members(
            &mut self,
            _from_actor: &str,
            payload_json: &str,
        ) -> Result<String, String> {
            let payload = parse_payload(payload_json)?;
            Ok(handle_group_members(&payload))
        }

        #[handler("publish")]
        fn publish(&mut self, _from_actor: &str, payload_json: &str) -> Result<String, String> {
            let payload = parse_payload(payload_json)?;
            Ok(handle_channel_publish(&payload))
        }

        #[handler("workflow_run")]
        fn workflow_run(
            &mut self,
            _from_actor: &str,
            payload_json: &str,
        ) -> Result<String, String> {
            let payload = parse_payload(payload_json)?;
            Ok(handle_workflow_run(&payload))
        }

        #[handler("workflow_signal:cancel")]
        fn workflow_signal_cancel(
            &mut self,
            _from_actor: &str,
            payload_json: &str,
        ) -> Result<String, String> {
            let payload = parse_payload(payload_json)?;
            Ok(handle_workflow_signal(&payload))
        }

        #[handler("workflow_query:status")]
        fn workflow_query_status(
            &mut self,
            _from_actor: &str,
            _payload_json: &str,
        ) -> Result<String, String> {
            Ok(handle_workflow_query())
        }

        #[handler("stop_actor")]
        fn stop_actor(
            &mut self,
            _from_actor: &str,
            payload_json: &str,
        ) -> Result<String, String> {
            let payload = parse_payload(payload_json)?;
            Ok(handle_stop_target(&payload))
        }

        #[handler("spawn_actor")]
        fn spawn_actor(
            &mut self,
            _from_actor: &str,
            payload_json: &str,
        ) -> Result<String, String> {
            let payload = parse_payload(payload_json)?;
            Ok(handle_spawn_actor(&payload))
        }
    }

    struct AbstractionsBridge;

    impl Guest for AbstractionsBridge {
        fn init(config_json: String) -> String {
            let mut actor = AbstractionsWasmActor::default();
            match SimpleActorHandlers::init(&mut actor, &config_json) {
                Ok(()) => String::new(),
                Err(err) => err,
            }
        }

        fn handle(from_actor: String, msg_type: String, payload_json: String) -> String {
            let op = match parse_op(&msg_type, &payload_json) {
                Ok(op) => op,
                Err(err) => return json!({ "error": err }).to_string(),
            };
            let mut actor = AbstractionsWasmActor::default();
            actor
                .handle_operation(&from_actor, &op, &payload_json)
                .unwrap_or_else(|err| json!({ "error": err }).to_string())
        }

        fn get_state() -> String {
            with_state(|state| serde_json::to_string(state).unwrap_or_else(|_| "{}".to_string()))
        }

        fn set_state(state_json: String) -> String {
            if state_json.is_empty() {
                return String::new();
            }
            match serde_json::from_str::<AbstractionsState>(&state_json) {
                Ok(next) => {
                    let mut guard = state_cell().lock().expect("set_state lock");
                    *guard = next;
                    String::new()
                }
                Err(err) => format!("ERROR: invalid state JSON: {}", err),
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
    async fn cancel(
        &mut self,
        _ctx: &ActorContext,
        _input: Message,
    ) -> Result<(), BehaviorError> {
        self.signals.push("user".to_string());
        self.status = "cancelled".to_string();
        Ok(())
    }

    #[query_handler("status")]
    async fn status(
        &self,
        _ctx: &ActorContext,
        _input: Message,
    ) -> Result<Message, BehaviorError> {
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
    async fn publish(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<(), BehaviorError> {
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
        assert_eq!(workflow.behavior_type(), plexspaces_core::BehaviorType::Workflow);
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
        let message = new_message("cast", json!({
            "op": "publish",
            "channel": "alerts",
            "body": "hello"
        }));

        channel
            .handle_message(&ctx, message)
            .await
            .expect("publish event");
        assert_eq!(channel.received, vec!["alerts:hello".to_string()]);
    }
}
