// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// EFlows4HPC → PlexSpaces: HPC Ensemble (Rust WASM)
//
// Single actor: coordinator (workflow_run) and workers (tasks_ready).
// Uses host ts_write, ts_take, ts_read_all, pg_join, pg_broadcast, now_ms (see host stubs below).
// Build with build.sh; deploy via test.sh.

use serde::{Deserialize, Serialize};
use std::alloc::{alloc, dealloc, Layout};
use std::ptr::write_volatile;
use std::slice;

const TASK_MS: u64 = 18;
const RESULT_PREFIX: &str = "ensemble";
const WORKER_GROUP: &str = "ensemble-workers";
const GATHER_POLL_MAX: usize = 200;

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct EnsembleState {
    ensemble_id: String,
    num_tasks: u32,
    num_completed: u32,
    status: String,
    total_compute_ms: f64,
    total_coord_ms: f64,
    created_at_ms: u64,
    updated_at_ms: u64,
    cancel_requested: bool,
    worker_joined: bool,
}

impl EnsembleState {
    fn new() -> Self {
        Self {
            status: "idle".to_string(),
            ..Default::default()
        }
    }
}

static mut STATE: Option<EnsembleState> = None;
static mut RETURN_AREA: [u8; 8] = [0; 8];
static mut RETURN_BUF: Option<Vec<u8>> = None;
static mut NOW_MS: u64 = 0;

// ---------------------------------------------------------------------------
// Host stubs (at runtime the component host provides these; here we use local simulation)
// ---------------------------------------------------------------------------

fn host_now_ms() -> u64 {
    unsafe {
        NOW_MS += 1;
        NOW_MS
    }
}

fn host_log(_level: &str, _msg: &str) {}

fn host_ts_write(tuple_json: &str) -> String {
    let tuple: Vec<serde_json::Value> = match serde_json::from_str(tuple_json) {
        Ok(t) => t,
        Err(_) => return "ERROR:invalid json".to_string(),
    };
    if tuple.len() >= 3 {
        let kind = tuple[2].as_str().unwrap_or("");
        if kind == "task" {
            host_local_tasks_push(tuple);
        } else if kind == "result" {
            host_local_results_push(tuple);
        }
    }
    String::new()
}

fn host_ts_take(pattern_json: &str) -> String {
    let pattern: Vec<serde_json::Value> = match serde_json::from_str(pattern_json) {
        Ok(p) => p,
        Err(_) => return String::new(),
    };
    if pattern.len() >= 3 && pattern[2].as_str() == Some("task") {
        if let Some(t) = host_local_tasks_pop() {
            return serde_json::to_string(&t).unwrap_or_default();
        }
    }
    String::new()
}

fn host_ts_read_all(pattern_json: &str) -> String {
    let pattern: Vec<serde_json::Value> = match serde_json::from_str(pattern_json) {
        Ok(p) => p,
        Err(_) => return "[]".to_string(),
    };
    if pattern.len() >= 3 && pattern[2].as_str() == Some("result") {
        return serde_json::to_string(&host_local_results_snapshot()).unwrap_or_else(|_| "[]".to_string());
    }
    "[]".to_string()
}

fn host_pg_join(_group: &str) -> String {
    String::new()
}

fn host_pg_broadcast(_group: &str, _msg_type: &str, _payload_json: &str) -> String {
    String::new()
}

// Local simulation: task and result bags (per-instance; real host would be shared)
static mut LOCAL_TASKS: Option<Vec<Vec<serde_json::Value>>> = None;
static mut LOCAL_RESULTS: Option<Vec<Vec<serde_json::Value>>> = None;

fn host_local_tasks_push(tuple: Vec<serde_json::Value>) {
    unsafe {
        if LOCAL_TASKS.is_none() {
            LOCAL_TASKS = Some(Vec::new());
        }
        if let Some(ref mut v) = LOCAL_TASKS {
            v.push(tuple);
        }
    }
}

fn host_local_tasks_pop() -> Option<Vec<serde_json::Value>> {
    unsafe {
        LOCAL_TASKS.as_mut().and_then(|v| if v.is_empty() { None } else { Some(v.remove(0)) })
    }
}

fn host_local_results_push(tuple: Vec<serde_json::Value>) {
    unsafe {
        if LOCAL_RESULTS.is_none() {
            LOCAL_RESULTS = Some(Vec::new());
        }
        if let Some(ref mut v) = LOCAL_RESULTS {
            v.push(tuple);
        }
    }
}

