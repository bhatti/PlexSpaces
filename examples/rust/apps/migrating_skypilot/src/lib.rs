// SPDX-License-Identifier: LGPL-2.1-or-later
//
// SkyPilot → PlexSpaces: multi-cloud ML orchestration (Rust WASM).
// SDK: `#[gen_server_actor(wasm)]` + `#[plexspaces_handlers(wasm)]` + `host::application_metrics_add`
// (never merge metrics while holding the scheduler state mutex — wasm32 mutex is non-reentrant).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

wit_bindgen::generate!({
    path: "../../../../wit/plexspaces-simple-actor",
    world: "actor-world",
});

use exports::plexspaces::simple_actor::actor::Guest;
use plexspaces::simple_actor::host;
use plexspaces_sdk::simple_actor::SimpleActorHandlers;
use plexspaces_sdk::{gen_server_actor, plexspaces_handlers};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AITask {
    task_id: String,
    task_type: String,
    gpu_required: bool,
    gpu_memory_gb: u32,
    cpu_cores: u32,
    memory_gb: u32,
    cloud_preference: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ResourceAllocation {
    task_id: String,
    cloud_provider: String,
    instance_type: String,
    cost_per_hour: f64,
    estimated_duration_hours: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CloudInstance {
    instance_type: String,
    gpu_count: u32,
    gpu_memory_gb: u32,
    cpu_cores: u32,
    memory_gb: u32,
    cost_per_hour: f64,
    availability: f64,
}

const COMPUTE_MS_SUBMIT: f64 = 2.0;
const COMPUTE_MS_GET_BEST: f64 = 1.0;
const COORD_MS_PER_OP: f64 = 0.5;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct SchedulerState {
    application_id: String,
    task_queue: Vec<AITask>,
    running_tasks: HashMap<String, ResourceAllocation>,
    #[serde(skip_serializing, default)]
    cloud_catalog: HashMap<String, CloudInstance>,
    total_compute_ms: f64,
    total_coord_ms: f64,
    tasks_scheduled: u64,
}

impl SchedulerState {
    fn new() -> Self {
        Self {
            cloud_catalog: build_cloud_catalog(),
            ..Default::default()
        }
    }

    fn find_best_resources(&self, task: &AITask) -> Option<ResourceAllocation> {
        let mut best_allocation: Option<ResourceAllocation> = None;
        let mut best_cost = f64::MAX;
        for (instance_id, instance) in &self.cloud_catalog {
            let meets_gpu = !task.gpu_required
                || (instance.gpu_count > 0 && instance.gpu_memory_gb >= task.gpu_memory_gb);
            let meets_cpu = instance.cpu_cores >= task.cpu_cores;
            let meets_memory = instance.memory_gb >= task.memory_gb;
            let meets_cloud = task.cloud_preference.as_ref().map_or(true, |p| {
                instance_id.starts_with(&format!("{}:", p))
            });
            if meets_gpu && meets_cpu && meets_memory && meets_cloud {
                let effective_cost = instance.cost_per_hour / instance.availability;
                if effective_cost < best_cost {
                    let (provider, _) = instance_id.split_once(':').unwrap_or(("unknown", ""));
                    best_allocation = Some(ResourceAllocation {
                        task_id: task.task_id.clone(),
                        cloud_provider: provider.to_string(),
                        instance_type: instance.instance_type.clone(),
                        cost_per_hour: instance.cost_per_hour,
                        estimated_duration_hours: 1.0,
                    });
                    best_cost = effective_cost;
                }
            }
        }
        best_allocation
    }
}

fn build_cloud_catalog() -> HashMap<String, CloudInstance> {
    let mut m = HashMap::new();
    m.insert(
        "aws:g4dn.xlarge".to_string(),
        CloudInstance {
            instance_type: "g4dn.xlarge".to_string(),
            gpu_count: 1,
            gpu_memory_gb: 16,
            cpu_cores: 4,
            memory_gb: 16,
            cost_per_hour: 0.526,
            availability: 0.95,
        },
    );
    m.insert(
        "aws:p3.2xlarge".to_string(),
        CloudInstance {
            instance_type: "p3.2xlarge".to_string(),
            gpu_count: 1,
            gpu_memory_gb: 16,
            cpu_cores: 8,
            memory_gb: 61,
            cost_per_hour: 3.06,
            availability: 0.90,
        },
    );
    m.insert(
        "gcp:n1-standard-4".to_string(),
        CloudInstance {
            instance_type: "n1-standard-4".to_string(),
            gpu_count: 0,
            gpu_memory_gb: 0,
            cpu_cores: 4,
            memory_gb: 15,
            cost_per_hour: 0.19,
            availability: 0.98,
        },
    );
    m.insert(
        "gcp:n1-standard-8-gpu".to_string(),
        CloudInstance {
            instance_type: "n1-standard-8-gpu".to_string(),
            gpu_count: 1,
            gpu_memory_gb: 16,
            cpu_cores: 8,
            memory_gb: 30,
            cost_per_hour: 1.50,
            availability: 0.92,
        },
    );
    m
}

fn state_cell() -> &'static Mutex<SchedulerState> {
    static STATE: OnceLock<Mutex<SchedulerState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(SchedulerState::new()))
}

