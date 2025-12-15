// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Heat Diffusion WASM Actor - Grid Cell for Jacobi Iteration
//
// This actor implements a single grid cell for 2D heat diffusion simulation
// using Jacobi iterative method. Compiled to WASM for portable execution.
//
// ## Algorithm: Jacobi Iteration for 2D Heat Equation
// For each cell (i, j), update temperature as average of 4 neighbors:
//   T_new[i,j] = (T[i-1,j] + T[i+1,j] + T[i,j-1] + T[i,j+1]) / 4
//
// ## Interface
// - init(state_ptr, state_len) -> result (0=success, -1=error)
// - handle_message(from_ptr, from_len, type_ptr, type_len, payload_ptr, payload_len) -> result_ptr
// - snapshot_state() -> (state_ptr, state_len) as u64
//
// ## Build
// ```bash
// cargo build --target wasm32-unknown-unknown --release
// ```

#![no_std]

extern crate alloc;

use alloc::vec::Vec;
use core::slice;
use serde::{Deserialize, Serialize};

/// Grid cell state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CellState {
    /// Current temperature value
    temperature: f64,

    /// Neighbor temperatures (N, S, E, W)
    north: f64,
    south: f64,
    east: f64,
    west: f64,

    /// Cell position in grid
    row: usize,
    col: usize,

    /// Iteration count
    iterations: u64,
}

impl Default for CellState {
    fn default() -> Self {
        Self {
            temperature: 0.0,
            north: 0.0,
            south: 0.0,
            east: 0.0,
            west: 0.0,
            row: 0,
            col: 0,
            iterations: 0,
        }
    }
}

/// Message types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum CellMessage {
    /// Set initial position and temperature
    Initialize {
        row: usize,
        col: usize,
        temperature: f64,
    },

    /// Update neighbor temperature
    SetNeighbor {
        direction: Direction,
        temperature: f64,
    },

    /// Perform Jacobi update iteration
    UpdateTemperature,

    /// Get current temperature
    GetTemperature,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Direction {
    North,
    South,
    East,
    West,
}

/// Response message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CellResponse {
    temperature: f64,
    iterations: u64,
}

// Global state (WASM actors are single-threaded, safe to use static mut)
static mut STATE: CellState = CellState {
    temperature: 0.0,
    north: 0.0,
    south: 0.0,
    east: 0.0,
    west: 0.0,
    row: 0,
    col: 0,
    iterations: 0,
};

// Global buffer for snapshot_state result
static mut SNAPSHOT_BUFFER: [u8; 4096] = [0; 4096];

/// Initialize actor with state
#[no_mangle]
pub extern "C" fn init(state_ptr: *const u8, state_len: usize) -> i32 {
    unsafe {
        if state_len == 0 {
            // No initial state, use default
            STATE = CellState::default();
            return 0;
        }

        // Read state from WASM memory
        let state_bytes = slice::from_raw_parts(state_ptr, state_len);

        // Deserialize state
        match serde_json::from_slice::<CellState>(state_bytes) {
            Ok(state) => {
                STATE = state;
                0
            }
            Err(_) => -1,
        }
    }
}

/// Handle incoming message
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
            "message" => {
                // Read payload
                let payload_bytes = slice::from_raw_parts(payload_ptr, payload_len);

                // Deserialize message
                let message: CellMessage = match serde_json::from_slice(payload_bytes) {
                    Ok(msg) => msg,
                    Err(_) => return core::ptr::null(),
                };

                // Process message
                match message {
                    CellMessage::Initialize { row, col, temperature } => {
                        STATE.row = row;
                        STATE.col = col;
                        STATE.temperature = temperature;
                    }
                    CellMessage::SetNeighbor { direction, temperature } => {
                        match direction {
                            Direction::North => STATE.north = temperature,
                            Direction::South => STATE.south = temperature,
                            Direction::East => STATE.east = temperature,
                            Direction::West => STATE.west = temperature,
                        }
                    }
                    CellMessage::UpdateTemperature => {
                        // Jacobi iteration: average of 4 neighbors
                        STATE.temperature = (STATE.north + STATE.south + STATE.east + STATE.west) / 4.0;
                        STATE.iterations += 1;
                    }
                    CellMessage::GetTemperature => {
                        // Return current state (handled by snapshot for now)
                    }
                }

                // Return empty response (fire-and-forget)
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
/// Returns pointer and length as packed u64 value.
/// Lower 32 bits = pointer to JSON data
/// Upper 32 bits = length of JSON data
#[no_mangle]
#[inline(never)]
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

// Panic handler for no_std
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

// Required for alloc
#[global_allocator]
static ALLOCATOR: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;
