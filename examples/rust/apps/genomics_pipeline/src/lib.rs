// SPDX-License-Identifier: LGPL-2.1-or-later
//
// Genomics Pipeline - DNA Sequencing Workflow (Rust WASM app).
//
// SCOPE: Single-actor only. This actor runs the full pipeline (QC → Alignment → Variant Calling)
// in one process. There is no leader, no workers, and no multi-node distribution.
// Leader: N/A. Worker: N/A.
//
// GenServer-style ops: run (execute pipeline), query (status/progress/metrics), signal (pause/resume/cancel).
// Build: ./build.sh; deploy and run: ./test.sh [HTTP_PORT]

use serde::{Deserialize, Serialize};
use std::alloc::{alloc, dealloc, Layout};
use std::ptr::write_volatile;
use std::slice;

// ---------------------------------------------------------------------------
// Data types (match workflow semantics)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct QCResult {
    total_reads: usize,
    passed_reads: usize,
    failed_reads: usize,
    avg_quality: f64,
    gc_content: f64,
    adapter_contamination: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct AlignmentResult {
    aligned_reads: usize,
    unaligned_reads: usize,
    alignment_rate: f64,
    avg_mapping_quality: f64,
    reference_genome: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct VariantResult {
    total_variants: usize,
    snps: usize,
    indels: usize,
    novel_variants: usize,
    known_variants: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PipelineInput {
    sample_id: String,
    num_reads: usize,
    reference_genome: String,
    min_quality_score: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PipelineState {
    sample_id: String,
    status: String,
    current_step: String,
    qc_result: Option<QCResult>,
    alignment_result: Option<AlignmentResult>,
    variant_result: Option<VariantResult>,
    error: Option<String>,
    is_paused: bool,
    compute_time_ms: u64,
    coordinate_time_ms: u64,
    /// Always true: this example is single-actor (no leader/worker split).
    #[serde(skip_serializing_if = "Option::is_none")]
    single_actor: Option<bool>,
    /// Input data size (reads) for benchmark clarity.
    #[serde(skip_serializing_if = "Option::is_none")]
    data_size_reads: Option<usize>,
}

// ---------------------------------------------------------------------------
// Sync pipeline execution (no async in WASM; simulate compute/coord times)
// ---------------------------------------------------------------------------

fn run_pipeline_sync(input: &PipelineInput) -> PipelineState {
    let mut state = PipelineState {
        sample_id: input.sample_id.clone(),
        status: "running".to_string(),
        current_step: "quality_control".to_string(),
        ..Default::default()
    };

    let num_reads = input.num_reads;
    let passed = (num_reads as f64 * 0.95) as usize;
    let failed = num_reads - passed;

    let qc_compute = 50u64.saturating_add((num_reads / 1000) as u64);
    state.compute_time_ms += qc_compute;
    state.coordinate_time_ms += 10;

    state.qc_result = Some(QCResult {
        total_reads: num_reads,
        passed_reads: passed,
        failed_reads: failed,
        avg_quality: 35.0 + (input.min_quality_score as f64 * 0.1),
        gc_content: 0.42,
        adapter_contamination: 0.02,
    });

    state.current_step = "alignment".to_string();
    let align_compute = 100u64.saturating_add((passed / 500) as u64);
    state.compute_time_ms += align_compute;
    state.coordinate_time_ms += 10;

    let aligned = (passed as f64 * 0.98) as usize;
    let unaligned = passed - aligned;
    state.alignment_result = Some(AlignmentResult {
        aligned_reads: aligned,
        unaligned_reads: unaligned,
        alignment_rate: aligned as f64 / passed as f64,
        avg_mapping_quality: 45.0,
        reference_genome: input.reference_genome.clone(),
    });

    state.current_step = "variant_calling".to_string();
    let var_compute = 150u64.saturating_add((aligned / 333) as u64);
    state.compute_time_ms += var_compute;
    state.coordinate_time_ms += 10;

    let total_var = (aligned as f64 * 0.005) as usize;
    let snps = (total_var as f64 * 0.85) as usize;
    let indels = total_var - snps;
    state.variant_result = Some(VariantResult {
        total_variants: total_var,
        snps,
        indels,
        novel_variants: (total_var as f64 * 0.1) as usize,
        known_variants: (total_var as f64 * 0.9) as usize,
    });

    state.status = "completed".to_string();
    state.current_step = "done".to_string();
    state.single_actor = Some(true);
    state.data_size_reads = Some(num_reads);
    state
}

// ---------------------------------------------------------------------------
// WASM globals and helpers
// ---------------------------------------------------------------------------

static mut STATE: Option<PipelineState> = None;
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

fn state_mut() -> Option<&'static mut PipelineState> {
    unsafe { STATE.as_mut() }
}

// ---------------------------------------------------------------------------
// Handle ops: run, query (status/progress/metrics), signal (pause/resume/cancel)
// ---------------------------------------------------------------------------

fn handle_run(payload: &str) -> String {
    let input: PipelineInput = match serde_json::from_str::<serde_json::Value>(payload)
        .and_then(serde_json::from_value::<PipelineInput>)
    {
        Ok(i) => i,
        Err(e) => {
            return serde_json::json!({ "error": format!("invalid input: {}", e) }).to_string();
        }
    };
    let state = run_pipeline_sync(&input);
    unsafe {
        STATE = Some(state.clone());
    }
    serde_json::to_string(&state).unwrap_or_else(|_| "{}".to_string())
}

fn handle_query(payload: &str) -> String {
    let query_name: String = serde_json::from_str::<serde_json::Value>(payload)
        .ok()
        .and_then(|v| {
            v.get("query")
                .or_else(|| v.get("name"))
                .and_then(|v| v.as_str().map(String::from))
        })
        .unwrap_or_else(|| "metrics".to_string());

    let state = unsafe { STATE.as_ref() };
    let state = match state {
        Some(s) => s,
        None => {
            return serde_json::json!({ "error": "no state (run pipeline first)" }).to_string();
        }
    };

    match query_name.as_str() {
        "status" => serde_json::json!({
            "sample_id": state.sample_id,
            "status": state.status,
            "current_step": state.current_step,
            "is_paused": state.is_paused
        })
        .to_string(),
        "progress" => {
            let steps: Vec<&str> = [
                state.qc_result.as_ref().map(|_| "quality_control"),
                state.alignment_result.as_ref().map(|_| "alignment"),
                state.variant_result.as_ref().map(|_| "variant_calling"),
            ]
            .into_iter()
            .flatten()
            .collect();
            serde_json::json!({
                "sample_id": state.sample_id,
                "status": state.status,
                "current_step": state.current_step,
                "steps_completed": steps,
                "qc_passed": state.qc_result.as_ref().map(|q| q.passed_reads),
                "aligned_reads": state.alignment_result.as_ref().map(|a| a.aligned_reads),
                "variants_found": state.variant_result.as_ref().map(|v| v.total_variants)
            })
            .to_string()
        }
        _ => {
            let total = state.compute_time_ms + state.coordinate_time_ms;
            let ratio = if state.coordinate_time_ms > 0 {
                state.compute_time_ms as f64 / state.coordinate_time_ms as f64
            } else {
                0.0
            };
            let efficiency = if total > 0 {
                state.compute_time_ms as f64 / total as f64
            } else {
                0.0
            };
            let mut m = serde_json::json!({
                "compute_time_ms": state.compute_time_ms,
                "coordinate_time_ms": state.coordinate_time_ms,
                "total_time_ms": total,
                "granularity_ratio": ratio,
                "efficiency": efficiency
            });
            if let Some(b) = state.single_actor {
                m["single_actor"] = serde_json::json!(b);
            }
            if let Some(n) = state.data_size_reads {
                m["data_size_reads"] = serde_json::json!(n);
            }
            m.to_string()
        }
    }
}

fn handle_signal(payload: &str) -> String {
    let name: String = serde_json::from_str::<serde_json::Value>(payload)
        .ok()
        .and_then(|v| {
            v.get("signal")
                .or_else(|| v.get("name"))
                .and_then(|v| v.as_str().map(String::from))
        })
        .unwrap_or_else(|| String::new());
    if let Some(state) = state_mut() {
        match name.as_str() {
            "pause" => {
                state.is_paused = true;
                state.status = "paused".to_string();
            }
            "resume" => {
                state.is_paused = false;
                state.status = "running".to_string();
            }
            "cancel" => {
                state.status = "cancelled".to_string();
                state.error = Some(
                    serde_json::from_str::<serde_json::Value>(payload)
                        .ok()
                        .and_then(|v| v.get("reason").and_then(|v| v.as_str()).map(String::from))
                        .unwrap_or_else(|| "User requested".to_string()),
                );
            }
            _ => {}
        }
    }
    serde_json::json!({ "ok": true }).to_string()
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
pub extern "C" fn wasm_init(_config_ptr: u32, _config_len: u32) -> u32 {
    unsafe {
        STATE = None;
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
    let _msg_type = ptr_len_to_string(msg_type_ptr, msg_type_len);
    let payload_json = ptr_len_to_string(payload_ptr, payload_len);

    let op = serde_json::from_str::<serde_json::Value>(&payload_json)
        .ok()
        .and_then(|v| v.get("op").or_else(|| v.get("message_type")).and_then(|x| x.as_str()).map(String::from))
        .unwrap_or_else(|| "run".to_string());

    let result = match op.as_str() {
        "run" => handle_run(&payload_json),
        "query" => handle_query(if payload_json.is_empty() { "{}" } else { &payload_json }),
        "signal" => handle_signal(if payload_json.is_empty() { "{}" } else { &payload_json }),
        "get_metrics" | "metrics" => handle_query(r#"{"query":"metrics"}"#),
        "get_status" | "status" => handle_query(r#"{"query":"status"}"#),
        "get_progress" | "progress" => handle_query(r#"{"query":"progress"}"#),
        _ => serde_json::json!({ "error": format!("unknown op: {}", op) }).to_string(),
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
    match serde_json::from_str::<PipelineState>(&state_json) {
        Ok(s) => {
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