fn with_state<T>(f: impl FnOnce(&mut SchedulerState) -> T) -> T {
    let mut g = state_cell().lock().expect("scheduler state lock poisoned");
    f(&mut *g)
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

fn resolve_application_id() -> String {
    let id = state_cell()
        .lock()
        .expect("scheduler state lock poisoned")
        .application_id
        .clone();
    if !id.is_empty() {
        return id;
    }
    actor_application_id(&host::self_id())
}

fn merge_application_metrics_for(
    application_id: &str,
    metrics: serde_json::Value,
    context: &str,
) -> Result<(), String> {
    let response = host::application_metrics_add(application_id, &metrics.to_string());
    if response.starts_with("ERROR:") {
        Err(format!("{}: {}", context, response))
    } else {
        Ok(())
    }
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

fn ensure_state_has_catalog(state: &mut SchedulerState) {
    if state.cloud_catalog.is_empty() {
        state.cloud_catalog = build_cloud_catalog();
    }
}

fn metrics_submit(compute_ms: u64, coord_ms: u64, scheduled_delta: u64) -> serde_json::Value {
    serde_json::json!({
        "message_count": 1,
        "counter_metrics": {
            "scheduler_submits": 1,
            "scheduler_tasks_scheduled_delta": scheduled_delta,
        },
        "latency_totals_ms": {
            "scheduler.compute": compute_ms,
            "scheduler.coordination": coord_ms,
        },
        "latency_max_ms": {
            "scheduler.compute": compute_ms,
            "scheduler.coordination": coord_ms,
        },
        "latency_samples": {
            "scheduler.compute": 1,
            "scheduler.coordination": 1,
        },
    })
}

fn metrics_get_best(compute_ms: u64, coord_ms: u64) -> serde_json::Value {
    serde_json::json!({
        "message_count": 1,
        "counter_metrics": { "scheduler_get_best": 1 },
        "latency_totals_ms": {
            "scheduler.compute": compute_ms,
            "scheduler.coordination": coord_ms,
        },
        "latency_max_ms": {
            "scheduler.compute": compute_ms,
            "scheduler.coordination": coord_ms,
        },
        "latency_samples": {
            "scheduler.compute": 1,
            "scheduler.coordination": 1,
        },
    })
}

fn metrics_status_query() -> serde_json::Value {
    serde_json::json!({
        "message_count": 1,
        "counter_metrics": { "scheduler_status_queries": 1 },
    })
}

fn handle_submit_task(payload: &serde_json::Value) -> String {
    let application_id = resolve_application_id();
    let phase = with_state(|state| {
        ensure_state_has_catalog(state);
        let task: AITask = match serde_json::from_value(payload.clone()) {
            Ok(t) => t,
            Err(e) => {
                return (
                    serde_json::json!({ "error": format!("invalid task: {}", e) }).to_string(),
                    None,
                );
            }
        };
        state.total_compute_ms += COMPUTE_MS_SUBMIT;
        state.total_coord_ms += COORD_MS_PER_OP;
        let compute_u = COMPUTE_MS_SUBMIT as u64;
        let coord_u = COORD_MS_PER_OP as u64;
        if let Some(allocation) = state.find_best_resources(&task) {
            state.running_tasks.insert(task.task_id.clone(), allocation.clone());
            state.tasks_scheduled += 1;
            let body = serde_json::json!({ "allocation": allocation }).to_string();
            let m = metrics_submit(compute_u, coord_u, 1);
            (body, Some(m))
        } else {
            state.task_queue.push(task);
            let body = serde_json::json!({ "error": "No resources available, queued" }).to_string();
            let m = metrics_submit(compute_u, coord_u, 0);
            (body, Some(m))
        }
    });
    let (body, maybe_metrics) = phase;
    if let Some(metrics) = maybe_metrics {
        if let Err(err) = merge_application_metrics_for(&application_id, metrics, "submit_task metrics") {
            return serde_json::json!({ "error": err }).to_string();
        }
    }
    body
}

fn handle_get_best_resources(payload: &serde_json::Value) -> String {
    let application_id = resolve_application_id();
    let phase = with_state(|state| {
        ensure_state_has_catalog(state);
        let task: AITask = match serde_json::from_value(payload.clone()) {
            Ok(t) => t,
            Err(e) => {
                return (
                    serde_json::json!({ "error": format!("invalid task: {}", e) }).to_string(),
                    None,
                );
            }
        };
        state.total_compute_ms += COMPUTE_MS_GET_BEST;
        state.total_coord_ms += COORD_MS_PER_OP;
        let compute_u = COMPUTE_MS_GET_BEST as u64;
        let coord_u = COORD_MS_PER_OP as u64;
        let m = metrics_get_best(compute_u, coord_u);
        match state.find_best_resources(&task) {
            Some(allocation) => (
                serde_json::json!({ "allocation": allocation }).to_string(),
                Some(m),
            ),
            None => (
                serde_json::json!({ "error": "No suitable resources found" }).to_string(),
                Some(m),
            ),
        }
    });
    let (body, maybe_metrics) = phase;
    if let Some(metrics) = maybe_metrics {
        if let Err(err) =
            merge_application_metrics_for(&application_id, metrics, "get_best_resources metrics")
        {
            return serde_json::json!({ "error": err }).to_string();
        }
    }
    body
}

fn handle_get_status() -> String {
    let application_id = resolve_application_id();
    if let Err(err) = merge_application_metrics_for(
        &application_id,
        metrics_status_query(),
        "get_status metrics",
    ) {
        return serde_json::json!({ "error": err }).to_string();
    }
    with_state(|state| {
        let total_ms = state.total_compute_ms + state.total_coord_ms;
        let compute_pct = if total_ms > 0.0 {
            100.0 * state.total_compute_ms / total_ms
        } else {
            0.0
        };
        let coord_pct = if total_ms > 0.0 {
            100.0 * state.total_coord_ms / total_ms
        } else {
            0.0
        };
        let granularity = if state.total_coord_ms > 0.0 {
            state.total_compute_ms / state.total_coord_ms
        } else {
            0.0
        };
        serde_json::json!({
            "queue_size": state.task_queue.len(),
            "running": state.running_tasks.len(),
            "tasks_scheduled": state.tasks_scheduled,
            "total_compute_ms": state.total_compute_ms,
            "total_coord_ms": state.total_coord_ms,
            "compute_pct": compute_pct,
            "coord_pct": coord_pct,
            "granularity_ratio": granularity
        })
        .to_string()
    })
}

#[gen_server_actor(wasm)]
#[derive(Default)]
struct SkyPilotActor;

#[plexspaces_handlers(wasm)]
impl SkyPilotActor {
    #[init_handler]
    fn configure(&mut self, config_json: &str) -> Result<(), String> {
        let v: serde_json::Value =
            serde_json::from_str(config_json).map_err(|e| format!("invalid init JSON: {}", e))?;
        with_state(|state| {
            let actor_id = v.get("actor_id").and_then(|x| x.as_str()).unwrap_or("");
            state.application_id = if actor_id.is_empty() {
                actor_application_id(&host::self_id())
            } else {
                actor_application_id(actor_id)
            };
        });
        Ok(())
    }

    #[handler("submit_task")]
    fn submit_task(
        &mut self,
        _from_actor: &str,
        payload_json: &str,
    ) -> Result<String, String> {
        let payload: serde_json::Value =
            serde_json::from_str(payload_json).map_err(|e| format!("invalid payload: {}", e))?;
        let task_val = payload.get("task").cloned().unwrap_or(payload);
        Ok(handle_submit_task(&task_val))
    }

    #[handler("get_best_resources")]
    fn get_best_resources(
        &mut self,
        _from_actor: &str,
        payload_json: &str,
    ) -> Result<String, String> {
        let payload: serde_json::Value =
            serde_json::from_str(payload_json).map_err(|e| format!("invalid payload: {}", e))?;
        let task_val = payload.get("task").cloned().unwrap_or(payload);
        Ok(handle_get_best_resources(&task_val))
    }

    #[handler("get_status")]
    fn get_status_op(
        &mut self,
        _from_actor: &str,
        _payload_json: &str,
    ) -> Result<String, String> {
        Ok(handle_get_status())
    }

    #[handler("status")]
    fn status_alias(
        &mut self,
        _from_actor: &str,
        _payload_json: &str,
    ) -> Result<String, String> {
        Ok(handle_get_status())
    }
}

struct SkyPilotBridge;

impl Guest for SkyPilotBridge {
    fn init(config_json: String) -> String {
        let mut actor = SkyPilotActor::default();
        match SimpleActorHandlers::init(&mut actor, &config_json) {
            Ok(()) => String::new(),
            Err(err) => err,
        }
    }

    fn handle(from_actor: String, msg_type: String, payload_json: String) -> String {
        let op = match parse_op(&msg_type, &payload_json) {
            Ok(op) => op,
            Err(err) => return serde_json::json!({ "error": err }).to_string(),
        };
        let mut actor = SkyPilotActor::default();
        actor
            .handle_operation(&from_actor, &op, &payload_json)
            .unwrap_or_else(|err| serde_json::json!({ "error": err }).to_string())
    }

    fn get_state() -> String {
        with_state(|state| serde_json::to_string(state).unwrap_or_else(|_| "{}".to_string()))
    }

    fn set_state(state_json: String) -> String {
        if state_json.is_empty() {
            return String::new();
        }
        match serde_json::from_str::<SchedulerState>(&state_json) {
            Ok(mut s) => {
                s.cloud_catalog = build_cloud_catalog();
                let mut g = state_cell().lock().expect("set_state lock");
                *g = s;
                String::new()
            }
            Err(_) => "ERROR: invalid state JSON".to_string(),
        }
    }
}

export!(SkyPilotBridge);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_op_reads_embedded_op() {
        assert_eq!(
            parse_op("call", r#"{"op":"submit_task","task":{}}"#).expect("op"),
            "submit_task"
        );
    }

    #[test]
    fn actor_application_id_parses_namespace() {
        assert_eq!(
            actor_application_id("x//scheduler::migrating-skypilot-scheduler-rust@node"),
            "migrating-skypilot-scheduler-rust"
        );
    }
}