fn host_local_results_snapshot() -> Vec<Vec<serde_json::Value>> {
    unsafe {
        LOCAL_RESULTS.as_ref().cloned().unwrap_or_default()
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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

fn state_mut() -> &'static mut EnsembleState {
    unsafe {
        if STATE.is_none() {
            STATE = Some(EnsembleState::new());
        }
        STATE.as_mut().unwrap()
    }
}

fn run_coordinator(payload: &serde_json::Value) -> String {
    let state = state_mut();
    let t0 = host_now_ms();
    if state.created_at_ms == 0 {
        state.created_at_ms = t0;
    }
    state.ensemble_id = payload
        .get("ensemble_id")
        .or_else(|| payload.get("job_id"))
        .and_then(|v| v.as_str())
        .unwrap_or(&state.ensemble_id)
        .to_string();
    state.updated_at_ms = host_now_ms();

    if state.cancel_requested {
        state.status = "cancelled".to_string();
        return finish(t0, 0.0, "cancelled");
    }
    if state.status == "completed" {
        return finish(t0, 0.0, "completed");
    }

    let num_tasks = payload.get("num_tasks").and_then(|v| v.as_u64()).unwrap_or(10) as u32;
    if num_tasks == 0 {
        return finish(t0, 0.0, "no_tasks");
    }

    state.num_tasks = num_tasks;
    state.num_completed = 0;
    state.status = "scattering".to_string();
    let mut compute_ms = 0.0f64;

    for i in 0..num_tasks {
        let task_id = format!("t{}", i);
        let tuple = serde_json::json!([RESULT_PREFIX, state.ensemble_id, "task", task_id, i]);
        let _ = host_ts_write(&tuple.to_string());
        compute_ms += 2.0;
    }
    state.updated_at_ms = host_now_ms();
    state.status = "running".to_string();

    let payload_broadcast = serde_json::json!({
        "ensemble_id": state.ensemble_id,
        "num_tasks": num_tasks
    });
    let _ = host_pg_broadcast(WORKER_GROUP, "tasks_ready", &payload_broadcast.to_string());
    state.total_coord_ms += (host_now_ms() - t0) as f64;

    // Simulate workers in same instance (when no separate worker instances share tuple space)
    let task_pattern = serde_json::json!([RESULT_PREFIX, state.ensemble_id, "task", serde_json::Value::Null, serde_json::Value::Null]);
    let task_pattern_str = task_pattern.to_string();
    loop {
        let raw = host_ts_take(&task_pattern_str);
        if raw.is_empty() || raw.starts_with("ERROR") {
            break;
        }
        let tuple: Vec<serde_json::Value> = match serde_json::from_str(&raw) {
            Ok(t) => t,
            Err(_) => break,
        };
        let task_id = tuple.get(3).cloned().unwrap_or(serde_json::Value::Null);
        compute_ms += TASK_MS as f64;
        let result = serde_json::json!([RESULT_PREFIX, state.ensemble_id, "result", task_id, { "ok": true, "ms": TASK_MS }]);
        let _ = host_ts_write(&result.to_string());
    }

    let pattern = serde_json::json!([RESULT_PREFIX, state.ensemble_id, "result", serde_json::Value::Null, serde_json::Value::Null]);
    let pattern_str = pattern.to_string();
    for _ in 0..GATHER_POLL_MAX {
        if state.cancel_requested {
            state.status = "cancelled".to_string();
            return finish(t0, compute_ms, "cancelled");
        }
        let raw = host_ts_read_all(&pattern_str);
        let results: Vec<serde_json::Value> = serde_json::from_str(&raw).unwrap_or_default();
        state.num_completed = results.len() as u32;
        if state.num_completed >= num_tasks {
            break;
        }
        state.updated_at_ms = host_now_ms();
    }

    state.status = "completed".to_string();
    state.updated_at_ms = host_now_ms();
    finish(t0, compute_ms, "completed")
}

fn run_worker(payload: &serde_json::Value) -> String {
    let state = state_mut();
    if !state.worker_joined {
        let _ = host_pg_join(WORKER_GROUP);
        state.worker_joined = true;
        host_log("info", &format!("Joined process group {}", WORKER_GROUP));
    }
    let ensemble_id = payload.get("ensemble_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let num_tasks = payload.get("num_tasks").and_then(|v| v.as_u64()).unwrap_or(0);
    if ensemble_id.is_empty() || num_tasks == 0 {
        return serde_json::json!({ "ok": true, "message": "warmup" }).to_string();
    }

    let t0 = host_now_ms();
    let pattern = serde_json::json!([RESULT_PREFIX, ensemble_id, "task", serde_json::Value::Null, serde_json::Value::Null]);
    let pattern_str = pattern.to_string();
    let mut processed = 0u32;
    let mut compute_ms = 0.0f64;
    loop {
        let raw = host_ts_take(&pattern_str);
        if raw.is_empty() || raw.starts_with("ERROR") {
            break;
        }
        let tuple: Vec<serde_json::Value> = match serde_json::from_str(&raw) {
            Ok(t) => t,
            Err(_) => break,
        };
        let task_id = tuple.get(3).cloned().unwrap_or(serde_json::Value::Null);
        compute_ms += TASK_MS as f64;
        let result = serde_json::json!([RESULT_PREFIX, ensemble_id, "result", task_id, { "ok": true, "ms": TASK_MS }]);
        let _ = host_ts_write(&result.to_string());
        processed += 1;
    }
    let elapsed = (host_now_ms() - t0) as f64;
    state.total_compute_ms += compute_ms;
    state.total_coord_ms += (elapsed - compute_ms).max(0.0);
    serde_json::json!({ "ok": true, "processed": processed, "ensemble_id": ensemble_id }).to_string()
}

fn finish(t0: u64, compute_ms: f64, status: &str) -> String {
    let state = state_mut();
    let elapsed = (host_now_ms() - t0) as f64;
    let elapsed = if elapsed < compute_ms { compute_ms } else { elapsed };
    let coord_ms = (elapsed - compute_ms).max(0.0);
    state.total_compute_ms += compute_ms;
    state.total_coord_ms += coord_ms;
    serde_json::json!({
        "status": status,
        "ensemble_id": state.ensemble_id,
        "num_tasks": state.num_tasks,
        "num_completed": state.num_completed,
        "total_compute_ms": state.total_compute_ms,
        "total_coord_ms": state.total_coord_ms
    })
    .to_string()
}

// ---------------------------------------------------------------------------
// cabi_realloc (component ABI)
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

// ---------------------------------------------------------------------------
// Actor exports
// ---------------------------------------------------------------------------

#[export_name = "plexspaces:simple-actor/actor@0.1.0#init"]
pub extern "C" fn wasm_init(config_ptr: u32, config_len: u32) -> u32 {
    let _ = ptr_len_to_string(config_ptr, config_len);
    str_to_return("")
}

#[export_name = "plexspaces:simple-actor/actor@0.1.0#handle"]
pub extern "C" fn wasm_handle(
    from_ptr: u32,
    from_len: u32,
    msg_type_ptr: u32,
    msg_type_len: u32,
    payload_ptr: u32,
    payload_len: u32,
) -> u32 {
    let _from = ptr_len_to_string(from_ptr, from_len);
    let mut msg_type = ptr_len_to_string(msg_type_ptr, msg_type_len);
    let payload_json = ptr_len_to_string(payload_ptr, payload_len);

    if msg_type == "call" || msg_type == "cast" {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&payload_json) {
            for key in ["message_type", "op", "msg_type"] {
                if let Some(s) = v.get(key).and_then(|x| x.as_str()) {
                    if !s.is_empty() && s != "call" && s != "cast" {
                        msg_type = s.to_string();
                        break;
                    }
                }
            }
        }
    }

    let result = if msg_type == "workflow_run" {
        let payload = serde_json::from_str(&payload_json).unwrap_or_default();
        run_coordinator(&payload)
    } else if msg_type.starts_with("workflow_signal:") {
        let name = msg_type.trim_start_matches("workflow_signal:").trim();
        if name == "cancel" {
            state_mut().cancel_requested = true;
            state_mut().updated_at_ms = host_now_ms();
        }
        "{}".to_string()
    } else if msg_type.starts_with("workflow_query:") {
        let name = msg_type.trim_start_matches("workflow_query:").trim();
        if name == "status" {
            let s = state_mut();
            serde_json::json!({
                "ensemble_id": s.ensemble_id,
                "status": s.status,
                "num_tasks": s.num_tasks,
                "num_completed": s.num_completed,
                "cancel_requested": s.cancel_requested,
                "total_compute_ms": s.total_compute_ms,
                "total_coord_ms": s.total_coord_ms,
                "created_at_ms": s.created_at_ms,
                "updated_at_ms": s.updated_at_ms
            })
            .to_string()
        } else {
            serde_json::json!({ "error": "unknown_query", "name": name }).to_string()
        }
    } else if msg_type == "tasks_ready" {
        let payload = serde_json::from_str(&payload_json).unwrap_or_default();
        run_worker(&payload)
    } else {
        serde_json::json!({ "error": "unknown_op", "op": msg_type }).to_string()
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
    match serde_json::from_str::<EnsembleState>(&state_json) {
        Ok(s) => {
            unsafe { STATE = Some(s) };
            str_to_return("")
        }
        Err(_) => str_to_return("ERROR: invalid state JSON"),
    }
}

#[export_name = "cabi_post_plexspaces:simple-actor/actor@0.1.0#init"]
pub extern "C" fn cabi_post_init(_: u32) {
    unsafe { RETURN_BUF = None };
}

#[export_name = "cabi_post_plexspaces:simple-actor/actor@0.1.0#handle"]
pub extern "C" fn cabi_post_handle(_: u32) {
    unsafe { RETURN_BUF = None };
}

#[export_name = "cabi_post_plexspaces:simple-actor/actor@0.1.0#get-state"]
pub extern "C" fn cabi_post_get_state(_: u32) {
    unsafe { RETURN_BUF = None };
}

#[export_name = "cabi_post_plexspaces:simple-actor/actor@0.1.0#set-state"]
pub extern "C" fn cabi_post_set_state(_: u32) {
    unsafe { RETURN_BUF = None };
}
