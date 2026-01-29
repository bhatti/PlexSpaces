// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is free software: you can redistribute it and/or modify
// it under the terms of the GNU Lesser General Public License as published by
// the Free Software Foundation, either version 2.1 of the License, or
// (at your option) any later version.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! Simplified PlexSpaces host function bindings for Python WASM components
//!
//! This module provides a simplified WIT interface that uses only strings
//! for all complex data. This avoids componentize-py's PyObject_SetItem
//! issues with complex WIT types like `list<u8>`.
//!
//! Key design decisions:
//! - All payloads are JSON strings (not bytes)
//! - Error handling uses string return values ("" = success, "ERROR:..." = failure)
//! - Compatible with componentize-py 0.19.x
//!
//! See: archived_docs/python-wasm-component-guide.md for details

#[cfg(feature = "component-model")]
use crate::HostFunctions;
use plexspaces_core::ActorId;
use std::sync::Arc;
use wasmtime::Result as WasmtimeResult;

// Generate bindings from simplified WIT files
#[cfg(feature = "component-model")]
wasmtime::component::bindgen!({
    world: "actor-world",
    path: "../../wit/plexspaces-simple-actor",
    async: true,
});

/// Simple host implementation for Python-compatible WASM actors
#[cfg(feature = "component-model")]
pub struct SimpleHostImpl {
    /// Actor ID of the component instance
    pub actor_id: ActorId,
    
    /// Host functions implementation
    pub host_functions: Arc<HostFunctions>,
}

#[cfg(feature = "component-model")]
impl SimpleHostImpl {
    pub fn new(actor_id: ActorId, host_functions: Arc<HostFunctions>) -> Self {
        Self {
            actor_id,
            host_functions,
        }
    }
}

/// Implement the simple-host interface
#[cfg(feature = "component-model")]
#[async_trait::async_trait]
impl plexspaces::simple_actor::host::Host for SimpleHostImpl {
    /// Send a message to another actor
    /// Returns empty string on success, error message on failure
    async fn send(&mut self, to: String, msg_type: String, payload_json: String) -> String {
        metrics::counter!("plexspaces_wasm_simple_send_total").increment(1);
        let start_time = std::time::Instant::now();
        
        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!(
                actor_id = %self.actor_id,
                to = %to,
                msg_type = %msg_type,
                payload_len = payload_json.len(),
                "Simple actor sending message"
            );
        }
        
        // Send message using existing host_functions API
        // Note: send_message takes from, to, message as strings
        let result = self.host_functions
            .send_message(&self.actor_id.to_string(), &to, &payload_json)
            .await;
        
        let duration = start_time.elapsed();
        metrics::histogram!("plexspaces_wasm_simple_send_duration_seconds").record(duration.as_secs_f64());
        
        match result {
            Ok(()) => {
                metrics::counter!("plexspaces_wasm_simple_send_success_total").increment(1);
                String::new() // Success
            }
            Err(e) => {
                metrics::counter!("plexspaces_wasm_simple_send_errors_total").increment(1);
                tracing::warn!(
                    actor_id = %self.actor_id,
                    to = %to,
                    error = %e,
                    "Simple actor send failed"
                );
                format!("ERROR: {}", e)
            }
        }
    }
    
    /// Log a message
    async fn log(&mut self, level: String, message: String) {
        metrics::counter!("plexspaces_wasm_simple_log_total").increment(1);
        
        match level.to_lowercase().as_str() {
            "debug" => tracing::debug!(actor_id = %self.actor_id, "[WASM] {}", message),
            "info" => tracing::info!(actor_id = %self.actor_id, "[WASM] {}", message),
            "warn" | "warning" => tracing::warn!(actor_id = %self.actor_id, "[WASM] {}", message),
            "error" => tracing::error!(actor_id = %self.actor_id, "[WASM] {}", message),
            _ => tracing::info!(actor_id = %self.actor_id, level = %level, "[WASM] {}", message),
        }
    }
    
    /// Get current timestamp in milliseconds
    async fn now_ms(&mut self) -> u64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }
}

/// Check if a WASM component uses the simple-actor interface
/// by examining its imports
#[cfg(feature = "component-model")]
pub fn is_simple_actor_component(component: &wasmtime::component::Component) -> bool {
    // Check if the component imports "plexspaces:simple-actor/host@0.1.0"
    // Use exact match to avoid false positives
    let component_type = component.component_type();
    for (name, _) in component_type.imports(&component.engine()) {
        // Match exact interface names
        if name == "plexspaces:simple-actor/host@0.1.0" 
            || name.starts_with("plexspaces:simple-actor@") 
        {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_simple_actor_module_compiles() {
        // This test ensures the module compiles correctly
        // Actual functional tests require WASM components
    }
}
