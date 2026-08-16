// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Actor-CSP: Structured concurrency with supervised actors + Linda tuplespace.
//
// Scatter-gather pattern:
// - Orchestrator spawns N worker actors via supervisor
// - Workers simulate service calls with variable latency
// - Results are coordinated through Linda-style tuplespace (out/in/rd)
// - Timeout triggers cancellation of remaining workers
// - Supervisor guarantees worker lifecycle management

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
    use plexspaces::actor::host_actor::{self_id, send, send_after, spawn, stop};
    use plexspaces::actor::host_ts::{ts_read, ts_read_all, ts_take, ts_write};
    use plexspaces_sdk::simple_actor::ActorWorldHandlers;
    use plexspaces_sdk::{gen_server_actor, plexspaces_handlers};

    // -----------------------------------------------------------------------
    // State
    // -----------------------------------------------------------------------

    #[derive(Debug, Clone, Default, Serialize, Deserialize)]
    struct ScatterGatherState {
        role: String,
        worker_id: usize,
        request_count: u64,
        results_collected: u64,
        timeouts_fired: u64,
    }

    fn state_cell() -> &'static Mutex<ScatterGatherState> {
        static STATE: OnceLock<Mutex<ScatterGatherState>> = OnceLock::new();
        STATE.get_or_init(|| Mutex::new(ScatterGatherState::default()))
    }

    fn with_state<T>(f: impl FnOnce(&mut ScatterGatherState) -> T) -> T {
        let mut g = state_cell().lock().expect("state lock poisoned");
        f(&mut *g)
    }

    // -----------------------------------------------------------------------
    // Linda-style thin wrappers over tuplespace host functions
    // -----------------------------------------------------------------------

    /// Linda OUT: insert a tuple into the tuplespace (non-blocking, non-destructive).
    fn linda_out(fields: &[Value]) -> Result<(), String> {
        let request = WriteRequest {
            request_id: String::new(),
            tuples: vec![json_array_to_proto_tuple(fields)?],
            transaction_id: String::new(),
        };
        ts_write(&request.encode_to_vec())
            .map(|_| ())
            .map_err(|e| format!("linda_out failed: {e}"))
    }

    /// Linda IN: atomically remove and return the first matching tuple (destructive read).
    #[allow(dead_code)]
    fn linda_in(pattern: &[Value]) -> Result<Option<Vec<Value>>, String> {
        let request = ReadRequest {
            request_id: String::new(),
            template: Some(json_array_to_proto_tuple_pattern(pattern)?),
            timeout: None,
            blocking: false,
            take: true,
            max_results: 1,
            transaction_id: String::new(),
            spatial_filter: None,
        };
        let bytes = ts_take(&request.encode_to_vec())
            .map_err(|e| format!("linda_in failed: {e}"))?;
        let response = ReadResponse::decode(bytes.as_slice())
            .map_err(|e| format!("linda_in decode: {e}"))?;
        Ok(response.tuples.first().map(proto_tuple_to_json_array))
    }

    /// Linda RD: read the first matching tuple without removing it (non-destructive).
    #[allow(dead_code)]
    fn linda_rd(pattern: &[Value]) -> Result<Option<Vec<Value>>, String> {
        let request = ReadRequest {
            request_id: String::new(),
            template: Some(json_array_to_proto_tuple_pattern(pattern)?),
            timeout: None,
            blocking: false,
            take: false,
            max_results: 1,
            transaction_id: String::new(),
            spatial_filter: None,
        };
        let bytes = ts_read(&request.encode_to_vec())
            .map_err(|e| format!("linda_rd failed: {e}"))?;
        let response = ReadResponse::decode(bytes.as_slice())
            .map_err(|e| format!("linda_rd decode: {e}"))?;
        Ok(response.tuples.first().map(proto_tuple_to_json_array))
    }

    /// Linda RD-ALL: read all matching tuples.
    fn linda_rd_all(pattern: &[Value]) -> Result<Vec<Vec<Value>>, String> {
        let request = ReadRequest {
            request_id: String::new(),
            template: Some(json_array_to_proto_tuple_pattern(pattern)?),
            timeout: None,
            blocking: false,
            take: false,
            max_results: 1024,
            transaction_id: String::new(),
            spatial_filter: None,
        };
        let bytes = ts_read_all(&request.encode_to_vec())
            .map_err(|e| format!("linda_rd_all failed: {e}"))?;
        let response = ReadResponse::decode(bytes.as_slice())
            .map_err(|e| format!("linda_rd_all decode: {e}"))?;
        Ok(response.tuples.iter().map(proto_tuple_to_json_array).collect())
    }

    // -----------------------------------------------------------------------
    // Proto encoding helpers
    // -----------------------------------------------------------------------

    fn json_value_to_proto_field(v: &Value, allow_wildcard: bool) -> ProtoTupleField {
        let value = match v {
            Value::Null => Some(ProtoTupleValue::Wildcard(true)),
            Value::String(s) if allow_wildcard && s == "*" => Some(ProtoTupleValue::Wildcard(true)),
            Value::String(s) => Some(ProtoTupleValue::String(s.clone())),
            Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Some(ProtoTupleValue::Integer(i))
                } else {
                    Some(ProtoTupleValue::Float(n.as_f64().unwrap_or(0.0)))
                }
            }
            Value::Bool(b) => Some(ProtoTupleValue::Boolean(*b)),
            _ => Some(ProtoTupleValue::String(v.to_string())),
        };
        ProtoTupleField { value }
    }

    fn json_array_to_proto_tuple(values: &[Value]) -> Result<Tuple, String> {
        let fields = values.iter().map(|v| json_value_to_proto_field(v, false)).collect();
        Ok(Tuple {
            id: String::new(),
            fields,
            timestamp: None,
            lease: None,
            metadata: Default::default(),
            location: None,
        })
    }

    fn json_array_to_proto_tuple_pattern(values: &[Value]) -> Result<Tuple, String> {
        let fields = values.iter().map(|v| json_value_to_proto_field(v, true)).collect();
        Ok(Tuple {
            id: String::new(),
            fields,
            timestamp: None,
            lease: None,
            metadata: Default::default(),
            location: None,
        })
    }

    fn proto_tuple_to_json_array(tuple: &Tuple) -> Vec<Value> {
        tuple
            .fields
            .iter()
            .map(|f| match &f.value {
                Some(ProtoTupleValue::String(s)) => Value::String(s.clone()),
                Some(ProtoTupleValue::Integer(i)) => Value::Number((*i).into()),
                Some(ProtoTupleValue::Float(f)) => {
                    serde_json::Number::from_f64(*f)
                        .map(Value::Number)
                        .unwrap_or(Value::Null)
                }
                Some(ProtoTupleValue::Boolean(b)) => Value::Bool(*b),
                Some(ProtoTupleValue::Binary(b)) => {
                    Value::String(std::string::String::from_utf8_lossy(b).to_string())
                }
                Some(ProtoTupleValue::Null(_)) | Some(ProtoTupleValue::Wildcard(_)) | None => Value::Null,
            })
            .collect()
    }

    // -----------------------------------------------------------------------
    // Actor logic
    // -----------------------------------------------------------------------

    fn json_bytes(value: Value) -> Vec<u8> {
        value.to_string().into_bytes()
    }

    fn json_error(err: impl Into<String>) -> Vec<u8> {
        json_bytes(serde_json::json!({ "error": err.into() }))
    }

    fn parse_payload(payload: &[u8]) -> Result<Value, String> {
        if payload.is_empty() {
            return Ok(serde_json::json!({}));
        }
        serde_json::from_slice(payload).map_err(|e| format!("invalid payload: {e}"))
    }

    /// Orchestrator: handles "scatter_gather" requests.
    /// Spawns N workers, sets a timeout, collects first K results from tuplespace.
    fn handle_scatter_gather(payload: &Value) -> Vec<u8> {
        let request_id = payload
            .get("request_id")
            .and_then(|v| v.as_str())
            .unwrap_or("req-001");
        let num_services: usize = payload
            .get("num_services")
            .and_then(|v| v.as_u64())
            .unwrap_or(5) as usize;
        let first_k: usize = payload
            .get("first_k")
            .and_then(|v| v.as_u64())
            .unwrap_or(3) as usize;
        let timeout_ms: u64 = payload
            .get("timeout_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(300);

        // Spawn worker actors for each service
        let my_id = self_id();
        let mut worker_ids = Vec::new();
        for i in 0..num_services {
            let worker_id = format!("csp-worker-{request_id}-{i}");
            let spawn_config = serde_json::json!({
                "role": "worker",
                "worker_id": i,
            });
            let init_json = spawn_config.to_string();
            match spawn("actor_csp", &worker_id, "", &init_json) {
                Ok(_) => worker_ids.push(worker_id.clone()),
                Err(e) => {
                    return json_error(format!("failed to spawn worker-{i}: {e}"));
                }
            }

            // Tell worker to start processing
            let work_msg = serde_json::json!({
                "op": "process",
                "request_id": request_id,
                "service_id": i,
                "orchestrator": my_id,
            });
            let _ = send(&worker_id, "cast", &work_msg.to_string().into_bytes());
        }

        // Set timeout — send ourselves a "collect_results" message after deadline
        let timeout_msg = serde_json::json!({
            "op": "collect_results",
            "request_id": request_id,
            "first_k": first_k,
            "worker_ids": worker_ids,
        });
        let _ = send_after(timeout_ms, "cast", &timeout_msg.to_string().into_bytes());

        with_state(|s| s.request_count += 1);

        json_bytes(serde_json::json!({
            "status": "scattered",
            "request_id": request_id,
            "workers_spawned": num_services,
            "first_k": first_k,
            "timeout_ms": timeout_ms,
        }))
    }

    /// Collect results from tuplespace after timeout fires.
    fn handle_collect_results(payload: &Value) -> Vec<u8> {
        let request_id = payload
            .get("request_id")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let first_k = payload
            .get("first_k")
            .and_then(|v| v.as_u64())
            .unwrap_or(3) as usize;
        let worker_ids: Vec<String> = payload
            .get("worker_ids")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        // Linda IN: collect results from tuplespace
        // Pattern: ["result", request_id, *, *] — match all results for this request
        let pattern = vec![
            Value::String("result".to_string()),
            Value::String(request_id.to_string()),
            Value::Null, // wildcard: service_id
            Value::Null, // wildcard: data
        ];

        let results = match linda_rd_all(&pattern) {
            Ok(tuples) => tuples,
            Err(e) => return json_error(format!("collect failed: {e}")),
        };

        let collected: Vec<Value> = results
            .iter()
            .take(first_k)
            .map(|tuple| {
                serde_json::json!({
                    "service_id": tuple.get(2).cloned().unwrap_or(Value::Null),
                    "data": tuple.get(3).cloned().unwrap_or(Value::Null),
                })
            })
            .collect();

        // Stop remaining workers (structured cleanup)
        for wid in &worker_ids {
            let _ = stop(wid);
        }

        with_state(|s| {
            s.results_collected += collected.len() as u64;
            s.timeouts_fired += 1;
        });

        json_bytes(serde_json::json!({
            "status": "gathered",
            "request_id": request_id,
            "results": collected,
            "total_available": results.len(),
            "returned": collected.len(),
        }))
    }

    /// Worker: simulates a service call and writes result to tuplespace.
    fn handle_worker_process(payload: &Value) -> Vec<u8> {
        let request_id = payload
            .get("request_id")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let service_id = payload
            .get("service_id")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        // Simulate work — in real code this would be an HTTP call or computation
        let result_data = format!("response-from-service-{service_id}");

        // Linda OUT: write result tuple to tuplespace
        let tuple = vec![
            Value::String("result".to_string()),
            Value::String(request_id.to_string()),
            Value::Number(service_id.into()),
            Value::String(result_data.clone()),
        ];

        match linda_out(&tuple) {
            Ok(_) => json_bytes(serde_json::json!({
                "status": "completed",
                "service_id": service_id,
                "data": result_data,
            })),
            Err(e) => json_error(format!("worker write failed: {e}")),
        }
    }

    /// Handle "status" request — return current state metrics.
    fn handle_status() -> Vec<u8> {
        let state = state_cell().lock().expect("state lock");
        json_bytes(serde_json::json!({
            "role": state.role,
            "request_count": state.request_count,
            "results_collected": state.results_collected,
            "timeouts_fired": state.timeouts_fired,
        }))
    }

    // -----------------------------------------------------------------------
    // SDK annotations + WIT bridge
    // -----------------------------------------------------------------------

    #[gen_server_actor(wasm)]
    #[derive(Default)]
    struct ScatterGatherActor;

    #[plexspaces_handlers(wasm)]
    impl ScatterGatherActor {
        #[handler("scatter_gather")]
        fn scatter_gather(&mut self, _from_actor: &str, payload: &[u8]) -> Result<Vec<u8>, String> {
            let payload = parse_payload(payload)?;
            Ok(handle_scatter_gather(&payload))
        }

        #[handler("collect_results")]
        fn collect_results(&mut self, _from_actor: &str, payload: &[u8]) -> Result<Vec<u8>, String> {
            let payload = parse_payload(payload)?;
            Ok(handle_collect_results(&payload))
        }

        #[handler("process")]
        fn process(&mut self, _from_actor: &str, payload: &[u8]) -> Result<Vec<u8>, String> {
            let payload = parse_payload(payload)?;
            Ok(handle_worker_process(&payload))
        }

        #[handler("status")]
        fn status(&mut self, _from_actor: &str, _payload: &[u8]) -> Result<Vec<u8>, String> {
            Ok(handle_status())
        }
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

    struct ActorCspBridge;

    impl Guest for ActorCspBridge {
        fn init(config: Vec<u8>) -> Result<(), String> {
            if !config.is_empty() {
                if let Ok(cfg) = serde_json::from_slice::<Value>(&config) {
                    let role = cfg
                        .get("role")
                        .and_then(|v| v.as_str())
                        .or_else(|| cfg.get("args").and_then(|a| a.get("role")).and_then(|v| v.as_str()))
                        .unwrap_or("orchestrator")
                        .to_string();
                    let worker_id = cfg
                        .get("worker_id")
                        .and_then(|v| v.as_u64())
                        .or_else(|| cfg.get("args").and_then(|a| a.get("worker_id")).and_then(|v| v.as_u64()))
                        .unwrap_or(0) as usize;
                    with_state(|s| {
                        s.role = role;
                        s.worker_id = worker_id;
                    });
                }
            }
            let mut actor = ScatterGatherActor;
            ActorWorldHandlers::init(&mut actor, &config)
        }

        fn handle(from_actor: String, msg_type: String, payload: Vec<u8>) -> Result<Vec<u8>, String> {
            let op = match parse_op(&msg_type, &payload) {
                Ok(op) => op,
                Err(err) => return Ok(json_error(err)),
            };
            let mut actor = ScatterGatherActor;
            Ok(actor
                .handle_operation(&from_actor, &op, &payload)
                .unwrap_or_else(json_error))
        }

        fn get_state() -> Result<Vec<u8>, String> {
            let state = state_cell().lock().expect("state lock");
            serde_json::to_vec(&*state).map_err(|e| e.to_string())
        }

        fn set_state(state: Vec<u8>) -> Result<(), String> {
            if let Ok(loaded) = serde_json::from_slice::<ScatterGatherState>(&state) {
                with_state(|s| *s = loaded);
            }
            Ok(())
        }
    }

    export!(ActorCspBridge);
}
