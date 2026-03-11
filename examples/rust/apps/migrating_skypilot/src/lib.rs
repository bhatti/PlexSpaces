// SPDX-License-Identifier: LGPL-2.1-or-later
//
// SkyPilot → PlexSpaces: Multi-cloud ML orchestration (Rust WASM app).
//
// GenServer-style ops: submit_task, get_best_resources, get_status.
// Spot instance management, cost optimization across AWS/GCP.
// Build: ./build.sh; deploy and run: ./test.sh [HTTP_PORT]

use serde::{Deserialize, Serialize};
use std::alloc::{alloc, dealloc, Layout};
use std::collections::HashMap;
use std::ptr::write_volatile;
use std::slice;

// ---------------------------------------------------------------------------
// Data types (match embedded example)
// ---------------------------------------------------------------------------

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

/// Simulated compute ms per op (catalog scan / matching).
const COMPUTE_MS_SUBMIT: f64 = 2.0;
const COMPUTE_MS_GET_BEST: f64 = 1.0;
/// Simulated coordination overhead per op (message handling, state update).
const COORD_MS_PER_OP: f64 = 0.5;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct SchedulerState {
    task_queue: Vec<AITask>,
    running_tasks: HashMap<String, ResourceAllocation>,
    #[serde(skip_serializing, default)]
    cloud_catalog: HashMap<String, CloudInstance>,
    /// Total time spent in compute (catalog matching) across all ops.
    total_compute_ms: f64,
    /// Total coordination overhead (message handling, state).
    total_coord_ms: f64,
    /// Number of tasks successfully scheduled (for throughput).
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

// ---------------------------------------------------------------------------
// WASM globals and helpers
// ---------------------------------------------------------------------------

static mut STATE: Option<SchedulerState> = None;
static mut RETURN_AREA: [u8; 8] = [0; 8];
static mut RETURN_BUF: Option<Vec<u8>> = None;

fn ptr_len_to_string(ptr: u32, len: u32) -> String {
    if len == 0 {
        return String::new();
    }
    let p = ptr as *const u8;
    unsafe {
        let s = slice::from_raw_parts(p, len as usize);
        String::from_utf8_unchecked(s.to_vec())
    }
}

fn str_to_return(s: &str) -> u32 {
    let bytes = s.as_bytes();
    let n = bytes.len();
    let ret_ptr = if n == 0 {
        0u32
    } else {
        let layout = Layout::array::<u8>(n).unwrap();
        let ptr = unsafe { alloc(layout) };
        if ptr.is_null() {
            0u32
        } else {
            unsafe {
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, n);
                RETURN_BUF = Some(Vec::from_raw_parts(ptr, n, n));
            }
            ptr as u32
        }
    };
    unsafe {
        let area = &mut RETURN_AREA as *mut [u8; 8] as *mut u8;
        write_volatile(area as *mut u32, ret_ptr);
        write_volatile(area.add(4) as *mut u32, n as u32);
        &RETURN_AREA as *const [u8; 8] as u32
    }
}

fn state_mut() -> &'static mut SchedulerState {
    unsafe {
        if STATE.is_none() {
            let mut s = SchedulerState::new();
            s.cloud_catalog = build_cloud_catalog();
            STATE = Some(s);
        }
        STATE.as_mut().unwrap()
    }
}

fn ensure_state_has_catalog() {
    let state = state_mut();
    if state.cloud_catalog.is_empty() {
        state.cloud_catalog = build_cloud_catalog();
    }
}

// ---------------------------------------------------------------------------
// Handle ops: submit_task, get_best_resources, get_status
// ---------------------------------------------------------------------------

fn handle_submit_task(payload: &serde_json::Value) -> String {
    ensure_state_has_catalog();
    let task: AITask = match serde_json::from_value(payload.clone()) {
        Ok(t) => t,
        Err(e) => {
            return serde_json::json!({ "error": format!("invalid task: {}", e) }).to_string();
        }
    };
    let state = state_mut();
    state.total_compute_ms += COMPUTE_MS_SUBMIT;
    state.total_coord_ms += COORD_MS_PER_OP;
    if let Some(allocation) = state.find_best_resources(&task) {
        state.running_tasks.insert(task.task_id.clone(), allocation.clone());
        state.tasks_scheduled += 1;
        serde_json::json!({ "allocation": allocation }).to_string()
    } else {
        state.task_queue.push(task);
        serde_json::json!({ "error": "No resources available, queued" }).to_string()
    }
}

fn handle_get_best_resources(payload: &serde_json::Value) -> String {
    ensure_state_has_catalog();
    let task: AITask = match serde_json::from_value(payload.clone()) {
        Ok(t) => t,
        Err(e) => {
            return serde_json::json!({ "error": format!("invalid task: {}", e) }).to_string();
        }
    };
    let state = state_mut();
    state.total_compute_ms += COMPUTE_MS_GET_BEST;
    state.total_coord_ms += COORD_MS_PER_OP;
    match state.find_best_resources(&task) {
        Some(allocation) => serde_json::json!({ "allocation": allocation }).to_string(),
        None => serde_json::json!({ "error": "No suitable resources found" }).to_string(),
    }
}

