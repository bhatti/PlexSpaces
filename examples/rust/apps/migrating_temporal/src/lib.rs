// SPDX-License-Identifier: LGPL-2.1-or-later
//
// Order Fulfillment Workflow - Rust WASM (Temporal-style)
//
// Exports the plexspaces-simple-actor interface (init, handle, get-state, set-state)
// with the same component ABI as the Go SDK. Build with build.sh then deploy via HTTP.
//
// Facets: virtual_actor + durability (app-config.toml).

use serde::{Deserialize, Serialize};
use std::alloc::{alloc, dealloc, Layout};
use std::ptr::write_volatile;
use std::slice;

// ---------------------------------------------------------------------------
// State (one workflow instance per actor ID)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct OrderStep {
    name: String,
    #[serde(rename = "completed_at_ms")]
    completed_at_ms: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct OrderState {
    order_id: String,
    customer_id: String,
    status: String,
    steps: Vec<OrderStep>,
    cancel_requested: bool,
    total_compute_ms: f64,
    total_coord_ms: f64,
    created_at_ms: u64,
    updated_at_ms: u64,
}

impl OrderState {
    fn new() -> Self {
        Self {
            status: "pending".to_string(),
            ..Default::default()
        }
    }
}

// Global state (WASM is single-threaded). Set by init/restore, read/written by handle/get_state/set_state.
static mut STATE: Option<OrderState> = None;

// Return area for canonical ABI: (ptr: u32, len: u32). Host reads then calls cabi_post_*.
static mut RETURN_AREA: [u8; 8] = [0; 8];
// Keep last return string alive until host calls cabi_post.
static mut RETURN_BUF: Option<Vec<u8>> = None;

static mut NOW_MS_COUNTER: u64 = 0;

