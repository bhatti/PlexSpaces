// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Calculator WASM Actor - Core WASM ABI (no Component Model yet)
//
// This actor implements a simple calculator that performs arithmetic operations.
// It's compiled to WASM and loaded by the PlexSpaces WASM runtime.
//
// ## Interface
// - init(state_ptr, state_len) -> result (0=success, -1=error)
// - handle_message(from_ptr, from_len, type_ptr, type_len, payload_ptr, payload_len) -> result_ptr
// - snapshot_state() -> (state_ptr, state_len)
//
// ## Build
// ```bash
// cargo build --target wasm32-wasip2 --release
// ```

#![no_std]

extern crate alloc;

use alloc::vec::Vec;
use core::slice;
use serde::{Deserialize, Serialize};

/// Operation types (basic four operations only for WASM version)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum Operation {
    Add,
    Subtract,
    Multiply,
    Divide,
}

/// Calculator state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalculatorState {
    calculation_count: u64,
    last_result: Option<f64>,
}

impl Default for CalculatorState {
    fn default() -> Self {
        Self {
            calculation_count: 0,
            last_result: None,
        }
    }
}

/// Calculation request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalculationRequest {
    operation: Operation,
    operands: Vec<f64>,
}

// Global state (WASM actors are single-threaded, safe to use static mut)
static mut STATE: CalculatorState = CalculatorState {
    calculation_count: 0,
    last_result: None,
};

// Global buffer for snapshot_state result (JSON data only, no length prefix)
static mut SNAPSHOT_BUFFER: [u8; 4096] = [0; 4096];

/// Initialize actor with state
///
/// # Arguments
/// - state_ptr: Pointer to state bytes
/// - state_len: Length of state bytes
///
/// # Returns
/// - 0 on success
/// - -1 on error
#[no_mangle]
pub extern "C" fn init(state_ptr: *const u8, state_len: usize) -> i32 {
    unsafe {
        if state_len == 0 {
            // No initial state, use default
            STATE = CalculatorState::default();
            return 0;
        }

        // Read state from WASM memory
        let state_bytes = slice::from_raw_parts(state_ptr, state_len);

        // Deserialize state
        match serde_json::from_slice::<CalculatorState>(state_bytes) {
            Ok(state) => {
                STATE = state;
                0
            }
            Err(_) => -1,
        }
    }
}

/// Handle incoming message
///
/// # Arguments
/// - from_ptr, from_len: Sender actor ID
/// - type_ptr, type_len: Message type
/// - payload_ptr, payload_len: Message payload
///
/// # Returns
/// - Pointer to result (caller reads length from first 4 bytes)
#[no_mangle]
pub extern "C" fn handle_message(
    _from_ptr: *const u8,
    _from_len: usize,
    type_ptr: *const u8,
    type_len: usize,
    payload_ptr: *const u8,
    payload_len: usize,
) -> *const u8 {
    unsafe {
        // Read message type
        let msg_type_bytes = slice::from_raw_parts(type_ptr, type_len);
        let msg_type = match core::str::from_utf8(msg_type_bytes) {
            Ok(s) => s,
            Err(_) => return core::ptr::null(),
        };

        match msg_type {
            "calculate" => {
                // Read payload
                let payload_bytes = slice::from_raw_parts(payload_ptr, payload_len);

                // Deserialize request
                let request: CalculationRequest = match serde_json::from_slice(payload_bytes) {
                    Ok(req) => req,
                    Err(_) => return core::ptr::null(),
                };

                // Perform calculation
                let result = match execute_calculation(&request) {
                    Ok(r) => r,
                    Err(_) => return core::ptr::null(),
                };

                // Update state
                STATE.calculation_count += 1;
                STATE.last_result = Some(result);

                // Return empty response (fire-and-forget)
                core::ptr::null()
            }
            "get_stats" => {
                // Return current stats as JSON
                // For now, just return empty
                core::ptr::null()
            }
            _ => {
                // Unknown message type
                core::ptr::null()
            }
        }
    }
}

/// Snapshot actor state
///
/// Returns pointer and length as packed i64 value.
/// Lower 32 bits = pointer to JSON data
/// Upper 32 bits = length of JSON data
#[no_mangle]
#[inline(never)]  // Prevent inlining to ensure proper WASM export
pub extern "C" fn snapshot_state() -> u64 {
    unsafe {
        // Serialize state directly to buffer (no length prefix)
        let len = match serde_json_core::to_slice(&STATE, &mut SNAPSHOT_BUFFER) {
            Ok(written) => written as u32,
            Err(_) => 0u32,
        };

        // Return pointer to data (lower 32 bits) and length (upper 32 bits) as u64
        let ptr = SNAPSHOT_BUFFER.as_ptr() as u32;
        let len_u32 = len;

        // Pack as u64: (len << 32) | ptr
        ((len_u32 as u64) << 32) | (ptr as u64)
    }
}

/// Execute calculation
fn execute_calculation(request: &CalculationRequest) -> Result<f64, &'static str> {
    match request.operation {
        Operation::Add => {
            if request.operands.len() != 2 {
                return Err("Add requires exactly 2 operands");
            }
            Ok(request.operands[0] + request.operands[1])
        }
        Operation::Subtract => {
            if request.operands.len() != 2 {
                return Err("Subtract requires exactly 2 operands");
            }
            Ok(request.operands[0] - request.operands[1])
        }
        Operation::Multiply => {
            if request.operands.len() != 2 {
                return Err("Multiply requires exactly 2 operands");
            }
            Ok(request.operands[0] * request.operands[1])
        }
        Operation::Divide => {
            if request.operands.len() != 2 {
                return Err("Divide requires exactly 2 operands");
            }
            if request.operands[1] == 0.0 {
                return Err("Division by zero");
            }
            Ok(request.operands[0] / request.operands[1])
        }
    }
}

// Panic handler for no_std
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

// Required for alloc
#[global_allocator]
static ALLOCATOR: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;