fn handle_get_status() -> String {
    let state = state_mut();
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
}

// ---------------------------------------------------------------------------
// WASM exports (plexspaces:simple-actor)
// ---------------------------------------------------------------------------

#[export_name = "cabi_realloc"]
pub extern "C" fn cabi_realloc(old_ptr: u32, old_size: u32, align: u32, new_size: u32) -> u32 {
    if new_size == 0 {
        if old_ptr != 0 {
            unsafe {
                dealloc(
                    old_ptr as *mut u8,
                    Layout::from_size_align(old_size as usize, align as usize).unwrap(),
                );
            }
        }
        return 0;
    }
    let layout = Layout::from_size_align(new_size as usize, align as usize).unwrap();
    let ptr = unsafe { alloc(layout) };
    if ptr.is_null() {
        return 0;
    }
    if old_ptr != 0 && old_size > 0 {
        let copy_len = old_size.min(new_size) as usize;
        unsafe {
            std::ptr::copy_nonoverlapping(old_ptr as *const u8, ptr, copy_len);
        }
        unsafe {
            dealloc(
                old_ptr as *mut u8,
                Layout::from_size_align(old_size as usize, align as usize).unwrap(),
            );
        }
    }
    ptr as u32
}

#[export_name = "plexspaces:simple-actor/actor@0.1.0#init"]
pub extern "C" fn wasm_init(config_ptr: u32, config_len: u32) -> u32 {
    let _ = ptr_len_to_string(config_ptr, config_len);
    unsafe {
        if STATE.is_none() {
            let mut s = SchedulerState::new();
            s.cloud_catalog = build_cloud_catalog();
            STATE = Some(s);
        }
    }
    str_to_return("")
}

#[export_name = "plexspaces:simple-actor/actor@0.1.0#handle"]
pub extern "C" fn wasm_handle(
    _from_ptr: u32,
    _from_len: u32,
    msg_type_ptr: u32,
    msg_type_len: u32,
    payload_ptr: u32,
    payload_len: u32,
) -> u32 {
    let msg_type = ptr_len_to_string(msg_type_ptr, msg_type_len);
    let payload_json = ptr_len_to_string(payload_ptr, payload_len);

    let op = if let Ok(v) = serde_json::from_str::<serde_json::Value>(&payload_json) {
        v.get("op")
            .or_else(|| v.get("message_type"))
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string()
    } else {
        String::new()
    };

    let effective_op = if !op.is_empty() { op.as_str() } else { &msg_type };

    let result = match effective_op {
        "submit_task" => {
            let payload = serde_json::from_str(&payload_json).unwrap_or(serde_json::Value::Null);
            let task_val = payload.get("task").cloned().unwrap_or(payload);
            handle_submit_task(&task_val)
        }
        "get_best_resources" => {
            let payload = serde_json::from_str(&payload_json).unwrap_or(serde_json::Value::Null);
            let task_val = payload.get("task").cloned().unwrap_or(payload);
            handle_get_best_resources(&task_val)
        }
        "get_status" | "status" => handle_get_status(),
        _ => serde_json::json!({ "error": format!("unknown op: {}", effective_op) }).to_string(),
    };

    str_to_return(&result)
}

#[export_name = "plexspaces:simple-actor/actor@0.1.0#get-state"]
pub extern "C" fn wasm_get_state() -> u32 {
    let state = unsafe { STATE.as_ref() };
    let json = state
        .map(|s| serde_json::to_string(s).unwrap_or_else(|_| "{}".to_string()))
        .unwrap_or_else(|| "{}".to_string());
    str_to_return(&json)
}

#[export_name = "plexspaces:simple-actor/actor@0.1.0#set-state"]
pub extern "C" fn wasm_set_state(state_ptr: u32, state_len: u32) -> u32 {
    let state_json = ptr_len_to_string(state_ptr, state_len);
    if state_json.is_empty() {
        return str_to_return("");
    }
    match serde_json::from_str::<SchedulerState>(&state_json) {
        Ok(mut s) => {
            s.cloud_catalog = build_cloud_catalog();
            unsafe {
                STATE = Some(s);
            }
            str_to_return("")
        }
        Err(_) => str_to_return("ERROR: invalid state JSON"),
    }
}

#[export_name = "cabi_post_plexspaces:simple-actor/actor@0.1.0#init"]
pub extern "C" fn cabi_post_init(_: u32) {
    unsafe {
        RETURN_BUF = None;
    }
}

#[export_name = "cabi_post_plexspaces:simple-actor/actor@0.1.0#handle"]
pub extern "C" fn cabi_post_handle(_: u32) {
    unsafe {
        RETURN_BUF = None;
    }
}

#[export_name = "cabi_post_plexspaces:simple-actor/actor@0.1.0#get-state"]
pub extern "C" fn cabi_post_get_state(_: u32) {
    unsafe {
        RETURN_BUF = None;
    }
}

#[export_name = "cabi_post_plexspaces:simple-actor/actor@0.1.0#set-state"]
pub extern "C" fn cabi_post_set_state(_: u32) {
    unsafe {
        RETURN_BUF = None;
    }
}