fn now_ms() -> u64 {
    // Use counter so we build without host import (component embed validates imports).
    // At runtime the node provides host; for now timestamps are monotonic counters.
    #[cfg(target_arch = "wasm32")]
    {
        unsafe {
            NOW_MS_COUNTER += 1;
            NOW_MS_COUNTER
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    0u64
}

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

fn state_mut() -> &'static mut OrderState {
    unsafe {
        if STATE.is_none() {
            STATE = Some(OrderState::new());
        }
        STATE.as_mut().unwrap()
    }
}

fn has_step(steps: &[OrderStep], name: &str) -> bool {
    steps.iter().any(|s| s.name == name)
}

fn run_workflow(payload: &serde_json::Value) -> String {
    let state = state_mut();
    let t0 = now_ms();
    if state.created_at_ms == 0 {
        state.created_at_ms = t0;
    }
    state.order_id = payload
        .get("order_id")
        .and_then(|v| v.as_str())
        .unwrap_or(&state.order_id)
        .to_string();
    state.customer_id = payload
        .get("customer_id")
        .and_then(|v| v.as_str())
        .unwrap_or(&state.customer_id)
        .to_string();
    state.updated_at_ms = now_ms();

    if state.status != "pending" && state.status != "validated" {
        return serde_json::json!({
            "status": state.status,
            "order_id": state.order_id,
            "message": "Workflow already completed or cancelled"
        })
        .to_string();
    }

    let mut compute_ms = 0.0f64;

    if !has_step(&state.steps, "validate") {
        compute_ms += 8.0;
        state.steps.push(OrderStep {
            name: "validate".to_string(),
            completed_at_ms: now_ms(),
        });
        state.status = "validated".to_string();
        state.updated_at_ms = now_ms();
    }
    if state.cancel_requested {
        return compensate("cancel_requested");
    }

    if !has_step(&state.steps, "reserve_inventory") {
        compute_ms += 12.0;
        state.steps.push(OrderStep {
            name: "reserve_inventory".to_string(),
            completed_at_ms: now_ms(),
        });
        state.status = "inventory_reserved".to_string();
        state.updated_at_ms = now_ms();
    }
    if state.cancel_requested {
        return compensate("cancel_requested");
    }

    if !has_step(&state.steps, "charge_payment") {
        compute_ms += 10.0;
        state.steps.push(OrderStep {
            name: "charge_payment".to_string(),
            completed_at_ms: now_ms(),
        });
        state.status = "payment_charged".to_string();
        state.updated_at_ms = now_ms();
    }
    if state.cancel_requested {
        return compensate("cancel_requested");
    }

    if !has_step(&state.steps, "ship") {
        compute_ms += 15.0;
        state.steps.push(OrderStep {
            name: "ship".to_string(),
            completed_at_ms: now_ms(),
        });
        state.status = "shipped".to_string();
        state.updated_at_ms = now_ms();
    }

    let total_elapsed = (now_ms() - t0) as f64;
    let compute_reported = compute_ms.min(total_elapsed);
    let coord_reported = (total_elapsed - compute_reported).max(0.0);
    state.total_compute_ms += compute_reported;
    state.total_coord_ms += coord_reported;

    serde_json::json!({
        "status": state.status,
        "order_id": state.order_id,
        "steps_completed": state.steps.len(),
        "total_compute_ms": state.total_compute_ms,
        "total_coord_ms": state.total_coord_ms
    })
    .to_string()
}

fn compensate(reason: &str) -> String {
    let state = state_mut();
    state.status = "cancelled".to_string();
    state.updated_at_ms = now_ms();
    serde_json::json!({
        "status": "cancelled",
        "order_id": state.order_id,
        "reason": reason,
        "steps_rolled_back": state.steps.len()
    })
    .to_string()
}

fn query_status() -> String {
    let state = state_mut();
    serde_json::json!({
        "order_id": state.order_id,
        "customer_id": state.customer_id,
        "status": state.status,
        "steps_count": state.steps.len(),
        "cancel_requested": state.cancel_requested,
        "total_compute_ms": state.total_compute_ms,
        "total_coord_ms": state.total_coord_ms,
        "created_at_ms": state.created_at_ms,
        "updated_at_ms": state.updated_at_ms
    })
    .to_string()
}

// ---------------------------------------------------------------------------
// Canonical ABI: cabi_realloc (required by component model)
// ---------------------------------------------------------------------------

#[export_name = "cabi_realloc"]
pub extern "C" fn cabi_realloc(
    old_ptr: u32,
    old_size: u32,
    align: u32,
    new_size: u32,
) -> u32 {
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
// Actor exports (plexspaces:simple-actor/actor@0.1.0#...)
// ---------------------------------------------------------------------------

#[export_name = "plexspaces:simple-actor/actor@0.1.0#init"]
pub extern "C" fn wasm_init(config_ptr: u32, config_len: u32) -> u32 {
    let config_json = ptr_len_to_string(config_ptr, config_len);
    let _ = serde_json::from_str::<serde_json::Value>(&config_json).map(|v| {
        if let Some(id) = v.get("order_id").and_then(|x| x.as_str()) {
            state_mut().order_id = id.to_string();
        }
        if let Some(id) = v.get("customer_id").and_then(|x| x.as_str()) {
            state_mut().customer_id = id.to_string();
        }
        let now = now_ms();
        state_mut().created_at_ms = now;
        state_mut().updated_at_ms = now;
    });
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
    let msg_type = ptr_len_to_string(msg_type_ptr, msg_type_len);
    let payload_json = ptr_len_to_string(payload_ptr, payload_len);

    let effective_type = if msg_type == "call" || msg_type == "cast" {
        serde_json::from_str::<serde_json::Value>(&payload_json)
            .ok()
            .and_then(|v| {
                for key in ["message_type", "op", "msg_type"] {
                    if let Some(s) = v.get(key).and_then(|x| x.as_str()) {
                        if !s.is_empty() && s != "call" && s != "cast" {
                            return Some(s.to_string());
                        }
                    }
                }
                None
            })
            .unwrap_or_else(|| msg_type.to_string())
    } else {
        msg_type.to_string()
    };

    let result = if effective_type == "workflow_run" {
        let payload: serde_json::Value = serde_json::from_str(&payload_json).unwrap_or_default();
        run_workflow(&payload)
    } else if effective_type.starts_with("workflow_signal:") {
        let name = effective_type.trim_start_matches("workflow_signal:").trim();
        if name == "cancel" {
            state_mut().cancel_requested = true;
            state_mut().updated_at_ms = now_ms();
        }
        "{}".to_string()
    } else if effective_type.starts_with("workflow_query:") {
        let name = effective_type.trim_start_matches("workflow_query:").trim();
        if name == "status" {
            query_status()
        } else {
            serde_json::json!({ "error": "unknown_query", "name": name }).to_string()
        }
    } else {
        serde_json::json!({ "error": "use workflow_run / workflow_signal / workflow_query" }).to_string()
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
    match serde_json::from_str::<OrderState>(&state_json) {
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
