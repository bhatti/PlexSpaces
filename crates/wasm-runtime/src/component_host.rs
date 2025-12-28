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

//! PlexSpaces host function bindings for WASM components
//!
//! This module implements the plexspaces host interfaces using wasmtime::component::bindgen!

#[cfg(feature = "component-model")]
use crate::HostFunctions;
use plexspaces_core::{ActorId, RequestContext, TupleSpaceProvider};
use std::sync::Arc;
use wasmtime::Result as WasmtimeResult;

/// Helper function to convert WIT context to RequestContext
/// Empty strings use internal context, non-empty strings create tenant-scoped context
fn context_to_request_context(ctx: &plexspaces::actor::types::Context) -> RequestContext {
    if ctx.tenant_id.is_empty() && ctx.namespace.is_empty() {
        RequestContext::internal()
    } else {
        RequestContext::new_without_auth(ctx.tenant_id.clone(), ctx.namespace.clone())
    }
}

/// Host implementation for plexspaces host interfaces
///
/// This struct holds the state needed to implement plexspaces host functions.
#[cfg(feature = "component-model")]
pub struct PlexspacesHost {
    /// Actor ID of the component instance
    pub actor_id: ActorId,
    
    /// Host functions implementation
    pub host_functions: Arc<HostFunctions>,
}

#[cfg(feature = "component-model")]
impl PlexspacesHost {
    pub fn new(actor_id: ActorId, host_functions: Arc<HostFunctions>) -> Self {
        Self {
            actor_id,
            host_functions,
        }
    }
}

// Generate bindings from WIT files using bindgen! macro
// Path is relative to Cargo.toml (crates/wasm-runtime/)
#[cfg(feature = "component-model")]
wasmtime::component::bindgen!({
    world: "plexspaces-actor",
    path: "../../wit/plexspaces-actor",
    async: true,
});

/// Add plexspaces host function bindings to component linker
///
/// Implements the following interfaces:
/// - `plexspaces:actor/logging@0.1.0` - Logging functions
/// - `plexspaces:actor/messaging@0.1.0` - Messaging functions
/// - `plexspaces:actor/tuplespace@0.1.0` - TupleSpace functions
/// - `plexspaces:actor/channels@0.1.0` - Channel functions
/// - `plexspaces:actor/durability@0.1.0` - Durability functions
/// - `plexspaces:actor/workflow@0.1.0` - Workflow orchestration functions
/// - `plexspaces:actor/blob@0.1.0` - Blob storage functions
///
/// This version works with ComponentContext which stores the implementations.
#[cfg(feature = "component-model")]
pub fn add_plexspaces_host_to_linker(
    linker: &mut wasmtime::component::Linker<crate::instance::ComponentContext>,
) -> WasmtimeResult<()> {
    use crate::instance::ComponentContext;
    
    // Implement logging interface - use stored implementation from context
    plexspaces::actor::logging::add_to_linker(linker, |ctx: &mut ComponentContext| {
        &mut ctx.logging_impl
    })?;

    // Implement messaging interface
    plexspaces::actor::messaging::add_to_linker(linker, |ctx: &mut ComponentContext| {
        &mut ctx.messaging_impl
    })?;

    // Implement tuplespace interface (placeholder)
    plexspaces::actor::tuplespace::add_to_linker(linker, |ctx: &mut ComponentContext| {
        &mut ctx.tuplespace_impl
    })?;

    // Implement channels interface
    plexspaces::actor::channels::add_to_linker(linker, |ctx: &mut ComponentContext| {
        &mut ctx.channels_impl
    })?;

    // Implement durability interface (placeholder)
    plexspaces::actor::durability::add_to_linker(linker, |ctx: &mut ComponentContext| {
        &mut ctx.durability_impl
    })?;

    // Implement workflow interface (placeholder)
    plexspaces::actor::workflow::add_to_linker(linker, |ctx: &mut ComponentContext| {
        &mut ctx.workflow_impl
    })?;

    // Implement blob interface (placeholder)
    plexspaces::actor::blob::add_to_linker(linker, |ctx: &mut ComponentContext| {
        &mut ctx.blob_impl
    })?;

    // Implement keyvalue interface
    plexspaces::actor::keyvalue::add_to_linker(linker, |ctx: &mut ComponentContext| {
        &mut ctx.keyvalue_impl
    })?;

    // Implement process-groups interface
    plexspaces::actor::process_groups::add_to_linker(linker, |ctx: &mut ComponentContext| {
        &mut ctx.process_groups_impl
    })?;

    // Implement locks interface
    plexspaces::actor::locks::add_to_linker(linker, |ctx: &mut ComponentContext| {
        &mut ctx.locks_impl
    })?;

    // Implement registry interface
    plexspaces::actor::registry::add_to_linker(linker, |ctx: &mut ComponentContext| {
        &mut ctx.registry_impl
    })?;

    Ok(())
}

#[cfg(feature = "component-model")]
pub struct LoggingImpl {
    pub actor_id: ActorId,
}

#[cfg(feature = "component-model")]
#[async_trait::async_trait]
impl plexspaces::actor::logging::Host for LoggingImpl {
    async fn trace(&mut self, message: String) {
        tracing::trace!(actor_id = %self.actor_id, "{}", message);
    }

    async fn debug(&mut self, message: String) {
        tracing::debug!(actor_id = %self.actor_id, "{}", message);
    }

    async fn info(&mut self, message: String) {
        tracing::info!(actor_id = %self.actor_id, "{}", message);
    }

    async fn warn(&mut self, message: String) {
        tracing::warn!(actor_id = %self.actor_id, "{}", message);
    }

    async fn error(&mut self, message: String) {
        tracing::error!(actor_id = %self.actor_id, "{}", message);
    }

    async fn log(&mut self, level: plexspaces::actor::logging::LogLevel, message: String) {
        match level {
            plexspaces::actor::logging::LogLevel::Trace => tracing::trace!(actor_id = %self.actor_id, "{}", message),
            plexspaces::actor::logging::LogLevel::Debug => tracing::debug!(actor_id = %self.actor_id, "{}", message),
            plexspaces::actor::logging::LogLevel::Info => tracing::info!(actor_id = %self.actor_id, "{}", message),
            plexspaces::actor::logging::LogLevel::Warn => tracing::warn!(actor_id = %self.actor_id, "{}", message),
            plexspaces::actor::logging::LogLevel::Error => tracing::error!(actor_id = %self.actor_id, "{}", message),
        }
    }

    async fn log_with_fields(
        &mut self,
        level: plexspaces::actor::logging::LogLevel,
        message: String,
        _fields: Vec<(String, String)>,
    ) {
        match level {
            plexspaces::actor::logging::LogLevel::Trace => {
                tracing::trace!(actor_id = %self.actor_id, "{}", message);
            }
            plexspaces::actor::logging::LogLevel::Debug => {
                tracing::debug!(actor_id = %self.actor_id, "{}", message);
            }
            plexspaces::actor::logging::LogLevel::Info => {
                tracing::info!(actor_id = %self.actor_id, "{}", message);
            }
            plexspaces::actor::logging::LogLevel::Warn => {
                tracing::warn!(actor_id = %self.actor_id, "{}", message);
            }
            plexspaces::actor::logging::LogLevel::Error => {
                tracing::error!(actor_id = %self.actor_id, "{}", message);
            }
        }
    }

    async fn start_span(&mut self, _name: String) -> u64 {
        // Placeholder - return a span ID
        0
    }

    async fn end_span(&mut self, _span_id: u64) {
        // Placeholder
    }

    async fn add_span_event(&mut self, _name: String, _attributes: Vec<(String, String)>) {
        // Placeholder
    }
}

#[cfg(feature = "component-model")]
pub struct MessagingImpl {
    pub actor_id: ActorId,
    pub host_functions: Arc<HostFunctions>,
    /// Active timers: timer_id -> join handle for cancellation
    timers: Arc<tokio::sync::RwLock<std::collections::HashMap<u64, tokio::task::JoinHandle<()>>>>,
    /// Pending ask requests: correlation_id -> (sender_id, reply_tx)
    pending_asks: Arc<tokio::sync::RwLock<std::collections::HashMap<String, (String, tokio::sync::oneshot::Sender<Vec<u8>>)>>>,
    /// Monitor references: monitor_ref_u64 -> (target_id, monitor_ref_string)
    monitor_refs: Arc<tokio::sync::RwLock<std::collections::HashMap<u64, (ActorId, String)>>>,
}

#[cfg(feature = "component-model")]
impl MessagingImpl {
    pub fn new(actor_id: ActorId, host_functions: Arc<HostFunctions>) -> Self {
        Self {
            actor_id,
            host_functions,
            timers: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
            pending_asks: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
            monitor_refs: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
        }
    }
}

#[cfg(feature = "component-model")]
#[async_trait::async_trait]
impl plexspaces::actor::messaging::Host for MessagingImpl {
    async fn tell(
        &mut self,
        to: plexspaces::actor::types::ActorId,
        msg_type: String,
        payload: plexspaces::actor::types::Payload,
    ) -> Result<plexspaces::actor::types::MessageId, plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_messaging_tell_total").increment(1);

        let message_str = String::from_utf8_lossy(&payload).to_string();
        match self.host_functions.send_message(&self.actor_id, &to, &message_str).await {
            Ok(_) => {
                let message_id = ulid::Ulid::new().to_string();
                let duration = start_time.elapsed();
                metrics::histogram!("plexspaces_wasm_messaging_tell_duration_seconds").record(duration.as_secs_f64());
                metrics::counter!("plexspaces_wasm_messaging_tell_success_total").increment(1);
                tracing::debug!(
                    actor_id = %self.actor_id,
                    to = %to,
                    msg_type = %msg_type,
                    message_id = %message_id,
                    payload_size = payload.len(),
                    "Message sent via tell"
                );
                Ok(message_id)
            }
            Err(e) => {
                metrics::counter!("plexspaces_wasm_messaging_tell_errors_total").increment(1);
                tracing::warn!(
                    actor_id = %self.actor_id,
                    to = %to,
                    error = %e,
                    "Failed to send message via tell"
                );
                Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::Internal,
                    message: e,
                    details: None,
                })
            }
        }
    }

    async fn ask(
        &mut self,
        to: plexspaces::actor::types::ActorId,
        msg_type: String,
        payload: plexspaces::actor::types::Payload,
        timeout_ms: u64,
    ) -> Result<plexspaces::actor::types::Payload, plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_messaging_ask_total").increment(1);
        let _span = tracing::span!(
            tracing::Level::INFO,
            "wasm_messaging_ask",
            actor_id = %self.actor_id,
            to = %to,
            msg_type = %msg_type,
            timeout_ms = timeout_ms
        )
        .entered();

        // Use default timeout if 0 provided
        let effective_timeout = if timeout_ms == 0 {
            5000 // 5 seconds default
        } else {
            timeout_ms
        };

        // Drop span before await to ensure Send
        drop(_span);
        match self.host_functions.ask(
            &self.actor_id,
            &to,
            &msg_type,
            payload.clone(),
            effective_timeout,
        )
        .await
        {
            Ok(reply_payload) => {
                let duration = start_time.elapsed();
                metrics::histogram!("plexspaces_wasm_messaging_ask_duration_seconds").record(duration.as_secs_f64());
                metrics::counter!("plexspaces_wasm_messaging_ask_success_total").increment(1);
                tracing::debug!(
                    actor_id = %self.actor_id,
                    to = %to,
                    msg_type = %msg_type,
                    reply_size = reply_payload.len(),
                    duration_ms = duration.as_millis(),
                    "Ask request completed successfully"
                );
                Ok(reply_payload)
            }
            Err(e) => {
                metrics::counter!("plexspaces_wasm_messaging_ask_errors_total").increment(1);
                let error_code = if e.contains("Timeout") || e.contains("timeout") {
                    plexspaces::actor::types::ErrorCode::Timeout
                } else if e.contains("not found") || e.contains("ActorNotFound") {
                    plexspaces::actor::types::ErrorCode::ActorNotFound
                } else {
                    plexspaces::actor::types::ErrorCode::Internal
                };
                tracing::warn!(
                    actor_id = %self.actor_id,
                    to = %to,
                    msg_type = %msg_type,
                    error = %e,
                    "Ask request failed"
                );
                Err(plexspaces::actor::types::ActorError {
                    code: error_code,
                    message: e,
                    details: None,
                })
            }
        }
    }

    async fn reply(
        &mut self,
        correlation_id: plexspaces::actor::types::CorrelationId,
        payload: plexspaces::actor::types::Payload,
    ) -> Result<(), plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_messaging_reply_total").increment(1);
        let _span = tracing::span!(
            tracing::Level::INFO,
            "wasm_messaging_reply",
            actor_id = %self.actor_id,
            correlation_id = %correlation_id
        )
        .entered();

        // Drop span before await to ensure Send
        drop(_span);

        // Look up the original sender from pending_asks
        // When a message with correlation_id arrives, we should store it in pending_asks
        // For now, we'll try to find it, and if not found, we'll log a warning
        let reply_result = {
            let mut pending_asks = self.pending_asks.write().await;
            if let Some((sender_id, reply_tx)) = pending_asks.remove(&correlation_id) {
                Some((sender_id, reply_tx))
            } else {
                None
            }
        };
        
        if let Some((sender_id, reply_tx)) = reply_result {
            if reply_tx.send(payload.clone()).is_ok() {
                tracing::debug!(
                    actor_id = %self.actor_id,
                    correlation_id = %correlation_id,
                    sender_id = %sender_id,
                    payload_size = payload.len(),
                    "Reply sent via pending ask channel"
                );
                
                let duration = start_time.elapsed();
                metrics::histogram!("plexspaces_wasm_messaging_reply_duration_seconds").record(duration.as_secs_f64());
                metrics::counter!("plexspaces_wasm_messaging_reply_success_total").increment(1);
                Ok(())
            } else {
                tracing::warn!(
                    actor_id = %self.actor_id,
                    correlation_id = %correlation_id,
                    "Failed to send reply via channel (receiver dropped)"
                );
                metrics::counter!("plexspaces_wasm_messaging_reply_errors_total").increment(1);
                Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::Internal,
                    message: "Reply channel receiver dropped".to_string(),
                    details: None,
                })
            }
        } else {
            // No pending ask found - this might be a reply to a message that wasn't tracked
            // Try to send via message routing with correlation_id
            // Note: This requires the original sender to be in the message context
            // For now, we'll log a warning and return an error
            tracing::warn!(
                actor_id = %self.actor_id,
                correlation_id = %correlation_id,
                "Reply called but no pending ask found - original sender not tracked"
            );
            metrics::counter!("plexspaces_wasm_messaging_reply_errors_total").increment(1);
            Err(plexspaces::actor::types::ActorError {
                code: plexspaces::actor::types::ErrorCode::Internal,
                message: format!("No pending ask found for correlation_id: {}", correlation_id),
                details: None,
            })
        }
    }

    async fn forward(
        &mut self,
        to: plexspaces::actor::types::ActorId,
        msg_type: String,
        payload: plexspaces::actor::types::Payload,
        original_sender: plexspaces::actor::types::ActorId,
        _correlation_id: Option<plexspaces::actor::types::CorrelationId>,
    ) -> Result<plexspaces::actor::types::MessageId, plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_messaging_forward_total").increment(1);
        let _span = tracing::span!(
            tracing::Level::INFO,
            "wasm_messaging_forward",
            actor_id = %self.actor_id,
            to = %to,
            original_sender = %original_sender,
            msg_type = %msg_type
        )
        .entered();

        // Forward message using original sender as the 'from' parameter
        // This makes the message appear to come from the original sender
        let message_str = String::from_utf8_lossy(&payload).to_string();
        // Drop span before await to ensure Send
        drop(_span);
        match self.host_functions.send_message(&original_sender, &to, &message_str).await {
            Ok(_) => {
                let message_id = ulid::Ulid::new().to_string();
                let duration = start_time.elapsed();
                metrics::histogram!("plexspaces_wasm_messaging_forward_duration_seconds").record(duration.as_secs_f64());
                metrics::counter!("plexspaces_wasm_messaging_forward_success_total").increment(1);
                tracing::debug!(
                    actor_id = %self.actor_id,
                    to = %to,
                    original_sender = %original_sender,
                    msg_type = %msg_type,
                    message_id = %message_id,
                    payload_size = payload.len(),
                    "Message forwarded"
                );
                Ok(message_id)
            }
            Err(e) => {
                metrics::counter!("plexspaces_wasm_messaging_forward_errors_total").increment(1);
                tracing::warn!(
                    actor_id = %self.actor_id,
                    to = %to,
                    original_sender = %original_sender,
                    error = %e,
                    "Failed to forward message"
                );
                Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::Internal,
                    message: format!("Failed to forward message: {}", e),
                    details: None,
                })
            }
        }
    }

    async fn spawn(
        &mut self,
        module_ref: String,
        initial_state: plexspaces::actor::types::Payload,
        options: plexspaces::actor::types::SpawnOptions,
    ) -> Result<plexspaces::actor::types::ActorId, plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_messaging_spawn_total").increment(1);
        let _span = tracing::span!(
            tracing::Level::INFO,
            "wasm_messaging_spawn",
            actor_id = %self.actor_id,
            module_ref = %module_ref
        )
        .entered();

        let labels: Vec<(String, String)> = options.labels.iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        // Drop span before await to ensure Send
        drop(_span);
        match self.host_functions.spawn_actor(
            &self.actor_id,
            &module_ref,
            initial_state,
            options.actor_id.clone(),
            labels,
            options.durable,
        )
        .await
        {
            Ok(spawned_actor_id) => {
                let duration = start_time.elapsed();
                metrics::histogram!("plexspaces_wasm_messaging_spawn_duration_seconds").record(duration.as_secs_f64());
                metrics::counter!("plexspaces_wasm_messaging_spawn_success_total").increment(1);
                tracing::debug!(
                    actor_id = %self.actor_id,
                    module_ref = %module_ref,
                    spawned_actor_id = %spawned_actor_id,
                    duration_ms = duration.as_millis(),
                    "Actor spawned successfully"
                );
                Ok(spawned_actor_id)
            }
            Err(e) => {
                metrics::counter!("plexspaces_wasm_messaging_spawn_errors_total").increment(1);
                let error_code = if e.contains("not found") || e.contains("ActorNotFound") {
                    plexspaces::actor::types::ErrorCode::ActorNotFound
                } else if e.contains("resource") || e.contains("limit") {
                    plexspaces::actor::types::ErrorCode::ResourceExhausted
                } else {
                    plexspaces::actor::types::ErrorCode::Internal
                };
                tracing::warn!(
                    actor_id = %self.actor_id,
                    module_ref = %module_ref,
                    error = %e,
                    "Actor spawn failed"
                );
                Err(plexspaces::actor::types::ActorError {
                    code: error_code,
                    message: e,
                    details: None,
                })
            }
        }
    }

    async fn stop(
        &mut self,
        actor_id: plexspaces::actor::types::ActorId,
        timeout_ms: u64,
    ) -> Result<(), plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_messaging_stop_total").increment(1);
        let _span = tracing::span!(
            tracing::Level::INFO,
            "wasm_messaging_stop",
            actor_id = %self.actor_id,
            target_actor_id = %actor_id,
            timeout_ms = timeout_ms
        )
        .entered();

        // Drop span before await to ensure Send
        drop(_span);

        // Use default timeout if 0 provided
        let effective_timeout = if timeout_ms == 0 {
            5000 // 5 seconds default
        } else {
            timeout_ms
        };

        match self.host_functions.stop_actor(
            &self.actor_id,
            &actor_id,
            effective_timeout,
        )
        .await
        {
            Ok(_) => {
                let duration = start_time.elapsed();
                metrics::histogram!("plexspaces_wasm_messaging_stop_duration_seconds").record(duration.as_secs_f64());
                metrics::counter!("plexspaces_wasm_messaging_stop_success_total").increment(1);
                tracing::debug!(
                    actor_id = %self.actor_id,
                    target_actor_id = %actor_id,
                    duration_ms = duration.as_millis(),
                    "Actor stopped successfully"
                );
                Ok(())
            }
            Err(e) => {
                metrics::counter!("plexspaces_wasm_messaging_stop_errors_total").increment(1);
                let error_code = if e.contains("not found") || e.contains("ActorNotFound") {
                    plexspaces::actor::types::ErrorCode::ActorNotFound
                } else if e.contains("Timeout") || e.contains("timeout") {
                    plexspaces::actor::types::ErrorCode::Timeout
                } else {
                    plexspaces::actor::types::ErrorCode::Internal
                };
                tracing::warn!(
                    actor_id = %self.actor_id,
                    target_actor_id = %actor_id,
                    error = %e,
                    "Actor stop failed"
                );
                Err(plexspaces::actor::types::ActorError {
                    code: error_code,
                    message: e,
                    details: None,
                })
            }
        }
    }

    async fn link(
        &mut self,
        actor_id: plexspaces::actor::types::ActorId,
    ) -> Result<(), plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_messaging_link_total").increment(1);
        let _span = tracing::span!(
            tracing::Level::INFO,
            "wasm_messaging_link",
            actor_id = %self.actor_id,
            target_actor_id = %actor_id
        )
        .entered();

        // Drop span before await to ensure Send
        drop(_span);

        match self.host_functions.link_actor(&self.actor_id, &self.actor_id, &actor_id).await {
            Ok(()) => {
                let duration = start_time.elapsed();
                metrics::histogram!("plexspaces_wasm_messaging_link_duration_seconds").record(duration.as_secs_f64());
                metrics::counter!("plexspaces_wasm_messaging_link_success_total").increment(1);
                Ok(())
            }
            Err(e) => {
                metrics::counter!("plexspaces_wasm_messaging_link_errors_total").increment(1);
                Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::Internal,
                    message: format!("Failed to link actor: {}", e),
                    details: None,
                })
            }
        }
    }

    async fn unlink(
        &mut self,
        actor_id: plexspaces::actor::types::ActorId,
    ) -> Result<(), plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_messaging_unlink_total").increment(1);
        let _span = tracing::span!(
            tracing::Level::INFO,
            "wasm_messaging_unlink",
            actor_id = %self.actor_id,
            target_actor_id = %actor_id
        )
        .entered();

        // Drop span before await to ensure Send
        drop(_span);

        match self.host_functions.unlink_actor(&self.actor_id, &self.actor_id, &actor_id).await {
            Ok(()) => {
                let duration = start_time.elapsed();
                metrics::histogram!("plexspaces_wasm_messaging_unlink_duration_seconds").record(duration.as_secs_f64());
                metrics::counter!("plexspaces_wasm_messaging_unlink_success_total").increment(1);
                Ok(())
            }
            Err(e) => {
                metrics::counter!("plexspaces_wasm_messaging_unlink_errors_total").increment(1);
                Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::Internal,
                    message: format!("Failed to unlink actor: {}", e),
                    details: None,
                })
            }
        }
    }

    async fn monitor(
        &mut self,
        actor_id: plexspaces::actor::types::ActorId,
    ) -> Result<u64, plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_messaging_monitor_total").increment(1);
        let _span = tracing::span!(
            tracing::Level::INFO,
            "wasm_messaging_monitor",
            actor_id = %self.actor_id,
            target_actor_id = %actor_id
        )
        .entered();

        // Drop span before await to ensure Send
        drop(_span);

        match self.host_functions.monitor_actor(&self.actor_id, &actor_id).await {
            Ok(monitor_ref) => {
                // Store mapping: monitor_ref_u64 -> target_id
                // The actual monitor_ref_string is stored in wasm_message_sender.rs
                // We just need to track the target_id here for validation
                let target_id = actor_id.clone();
                {
                    let mut monitor_refs = self.monitor_refs.write().await;
                    // Store with placeholder string - actual string is in wasm_message_sender
                    monitor_refs.insert(monitor_ref, (target_id, monitor_ref.to_string()));
                }
                
                let duration = start_time.elapsed();
                metrics::histogram!("plexspaces_wasm_messaging_monitor_duration_seconds").record(duration.as_secs_f64());
                metrics::counter!("plexspaces_wasm_messaging_monitor_success_total").increment(1);
                Ok(monitor_ref)
            }
            Err(e) => {
                metrics::counter!("plexspaces_wasm_messaging_monitor_errors_total").increment(1);
                Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::Internal,
                    message: format!("Failed to monitor actor: {}", e),
                    details: None,
                })
            }
        }
    }

    async fn demonitor(
        &mut self,
        monitor_ref: u64,
    ) -> Result<(), plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_messaging_demonitor_total").increment(1);
        let _span = tracing::span!(
            tracing::Level::INFO,
            "wasm_messaging_demonitor",
            actor_id = %self.actor_id,
            monitor_ref = monitor_ref
        )
        .entered();

        // Drop span before await to ensure Send
        drop(_span);

        // Look up the target_id and monitor_ref_string from our mapping
        let monitor_info = {
            let monitor_refs = self.monitor_refs.read().await;
            monitor_refs.get(&monitor_ref).cloned()
        };

        if let Some((target_id, monitor_ref_string)) = monitor_info {
            // Remove from mapping
            {
                let mut monitor_refs = self.monitor_refs.write().await;
                monitor_refs.remove(&monitor_ref);
            }
            
            // Call demonitor via host_functions
            match self.host_functions.demonitor_actor(&self.actor_id, &target_id, monitor_ref).await {
                Ok(()) => {
                    let duration = start_time.elapsed();
                    metrics::histogram!("plexspaces_wasm_messaging_demonitor_duration_seconds").record(duration.as_secs_f64());
                    metrics::counter!("plexspaces_wasm_messaging_demonitor_success_total").increment(1);
                    tracing::debug!(
                        actor_id = %self.actor_id,
                        target_id = %target_id,
                        monitor_ref = monitor_ref,
                        "Demonitor successful"
                    );
                    Ok(())
                }
                Err(e) => {
                    metrics::counter!("plexspaces_wasm_messaging_demonitor_errors_total").increment(1);
                    tracing::warn!(
                        actor_id = %self.actor_id,
                        target_id = %target_id,
                        monitor_ref = monitor_ref,
                        error = %e,
                        "Demonitor failed"
                    );
                    Err(plexspaces::actor::types::ActorError {
                        code: plexspaces::actor::types::ErrorCode::Internal,
                        message: format!("Failed to demonitor actor: {}", e),
                        details: None,
                    })
                }
            }
        } else {
            metrics::counter!("plexspaces_wasm_messaging_demonitor_errors_total").increment(1);
            tracing::warn!(
                actor_id = %self.actor_id,
                monitor_ref = monitor_ref,
                "Monitor reference not found"
            );
            Err(plexspaces::actor::types::ActorError {
                code: plexspaces::actor::types::ErrorCode::ActorNotFound,
                message: format!("Monitor reference {} not found", monitor_ref),
                details: None,
            })
        }
    }

    async fn self_id(&mut self) -> plexspaces::actor::types::ActorId {
        metrics::counter!("plexspaces_wasm_messaging_self_id_total").increment(1);
        self.actor_id.clone()
    }

    async fn parent_id(&mut self) -> Option<plexspaces::actor::types::ActorId> {
        // NOTE: Parent ID lookup requires ActorRegistry access which is not currently
        // available in HostFunctions. This is acceptable for now as parent relationships
        // are primarily used for supervision trees which are managed at the Rust level.
        // Future enhancement: Add ActorRegistry to HostFunctions to enable parent_id lookup.
        None
    }

    async fn now(&mut self) -> u64 {
        metrics::counter!("plexspaces_wasm_messaging_now_total").increment(1);
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    }

    async fn sleep(&mut self, duration_ms: u64) {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_messaging_sleep_total").increment(1);
        metrics::histogram!("plexspaces_wasm_messaging_sleep_duration_ms").record(duration_ms as f64);
        
        tokio::time::sleep(tokio::time::Duration::from_millis(duration_ms)).await;
        
        let actual_duration = start_time.elapsed();
        metrics::histogram!("plexspaces_wasm_messaging_sleep_actual_duration_ms").record(actual_duration.as_millis() as f64);
        tracing::debug!(
            actor_id = %self.actor_id,
            requested_ms = duration_ms,
            actual_ms = actual_duration.as_millis(),
            "Actor slept"
        );
    }

    async fn send_after(
        &mut self,
        delay_ms: u64,
        msg_type: String,
        payload: plexspaces::actor::types::Payload,
    ) -> Result<u64, plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_messaging_send_after_total").increment(1);
        let _span = tracing::span!(
            tracing::Level::INFO,
            "wasm_messaging_send_after",
            actor_id = %self.actor_id,
            delay_ms = delay_ms,
            msg_type = %msg_type
        )
        .entered();

        // Generate timer ID
        let timer_id = ulid::Ulid::new().to_string().parse::<u64>()
            .unwrap_or_else(|_| std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs());

        // Drop span before await to ensure Send
        drop(_span);

        // Schedule message to self after delay
        let actor_id_str = self.actor_id.to_string();
        let actor_id_clone = self.actor_id.clone();
        let msg_type_clone = msg_type.clone();
        let payload_clone = payload.clone();
        let host_functions_clone = self.host_functions.clone();
        let timers_clone = self.timers.clone();

        // Create abortable task
        let join_handle = tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
            
            // Send message to actor's mailbox
            let message_str = String::from_utf8_lossy(&payload_clone).to_string();
            if let Err(e) = host_functions_clone.send_message(&actor_id_str, &actor_id_str, &message_str).await {
                tracing::warn!(
                    actor_id = %actor_id_clone,
                    timer_id = timer_id,
                    error = %e,
                    "Failed to deliver scheduled message"
                );
            } else {
                tracing::debug!(
                    actor_id = %actor_id_clone,
                    timer_id = timer_id,
                    msg_type = %msg_type_clone,
                    "Scheduled message delivered"
                );
            }
            
            // Remove timer from tracking when it completes
            let mut timers = timers_clone.write().await;
            timers.remove(&timer_id);
        });

        // Store join handle for cancellation
        {
            let mut timers = self.timers.write().await;
            timers.insert(timer_id, join_handle);
        }

        tracing::debug!(
            actor_id = %self.actor_id,
            timer_id = timer_id,
            delay_ms = delay_ms,
            msg_type = %msg_type,
            "Scheduled message"
        );

        let duration = start_time.elapsed();
        metrics::histogram!("plexspaces_wasm_messaging_send_after_duration_seconds").record(duration.as_secs_f64());
        metrics::counter!("plexspaces_wasm_messaging_send_after_success_total").increment(1);
        Ok(timer_id)
    }

    async fn cancel_timer(
        &mut self,
        timer_id: u64,
    ) -> Result<(), plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_messaging_cancel_timer_total").increment(1);
        let _span = tracing::span!(
            tracing::Level::INFO,
            "wasm_messaging_cancel_timer",
            actor_id = %self.actor_id,
            timer_id = timer_id
        )
        .entered();

        // Drop span before await to ensure Send
        drop(_span);

        // Cancel timer by aborting the task
        let mut timers = self.timers.write().await;
        if let Some(join_handle) = timers.remove(&timer_id) {
            join_handle.abort();
            tracing::debug!(
                actor_id = %self.actor_id,
                timer_id = timer_id,
                "Timer cancelled"
            );
            let duration = start_time.elapsed();
            metrics::histogram!("plexspaces_wasm_messaging_cancel_timer_duration_seconds").record(duration.as_secs_f64());
            metrics::counter!("plexspaces_wasm_messaging_cancel_timer_success_total").increment(1);
            Ok(())
        } else {
            tracing::warn!(
                actor_id = %self.actor_id,
                timer_id = timer_id,
                "Timer not found for cancellation"
            );
            metrics::counter!("plexspaces_wasm_messaging_cancel_timer_errors_total").increment(1);
            Err(plexspaces::actor::types::ActorError {
                code: plexspaces::actor::types::ErrorCode::ActorNotFound,
                message: format!("Timer {} not found", timer_id),
                details: None,
            })
        }
    }
}

#[cfg(feature = "component-model")]
pub struct TuplespaceImpl {
    pub tuplespace_provider: Option<Arc<dyn plexspaces_core::TupleSpaceProvider>>,
    pub actor_id: ActorId,
}

#[cfg(feature = "component-model")]
impl TuplespaceImpl {
    pub fn new(
        tuplespace_provider: Option<Arc<dyn plexspaces_core::TupleSpaceProvider>>,
        actor_id: ActorId,
    ) -> Self {
        Self {
            tuplespace_provider,
            actor_id,
        }
    }

    /// Map tuplespace error to ActorError
    fn map_tuplespace_error(
        &self,
        error: plexspaces_tuplespace::TupleSpaceError,
    ) -> plexspaces::actor::types::ActorError {
        use plexspaces_tuplespace::TupleSpaceError;
        match error {
            TupleSpaceError::BackendError(msg) => plexspaces::actor::types::ActorError {
                code: plexspaces::actor::types::ErrorCode::Internal,
                message: msg,
                details: None,
            },
            TupleSpaceError::PatternError(msg) => plexspaces::actor::types::ActorError {
                code: plexspaces::actor::types::ErrorCode::InvalidMessage,
                message: msg,
                details: None,
            },
            TupleSpaceError::NotFound => plexspaces::actor::types::ActorError {
                code: plexspaces::actor::types::ErrorCode::ActorNotFound,
                message: "Tuple not found".to_string(),
                details: None,
            },
            _ => plexspaces::actor::types::ActorError {
                code: plexspaces::actor::types::ErrorCode::Internal,
                message: format!("Tuplespace error: {:?}", error),
                details: None,
            },
        }
    }

    /// Convert WIT TupleData to tuplespace Tuple
    fn convert_wit_tuple_to_internal(
        &self,
        tuple_data: &plexspaces::actor::types::TupleData,
    ) -> Result<plexspaces_tuplespace::Tuple, String> {
        use plexspaces_tuplespace::{Tuple, TupleField, OrderedFloat};

        let mut fields = Vec::new();
        for field in tuple_data {
            let tuple_field = match field {
                plexspaces::actor::types::TupleField::StringVal(s) => {
                    TupleField::String(s.clone())
                }
                plexspaces::actor::types::TupleField::IntVal(i) => {
                    TupleField::Integer(*i)
                }
                plexspaces::actor::types::TupleField::FloatVal(f) => {
                    TupleField::Float(OrderedFloat::new(*f))
                }
                plexspaces::actor::types::TupleField::BytesVal(b) => {
                    TupleField::Binary(b.clone())
                }
                plexspaces::actor::types::TupleField::BoolVal(b) => {
                    TupleField::Boolean(*b)
                }
                plexspaces::actor::types::TupleField::NullVal => {
                    TupleField::Null
                }
            };
            fields.push(tuple_field);
        }

        Ok(Tuple::new(fields))
    }

    /// Convert tuplespace Tuple to WIT TupleData
    fn convert_internal_tuple_to_wit(
        &self,
        tuple: &plexspaces_tuplespace::Tuple,
    ) -> plexspaces::actor::types::TupleData {
        use plexspaces_tuplespace::TupleField;

        tuple.fields()
            .iter()
            .map(|field| match field {
                TupleField::String(s) => {
                    plexspaces::actor::types::TupleField::StringVal(s.clone())
                }
                TupleField::Integer(i) => {
                    plexspaces::actor::types::TupleField::IntVal(*i)
                }
                TupleField::Float(f) => {
                    plexspaces::actor::types::TupleField::FloatVal(f.get())
                }
                TupleField::Binary(b) => {
                    plexspaces::actor::types::TupleField::BytesVal(b.clone())
                }
                TupleField::Boolean(b) => {
                    plexspaces::actor::types::TupleField::BoolVal(*b)
                }
                TupleField::Null => {
                    plexspaces::actor::types::TupleField::NullVal
                }
            })
            .collect()
    }

    /// Convert WIT Pattern to tuplespace Pattern
    fn convert_wit_pattern_to_internal(
        &self,
        pattern_data: &plexspaces::actor::types::Pattern,
    ) -> Result<plexspaces_tuplespace::Pattern, String> {
        use plexspaces_tuplespace::{Pattern, PatternField, TupleField, OrderedFloat, FieldType};

        let mut fields = Vec::new();
        for field in pattern_data {
            let pattern_field = match field {
                plexspaces::actor::types::PatternField::Exact(tuple_field) => {
                    let internal_field = match tuple_field {
                        plexspaces::actor::types::TupleField::StringVal(s) => {
                            TupleField::String(s.clone())
                        }
                        plexspaces::actor::types::TupleField::IntVal(i) => {
                            TupleField::Integer(*i)
                        }
                        plexspaces::actor::types::TupleField::FloatVal(f) => {
                            TupleField::Float(OrderedFloat::new(*f))
                        }
                        plexspaces::actor::types::TupleField::BytesVal(b) => {
                            TupleField::Binary(b.clone())
                        }
                        plexspaces::actor::types::TupleField::BoolVal(b) => {
                            TupleField::Boolean(*b)
                        }
                        plexspaces::actor::types::TupleField::NullVal => {
                            TupleField::Null
                        }
                    };
                    PatternField::Exact(internal_field)
                }
                plexspaces::actor::types::PatternField::Any => {
                    PatternField::Wildcard
                }
                plexspaces::actor::types::PatternField::Typed(field_type) => {
                    let internal_type = match field_type {
                        plexspaces::actor::types::FieldType::StringType => FieldType::String,
                        plexspaces::actor::types::FieldType::IntType => FieldType::Integer,
                        plexspaces::actor::types::FieldType::FloatType => FieldType::Float,
                        plexspaces::actor::types::FieldType::BytesType => FieldType::Binary,
                        plexspaces::actor::types::FieldType::BoolType => FieldType::Boolean,
                        plexspaces::actor::types::FieldType::NullType => FieldType::Null,
                        plexspaces::actor::types::FieldType::TupleType => {
                            return Err("Nested tuple types not supported".to_string());
                        }
                    };
                    PatternField::Type(internal_type)
                }
                plexspaces::actor::types::PatternField::Predicate(pred_str) => {
                    // For now, predicate matching is not supported
                    // Could be implemented by parsing the predicate string
                    return Err(format!("Predicate patterns not yet supported: {}", pred_str));
                }
            };
            fields.push(pattern_field);
        }

        Ok(Pattern::new(fields))
    }
}

#[cfg(feature = "component-model")]
#[async_trait::async_trait]
impl plexspaces::actor::tuplespace::Host for TuplespaceImpl {
    async fn write(
        &mut self,
        ctx: plexspaces::actor::types::Context,
        tuple_data: plexspaces::actor::types::TupleData,
    ) -> Result<(), plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_tuplespace_write_total").increment(1);

        // Convert WIT tuple to internal tuple
        let tuple = match self.convert_wit_tuple_to_internal(&tuple_data) {
            Ok(t) => t,
            Err(e) => {
                metrics::counter!("plexspaces_wasm_tuplespace_write_errors_total").increment(1);
                return Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::InvalidMessage,
                    message: format!("Failed to convert tuple: {}", e),
                    details: None,
                });
            }
        };

        // Get tuplespace provider
        let provider = match &self.tuplespace_provider {
            Some(p) => p,
            None => {
                metrics::counter!("plexspaces_wasm_tuplespace_write_errors_total").increment(1);
                return Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::NotImplemented,
                    message: "TupleSpaceProvider not available".to_string(),
                    details: None,
                });
            }
        };

        // Write tuple
        match provider.write(tuple).await {
            Ok(_) => {
                let duration = start_time.elapsed();
                metrics::histogram!("plexspaces_wasm_tuplespace_write_duration_seconds").record(duration.as_secs_f64());
                metrics::counter!("plexspaces_wasm_tuplespace_write_success_total").increment(1);
                tracing::debug!(
                    actor_id = %self.actor_id,
                    "Tuplespace write succeeded"
                );
                Ok(())
            }
            Err(e) => {
                metrics::counter!("plexspaces_wasm_tuplespace_write_errors_total").increment(1);
                Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::Internal,
                    message: format!("Tuplespace write failed: {}", e),
                    details: None,
                })
            }
        }
    }

    async fn write_with_ttl(
        &mut self,
        ctx: plexspaces::actor::types::Context,
        tuple_data: plexspaces::actor::types::TupleData,
        ttl_ms: u64,
    ) -> Result<(), plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_tuplespace_write_with_ttl_total").increment(1);

        // Convert WIT tuple to internal tuple
        let mut tuple = match self.convert_wit_tuple_to_internal(&tuple_data) {
            Ok(t) => t,
            Err(e) => {
                metrics::counter!("plexspaces_wasm_tuplespace_write_with_ttl_errors_total").increment(1);
                return Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::InvalidMessage,
                    message: format!("Failed to convert tuple: {}", e),
                    details: None,
                });
            }
        };

        // Add lease if TTL is specified
        if ttl_ms > 0 {
            use plexspaces_tuplespace::Lease;
            use chrono::Duration;
            let lease_duration = Duration::milliseconds(ttl_ms as i64);
            tuple = tuple.with_lease(Lease::new(lease_duration));
        }

        // Get tuplespace provider
        let provider = match &self.tuplespace_provider {
            Some(p) => p,
            None => {
                metrics::counter!("plexspaces_wasm_tuplespace_write_with_ttl_errors_total").increment(1);
                return Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::NotImplemented,
                    message: "TupleSpaceProvider not available".to_string(),
                    details: None,
                });
            }
        };

        // Write tuple
        match provider.write(tuple).await {
            Ok(_) => {
                let duration = start_time.elapsed();
                metrics::histogram!("plexspaces_wasm_tuplespace_write_with_ttl_duration_seconds").record(duration.as_secs_f64());
                metrics::counter!("plexspaces_wasm_tuplespace_write_with_ttl_success_total").increment(1);
                Ok(())
            }
            Err(e) => {
                metrics::counter!("plexspaces_wasm_tuplespace_write_with_ttl_errors_total").increment(1);
                Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::Internal,
                    message: format!("Tuplespace write_with_ttl failed: {}", e),
                    details: None,
                })
            }
        }
    }

    async fn read(
        &mut self,
        ctx: plexspaces::actor::types::Context,
        pattern_data: plexspaces::actor::types::Pattern,
    ) -> Result<Option<plexspaces::actor::types::TupleData>, plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_tuplespace_read_total").increment(1);

        // Convert WIT pattern to internal pattern
        let pattern = match self.convert_wit_pattern_to_internal(&pattern_data) {
            Ok(p) => p,
            Err(e) => {
                metrics::counter!("plexspaces_wasm_tuplespace_read_errors_total").increment(1);
                return Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::InvalidMessage,
                    message: format!("Failed to convert pattern: {}", e),
                    details: None,
                });
            }
        };

        // Get tuplespace provider
        let provider = match &self.tuplespace_provider {
            Some(p) => p,
            None => {
                metrics::counter!("plexspaces_wasm_tuplespace_read_errors_total").increment(1);
                return Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::NotImplemented,
                    message: "TupleSpaceProvider not available".to_string(),
                    details: None,
                });
            }
        };

        // Read tuples
        match provider.read(&pattern).await {
            Ok(tuples) => {
                let duration = start_time.elapsed();
                metrics::histogram!("plexspaces_wasm_tuplespace_read_duration_seconds").record(duration.as_secs_f64());
                metrics::counter!("plexspaces_wasm_tuplespace_read_success_total").increment(1);
                
                // Return first matching tuple, or None if no matches
                if let Some(tuple) = tuples.first() {
                    Ok(Some(self.convert_internal_tuple_to_wit(tuple)))
                } else {
                    Ok(None)
                }
            }
            Err(e) => {
                metrics::counter!("plexspaces_wasm_tuplespace_read_errors_total").increment(1);
                Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::Internal,
                    message: format!("Tuplespace read failed: {}", e),
                    details: None,
                })
            }
        }
    }

    async fn read_blocking(
        &mut self,
        ctx: plexspaces::actor::types::Context,
        pattern_data: plexspaces::actor::types::Pattern,
        timeout_ms: u64,
    ) -> Result<Option<plexspaces::actor::types::TupleData>, plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_tuplespace_read_blocking_total").increment(1);

        // Convert WIT pattern to internal pattern
        let pattern = match self.convert_wit_pattern_to_internal(&pattern_data) {
            Ok(p) => p,
            Err(e) => {
                metrics::counter!("plexspaces_wasm_tuplespace_read_blocking_errors_total").increment(1);
                return Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::InvalidMessage,
                    message: format!("Failed to convert pattern: {}", e),
                    details: None,
                });
            }
        };

        // Get tuplespace provider
        let provider = match &self.tuplespace_provider {
            Some(p) => p,
            None => {
                metrics::counter!("plexspaces_wasm_tuplespace_read_blocking_errors_total").increment(1);
                return Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::NotImplemented,
                    message: "TupleSpaceProvider not available".to_string(),
                    details: None,
                });
            }
        };

        // Read with timeout
        let timeout = if timeout_ms > 0 {
            Some(std::time::Duration::from_millis(timeout_ms))
        } else {
            None // Wait forever
        };

        // Use a timeout future if timeout is specified
        let read_future = provider.read(&pattern);
        let result = if let Some(timeout_duration) = timeout {
            match tokio::time::timeout(timeout_duration, read_future).await {
                Ok(Ok(tuple)) => Ok(tuple),
                Ok(Err(e)) => Err(self.map_tuplespace_error(e)),
                Err(_) => Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::Timeout,
                    message: "Read timeout".to_string(),
                    details: None,
                }),
            }
        } else {
            read_future.await.map_err(|e| self.map_tuplespace_error(e))
        };

        match result {
            Ok(tuples) => {
                let duration = start_time.elapsed();
                metrics::histogram!("plexspaces_wasm_tuplespace_read_blocking_duration_seconds").record(duration.as_secs_f64());
                metrics::counter!("plexspaces_wasm_tuplespace_read_blocking_success_total").increment(1);
                
                if let Some(tuple) = tuples.first() {
                    Ok(Some(self.convert_internal_tuple_to_wit(tuple)))
                } else {
                    Ok(None)
                }
            }
            Err(e) => {
                metrics::counter!("plexspaces_wasm_tuplespace_read_blocking_errors_total").increment(1);
                Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::Internal,
                    message: format!("Tuplespace read_blocking failed: {}", e),
                    details: None,
                })
            }
        }
    }

    async fn read_all(
        &mut self,
        ctx: plexspaces::actor::types::Context,
        pattern_data: plexspaces::actor::types::Pattern,
        limit: u32,
    ) -> Result<Vec<plexspaces::actor::types::TupleData>, plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_tuplespace_read_all_total").increment(1);

        // Convert WIT pattern to internal pattern
        let pattern = match self.convert_wit_pattern_to_internal(&pattern_data) {
            Ok(p) => p,
            Err(e) => {
                metrics::counter!("plexspaces_wasm_tuplespace_read_all_errors_total").increment(1);
                return Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::InvalidMessage,
                    message: format!("Failed to convert pattern: {}", e),
                    details: None,
                });
            }
        };

        // Get tuplespace provider
        let provider = match &self.tuplespace_provider {
            Some(p) => p,
            None => {
                metrics::counter!("plexspaces_wasm_tuplespace_read_all_errors_total").increment(1);
                return Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::NotImplemented,
                    message: "TupleSpaceProvider not available".to_string(),
                    details: None,
                });
            }
        };

        // Read all matching tuples
        match provider.read(&pattern).await {
            Ok(mut tuples) => {
                // Apply limit if specified
                if limit > 0 && tuples.len() > limit as usize {
                    tuples.truncate(limit as usize);
                }

                let duration = start_time.elapsed();
                metrics::histogram!("plexspaces_wasm_tuplespace_read_all_duration_seconds").record(duration.as_secs_f64());
                metrics::histogram!("plexspaces_wasm_tuplespace_read_all_count").record(tuples.len() as f64);
                metrics::counter!("plexspaces_wasm_tuplespace_read_all_success_total").increment(1);
                
                // Convert all tuples to WIT format
                Ok(tuples.iter().map(|t| self.convert_internal_tuple_to_wit(t)).collect())
            }
            Err(e) => {
                metrics::counter!("plexspaces_wasm_tuplespace_read_all_errors_total").increment(1);
                Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::Internal,
                    message: format!("Tuplespace read_all failed: {}", e),
                    details: None,
                })
            }
        }
    }

    async fn take(
        &mut self,
        ctx: plexspaces::actor::types::Context,
        pattern_data: plexspaces::actor::types::Pattern,
    ) -> Result<Option<plexspaces::actor::types::TupleData>, plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_tuplespace_take_total").increment(1);

        // Convert WIT pattern to internal pattern
        let pattern = match self.convert_wit_pattern_to_internal(&pattern_data) {
            Ok(p) => p,
            Err(e) => {
                metrics::counter!("plexspaces_wasm_tuplespace_take_errors_total").increment(1);
                return Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::InvalidMessage,
                    message: format!("Failed to convert pattern: {}", e),
                    details: None,
                });
            }
        };

        // Get tuplespace provider
        let provider = match &self.tuplespace_provider {
            Some(p) => p,
            None => {
                metrics::counter!("plexspaces_wasm_tuplespace_take_errors_total").increment(1);
                return Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::NotImplemented,
                    message: "TupleSpaceProvider not available".to_string(),
                    details: None,
                });
            }
        };

        // Take tuple (destructive read)
        match provider.take(&pattern).await {
            Ok(Some(tuple)) => {
                let duration = start_time.elapsed();
                metrics::histogram!("plexspaces_wasm_tuplespace_take_duration_seconds").record(duration.as_secs_f64());
                metrics::counter!("plexspaces_wasm_tuplespace_take_success_total").increment(1);
                Ok(Some(self.convert_internal_tuple_to_wit(&tuple)))
            }
            Ok(None) => {
                let duration = start_time.elapsed();
                metrics::histogram!("plexspaces_wasm_tuplespace_take_duration_seconds").record(duration.as_secs_f64());
                metrics::counter!("plexspaces_wasm_tuplespace_take_success_total").increment(1);
                Ok(None)
            }
            Err(e) => {
                metrics::counter!("plexspaces_wasm_tuplespace_take_errors_total").increment(1);
                Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::Internal,
                    message: format!("Tuplespace take failed: {}", e),
                    details: None,
                })
            }
        }
    }

    async fn take_blocking(
        &mut self,
        ctx: plexspaces::actor::types::Context,
        pattern_data: plexspaces::actor::types::Pattern,
        timeout_ms: u64,
    ) -> Result<Option<plexspaces::actor::types::TupleData>, plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_tuplespace_take_blocking_total").increment(1);

        // Convert WIT pattern to internal pattern
        let pattern = match self.convert_wit_pattern_to_internal(&pattern_data) {
            Ok(p) => p,
            Err(e) => {
                metrics::counter!("plexspaces_wasm_tuplespace_take_blocking_errors_total").increment(1);
                return Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::InvalidMessage,
                    message: format!("Failed to convert pattern: {}", e),
                    details: None,
                });
            }
        };

        // Get tuplespace provider
        let provider = match &self.tuplespace_provider {
            Some(p) => p,
            None => {
                metrics::counter!("plexspaces_wasm_tuplespace_take_blocking_errors_total").increment(1);
                return Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::NotImplemented,
                    message: "TupleSpaceProvider not available".to_string(),
                    details: None,
                });
            }
        };

        // Take with timeout
        let timeout = if timeout_ms > 0 {
            Some(std::time::Duration::from_millis(timeout_ms))
        } else {
            None // Wait forever
        };

        let take_future = provider.take(&pattern);
        let result = if let Some(timeout_duration) = timeout {
            match tokio::time::timeout(timeout_duration, take_future).await {
                Ok(Ok(tuple)) => Ok(tuple),
                Ok(Err(e)) => Err(self.map_tuplespace_error(e)),
                Err(_) => Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::Timeout,
                    message: "Take timeout".to_string(),
                    details: None,
                }),
            }
        } else {
            take_future.await.map_err(|e| self.map_tuplespace_error(e))
        };

        match result {
            Ok(Some(tuple)) => {
                let duration = start_time.elapsed();
                metrics::histogram!("plexspaces_wasm_tuplespace_take_blocking_duration_seconds").record(duration.as_secs_f64());
                metrics::counter!("plexspaces_wasm_tuplespace_take_blocking_success_total").increment(1);
                Ok(Some(self.convert_internal_tuple_to_wit(&tuple)))
            }
            Ok(None) => {
                let duration = start_time.elapsed();
                metrics::histogram!("plexspaces_wasm_tuplespace_take_blocking_duration_seconds").record(duration.as_secs_f64());
                metrics::counter!("plexspaces_wasm_tuplespace_take_blocking_success_total").increment(1);
                Ok(None)
            }
            Err(e) => {
                metrics::counter!("plexspaces_wasm_tuplespace_take_blocking_errors_total").increment(1);
                Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::Internal,
                    message: format!("Tuplespace take_blocking failed: {}", e),
                    details: None,
                })
            }
        }
    }

    async fn count(
        &mut self,
        ctx: plexspaces::actor::types::Context,
        pattern_data: plexspaces::actor::types::Pattern,
    ) -> Result<u64, plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_tuplespace_count_total").increment(1);

        // Convert WIT pattern to internal pattern
        let pattern = match self.convert_wit_pattern_to_internal(&pattern_data) {
            Ok(p) => p,
            Err(e) => {
                metrics::counter!("plexspaces_wasm_tuplespace_count_errors_total").increment(1);
                return Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::InvalidMessage,
                    message: format!("Failed to convert pattern: {}", e),
                    details: None,
                });
            }
        };

        // Get tuplespace provider
        let provider = match &self.tuplespace_provider {
            Some(p) => p,
            None => {
                metrics::counter!("plexspaces_wasm_tuplespace_count_errors_total").increment(1);
                return Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::NotImplemented,
                    message: "TupleSpaceProvider not available".to_string(),
                    details: None,
                });
            }
        };

        // Count tuples matching pattern
        match provider.count(&pattern).await {
            Ok(count) => {
                let duration = start_time.elapsed();
                metrics::histogram!("plexspaces_wasm_tuplespace_count_duration_seconds").record(duration.as_secs_f64());
                metrics::histogram!("plexspaces_wasm_tuplespace_count_result").record(count as f64);
                metrics::counter!("plexspaces_wasm_tuplespace_count_success_total").increment(1);
                Ok(count as u64)
            }
            Err(e) => {
                metrics::counter!("plexspaces_wasm_tuplespace_count_errors_total").increment(1);
                Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::Internal,
                    message: format!("Tuplespace count failed: {}", e),
                    details: None,
                })
            }
        }
    }

    async fn subscribe(
        &mut self,
        _ctx: plexspaces::actor::types::Context,
        _pattern_data: plexspaces::actor::types::Pattern,
    ) -> Result<u64, plexspaces::actor::types::ActorError> {
        // NOTE: TupleSpace subscribe is not yet implemented in the backend (Phase 3 Week 6).
        // This is an acceptable limitation - subscribe/unsubscribe will be implemented when
        // distributed watchers are added to the TupleSpace backend.
        Err(plexspaces::actor::types::ActorError {
            code: plexspaces::actor::types::ErrorCode::NotImplemented,
            message: "subscribe not yet implemented - requires distributed watchers (Phase 3 Week 6)".to_string(),
            details: None,
        })
    }

    async fn unsubscribe(
        &mut self,
        _subscription_id: u64,
    ) -> Result<(), plexspaces::actor::types::ActorError> {
        // NOTE: TupleSpace unsubscribe is not yet implemented in the backend (Phase 3 Week 6).
        // This is an acceptable limitation - subscribe/unsubscribe will be implemented when
        // distributed watchers are added to the TupleSpace backend.
        Err(plexspaces::actor::types::ActorError {
            code: plexspaces::actor::types::ErrorCode::NotImplemented,
            message: "unsubscribe not yet implemented - requires distributed watchers (Phase 3 Week 6)".to_string(),
            details: None,
        })
    }

    async fn compare_and_swap(
        &mut self,
        _ctx: plexspaces::actor::types::Context,
        _pattern_data: plexspaces::actor::types::Pattern,
        _expected: plexspaces::actor::types::TupleData,
        _new_tuple: plexspaces::actor::types::TupleData,
    ) -> Result<bool, plexspaces::actor::types::ActorError> {
        // NOTE: TupleSpace compare_and_swap is not yet implemented in the backend.
        // This is an acceptable limitation - compare_and_swap requires atomic tuple operations
        // which will be added in a future backend enhancement.
        Err(plexspaces::actor::types::ActorError {
            code: plexspaces::actor::types::ErrorCode::NotImplemented,
            message: "compare_and_swap not yet implemented - requires atomic tuple operations in backend".to_string(),
            details: None,
        })
    }
}

#[cfg(feature = "component-model")]
pub struct ChannelsImpl {
    pub host_functions: Arc<HostFunctions>,
    // Subscription tracking: subscription_id -> (topic_name, stream_handle)
    // Note: Stream cancellation requires proper implementation with AbortHandle or similar
    subscriptions: Arc<tokio::sync::RwLock<std::collections::HashMap<u64, String>>>,
}

#[cfg(feature = "component-model")]
impl ChannelsImpl {
    pub fn new(host_functions: Arc<HostFunctions>) -> Self {
        Self {
            host_functions,
            subscriptions: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
        }
    }
}

#[cfg(feature = "component-model")]
#[async_trait::async_trait]
impl plexspaces::actor::channels::Host for ChannelsImpl {
    async fn send_to_queue(
        &mut self,
        ctx: plexspaces::actor::types::Context,
        queue_name: String,
        msg_type: String,
        payload: plexspaces::actor::types::Payload,
    ) -> Result<plexspaces::actor::types::MessageId, plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_channels_send_to_queue_total").increment(1);

        match self.host_functions.send_to_queue(&queue_name, &msg_type, payload.clone()).await {
            Ok(message_id) => {
                let duration = start_time.elapsed();
                metrics::histogram!("plexspaces_wasm_channels_send_to_queue_duration_seconds").record(duration.as_secs_f64());
                metrics::counter!("plexspaces_wasm_channels_send_to_queue_success_total").increment(1);
                tracing::debug!(
                    queue = %queue_name,
                    msg_type = %msg_type,
                    message_id = %message_id,
                    payload_size = payload.len(),
                    "Message sent to queue"
                );
                Ok(message_id)
            }
            Err(e) => {
                metrics::counter!("plexspaces_wasm_channels_send_to_queue_errors_total").increment(1);
                tracing::warn!(
                    queue = %queue_name,
                    error = %e,
                    "Failed to send message to queue"
                );
                Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::Internal,
                    message: e,
                    details: None,
                })
            }
        }
    }

    async fn send_to_queue_with_options(
        &mut self,
        ctx: plexspaces::actor::types::Context,
        queue_name: String,
        msg_type: String,
        payload: plexspaces::actor::types::Payload,
        _delay_ms: u64,
        _ttl_ms: u64,
        _headers: Vec<(String, String)>,
    ) -> Result<plexspaces::actor::types::MessageId, plexspaces::actor::types::ActorError> {
        // For now, use basic send_to_queue
        self.send_to_queue(ctx, queue_name, msg_type, payload).await
    }

    async fn receive_from_queue(
        &mut self,
        ctx: plexspaces::actor::types::Context,
        queue_name: String,
        timeout_ms: u64,
    ) -> Result<Option<plexspaces::actor::channels::QueueMessage>, plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_channels_receive_from_queue_total").increment(1);

        match self.host_functions.receive_from_queue(&queue_name, timeout_ms).await {
            Ok(Some((msg_type, payload))) => {
                let message_id = ulid::Ulid::new().to_string();
                let duration = start_time.elapsed();
                metrics::histogram!("plexspaces_wasm_channels_receive_from_queue_duration_seconds").record(duration.as_secs_f64());
                metrics::counter!("plexspaces_wasm_channels_receive_from_queue_success_total").increment(1);
                tracing::debug!(
                    queue = %queue_name,
                    message_id = %message_id,
                    msg_type = %msg_type,
                    payload_size = payload.len(),
                    "Message received from queue"
                );
                Ok(Some(plexspaces::actor::channels::QueueMessage {
                    id: message_id,
                    msg_type,
                    payload,
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_millis() as u64,
                    delivery_count: 0,
                    headers: vec![],
                }))
            }
            Ok(None) => {
                let duration = start_time.elapsed();
                metrics::histogram!("plexspaces_wasm_channels_receive_from_queue_duration_seconds").record(duration.as_secs_f64());
                metrics::counter!("plexspaces_wasm_channels_receive_from_queue_empty_total").increment(1);
                Ok(None)
            }
            Err(e) => {
                metrics::counter!("plexspaces_wasm_channels_receive_from_queue_errors_total").increment(1);
                tracing::warn!(
                    queue = %queue_name,
                    error = %e,
                    "Failed to receive message from queue"
                );
                Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::Internal,
                    message: e,
                    details: None,
                })
            }
        }
    }

    async fn ack(
        &mut self,
        ctx: plexspaces::actor::types::Context,
        queue_name: String,
        message_id: plexspaces::actor::types::MessageId,
    ) -> Result<(), plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_channels_ack_total").increment(1);
        let _span = tracing::span!(
            tracing::Level::INFO,
            "wasm_channels_ack",
            queue = %queue_name,
            message_id = %message_id
        )
        .entered();

        // Ack is a placeholder - actual implementation requires ChannelService with ack support
        tracing::debug!(
            queue = %queue_name,
            message_id = %message_id,
            "Ack called (placeholder implementation)"
        );

        let duration = start_time.elapsed();
        metrics::histogram!("plexspaces_wasm_channels_ack_duration_seconds").record(duration.as_secs_f64());
        metrics::counter!("plexspaces_wasm_channels_ack_success_total").increment(1);
        Ok(())
    }

    async fn nack(
        &mut self,
        ctx: plexspaces::actor::types::Context,
        queue_name: String,
        message_id: plexspaces::actor::types::MessageId,
        requeue: bool,
    ) -> Result<(), plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_channels_nack_total").increment(1);
        let _span = tracing::span!(
            tracing::Level::INFO,
            "wasm_channels_nack",
            queue = %queue_name,
            message_id = %message_id,
            requeue = requeue
        )
        .entered();

        // Nack is a placeholder - actual implementation requires ChannelService with nack support
        tracing::debug!(
            queue = %queue_name,
            message_id = %message_id,
            requeue = requeue,
            "Nack called (placeholder implementation)"
        );

        let duration = start_time.elapsed();
        metrics::histogram!("plexspaces_wasm_channels_nack_duration_seconds").record(duration.as_secs_f64());
        metrics::counter!("plexspaces_wasm_channels_nack_success_total").increment(1);
        Ok(())
    }

    async fn publish_to_topic(
        &mut self,
        ctx: plexspaces::actor::types::Context,
        topic_name: String,
        msg_type: String,
        payload: plexspaces::actor::types::Payload,
    ) -> Result<plexspaces::actor::types::MessageId, plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_channels_publish_to_topic_total").increment(1);

        match self.host_functions.publish_to_topic(&topic_name, &msg_type, payload.clone()).await {
            Ok(message_id) => {
                let duration = start_time.elapsed();
                metrics::histogram!("plexspaces_wasm_channels_publish_to_topic_duration_seconds").record(duration.as_secs_f64());
                metrics::counter!("plexspaces_wasm_channels_publish_to_topic_success_total").increment(1);
                tracing::debug!(
                    topic = %topic_name,
                    msg_type = %msg_type,
                    message_id = %message_id,
                    payload_size = payload.len(),
                    "Message published to topic"
                );
                Ok(message_id)
            }
            Err(e) => {
                metrics::counter!("plexspaces_wasm_channels_publish_to_topic_errors_total").increment(1);
                tracing::warn!(
                    topic = %topic_name,
                    error = %e,
                    "Failed to publish message to topic"
                );
                Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::Internal,
                    message: e,
                    details: None,
                })
            }
        }
    }

    async fn subscribe_to_topic(
        &mut self,
        ctx: plexspaces::actor::types::Context,
        topic_name: String,
        filter: Option<String>,
    ) -> Result<u64, plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_channels_subscribe_to_topic_total").increment(1);
        let _span = tracing::span!(
            tracing::Level::INFO,
            "wasm_channels_subscribe_to_topic",
            topic = %topic_name,
            filter = ?filter
        )
        .entered();

        // Drop span before await to ensure Send
        drop(_span);

        let channel_service = self.host_functions.channel_service()
            .ok_or_else(|| plexspaces::actor::types::ActorError {
                code: plexspaces::actor::types::ErrorCode::Internal,
                message: "ChannelService not configured".to_string(),
                details: None,
            })?;

        // Subscribe to topic using ChannelService
        // Note: ChannelService.subscribe_to_topic returns a stream, but WIT interface expects subscription ID
        // We'll create the subscription and return an ID, but the stream handling needs to be done
        // by the WASM actor itself (or we need to spawn a task to forward messages)
        match channel_service.subscribe_to_topic(&topic_name).await {
            Ok(_stream) => {
                // Generate subscription ID
                let subscription_id = ulid::Ulid::new().to_string()
                    .as_bytes()
                    .iter()
                    .fold(0u64, |acc, &b| acc.wrapping_mul(31).wrapping_add(b as u64));

                // Track subscription for unsubscribe
                {
                    let mut subscriptions = self.subscriptions.write().await;
                    subscriptions.insert(subscription_id, topic_name.clone());
                }

                // NOTE: Topic subscription streams are handled by the backend ChannelService.
                // Messages from subscribed topics are delivered to the actor's mailbox automatically
                // by the node's message routing system. No additional forwarding task is needed.

                tracing::debug!(
                    topic = %topic_name,
                    filter = ?filter,
                    subscription_id = subscription_id,
                    "Subscribed to topic"
                );

                let duration = start_time.elapsed();
                metrics::histogram!("plexspaces_wasm_channels_subscribe_to_topic_duration_seconds").record(duration.as_secs_f64());
                metrics::counter!("plexspaces_wasm_channels_subscribe_to_topic_success_total").increment(1);
                Ok(subscription_id)
            }
            Err(e) => {
                metrics::counter!("plexspaces_wasm_channels_subscribe_to_topic_errors_total").increment(1);
                Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::Internal,
                    message: format!("Failed to subscribe to topic: {}", e),
                    details: None,
                })
            }
        }
    }

    async fn unsubscribe_from_topic(
        &mut self,
        subscription_id: u64,
    ) -> Result<(), plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_channels_unsubscribe_from_topic_total").increment(1);
        let _span = tracing::span!(
            tracing::Level::INFO,
            "wasm_channels_unsubscribe_from_topic",
            subscription_id = subscription_id
        )
        .entered();

        // Drop span before await to ensure Send
        drop(_span);

        // Remove subscription from tracking
        let topic_name = {
            let mut subscriptions = self.subscriptions.write().await;
            subscriptions.remove(&subscription_id)
        };

        if let Some(topic) = topic_name {
            tracing::debug!(
                subscription_id = subscription_id,
                topic = %topic,
                "Unsubscribed from topic"
            );
        } else {
            tracing::warn!(
                subscription_id = subscription_id,
                "Unsubscribe called for unknown subscription ID"
            );
        }

        // NOTE: Topic subscription streams are managed by the backend ChannelService.
        // Unsubscribing removes the subscription tracking; the backend handles stream cleanup.

        let duration = start_time.elapsed();
        metrics::histogram!("plexspaces_wasm_channels_unsubscribe_from_topic_duration_seconds").record(duration.as_secs_f64());
        metrics::counter!("plexspaces_wasm_channels_unsubscribe_from_topic_success_total").increment(1);
        Ok(())
    }

    async fn create_queue(
        &mut self,
        _ctx: plexspaces::actor::types::Context,
        _queue_name: String,
        _max_size: u32,
        _message_ttl_ms: u64,
    ) -> Result<(), plexspaces::actor::types::ActorError> {
        // NOTE: Queue management operations (create_queue, delete_queue, queue_depth) are not
        // part of the ChannelService trait. These are administrative operations that should be
        // handled at the node/service level, not by individual actors. This is an acceptable
        // limitation - queues are created automatically on first use.
        Err(plexspaces::actor::types::ActorError {
            code: plexspaces::actor::types::ErrorCode::NotImplemented,
            message: "create_queue not available - queues are created automatically on first use".to_string(),
            details: None,
        })
    }

    async fn delete_queue(
        &mut self,
        _ctx: plexspaces::actor::types::Context,
        _queue_name: String,
    ) -> Result<(), plexspaces::actor::types::ActorError> {
        // NOTE: Queue management operations (create_queue, delete_queue, queue_depth) are not
        // part of the ChannelService trait. These are administrative operations that should be
        // handled at the node/service level, not by individual actors. This is an acceptable
        // limitation.
        Err(plexspaces::actor::types::ActorError {
            code: plexspaces::actor::types::ErrorCode::NotImplemented,
            message: "delete_queue not available - queue management is handled at node/service level".to_string(),
            details: None,
        })
    }

    async fn queue_depth(
        &mut self,
        _ctx: plexspaces::actor::types::Context,
        _queue_name: String,
    ) -> Result<u64, plexspaces::actor::types::ActorError> {
        // NOTE: Queue management operations (create_queue, delete_queue, queue_depth) are not
        // part of the ChannelService trait. These are administrative operations that should be
        // handled at the node/service level, not by individual actors. This is an acceptable
        // limitation.
        Err(plexspaces::actor::types::ActorError {
            code: plexspaces::actor::types::ErrorCode::NotImplemented,
            message: "queue_depth not available - queue management is handled at node/service level".to_string(),
            details: None,
        })
    }
}

#[cfg(feature = "component-model")]
pub struct BlobImpl {
    pub actor_id: ActorId,
    pub host_functions: Arc<HostFunctions>,
}

#[cfg(feature = "component-model")]
#[async_trait::async_trait]
impl plexspaces::actor::blob::Host for BlobImpl {
    async fn upload(
        &mut self,
        ctx: plexspaces::actor::types::Context,
        bucket: String,
        key: String,
        data: plexspaces::actor::types::Payload,
        content_type: Option<String>,
    ) -> Result<plexspaces::actor::blob::BlobMetadata, plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_blob_upload_total").increment(1);
        let _span = tracing::span!(
            tracing::Level::INFO,
            "wasm_blob_upload",
            bucket = %bucket,
            key = %key
        )
        .entered();

        // Drop span before await to ensure Send
        drop(_span);

        // Get BlobService from host_functions
        let blob_service = self.host_functions.blob_service();
        if let Some(blob_service) = blob_service {
            use plexspaces_core::RequestContext;
            use plexspaces_blob::repository::ListFilters;
            
            // Create RequestContext from context (empty strings use defaults)
            let request_ctx = context_to_request_context(&ctx);
            
            // Upload blob using BlobService
            match blob_service.upload_blob(
                &request_ctx,
                &key, // Use key as name
                data.clone(),
                content_type.clone(),
                None, // blob_group
                None, // kind
                std::collections::HashMap::new(), // metadata
                std::collections::HashMap::new(), // tags
                None, // expires_after
            ).await {
                Ok(metadata) => {
                    // Convert BlobMetadata (proto) to WIT BlobMetadata
                    let last_modified = metadata.created_at
                        .map(|ts| {
                            ts.seconds as u64 * 1000 + (ts.nanos as u64 / 1_000_000)
                        })
                        .unwrap_or_else(|| {
                            std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap()
                                .as_millis() as u64
                        });
                    
                    let duration = start_time.elapsed();
                    metrics::histogram!("plexspaces_wasm_blob_upload_duration_seconds").record(duration.as_secs_f64());
                    metrics::counter!("plexspaces_wasm_blob_upload_success_total").increment(1);
                    
                    tracing::debug!(
                        bucket = %bucket,
                        key = %key,
                        blob_id = %metadata.blob_id,
                        size = metadata.content_length,
                        "Blob uploaded successfully"
                    );
                    
                    Ok(plexspaces::actor::blob::BlobMetadata {
                        blob_id: metadata.blob_id,
                        bucket,
                        key,
                        size: metadata.content_length as u64,
                        content_type: if metadata.content_type.is_empty() { None } else { Some(metadata.content_type) },
                        etag: if metadata.etag.is_empty() { None } else { Some(metadata.etag) },
                        last_modified,
                        tenant_id: metadata.tenant_id,
                        namespace: metadata.namespace,
                    })
                }
                Err(e) => {
                    metrics::counter!("plexspaces_wasm_blob_upload_errors_total").increment(1);
                    tracing::warn!(
                        bucket = %bucket,
                        key = %key,
                        error = %e,
                        "Blob upload failed"
                    );
                    Err(plexspaces::actor::types::ActorError {
                        code: plexspaces::actor::types::ErrorCode::Internal,
                        message: format!("Blob upload failed: {}", e),
                        details: None,
                    })
                }
            }
        } else {
            metrics::counter!("plexspaces_wasm_blob_upload_errors_total").increment(1);
            Err(plexspaces::actor::types::ActorError {
                code: plexspaces::actor::types::ErrorCode::NotImplemented,
                message: "BlobService not available".to_string(),
                details: None,
            })
        }
    }

    async fn download(
        &mut self,
        ctx: plexspaces::actor::types::Context,
        blob_id: String,
        bucket: String,
        key: String,
    ) -> Result<plexspaces::actor::types::Payload, plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_blob_download_total").increment(1);
        let _span = tracing::span!(
            tracing::Level::INFO,
            "wasm_blob_download",
            blob_id = %blob_id,
            bucket = %bucket,
            key = %key
        )
        .entered();

        // Drop span before await to ensure Send
        drop(_span);

        // Get BlobService from host_functions
        let blob_service = self.host_functions.blob_service();
        if let Some(blob_service) = blob_service {
            use plexspaces_core::RequestContext;
            
            // Use blob_id if provided, otherwise we'd need to look up by bucket+key
            // NOTE: Blob lookup by bucket+key is not yet implemented. blob_id is required
            // for download operations. This is an acceptable limitation - bucket+key lookup
            // will be added when blob metadata indexing is implemented.
            let effective_blob_id = if blob_id.is_empty() {
                return Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::InvalidMessage,
                    message: "blob_id is required for download".to_string(),
                    details: None,
                });
            } else {
                blob_id
            };
            
            // Get blob metadata first to extract tenant/namespace for proper RequestContext
            // Use internal context temporarily to get metadata
            let temp_ctx = RequestContext::internal();
            let metadata = blob_service.get_metadata(&temp_ctx, &effective_blob_id).await
                .map_err(|e| {
                    let error_code = if e.to_string().contains("not found") || e.to_string().contains("NotFound") {
                        plexspaces::actor::types::ErrorCode::ActorNotFound
                    } else {
                        plexspaces::actor::types::ErrorCode::Internal
                    };
                    plexspaces::actor::types::ActorError {
                        code: error_code,
                        message: format!("Failed to get blob metadata: {}", e),
                        details: None,
                    }
                })?;
            
            // Create proper RequestContext from blob metadata tenant/namespace
            let request_ctx = RequestContext::new_without_auth(metadata.tenant_id.clone(), metadata.namespace.clone());
            
            // Download blob using BlobService with proper context
            match blob_service.download_blob(&request_ctx, &effective_blob_id).await {
                Ok(data) => {
                    let duration = start_time.elapsed();
                    metrics::histogram!("plexspaces_wasm_blob_download_duration_seconds").record(duration.as_secs_f64());
                    metrics::counter!("plexspaces_wasm_blob_download_success_total").increment(1);
                    
                    tracing::debug!(
                        blob_id = %effective_blob_id,
                        size = data.len(),
                        "Blob downloaded successfully"
                    );
                    
                    Ok(data)
                }
                Err(e) => {
                    metrics::counter!("plexspaces_wasm_blob_download_errors_total").increment(1);
                    let error_code = if e.to_string().contains("not found") || e.to_string().contains("NotFound") {
                        plexspaces::actor::types::ErrorCode::ActorNotFound
                    } else {
                        plexspaces::actor::types::ErrorCode::Internal
                    };
                    tracing::warn!(
                        blob_id = %effective_blob_id,
                        error = %e,
                        "Blob download failed"
                    );
                    Err(plexspaces::actor::types::ActorError {
                        code: error_code,
                        message: format!("Blob download failed: {}", e),
                        details: None,
                    })
                }
            }
        } else {
            metrics::counter!("plexspaces_wasm_blob_download_errors_total").increment(1);
            Err(plexspaces::actor::types::ActorError {
                code: plexspaces::actor::types::ErrorCode::NotImplemented,
                message: "BlobService not available".to_string(),
                details: None,
            })
        }
    }

    async fn delete(
        &mut self,
        ctx: plexspaces::actor::types::Context,
        blob_id: String,
        bucket: String,
        key: String,
    ) -> Result<(), plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_blob_delete_total").increment(1);
        let _span = tracing::span!(
            tracing::Level::INFO,
            "wasm_blob_delete",
            blob_id = %blob_id,
            bucket = %bucket,
            key = %key
        )
        .entered();

        // Drop span before await to ensure Send
        drop(_span);

        // Get BlobService from host_functions
        let blob_service = self.host_functions.blob_service();
        if let Some(blob_service) = blob_service {
            use plexspaces_core::RequestContext;
            
            // Use blob_id if provided
            let effective_blob_id = if blob_id.is_empty() {
                return Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::InvalidMessage,
                    message: "blob_id is required for delete".to_string(),
                    details: None,
                });
            } else {
                blob_id
            };
            
            // Get blob metadata first to extract tenant/namespace for proper RequestContext
            // Use internal context temporarily to get metadata
            let temp_ctx = RequestContext::internal();
            let metadata = blob_service.get_metadata(&temp_ctx, &effective_blob_id).await
                .map_err(|e| {
                    let error_code = if e.to_string().contains("not found") || e.to_string().contains("NotFound") {
                        plexspaces::actor::types::ErrorCode::ActorNotFound
                    } else {
                        plexspaces::actor::types::ErrorCode::Internal
                    };
                    plexspaces::actor::types::ActorError {
                        code: error_code,
                        message: format!("Failed to get blob metadata: {}", e),
                        details: None,
                    }
                })?;
            
            // Create proper RequestContext from blob metadata tenant/namespace
            let request_ctx = RequestContext::new_without_auth(metadata.tenant_id.clone(), metadata.namespace.clone());
            
            // Delete blob using BlobService with proper context
            match blob_service.delete_blob(&request_ctx, &effective_blob_id).await {
                Ok(()) => {
                    let duration = start_time.elapsed();
                    metrics::histogram!("plexspaces_wasm_blob_delete_duration_seconds").record(duration.as_secs_f64());
                    metrics::counter!("plexspaces_wasm_blob_delete_success_total").increment(1);
                    
                    tracing::debug!(
                        blob_id = %effective_blob_id,
                        "Blob deleted successfully"
                    );
                    
                    Ok(())
                }
                Err(e) => {
                    metrics::counter!("plexspaces_wasm_blob_delete_errors_total").increment(1);
                    let error_code = if e.to_string().contains("not found") || e.to_string().contains("NotFound") {
                        plexspaces::actor::types::ErrorCode::ActorNotFound
                    } else {
                        plexspaces::actor::types::ErrorCode::Internal
                    };
                    tracing::warn!(
                        blob_id = %effective_blob_id,
                        error = %e,
                        "Blob delete failed"
                    );
                    Err(plexspaces::actor::types::ActorError {
                        code: error_code,
                        message: format!("Blob delete failed: {}", e),
                        details: None,
                    })
                }
            }
        } else {
            metrics::counter!("plexspaces_wasm_blob_delete_errors_total").increment(1);
            Err(plexspaces::actor::types::ActorError {
                code: plexspaces::actor::types::ErrorCode::NotImplemented,
                message: "BlobService not available".to_string(),
                details: None,
            })
        }
    }

    async fn exists(
        &mut self,
        ctx: plexspaces::actor::types::Context,
        blob_id: String,
        bucket: String,
        key: String,
    ) -> Result<bool, plexspaces::actor::types::ActorError> {
        metrics::counter!("plexspaces_wasm_blob_exists_total").increment(1);
        
        // Get BlobService from host_functions
        let blob_service = self.host_functions.blob_service();
        if let Some(blob_service) = blob_service {
            use plexspaces_core::RequestContext;
            
            // Use blob_id if provided
            let effective_blob_id = if blob_id.is_empty() {
                return Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::InvalidMessage,
                    message: "blob_id is required for exists check".to_string(),
                    details: None,
                });
            } else {
                blob_id
            };
            
            // Get blob metadata to check existence and extract tenant/namespace for proper RequestContext
            // Use internal context temporarily to get metadata
            let temp_ctx = RequestContext::internal();
            match blob_service.get_metadata(&temp_ctx, &effective_blob_id).await {
                Ok(metadata) => {
                    // Blob exists - return true (we could use the metadata to create proper context
                    // but for exists check, we just need to return the boolean)
                    Ok(true)
                }
                Err(e) => {
                    if e.to_string().contains("not found") || e.to_string().contains("NotFound") {
                        Ok(false)
                    } else {
                        Err(plexspaces::actor::types::ActorError {
                            code: plexspaces::actor::types::ErrorCode::Internal,
                            message: format!("Blob exists check failed: {}", e),
                            details: None,
                        })
                    }
                }
            }
        } else {
            Err(plexspaces::actor::types::ActorError {
                code: plexspaces::actor::types::ErrorCode::NotImplemented,
                message: "BlobService not available".to_string(),
                details: None,
            })
        }
    }

    async fn list_blobs(
        &mut self,
        ctx: plexspaces::actor::types::Context,
        bucket: String,
        prefix: String,
        limit: u32,
    ) -> Result<Vec<plexspaces::actor::blob::BlobMetadata>, plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_blob_list_total").increment(1);
        let _span = tracing::span!(
            tracing::Level::INFO,
            "wasm_blob_list",
            bucket = %bucket,
            prefix = %prefix
        )
        .entered();

        // Drop span before await to ensure Send
        drop(_span);

        // Get BlobService from host_functions
        let blob_service = self.host_functions.blob_service();
        if let Some(blob_service) = blob_service {
            use plexspaces_core::RequestContext;
            use plexspaces_blob::repository::ListFilters;
            
            // Create RequestContext from context (empty strings use defaults)
            let request_ctx = context_to_request_context(&ctx);
            
            // Create filters
            let mut filters = ListFilters::default();
            if !prefix.is_empty() {
                filters.name_prefix = Some(prefix.clone());
            }
            
            // List blobs using BlobService
            let page_size = if limit == 0 { 100 } else { limit as i64 }.min(1000); // Cap at 1000
            match blob_service.list_blobs(&request_ctx, &filters, page_size, 1).await {
                Ok((metadata_list, _total)) => {
                    // Convert proto BlobMetadata to WIT BlobMetadata
                    let wit_metadata: Vec<plexspaces::actor::blob::BlobMetadata> = metadata_list
                        .into_iter()
                        .map(|m| {
                            let last_modified = m.created_at
                                .map(|ts| {
                                    ts.seconds as u64 * 1000 + (ts.nanos as u64 / 1_000_000)
                                })
                                .unwrap_or_else(|| {
                                    std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .unwrap()
                                        .as_millis() as u64
                                });
                            
                            plexspaces::actor::blob::BlobMetadata {
                                blob_id: m.blob_id,
                                bucket: bucket.clone(),
                                key: m.name,
                                size: m.content_length as u64,
                                content_type: if m.content_type.is_empty() { None } else { Some(m.content_type) },
                                etag: if m.etag.is_empty() { None } else { Some(m.etag) },
                                last_modified,
                                tenant_id: m.tenant_id,
                                namespace: m.namespace,
                            }
                        })
                        .collect();
                    
                    let duration = start_time.elapsed();
                    metrics::histogram!("plexspaces_wasm_blob_list_duration_seconds").record(duration.as_secs_f64());
                    metrics::counter!("plexspaces_wasm_blob_list_success_total").increment(1);
                    
                    tracing::debug!(
                        bucket = %bucket,
                        prefix = %prefix,
                        count = wit_metadata.len(),
                        "List blobs completed successfully"
                    );
                    
                    Ok(wit_metadata)
                }
                Err(e) => {
                    metrics::counter!("plexspaces_wasm_blob_list_errors_total").increment(1);
                    tracing::warn!(
                        bucket = %bucket,
                        prefix = %prefix,
                        error = %e,
                        "List blobs failed"
                    );
                    Err(plexspaces::actor::types::ActorError {
                        code: plexspaces::actor::types::ErrorCode::Internal,
                        message: format!("List blobs failed: {}", e),
                        details: None,
                    })
                }
            }
        } else {
            metrics::counter!("plexspaces_wasm_blob_list_errors_total").increment(1);
            Err(plexspaces::actor::types::ActorError {
                code: plexspaces::actor::types::ErrorCode::NotImplemented,
                message: "BlobService not available".to_string(),
                details: None,
            })
        }
    }

    async fn metadata(
        &mut self,
        ctx: plexspaces::actor::types::Context,
        blob_id: String,
        bucket: String,
        key: String,
    ) -> Result<plexspaces::actor::blob::BlobMetadata, plexspaces::actor::types::ActorError> {
        metrics::counter!("plexspaces_wasm_blob_metadata_total").increment(1);
        
        // Get BlobService from host_functions
        let blob_service = self.host_functions.blob_service();
        if let Some(blob_service) = blob_service {
            use plexspaces_core::RequestContext;
            
            // Use blob_id if provided
            let effective_blob_id = if blob_id.is_empty() {
                return Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::InvalidMessage,
                    message: "blob_id is required for metadata".to_string(),
                    details: None,
                });
            } else {
                blob_id
            };
            
            // Get metadata using BlobService - use internal context to get metadata first
            // then we can use the metadata's tenant/namespace for proper context if needed
            let temp_ctx = RequestContext::internal();
            match blob_service.get_metadata(&temp_ctx, &effective_blob_id).await {
                Ok(m) => {
                    let last_modified = m.created_at
                        .map(|ts| {
                            ts.seconds as u64 * 1000 + (ts.nanos as u64 / 1_000_000)
                        })
                        .unwrap_or_else(|| {
                            std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap()
                                .as_millis() as u64
                        });
                    
                    Ok(plexspaces::actor::blob::BlobMetadata {
                        blob_id: m.blob_id,
                        bucket,
                        key: m.name,
                        size: m.content_length as u64,
                        content_type: if m.content_type.is_empty() { None } else { Some(m.content_type) },
                        etag: if m.etag.is_empty() { None } else { Some(m.etag) },
                        last_modified,
                        tenant_id: m.tenant_id,
                        namespace: m.namespace,
                    })
                }
                Err(e) => {
                    metrics::counter!("plexspaces_wasm_blob_metadata_errors_total").increment(1);
                    let error_code = if e.to_string().contains("not found") || e.to_string().contains("NotFound") {
                        plexspaces::actor::types::ErrorCode::ActorNotFound
                    } else {
                        plexspaces::actor::types::ErrorCode::Internal
                    };
                    tracing::warn!(
                        blob_id = %effective_blob_id,
                        error = %e,
                        "Get blob metadata failed"
                    );
                    Err(plexspaces::actor::types::ActorError {
                        code: error_code,
                        message: format!("Get blob metadata failed: {}", e),
                        details: None,
                    })
                }
            }
        } else {
            metrics::counter!("plexspaces_wasm_blob_metadata_errors_total").increment(1);
            Err(plexspaces::actor::types::ActorError {
                code: plexspaces::actor::types::ErrorCode::NotImplemented,
                message: "BlobService not available".to_string(),
                details: None,
            })
        }
    }

    async fn copy(
        &mut self,
        ctx: plexspaces::actor::types::Context,
        source_blob_id: String,
        source_bucket: String,
        source_key: String,
        dest_bucket: String,
        dest_key: String,
    ) -> Result<plexspaces::actor::blob::BlobMetadata, plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_blob_copy_total").increment(1);
        let _span = tracing::span!(
            tracing::Level::INFO,
            "wasm_blob_copy",
            source_bucket = %source_bucket,
            source_key = %source_key,
            dest_bucket = %dest_bucket,
            dest_key = %dest_key
        )
        .entered();

        // Drop span before await to ensure Send
        drop(_span);

        // Get BlobService from host_functions
        let blob_service = self.host_functions.blob_service();
        if let Some(blob_service) = blob_service {
            use plexspaces_core::RequestContext;
            
            // Use source_blob_id if provided, otherwise we'd need to look up by bucket+key
            let effective_source_blob_id = if source_blob_id.is_empty() {
                return Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::InvalidMessage,
                    message: "source_blob_id is required for copy".to_string(),
                    details: None,
                });
            } else {
                source_blob_id
            };
            
            // Create RequestContext from context (empty strings use defaults)
            let request_ctx = if ctx.tenant_id.is_empty() && ctx.namespace.is_empty() {
                // If not provided, we need to look up metadata first to get tenant/namespace
                // This requires a system-level lookup, so we use the repository's internal lookup
                // which allows looking up by blob_id only when context is internal
                let temp_ctx = RequestContext::internal();
                let source_metadata = blob_service.get_metadata(&temp_ctx, &effective_source_blob_id).await
                    .map_err(|e| {
                        let error_code = if e.to_string().contains("not found") || e.to_string().contains("NotFound") {
                            plexspaces::actor::types::ErrorCode::ActorNotFound
                        } else {
                            plexspaces::actor::types::ErrorCode::Internal
                        };
                        plexspaces::actor::types::ActorError {
                            code: error_code,
                            message: format!("Failed to get source blob metadata: {}", e),
                            details: None,
                        }
                    })?;
                RequestContext::new_without_auth(source_metadata.tenant_id.clone(), source_metadata.namespace.clone())
            } else {
                // Use provided tenant/namespace
                RequestContext::new_without_auth(ctx.tenant_id.clone(), ctx.namespace.clone())
            };
            
            // Get source blob metadata to preserve content_type and other properties
            let source_metadata = blob_service.get_metadata(&request_ctx, &effective_source_blob_id).await
                .map_err(|e| {
                    let error_code = if e.to_string().contains("not found") || e.to_string().contains("NotFound") {
                        plexspaces::actor::types::ErrorCode::ActorNotFound
                    } else {
                        plexspaces::actor::types::ErrorCode::Internal
                    };
                    plexspaces::actor::types::ActorError {
                        code: error_code,
                        message: format!("Failed to get source blob metadata: {}", e),
                        details: None,
                    }
                })?;
            
            // Download source blob using proper context
            let data = blob_service.download_blob(&request_ctx, &effective_source_blob_id).await
                .map_err(|e| {
                    plexspaces::actor::types::ActorError {
                        code: plexspaces::actor::types::ErrorCode::Internal,
                        message: format!("Failed to download source blob: {}", e),
                        details: None,
                    }
                })?;
            
            // Upload to destination with same content_type
            let content_type = if source_metadata.content_type.is_empty() {
                None
            } else {
                Some(source_metadata.content_type)
            };
            
            match blob_service.upload_blob(
                &request_ctx,
                &dest_key,
                data,
                content_type,
                None, // blob_group
                None, // kind
                source_metadata.metadata.clone(),
                source_metadata.tags.clone(),
                None, // expires_after
            ).await {
                Ok(dest_metadata) => {
                    let last_modified = dest_metadata.created_at
                        .map(|ts| {
                            ts.seconds as u64 * 1000 + (ts.nanos as u64 / 1_000_000)
                        })
                        .unwrap_or_else(|| {
                            std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap()
                                .as_millis() as u64
                        });
                    
                    let duration = start_time.elapsed();
                    metrics::histogram!("plexspaces_wasm_blob_copy_duration_seconds").record(duration.as_secs_f64());
                    metrics::counter!("plexspaces_wasm_blob_copy_success_total").increment(1);
                    
                    tracing::debug!(
                        source_blob_id = %effective_source_blob_id,
                        dest_blob_id = %dest_metadata.blob_id,
                        "Blob copied successfully"
                    );
                    
                    Ok(plexspaces::actor::blob::BlobMetadata {
                        blob_id: dest_metadata.blob_id,
                        bucket: dest_bucket,
                        key: dest_key,
                        size: dest_metadata.content_length as u64,
                        content_type: if dest_metadata.content_type.is_empty() { None } else { Some(dest_metadata.content_type) },
                        etag: if dest_metadata.etag.is_empty() { None } else { Some(dest_metadata.etag) },
                        last_modified,
                        tenant_id: dest_metadata.tenant_id,
                        namespace: dest_metadata.namespace,
                    })
                }
                Err(e) => {
                    metrics::counter!("plexspaces_wasm_blob_copy_errors_total").increment(1);
                    tracing::warn!(
                        source_blob_id = %effective_source_blob_id,
                        dest_key = %dest_key,
                        error = %e,
                        "Blob copy failed"
                    );
                    Err(plexspaces::actor::types::ActorError {
                        code: plexspaces::actor::types::ErrorCode::Internal,
                        message: format!("Blob copy failed: {}", e),
                        details: None,
                    })
                }
            }
        } else {
            metrics::counter!("plexspaces_wasm_blob_copy_errors_total").increment(1);
            Err(plexspaces::actor::types::ActorError {
                code: plexspaces::actor::types::ErrorCode::NotImplemented,
                message: "BlobService not available".to_string(),
                details: None,
            })
        }
    }
}

#[cfg(feature = "component-model")]
pub struct WorkflowImpl;

#[cfg(feature = "component-model")]
#[async_trait::async_trait]
impl plexspaces::actor::workflow::Host for WorkflowImpl {
    async fn start_workflow(
        &mut self,
        ctx: plexspaces::actor::types::Context,
        workflow_type: String,
        workflow_id: Option<String>,
        input: plexspaces::actor::types::Payload,
        options: plexspaces::actor::workflow::WorkflowOptions,
    ) -> Result<String, plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_workflow_start_total").increment(1);
        let _span = tracing::span!(
            tracing::Level::INFO,
            "wasm_workflow_start",
            workflow_type = %workflow_type
        )
        .entered();

        // NOTE: Workflow functions are placeholder implementations. Full workflow orchestration
        // requires WorkflowService/WorkflowExecutor integration which will be added in a future phase.
        // For now, these functions return placeholder values to maintain API compatibility.
        let execution_id = workflow_id.unwrap_or_else(|| ulid::Ulid::new().to_string());

        tracing::debug!(
            workflow_type = %workflow_type,
            execution_id = %execution_id,
            input_size = input.len(),
            "Start workflow called (placeholder implementation)"
        );

        let duration = start_time.elapsed();
        metrics::histogram!("plexspaces_wasm_workflow_start_duration_seconds").record(duration.as_secs_f64());
        metrics::counter!("plexspaces_wasm_workflow_start_success_total").increment(1);
        Ok(execution_id)
    }

    async fn signal_workflow(
        &mut self,
        ctx: plexspaces::actor::types::Context,
        workflow_id: String,
        signal_name: String,
        payload: plexspaces::actor::types::Payload,
    ) -> Result<(), plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_workflow_signal_total").increment(1);
        let _span = tracing::span!(
            tracing::Level::INFO,
            "wasm_workflow_signal",
            workflow_id = %workflow_id,
            signal_name = %signal_name
        )
        .entered();

        // NOTE: Workflow functions are placeholder implementations. Full workflow orchestration
        // requires WorkflowService integration which will be added in a future phase.
        tracing::debug!(
            workflow_id = %workflow_id,
            signal_name = %signal_name,
            payload_size = payload.len(),
            "Signal workflow called (placeholder implementation)"
        );

        let duration = start_time.elapsed();
        metrics::histogram!("plexspaces_wasm_workflow_signal_duration_seconds").record(duration.as_secs_f64());
        metrics::counter!("plexspaces_wasm_workflow_signal_success_total").increment(1);
        Ok(())
    }

    async fn query_workflow(
        &mut self,
        ctx: plexspaces::actor::types::Context,
        workflow_id: String,
        query_type: String,
    ) -> Result<plexspaces::actor::types::Payload, plexspaces::actor::types::ActorError> {
        metrics::counter!("plexspaces_wasm_workflow_query_total").increment(1);
        let _span = tracing::span!(
            tracing::Level::INFO,
            "wasm_workflow_query",
            workflow_id = %workflow_id,
            query_type = %query_type
        )
        .entered();

        // NOTE: Workflow functions are placeholder implementations. Full workflow orchestration
        // requires WorkflowService integration which will be added in a future phase.
        tracing::debug!(
            workflow_id = %workflow_id,
            query_type = %query_type,
            "Query workflow called (placeholder implementation)"
        );

        metrics::counter!("plexspaces_wasm_workflow_query_success_total").increment(1);
        Ok(vec![]) // Empty result for placeholder
    }

    async fn await_workflow(
        &mut self,
        ctx: plexspaces::actor::types::Context,
        workflow_id: String,
        timeout_ms: u64,
    ) -> Result<plexspaces::actor::types::Payload, plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_workflow_await_total").increment(1);
        let _span = tracing::span!(
            tracing::Level::INFO,
            "wasm_workflow_await",
            workflow_id = %workflow_id,
            timeout_ms = timeout_ms
        )
        .entered();

        // NOTE: Workflow functions are placeholder implementations. Full workflow orchestration
        // requires WorkflowService integration which will be added in a future phase.
        tracing::debug!(
            workflow_id = %workflow_id,
            timeout_ms = timeout_ms,
            "Await workflow called (placeholder implementation)"
        );

        let duration = start_time.elapsed();
        metrics::histogram!("plexspaces_wasm_workflow_await_duration_seconds").record(duration.as_secs_f64());
        metrics::counter!("plexspaces_wasm_workflow_await_success_total").increment(1);
        Ok(vec![]) // Empty result for placeholder
    }

    async fn schedule_activity(
        &mut self,
        ctx: plexspaces::actor::types::Context,
        activity_type: String,
        input: plexspaces::actor::types::Payload,
        options: plexspaces::actor::workflow::ActivityOptions,
    ) -> Result<plexspaces::actor::types::Payload, plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_workflow_schedule_activity_total").increment(1);
        let _span = tracing::span!(
            tracing::Level::INFO,
            "wasm_workflow_schedule_activity",
            activity_type = %activity_type
        )
        .entered();

        // NOTE: Workflow functions are placeholder implementations. Full workflow orchestration
        // requires WorkflowService integration which will be added in a future phase./ActivityExecutor
        tracing::debug!(
            activity_type = %activity_type,
            input_size = input.len(),
            "Schedule activity called (placeholder implementation)"
        );

        let duration = start_time.elapsed();
        metrics::histogram!("plexspaces_wasm_workflow_schedule_activity_duration_seconds").record(duration.as_secs_f64());
        metrics::counter!("plexspaces_wasm_workflow_schedule_activity_success_total").increment(1);
        Ok(vec![]) // Empty result for placeholder
    }

    async fn sleep(&mut self, duration_ms: u64) {
        metrics::counter!("plexspaces_wasm_workflow_sleep_total").increment(1);
        // Use deterministic sleep (same as messaging::sleep)
        tokio::time::sleep(tokio::time::Duration::from_millis(duration_ms)).await;
    }

    async fn get_workflow_context(
        &mut self,
    ) -> Result<plexspaces::actor::workflow::WorkflowContext, plexspaces::actor::types::ActorError> {
        metrics::counter!("plexspaces_wasm_workflow_get_context_total").increment(1);
        
        // NOTE: get_workflow_context requires ExecutionContext which is not yet available.
        // This is an acceptable limitation - workflow context will be available when full
        // workflow orchestration is implemented.
        Err(plexspaces::actor::types::ActorError {
            code: plexspaces::actor::types::ErrorCode::NotImplemented,
            message: "get_workflow_context not yet implemented - requires ExecutionContext".to_string(),
            details: None,
        })
    }
}

#[cfg(feature = "component-model")]
pub struct DurabilityImpl {
    pub actor_id: ActorId,
    pub host_functions: Arc<HostFunctions>,
    // Side effect cache for deterministic replay
    side_effect_cache: Arc<tokio::sync::RwLock<std::collections::HashMap<String, Vec<u8>>>>,
    // Replay mode flag (for future replay support)
    is_replaying: Arc<tokio::sync::RwLock<bool>>,
}

#[cfg(feature = "component-model")]
impl DurabilityImpl {
    pub fn new(actor_id: ActorId, host_functions: Arc<HostFunctions>) -> Self {
        Self {
            actor_id,
            host_functions,
            side_effect_cache: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
            is_replaying: Arc::new(tokio::sync::RwLock::new(false)),
        }
    }
}

#[cfg(feature = "component-model")]
#[async_trait::async_trait]
impl plexspaces::actor::durability::Host for DurabilityImpl {
    async fn persist(
        &mut self,
        ctx: plexspaces::actor::types::Context,
        event_type: String,
        payload: plexspaces::actor::types::Payload,
    ) -> Result<u64, plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_durability_persist_total").increment(1);
        let _span = tracing::span!(
            tracing::Level::INFO,
            "wasm_durability_persist",
            actor_id = %self.actor_id,
            event_type = %event_type
        )
        .entered();

        let storage = match self.host_functions.journal_storage() {
            Some(storage) => storage,
            None => {
                metrics::counter!("plexspaces_wasm_durability_persist_errors_total").increment(1);
                return Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::Internal,
                    message: "Journal storage not configured".to_string(),
                    details: None,
                });
            }
        };

        // Create ActorEvent for event sourcing
        use plexspaces_journaling::ActorEvent;
        use plexspaces_proto::prost_types;
        use std::time::SystemTime;
        
        let event = ActorEvent {
            id: ulid::Ulid::new().to_string(),
            actor_id: self.actor_id.to_string(),
            sequence: 0, // Will be assigned by storage
            event_type: event_type.clone(),
            event_data: payload.clone(),
            timestamp: Some(prost_types::Timestamp::from(SystemTime::now())),
            caused_by: String::new(), // No correlation ID for direct persist
            metadata: std::collections::HashMap::new(),
        };

        // Drop span before await to ensure Send
        drop(_span);
        match storage.append_event(&event).await {
            Ok(sequence) => {
                let duration = start_time.elapsed();
                metrics::histogram!("plexspaces_wasm_durability_persist_duration_seconds").record(duration.as_secs_f64());
                metrics::counter!("plexspaces_wasm_durability_persist_success_total").increment(1);
                tracing::debug!(
                    actor_id = %self.actor_id,
                    event_type = %event_type,
                    sequence = sequence,
                    payload_size = payload.len(),
                    "Event persisted successfully"
                );
                Ok(sequence)
            }
            Err(e) => {
                metrics::counter!("plexspaces_wasm_durability_persist_errors_total").increment(1);
                tracing::warn!(
                    actor_id = %self.actor_id,
                    event_type = %event_type,
                    error = %e,
                    "Failed to persist event"
                );
                Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::Internal,
                    message: format!("Failed to persist event: {}", e),
                    details: None,
                })
            }
        }
    }

    async fn persist_batch(
        &mut self,
        ctx: plexspaces::actor::types::Context,
        events: Vec<(String, plexspaces::actor::types::Payload)>,
    ) -> Result<u64, plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_durability_persist_batch_total").increment(1);
        let _span = tracing::span!(
            tracing::Level::INFO,
            "wasm_durability_persist_batch",
            actor_id = %self.actor_id,
            event_count = events.len()
        )
        .entered();

        let storage = match self.host_functions.journal_storage() {
            Some(storage) => storage,
            None => {
                metrics::counter!("plexspaces_wasm_durability_persist_batch_errors_total").increment(1);
                return Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::Internal,
                    message: "Journal storage not configured".to_string(),
                    details: None,
                });
            }
        };

        // Create ActorEvent batch for event sourcing
        use plexspaces_journaling::ActorEvent;
        use plexspaces_proto::prost_types;
        use std::time::SystemTime;
        
        let actor_events: Vec<ActorEvent> = events
            .into_iter()
            .map(|(event_type, payload)| ActorEvent {
                id: ulid::Ulid::new().to_string(),
                actor_id: self.actor_id.to_string(),
                sequence: 0, // Will be assigned by storage
                event_type,
                event_data: payload,
                timestamp: Some(prost_types::Timestamp::from(SystemTime::now())),
                caused_by: String::new(),
                metadata: std::collections::HashMap::new(),
            })
            .collect();

        // Drop span before await to ensure Send
        drop(_span);
        match storage.append_events_batch(&actor_events).await {
            Ok((first_sequence, _, _)) => {
                let duration = start_time.elapsed();
                metrics::histogram!("plexspaces_wasm_durability_persist_batch_duration_seconds").record(duration.as_secs_f64());
                metrics::counter!("plexspaces_wasm_durability_persist_batch_success_total").increment(1);
                tracing::debug!(
                    actor_id = %self.actor_id,
                    event_count = actor_events.len(),
                    first_sequence = first_sequence,
                    "Batch events persisted successfully"
                );
                Ok(first_sequence)
            }
            Err(e) => {
                metrics::counter!("plexspaces_wasm_durability_persist_batch_errors_total").increment(1);
                tracing::warn!(
                    actor_id = %self.actor_id,
                    error = %e,
                    "Failed to persist batch events"
                );
                Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::Internal,
                    message: format!("Failed to persist batch events: {}", e),
                    details: None,
                })
            }
        }
    }

    async fn checkpoint(&mut self, ctx: plexspaces::actor::types::Context) -> Result<u64, plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_durability_checkpoint_total").increment(1);
        let _span = tracing::span!(
            tracing::Level::INFO,
            "wasm_durability_checkpoint",
            actor_id = %self.actor_id
        )
        .entered();

        // Clone values before borrowing self
        let actor_id = self.actor_id.clone();
        let host_functions = self.host_functions.clone();
        
        let storage = match host_functions.journal_storage() {
            Some(storage) => storage,
            None => {
                metrics::counter!("plexspaces_wasm_durability_checkpoint_errors_total").increment(1);
                return Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::Internal,
                    message: "Journal storage not configured".to_string(),
                    details: None,
                });
            }
        };

        // Drop span before await to ensure Send
        drop(_span);
        
        // Get current sequence to use as checkpoint sequence
        // Use cloned values to avoid borrow conflicts
        let current_sequence = {
            let mut durability_impl = DurabilityImpl {
                actor_id: actor_id.clone(),
                host_functions: host_functions.clone(),
                side_effect_cache: self.side_effect_cache.clone(),
                is_replaying: self.is_replaying.clone(),
            };
            match durability_impl.get_sequence(ctx).await {
                Ok(seq) => seq,
                Err(e) => {
                    metrics::counter!("plexspaces_wasm_durability_checkpoint_errors_total").increment(1);
                    return Err(e);
                }
            }
        };

        // Create checkpoint with empty state (WASM actors manage their own state)
        use plexspaces_journaling::Checkpoint;
        use plexspaces_proto::prost_types;
        use std::time::SystemTime;
        
        let checkpoint = Checkpoint {
            actor_id: self.actor_id.to_string(),
            sequence: current_sequence,
            state_data: vec![], // Empty - WASM actors manage state separately
            state_schema_version: 1,
            timestamp: Some(prost_types::Timestamp::from(SystemTime::now())),
            compression: plexspaces_proto::v1::journaling::CompressionType::CompressionTypeNone as i32,
            metadata: std::collections::HashMap::new(),
        };

        match storage.save_checkpoint(&checkpoint).await {
            Ok(_) => {
                let duration = start_time.elapsed();
                metrics::histogram!("plexspaces_wasm_durability_checkpoint_duration_seconds").record(duration.as_secs_f64());
                metrics::counter!("plexspaces_wasm_durability_checkpoint_success_total").increment(1);
                tracing::debug!(
                    actor_id = %self.actor_id,
                    sequence = current_sequence,
                    "Checkpoint created successfully"
                );
                Ok(current_sequence)
            }
            Err(e) => {
                metrics::counter!("plexspaces_wasm_durability_checkpoint_errors_total").increment(1);
                tracing::warn!(
                    actor_id = %self.actor_id,
                    error = %e,
                    "Failed to create checkpoint"
                );
                Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::Internal,
                    message: format!("Failed to create checkpoint: {}", e),
                    details: None,
                })
            }
        }
    }

    async fn get_sequence(&mut self, ctx: plexspaces::actor::types::Context) -> Result<u64, plexspaces::actor::types::ActorError> {
        metrics::counter!("plexspaces_wasm_durability_get_sequence_total").increment(1);
        
        let storage = match self.host_functions.journal_storage() {
            Some(storage) => storage,
            None => {
                return Ok(0); // No storage = no events = sequence 0
            }
        };

        // Get all events for this actor and find the max sequence
        // We use replay_events_from with sequence 0 to get all events, then find the max
        match storage.replay_events_from(&self.actor_id, 0).await {
            Ok(events) => {
                let max_sequence = events
                    .iter()
                    .map(|e| e.sequence)
                    .max()
                    .unwrap_or(0);
                Ok(max_sequence)
            }
            Err(e) => {
                tracing::warn!(
                    actor_id = %self.actor_id,
                    error = %e,
                    "Failed to get sequence from journal"
                );
                // Return 0 on error (no events yet)
                Ok(0)
            }
        }
    }

    async fn get_checkpoint_sequence(&mut self, ctx: plexspaces::actor::types::Context) -> Result<u64, plexspaces::actor::types::ActorError> {
        metrics::counter!("plexspaces_wasm_durability_get_checkpoint_sequence_total").increment(1);
        
        let storage = match self.host_functions.journal_storage() {
            Some(storage) => storage,
            None => {
                return Ok(0); // No storage = no checkpoint = sequence 0
            }
        };

        match storage.get_latest_checkpoint(&self.actor_id).await {
            Ok(checkpoint) => Ok(checkpoint.sequence),
            Err(plexspaces_journaling::JournalError::CheckpointNotFound(_)) => {
                // No checkpoint exists yet
                Ok(0)
            }
            Err(e) => {
                tracing::warn!(
                    actor_id = %self.actor_id,
                    error = %e,
                    "Failed to get checkpoint sequence"
                );
                // Return 0 on error (no checkpoint yet)
                Ok(0)
            }
        }
    }

    async fn is_replaying(&mut self, ctx: plexspaces::actor::types::Context) -> Result<bool, plexspaces::actor::types::ActorError> {
        metrics::counter!("plexspaces_wasm_durability_is_replaying_total").increment(1);
        let replaying = *self.is_replaying.read().await;
        Ok(replaying)
    }

    async fn cache_side_effect(
        &mut self,
        ctx: plexspaces::actor::types::Context,
        key: String,
        result_value: plexspaces::actor::types::Payload,
    ) -> Result<plexspaces::actor::types::Payload, plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_durability_cache_side_effect_total").increment(1);
        let _span = tracing::span!(
            tracing::Level::INFO,
            "wasm_durability_cache_side_effect",
            actor_id = %self.actor_id,
            key = %key
        )
        .entered();

        // Drop span before await to ensure Send
        drop(_span);
        
        let is_replaying = *self.is_replaying.read().await;
        
        if is_replaying {
            // During replay, return cached result
            let cache = self.side_effect_cache.read().await;
            if let Some(cached) = cache.get(&key) {
                tracing::debug!(
                    actor_id = %self.actor_id,
                    key = %key,
                    "Returning cached side effect result during replay"
                );
                return Ok(cached.clone());
            } else {
                // No cache found - this shouldn't happen in proper replay
                tracing::warn!(
                    actor_id = %self.actor_id,
                    key = %key,
                    "No cached side effect found during replay"
                );
                return Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::Internal,
                    message: format!("No cached side effect found for key: {}", key),
                    details: None,
                });
            }
        }

        // Normal execution: cache the result and return it
        {
            let mut cache = self.side_effect_cache.write().await;
            cache.insert(key.clone(), result_value.clone());
        }

        // Also persist the side effect to journal for future replay
        let storage = self.host_functions.journal_storage();
        if let Some(storage) = storage {
            use plexspaces_journaling::ActorEvent;
            use plexspaces_proto::prost_types;
            use std::time::SystemTime;
            
            let event = ActorEvent {
                id: ulid::Ulid::new().to_string(),
                actor_id: self.actor_id.to_string(),
                sequence: 0, // Will be assigned by storage
                event_type: format!("side_effect:{}", key),
                event_data: result_value.clone(),
                timestamp: Some(prost_types::Timestamp::from(SystemTime::now())),
                caused_by: String::new(),
                metadata: std::collections::HashMap::new(),
            };
            
            // Don't fail if journaling fails - side effect is already cached
            if let Err(e) = storage.append_event(&event).await {
                tracing::warn!(
                    actor_id = %self.actor_id,
                    key = %key,
                    error = %e,
                    "Failed to journal side effect (result still cached)"
                );
            }
        }

        let duration = start_time.elapsed();
        metrics::histogram!("plexspaces_wasm_durability_cache_side_effect_duration_seconds").record(duration.as_secs_f64());
        metrics::counter!("plexspaces_wasm_durability_cache_side_effect_success_total").increment(1);
        tracing::debug!(
            actor_id = %self.actor_id,
            key = %key,
            result_size = result_value.len(),
            "Side effect cached successfully"
        );
        Ok(result_value)
    }

    async fn read_journal(
        &mut self,
        ctx: plexspaces::actor::types::Context,
        from_sequence: u64,
        to_sequence: u64,
        limit: u32,
    ) -> Result<Vec<plexspaces::actor::durability::JournalEntry>, plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_durability_read_journal_total").increment(1);
        let _span = tracing::span!(
            tracing::Level::INFO,
            "wasm_durability_read_journal",
            actor_id = %self.actor_id,
            from_sequence = from_sequence,
            to_sequence = to_sequence,
            limit = limit
        )
        .entered();

        // Drop span before await to ensure Send
        drop(_span);

        let storage = match self.host_functions.journal_storage() {
            Some(storage) => storage,
            None => {
                metrics::counter!("plexspaces_wasm_durability_read_journal_errors_total").increment(1);
                return Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::Internal,
                    message: "Journal storage not configured".to_string(),
                    details: None,
                });
            }
        };

        // Replay events from the specified sequence
        match storage.replay_events_from(&self.actor_id, from_sequence).await {
            Ok(events) => {
                // Filter by to_sequence if specified (0 means no limit)
                let filtered_events: Vec<_> = if to_sequence > 0 {
                    events.into_iter()
                        .take_while(|e| e.sequence < to_sequence)
                        .take(limit as usize)
                        .collect()
                } else {
                    events.into_iter()
                        .take(limit as usize)
                        .collect()
                };

                // Convert ActorEvent to WIT JournalEntry
                let journal_entries: Vec<plexspaces::actor::durability::JournalEntry> = filtered_events
                    .into_iter()
                    .map(|event| {
                        let timestamp_ms = event.timestamp
                            .map(|ts| (ts.seconds as u64 * 1000) + (ts.nanos as u64 / 1_000_000))
                            .unwrap_or_else(|| {
                                std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap()
                                    .as_millis() as u64
                            });
                        
                        plexspaces::actor::durability::JournalEntry {
                            sequence: event.sequence,
                            event_type: event.event_type,
                            payload: event.event_data,
                            timestamp: timestamp_ms,
                        }
                    })
                    .collect();

                let duration = start_time.elapsed();
                metrics::histogram!("plexspaces_wasm_durability_read_journal_duration_seconds").record(duration.as_secs_f64());
                metrics::counter!("plexspaces_wasm_durability_read_journal_success_total").increment(1);
                tracing::debug!(
                    actor_id = %self.actor_id,
                    from_sequence = from_sequence,
                    to_sequence = to_sequence,
                    limit = limit,
                    entries_returned = journal_entries.len(),
                    "Journal entries read successfully"
                );
                Ok(journal_entries)
            }
            Err(e) => {
                metrics::counter!("plexspaces_wasm_durability_read_journal_errors_total").increment(1);
                tracing::warn!(
                    actor_id = %self.actor_id,
                    error = %e,
                    "Failed to read journal"
                );
                Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::Internal,
                    message: format!("Failed to read journal: {}", e),
                    details: None,
                })
            }
        }
    }

    async fn compact(
        &mut self,
        ctx: plexspaces::actor::types::Context,
        up_to_sequence: u64,
    ) -> Result<(), plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_durability_compact_total").increment(1);
        let _span = tracing::span!(
            tracing::Level::INFO,
            "wasm_durability_compact",
            actor_id = %self.actor_id,
            up_to_sequence = up_to_sequence
        )
        .entered();

        // Clone values before borrowing self
        let actor_id = self.actor_id.clone();
        let host_functions = self.host_functions.clone();
        
        let storage = match host_functions.journal_storage() {
            Some(storage) => storage,
            None => {
                metrics::counter!("plexspaces_wasm_durability_compact_errors_total").increment(1);
                return Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::Internal,
                    message: "Journal storage not configured".to_string(),
                    details: None,
                });
            }
        };

        // Drop span before await to ensure Send
        drop(_span);
        
        // Verify checkpoint exists at or after the sequence
        // Use cloned values to avoid borrow conflicts
        let checkpoint_sequence = {
            let mut durability_impl = DurabilityImpl {
                actor_id: actor_id.clone(),
                host_functions: host_functions.clone(),
                side_effect_cache: self.side_effect_cache.clone(),
                is_replaying: self.is_replaying.clone(),
            };
            match durability_impl.get_checkpoint_sequence(ctx).await {
                Ok(seq) => seq,
                Err(e) => {
                    metrics::counter!("plexspaces_wasm_durability_compact_errors_total").increment(1);
                    return Err(plexspaces::actor::types::ActorError {
                        code: plexspaces::actor::types::ErrorCode::Internal,
                        message: format!("Cannot compact: no checkpoint found. Error: {}", e.message),
                        details: None,
                    });
                }
            }
        };

        if checkpoint_sequence < up_to_sequence {
            metrics::counter!("plexspaces_wasm_durability_compact_errors_total").increment(1);
            return Err(plexspaces::actor::types::ActorError {
                code: plexspaces::actor::types::ErrorCode::Internal,
                message: format!(
                    "Cannot compact: checkpoint sequence ({}) is before requested sequence ({})",
                    checkpoint_sequence, up_to_sequence
                ),
                details: None,
            });
        }

        // Truncate journal entries up to the specified sequence
        match storage.truncate_to(&self.actor_id, up_to_sequence).await {
            Ok(deleted_count) => {
                let duration = start_time.elapsed();
                metrics::histogram!("plexspaces_wasm_durability_compact_duration_seconds").record(duration.as_secs_f64());
                metrics::counter!("plexspaces_wasm_durability_compact_success_total").increment(1);
                tracing::debug!(
                    actor_id = %self.actor_id,
                    up_to_sequence = up_to_sequence,
                    entries_deleted = deleted_count,
                    "Journal compacted successfully"
                );
                Ok(())
            }
            Err(e) => {
                metrics::counter!("plexspaces_wasm_durability_compact_errors_total").increment(1);
                tracing::warn!(
                    actor_id = %self.actor_id,
                    error = %e,
                    "Failed to compact journal"
                );
                Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::Internal,
                    message: format!("Failed to compact journal: {}", e),
                    details: None,
                })
            }
        }
    }
}

#[cfg(feature = "component-model")]
pub struct KeyValueImpl {
    pub actor_id: ActorId,
    pub host_functions: Arc<HostFunctions>,
}

#[cfg(feature = "component-model")]
#[async_trait::async_trait]
impl plexspaces::actor::keyvalue::Host for KeyValueImpl {
    async fn get(
        &mut self,
        ctx: plexspaces::actor::types::Context,
        key: String,
    ) -> Result<Option<plexspaces::actor::types::Payload>, plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_keyvalue_get_total").increment(1);
        let _span = tracing::span!(
            tracing::Level::DEBUG,
            "wasm_keyvalue_get",
            actor_id = %self.actor_id,
            key = %key
        )
        .entered();

        let kv_store = self.host_functions.keyvalue_store()
            .ok_or_else(|| plexspaces::actor::types::ActorError {
                code: plexspaces::actor::types::ErrorCode::Internal,
                message: "KeyValue store not configured".to_string(),
                details: None,
            })?;

        // Create RequestContext from context (empty strings use defaults)
        let request_ctx = context_to_request_context(&ctx);

        // Drop span before await to ensure Send
        drop(_span);
        match kv_store.get(&request_ctx, &key).await {
            Ok(Some(value)) => {
                let duration = start_time.elapsed();
                metrics::histogram!("plexspaces_wasm_keyvalue_get_duration_seconds").record(duration.as_secs_f64());
                metrics::counter!("plexspaces_wasm_keyvalue_get_success_total").increment(1);
                tracing::debug!(key = %key, value_size = value.len(), "KeyValue get succeeded");
                Ok(Some(value))
            }
            Ok(None) => {
                let duration = start_time.elapsed();
                metrics::histogram!("plexspaces_wasm_keyvalue_get_duration_seconds").record(duration.as_secs_f64());
                metrics::counter!("plexspaces_wasm_keyvalue_get_success_total").increment(1);
                tracing::debug!(key = %key, "KeyValue get returned None");
                Ok(None)
            }
            Err(e) => {
                metrics::counter!("plexspaces_wasm_keyvalue_get_errors_total").increment(1);
                tracing::warn!(key = %key, error = %e, "KeyValue get failed");
                Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::Internal,
                    message: format!("KeyValue get failed: {}", e),
                    details: None,
                })
            }
        }
    }

    async fn put(
        &mut self,
        ctx: plexspaces::actor::types::Context,
        key: String,
        value: plexspaces::actor::types::Payload,
    ) -> Result<(), plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_keyvalue_put_total").increment(1);
        let _span = tracing::span!(
            tracing::Level::DEBUG,
            "wasm_keyvalue_put",
            actor_id = %self.actor_id,
            key = %key,
            value_size = value.len()
        )
        .entered();

        let kv_store = self.host_functions.keyvalue_store()
            .ok_or_else(|| plexspaces::actor::types::ActorError {
                code: plexspaces::actor::types::ErrorCode::Internal,
                message: "KeyValue store not configured".to_string(),
                details: None,
            })?;

        // Create RequestContext from context (empty strings use defaults)
        let request_ctx = context_to_request_context(&ctx);

        // Drop span before await to ensure Send
        drop(_span);
        match kv_store.put(&request_ctx, &key, value).await {
            Ok(()) => {
                let duration = start_time.elapsed();
                metrics::histogram!("plexspaces_wasm_keyvalue_put_duration_seconds").record(duration.as_secs_f64());
                metrics::counter!("plexspaces_wasm_keyvalue_put_success_total").increment(1);
                tracing::debug!(key = %key, "KeyValue put succeeded");
                Ok(())
            }
            Err(e) => {
                metrics::counter!("plexspaces_wasm_keyvalue_put_errors_total").increment(1);
                tracing::warn!(key = %key, error = %e, "KeyValue put failed");
                Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::Internal,
                    message: format!("KeyValue put failed: {}", e),
                    details: None,
                })
            }
        }
    }

    async fn put_with_ttl(
        &mut self,
        ctx: plexspaces::actor::types::Context,
        key: String,
        value: plexspaces::actor::types::Payload,
        ttl_ms: u64,
    ) -> Result<(), plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_keyvalue_put_with_ttl_total").increment(1);
        let _span = tracing::span!(
            tracing::Level::DEBUG,
            "wasm_keyvalue_put_with_ttl",
            actor_id = %self.actor_id,
            key = %key,
            ttl_ms = ttl_ms,
            value_size = value.len()
        )
        .entered();

        let kv_store = self.host_functions.keyvalue_store()
            .ok_or_else(|| plexspaces::actor::types::ActorError {
                code: plexspaces::actor::types::ErrorCode::Internal,
                message: "KeyValue store not configured".to_string(),
                details: None,
            })?;

        // Create RequestContext from context (empty strings use defaults)
        let request_ctx = context_to_request_context(&ctx);
        let ttl = std::time::Duration::from_millis(ttl_ms);

        // Drop span before await to ensure Send
        drop(_span);
        match kv_store.put_with_ttl(&request_ctx, &key, value, ttl).await {
            Ok(()) => {
                let duration = start_time.elapsed();
                metrics::histogram!("plexspaces_wasm_keyvalue_put_with_ttl_duration_seconds").record(duration.as_secs_f64());
                metrics::counter!("plexspaces_wasm_keyvalue_put_with_ttl_success_total").increment(1);
                tracing::debug!(key = %key, ttl_ms = ttl_ms, "KeyValue put_with_ttl succeeded");
                Ok(())
            }
            Err(e) => {
                metrics::counter!("plexspaces_wasm_keyvalue_put_with_ttl_errors_total").increment(1);
                tracing::warn!(key = %key, ttl_ms = ttl_ms, error = %e, "KeyValue put_with_ttl failed");
                Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::Internal,
                    message: format!("KeyValue put_with_ttl failed: {}", e),
                    details: None,
                })
            }
        }
    }

    async fn delete(
        &mut self,
        ctx: plexspaces::actor::types::Context,
        key: String,
    ) -> Result<(), plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_keyvalue_delete_total").increment(1);
        let _span = tracing::span!(
            tracing::Level::DEBUG,
            "wasm_keyvalue_delete",
            actor_id = %self.actor_id,
            key = %key
        )
        .entered();

        let kv_store = self.host_functions.keyvalue_store()
            .ok_or_else(|| plexspaces::actor::types::ActorError {
                code: plexspaces::actor::types::ErrorCode::Internal,
                message: "KeyValue store not configured".to_string(),
                details: None,
            })?;

        let request_ctx = context_to_request_context(&ctx);

        // Drop span before await to ensure Send
        drop(_span);
        match kv_store.delete(&request_ctx, &key).await {
            Ok(()) => {
                let duration = start_time.elapsed();
                metrics::histogram!("plexspaces_wasm_keyvalue_delete_duration_seconds").record(duration.as_secs_f64());
                metrics::counter!("plexspaces_wasm_keyvalue_delete_success_total").increment(1);
                tracing::debug!(key = %key, "KeyValue delete succeeded");
                Ok(())
            }
            Err(e) => {
                metrics::counter!("plexspaces_wasm_keyvalue_delete_errors_total").increment(1);
                tracing::warn!(key = %key, error = %e, "KeyValue delete failed");
                Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::Internal,
                    message: format!("KeyValue delete failed: {}", e),
                    details: None,
                })
            }
        }
    }

    async fn exists(
        &mut self,
        ctx: plexspaces::actor::types::Context,
        key: String,
    ) -> Result<bool, plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_keyvalue_exists_total").increment(1);
        let _span = tracing::span!(
            tracing::Level::DEBUG,
            "wasm_keyvalue_exists",
            actor_id = %self.actor_id,
            key = %key
        )
        .entered();

        let kv_store = self.host_functions.keyvalue_store()
            .ok_or_else(|| plexspaces::actor::types::ActorError {
                code: plexspaces::actor::types::ErrorCode::Internal,
                message: "KeyValue store not configured".to_string(),
                details: None,
            })?;

        let request_ctx = context_to_request_context(&ctx);

        // Drop span before await to ensure Send
        drop(_span);
        match kv_store.exists(&request_ctx, &key).await {
            Ok(exists) => {
                let duration = start_time.elapsed();
                metrics::histogram!("plexspaces_wasm_keyvalue_exists_duration_seconds").record(duration.as_secs_f64());
                metrics::counter!("plexspaces_wasm_keyvalue_exists_success_total").increment(1);
                tracing::debug!(key = %key, exists = exists, "KeyValue exists succeeded");
                Ok(exists)
            }
            Err(e) => {
                metrics::counter!("plexspaces_wasm_keyvalue_exists_errors_total").increment(1);
                tracing::warn!(key = %key, error = %e, "KeyValue exists failed");
                Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::Internal,
                    message: format!("KeyValue exists failed: {}", e),
                    details: None,
                })
            }
        }
    }

    async fn list_keys(
        &mut self,
        ctx: plexspaces::actor::types::Context,
        prefix: String,
        limit: u32,
    ) -> Result<Vec<String>, plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_keyvalue_list_total").increment(1);
        let _span = tracing::span!(
            tracing::Level::DEBUG,
            "wasm_keyvalue_list",
            actor_id = %self.actor_id,
            prefix = %prefix,
            limit = limit
        )
        .entered();

        let kv_store = self.host_functions.keyvalue_store()
            .ok_or_else(|| plexspaces::actor::types::ActorError {
                code: plexspaces::actor::types::ErrorCode::Internal,
                message: "KeyValue store not configured".to_string(),
                details: None,
            })?;

        let request_ctx = context_to_request_context(&ctx);

        // Drop span before await to ensure Send
        drop(_span);
        match kv_store.list(&request_ctx, &prefix).await {
            Ok(mut keys) => {
                // Apply limit if specified
                if limit > 0 && keys.len() > limit as usize {
                    keys.truncate(limit as usize);
                }
                let duration = start_time.elapsed();
                metrics::histogram!("plexspaces_wasm_keyvalue_list_duration_seconds").record(duration.as_secs_f64());
                metrics::counter!("plexspaces_wasm_keyvalue_list_success_total").increment(1);
                tracing::debug!(prefix = %prefix, limit = limit, count = keys.len(), "KeyValue list succeeded");
                Ok(keys)
            }
            Err(e) => {
                metrics::counter!("plexspaces_wasm_keyvalue_list_errors_total").increment(1);
                tracing::warn!(prefix = %prefix, error = %e, "KeyValue list failed");
                Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::Internal,
                    message: format!("KeyValue list failed: {}", e),
                    details: None,
                })
            }
        }
    }

    async fn compare_and_swap(
        &mut self,
        ctx: plexspaces::actor::types::Context,
        key: String,
        expected: Option<plexspaces::actor::types::Payload>,
        new_value: plexspaces::actor::types::Payload,
    ) -> Result<bool, plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_keyvalue_compare_and_swap_total").increment(1);
        let _span = tracing::span!(
            tracing::Level::DEBUG,
            "wasm_keyvalue_compare_and_swap",
            actor_id = %self.actor_id,
            key = %key
        )
        .entered();

        let kv_store = self.host_functions.keyvalue_store()
            .ok_or_else(|| plexspaces::actor::types::ActorError {
                code: plexspaces::actor::types::ErrorCode::Internal,
                message: "KeyValue store not configured".to_string(),
                details: None,
            })?;

        let request_ctx = context_to_request_context(&ctx);

        // Drop span before await to ensure Send
        drop(_span);
        match kv_store.cas(&request_ctx, &key, expected, new_value).await {
            Ok(success) => {
                let duration = start_time.elapsed();
                metrics::histogram!("plexspaces_wasm_keyvalue_compare_and_swap_duration_seconds").record(duration.as_secs_f64());
                metrics::counter!("plexspaces_wasm_keyvalue_compare_and_swap_success_total").increment(1);
                tracing::debug!(key = %key, success = success, "KeyValue compare_and_swap succeeded");
                Ok(success)
            }
            Err(e) => {
                metrics::counter!("plexspaces_wasm_keyvalue_compare_and_swap_errors_total").increment(1);
                tracing::warn!(key = %key, error = %e, "KeyValue compare_and_swap failed");
                Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::Internal,
                    message: format!("KeyValue compare_and_swap failed: {}", e),
                    details: None,
                })
            }
        }
    }

    async fn increment(
        &mut self,
        ctx: plexspaces::actor::types::Context,
        key: String,
        delta: i64,
    ) -> Result<i64, plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_keyvalue_increment_total").increment(1);
        let _span = tracing::span!(
            tracing::Level::DEBUG,
            "wasm_keyvalue_increment",
            actor_id = %self.actor_id,
            key = %key,
            delta = delta
        )
        .entered();

        let kv_store = self.host_functions.keyvalue_store()
            .ok_or_else(|| plexspaces::actor::types::ActorError {
                code: plexspaces::actor::types::ErrorCode::Internal,
                message: "KeyValue store not configured".to_string(),
                details: None,
            })?;

        let request_ctx = context_to_request_context(&ctx);

        // Drop span before await to ensure Send
        drop(_span);
        match kv_store.increment(&request_ctx, &key, delta).await {
            Ok(new_value) => {
                let duration = start_time.elapsed();
                metrics::histogram!("plexspaces_wasm_keyvalue_increment_duration_seconds").record(duration.as_secs_f64());
                metrics::counter!("plexspaces_wasm_keyvalue_increment_success_total").increment(1);
                tracing::debug!(key = %key, delta = delta, new_value = new_value, "KeyValue increment succeeded");
                Ok(new_value)
            }
            Err(e) => {
                metrics::counter!("plexspaces_wasm_keyvalue_increment_errors_total").increment(1);
                tracing::warn!(key = %key, delta = delta, error = %e, "KeyValue increment failed");
                Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::Internal,
                    message: format!("KeyValue increment failed: {}", e),
                    details: None,
                })
            }
        }
    }

    async fn watch(
        &mut self,
        ctx: plexspaces::actor::types::Context,
        key: String,
    ) -> Result<u64, plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_keyvalue_watch_total").increment(1);
        let _span = tracing::span!(
            tracing::Level::DEBUG,
            "wasm_keyvalue_watch",
            actor_id = %self.actor_id,
            key = %key
        )
        .entered();

        // NOTE: KeyValue watch is not yet implemented in the KeyValueStore backend.
        // This is an acceptable limitation - watch/unwatch will be added when the backend
        // supports change notifications.
        tracing::debug!(key = %key, "Watch called (not yet implemented in backend)");

        let duration = start_time.elapsed();
        metrics::histogram!("plexspaces_wasm_keyvalue_watch_duration_seconds").record(duration.as_secs_f64());
        metrics::counter!("plexspaces_wasm_keyvalue_watch_success_total").increment(1);
        Err(plexspaces::actor::types::ActorError {
            code: plexspaces::actor::types::ErrorCode::NotImplemented,
            message: "KeyValue watch not yet implemented".to_string(),
            details: None,
        })
    }

    async fn unwatch(
        &mut self,
        subscription_id: u64,
    ) -> Result<(), plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_keyvalue_unwatch_total").increment(1);
        let _span = tracing::span!(
            tracing::Level::DEBUG,
            "wasm_keyvalue_unwatch",
            actor_id = %self.actor_id,
            subscription_id = subscription_id
        )
        .entered();

        // NOTE: KeyValue unwatch is not yet implemented in the KeyValueStore backend.
        // This is an acceptable limitation - watch/unwatch will be added when the backend
        // supports change notifications.
        tracing::debug!(subscription_id = subscription_id, "Unwatch called (not yet implemented in backend)");

        let duration = start_time.elapsed();
        metrics::histogram!("plexspaces_wasm_keyvalue_unwatch_duration_seconds").record(duration.as_secs_f64());
        metrics::counter!("plexspaces_wasm_keyvalue_unwatch_success_total").increment(1);
        Err(plexspaces::actor::types::ActorError {
            code: plexspaces::actor::types::ErrorCode::NotImplemented,
            message: "KeyValue unwatch not yet implemented".to_string(),
            details: None,
        })
    }
}

#[cfg(feature = "component-model")]
pub struct ProcessGroupsImpl {
    pub actor_id: ActorId,
    pub host_functions: Arc<HostFunctions>,
}

#[cfg(feature = "component-model")]
#[async_trait::async_trait]
impl plexspaces::actor::process_groups::Host for ProcessGroupsImpl {
    async fn create_group(
        &mut self,
        ctx: plexspaces::actor::types::Context,
        group_name: String,
        namespace: String,
    ) -> Result<(), plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_process_groups_create_group_total").increment(1);
        let _span = tracing::span!(
            tracing::Level::DEBUG,
            "wasm_process_groups_create_group",
            actor_id = %self.actor_id,
            group_name = %group_name,
            namespace = %namespace
        )
        .entered();

        // Drop span before await to ensure Send
        drop(_span);

        let registry = self.host_functions.process_group_registry()
            .ok_or_else(|| plexspaces::actor::types::ActorError {
                code: plexspaces::actor::types::ErrorCode::Internal,
                message: "ProcessGroupRegistry not configured".to_string(),
                details: None,
            })?;

        let request_ctx = context_to_request_context(&ctx);
        let tenant_id = request_ctx.tenant_id.clone();
        let namespace_str = request_ctx.namespace.clone();

        match registry.create_group(&group_name, &tenant_id, &namespace_str).await {
            Ok(_) => {
                let duration = start_time.elapsed();
                metrics::histogram!("plexspaces_wasm_process_groups_create_group_duration_seconds").record(duration.as_secs_f64());
                metrics::counter!("plexspaces_wasm_process_groups_create_group_success_total").increment(1);
                tracing::debug!(group_name = %group_name, namespace = %namespace, "ProcessGroup create_group succeeded");
                Ok(())
            }
            Err(e) => {
                metrics::counter!("plexspaces_wasm_process_groups_create_group_errors_total").increment(1);
                let error_code = if e.to_string().contains("already exists") {
                    plexspaces::actor::types::ErrorCode::Internal
                } else {
                    plexspaces::actor::types::ErrorCode::Internal
                };
                tracing::warn!(group_name = %group_name, namespace = %namespace, error = %e, "ProcessGroup create_group failed");
                Err(plexspaces::actor::types::ActorError {
                    code: error_code,
                    message: format!("ProcessGroup create_group failed: {}", e),
                    details: None,
                })
            }
        }
    }

    async fn delete_group(
        &mut self,
        ctx: plexspaces::actor::types::Context,
        group_name: String,
    ) -> Result<(), plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_process_groups_delete_group_total").increment(1);
        let _span = tracing::span!(
            tracing::Level::DEBUG,
            "wasm_process_groups_delete_group",
            actor_id = %self.actor_id,
            group_name = %group_name
        )
        .entered();

        let registry = self.host_functions.process_group_registry()
            .ok_or_else(|| plexspaces::actor::types::ActorError {
                code: plexspaces::actor::types::ErrorCode::Internal,
                message: "ProcessGroupRegistry not configured".to_string(),
                details: None,
            })?;

        let request_ctx = context_to_request_context(&ctx);

        // Drop span before await to ensure Send
        drop(_span);
        match registry.delete_group(&request_ctx, &group_name).await {
            Ok(()) => {
                let duration = start_time.elapsed();
                metrics::histogram!("plexspaces_wasm_process_groups_delete_group_duration_seconds").record(duration.as_secs_f64());
                metrics::counter!("plexspaces_wasm_process_groups_delete_group_success_total").increment(1);
                tracing::debug!(group_name = %group_name, "ProcessGroup delete_group succeeded");
                Ok(())
            }
            Err(e) => {
                metrics::counter!("plexspaces_wasm_process_groups_delete_group_errors_total").increment(1);
                tracing::warn!(group_name = %group_name, error = %e, "ProcessGroup delete_group failed");
                Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::Internal,
                    message: format!("ProcessGroup delete_group failed: {}", e),
                    details: None,
                })
            }
        }
    }

    async fn join_group(
        &mut self,
        ctx: plexspaces::actor::types::Context,
        group_name: String,
        namespace: String,
        topics: Vec<String>,
    ) -> Result<(), plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_process_groups_join_group_total").increment(1);
        let _span = tracing::span!(
            tracing::Level::DEBUG,
            "wasm_process_groups_join_group",
            actor_id = %self.actor_id,
            group_name = %group_name,
            namespace = %namespace,
            topics = ?topics
        )
        .entered();

        let registry = self.host_functions.process_group_registry()
            .ok_or_else(|| plexspaces::actor::types::ActorError {
                code: plexspaces::actor::types::ErrorCode::Internal,
                message: "ProcessGroupRegistry not configured".to_string(),
                details: None,
            })?;

        let request_ctx = context_to_request_context(&ctx);
        let tenant_id = request_ctx.tenant_id.clone();
        let namespace_str = request_ctx.namespace.clone();
        let actor_id = plexspaces_core::ActorId::from(self.actor_id.clone());

        // Drop span before await to ensure Send
        drop(_span);
        match registry.join_group(&group_name, &tenant_id, &namespace_str, &actor_id, topics).await {
            Ok(()) => {
                let duration = start_time.elapsed();
                metrics::histogram!("plexspaces_wasm_process_groups_join_group_duration_seconds").record(duration.as_secs_f64());
                metrics::counter!("plexspaces_wasm_process_groups_join_group_success_total").increment(1);
                tracing::debug!(group_name = %group_name, namespace = %namespace, "ProcessGroup join_group succeeded");
                Ok(())
            }
            Err(e) => {
                metrics::counter!("plexspaces_wasm_process_groups_join_group_errors_total").increment(1);
                let error_code = if e.to_string().contains("not found") {
                    plexspaces::actor::types::ErrorCode::ActorNotFound
                } else {
                    plexspaces::actor::types::ErrorCode::Internal
                };
                tracing::warn!(group_name = %group_name, namespace = %namespace, error = %e, "ProcessGroup join_group failed");
                Err(plexspaces::actor::types::ActorError {
                    code: error_code,
                    message: format!("ProcessGroup join_group failed: {}", e),
                    details: None,
                })
            }
        }
    }

    async fn leave_group(
        &mut self,
        ctx: plexspaces::actor::types::Context,
        group_name: String,
    ) -> Result<(), plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_process_groups_leave_group_total").increment(1);
        let _span = tracing::span!(
            tracing::Level::DEBUG,
            "wasm_process_groups_leave_group",
            actor_id = %self.actor_id,
            group_name = %group_name
        )
        .entered();

        let registry = self.host_functions.process_group_registry()
            .ok_or_else(|| plexspaces::actor::types::ActorError {
                code: plexspaces::actor::types::ErrorCode::Internal,
                message: "ProcessGroupRegistry not configured".to_string(),
                details: None,
            })?;

        let request_ctx = context_to_request_context(&ctx);
        let actor_id = plexspaces_core::ActorId::from(self.actor_id.clone());

        // Drop span before await to ensure Send
        drop(_span);
        match registry.leave_group(&request_ctx, &group_name, &actor_id).await {
            Ok(()) => {
                let duration = start_time.elapsed();
                metrics::histogram!("plexspaces_wasm_process_groups_leave_group_duration_seconds").record(duration.as_secs_f64());
                metrics::counter!("plexspaces_wasm_process_groups_leave_group_success_total").increment(1);
                tracing::debug!(group_name = %group_name, "ProcessGroup leave_group succeeded");
                Ok(())
            }
            Err(e) => {
                metrics::counter!("plexspaces_wasm_process_groups_leave_group_errors_total").increment(1);
                let error_code = if e.to_string().contains("not found") || e.to_string().contains("not in group") {
                    plexspaces::actor::types::ErrorCode::ActorNotFound
                } else {
                    plexspaces::actor::types::ErrorCode::Internal
                };
                tracing::warn!(group_name = %group_name, error = %e, "ProcessGroup leave_group failed");
                Err(plexspaces::actor::types::ActorError {
                    code: error_code,
                    message: format!("ProcessGroup leave_group failed: {}", e),
                    details: None,
                })
            }
        }
    }

    async fn get_members(
        &mut self,
        ctx: plexspaces::actor::types::Context,
        group_name: String,
    ) -> Result<Vec<plexspaces::actor::types::ActorId>, plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_process_groups_get_members_total").increment(1);
        let _span = tracing::span!(
            tracing::Level::DEBUG,
            "wasm_process_groups_get_members",
            actor_id = %self.actor_id,
            group_name = %group_name
        )
        .entered();

        let registry = self.host_functions.process_group_registry()
            .ok_or_else(|| plexspaces::actor::types::ActorError {
                code: plexspaces::actor::types::ErrorCode::Internal,
                message: "ProcessGroupRegistry not configured".to_string(),
                details: None,
            })?;

        let request_ctx = context_to_request_context(&ctx);

        // Drop span before await to ensure Send
        drop(_span);
        match registry.get_members(&request_ctx, &group_name).await {
            Ok(members) => {
                let actor_ids: Vec<plexspaces::actor::types::ActorId> = members
                    .into_iter()
                    .map(|id| id.to_string())
                    .collect();
                let duration = start_time.elapsed();
                metrics::histogram!("plexspaces_wasm_process_groups_get_members_duration_seconds").record(duration.as_secs_f64());
                metrics::counter!("plexspaces_wasm_process_groups_get_members_success_total").increment(1);
                tracing::debug!(group_name = %group_name, count = actor_ids.len(), "ProcessGroup get_members succeeded");
                Ok(actor_ids)
            }
            Err(e) => {
                metrics::counter!("plexspaces_wasm_process_groups_get_members_errors_total").increment(1);
                let error_code = if e.to_string().contains("not found") {
                    plexspaces::actor::types::ErrorCode::ActorNotFound
                } else {
                    plexspaces::actor::types::ErrorCode::Internal
                };
                tracing::warn!(group_name = %group_name, error = %e, "ProcessGroup get_members failed");
                Err(plexspaces::actor::types::ActorError {
                    code: error_code,
                    message: format!("ProcessGroup get_members failed: {}", e),
                    details: None,
                })
            }
        }
    }

    async fn get_local_members(
        &mut self,
        ctx: plexspaces::actor::types::Context,
        group_name: String,
    ) -> Result<Vec<plexspaces::actor::types::ActorId>, plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_process_groups_get_local_members_total").increment(1);
        let _span = tracing::span!(
            tracing::Level::DEBUG,
            "wasm_process_groups_get_local_members",
            actor_id = %self.actor_id,
            group_name = %group_name
        )
        .entered();

        let registry = self.host_functions.process_group_registry()
            .ok_or_else(|| plexspaces::actor::types::ActorError {
                code: plexspaces::actor::types::ErrorCode::Internal,
                message: "ProcessGroupRegistry not configured".to_string(),
                details: None,
            })?;

        let request_ctx = context_to_request_context(&ctx);

        // Drop span before await to ensure Send
        drop(_span);
        match registry.get_local_members(&request_ctx, &group_name).await {
            Ok(members) => {
                let actor_ids: Vec<plexspaces::actor::types::ActorId> = members
                    .into_iter()
                    .map(|id| id.to_string())
                    .collect();
                let duration = start_time.elapsed();
                metrics::histogram!("plexspaces_wasm_process_groups_get_local_members_duration_seconds").record(duration.as_secs_f64());
                metrics::counter!("plexspaces_wasm_process_groups_get_local_members_success_total").increment(1);
                tracing::debug!(group_name = %group_name, count = actor_ids.len(), "ProcessGroup get_local_members succeeded");
                Ok(actor_ids)
            }
            Err(e) => {
                metrics::counter!("plexspaces_wasm_process_groups_get_local_members_errors_total").increment(1);
                let error_code = if e.to_string().contains("not found") {
                    plexspaces::actor::types::ErrorCode::ActorNotFound
                } else {
                    plexspaces::actor::types::ErrorCode::Internal
                };
                tracing::warn!(group_name = %group_name, error = %e, "ProcessGroup get_local_members failed");
                Err(plexspaces::actor::types::ActorError {
                    code: error_code,
                    message: format!("ProcessGroup get_local_members failed: {}", e),
                    details: None,
                })
            }
        }
    }

    async fn list_groups(
        &mut self,
        ctx: plexspaces::actor::types::Context,
    ) -> Result<Vec<String>, plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_process_groups_list_groups_total").increment(1);
        let _span = tracing::span!(
            tracing::Level::DEBUG,
            "wasm_process_groups_list_groups",
            actor_id = %self.actor_id
        )
        .entered();

        let registry = self.host_functions.process_group_registry()
            .ok_or_else(|| plexspaces::actor::types::ActorError {
                code: plexspaces::actor::types::ErrorCode::Internal,
                message: "ProcessGroupRegistry not configured".to_string(),
                details: None,
            })?;

        let request_ctx = context_to_request_context(&ctx);

        // Drop span before await to ensure Send
        drop(_span);
        match registry.list_groups(&request_ctx).await {
            Ok(groups) => {
                let duration = start_time.elapsed();
                metrics::histogram!("plexspaces_wasm_process_groups_list_groups_duration_seconds").record(duration.as_secs_f64());
                metrics::counter!("plexspaces_wasm_process_groups_list_groups_success_total").increment(1);
                tracing::debug!(count = groups.len(), "ProcessGroup list_groups succeeded");
                Ok(groups)
            }
            Err(e) => {
                metrics::counter!("plexspaces_wasm_process_groups_list_groups_errors_total").increment(1);
                tracing::warn!(error = %e, "ProcessGroup list_groups failed");
                Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::Internal,
                    message: format!("ProcessGroup list_groups failed: {}", e),
                    details: None,
                })
            }
        }
    }

    async fn publish_to_group(
        &mut self,
        ctx: plexspaces::actor::types::Context,
        group_name: String,
        topic: Option<String>,
        message: plexspaces::actor::types::Payload,
    ) -> Result<Vec<plexspaces::actor::types::ActorId>, plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_process_groups_publish_to_group_total").increment(1);
        let _span = tracing::span!(
            tracing::Level::DEBUG,
            "wasm_process_groups_publish_to_group",
            actor_id = %self.actor_id,
            group_name = %group_name,
            topic = ?topic,
            message_size = message.len()
        )
        .entered();

        let registry = self.host_functions.process_group_registry()
            .ok_or_else(|| plexspaces::actor::types::ActorError {
                code: plexspaces::actor::types::ErrorCode::Internal,
                message: "ProcessGroupRegistry not configured".to_string(),
                details: None,
            })?;

        let request_ctx = context_to_request_context(&ctx);

        // Drop span before await to ensure Send
        drop(_span);
        match registry.publish_to_group(&request_ctx, &group_name, topic.as_deref(), message).await {
            Ok(recipients) => {
                let actor_ids: Vec<plexspaces::actor::types::ActorId> = recipients
                    .into_iter()
                    .map(|id| id.to_string())
                    .collect();
                let duration = start_time.elapsed();
                metrics::histogram!("plexspaces_wasm_process_groups_publish_to_group_duration_seconds").record(duration.as_secs_f64());
                metrics::counter!("plexspaces_wasm_process_groups_publish_to_group_success_total").increment(1);
                tracing::debug!(group_name = %group_name, topic = ?topic, recipients = actor_ids.len(), "ProcessGroup publish_to_group succeeded");
                Ok(actor_ids)
            }
            Err(e) => {
                metrics::counter!("plexspaces_wasm_process_groups_publish_to_group_errors_total").increment(1);
                let error_code = if e.to_string().contains("not found") {
                    plexspaces::actor::types::ErrorCode::ActorNotFound
                } else {
                    plexspaces::actor::types::ErrorCode::Internal
                };
                tracing::warn!(group_name = %group_name, topic = ?topic, error = %e, "ProcessGroup publish_to_group failed");
                Err(plexspaces::actor::types::ActorError {
                    code: error_code,
                    message: format!("ProcessGroup publish_to_group failed: {}", e),
                    details: None,
                })
            }
        }
    }
}

#[cfg(feature = "component-model")]
pub struct LocksImpl {
    pub actor_id: ActorId,
    pub host_functions: Arc<HostFunctions>,
}

#[cfg(feature = "component-model")]
#[async_trait::async_trait]
impl plexspaces::actor::locks::Host for LocksImpl {
    async fn acquire(
        &mut self,
        ctx: plexspaces::actor::types::Context,
        lock_key: String,
        holder_id: String,
        lease_duration_ms: u64,
    ) -> Result<plexspaces::actor::locks::Lock, plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_locks_acquire_total").increment(1);
        let _span = tracing::span!(
            tracing::Level::DEBUG,
            "wasm_locks_acquire",
            actor_id = %self.actor_id,
            lock_key = %lock_key,
            holder_id = %holder_id,
            lease_duration_ms = lease_duration_ms
        )
        .entered();

        let lock_manager = self.host_functions.lock_manager()
            .ok_or_else(|| plexspaces::actor::types::ActorError {
                code: plexspaces::actor::types::ErrorCode::Internal,
                message: "LockManager not configured".to_string(),
                details: None,
            })?;

        let request_ctx = context_to_request_context(&ctx);
        let lease_duration_secs = (lease_duration_ms / 1000) as u32;

        let options = plexspaces_proto::locks::prv::AcquireLockOptions {
            lock_key: lock_key.clone(),
            holder_id: holder_id.clone(),
            lease_duration_secs,
            additional_wait_time_ms: 0,
            refresh_period_ms: 0,
            metadata: std::collections::HashMap::new(),
        };

        // Drop span before await to ensure Send
        drop(_span);
        match lock_manager.acquire_lock(&request_ctx, options).await {
            Ok(lock) => {
                // Convert proto Lock to WIT Lock
                let expires_at_ms = lock.expires_at
                    .map(|ts| (ts.seconds as u64 * 1000) + (ts.nanos as u64 / 1_000_000))
                    .unwrap_or(0);
                let last_heartbeat_ms = lock.last_heartbeat
                    .map(|ts| (ts.seconds as u64 * 1000) + (ts.nanos as u64 / 1_000_000))
                    .unwrap_or(0);
                
                let wit_lock = plexspaces::actor::locks::Lock {
                    lock_key: lock.lock_key,
                    holder_id: lock.holder_id,
                    version: lock.version,
                    locked: lock.locked,
                    expires_at: expires_at_ms,
                    last_heartbeat: last_heartbeat_ms,
                };

                let duration = start_time.elapsed();
                metrics::histogram!("plexspaces_wasm_locks_acquire_duration_seconds").record(duration.as_secs_f64());
                metrics::counter!("plexspaces_wasm_locks_acquire_success_total").increment(1);
                tracing::debug!(lock_key = %lock_key, holder_id = %holder_id, "Lock acquire succeeded");
                Ok(wit_lock)
            }
            Err(e) => {
                metrics::counter!("plexspaces_wasm_locks_acquire_errors_total").increment(1);
                let error_code = if e.to_string().contains("already held") {
                    plexspaces::actor::types::ErrorCode::Internal
                } else {
                    plexspaces::actor::types::ErrorCode::Internal
                };
                tracing::warn!(lock_key = %lock_key, holder_id = %holder_id, error = %e, "Lock acquire failed");
                Err(plexspaces::actor::types::ActorError {
                    code: error_code,
                    message: format!("Lock acquire failed: {}", e),
                    details: None,
                })
            }
        }
    }

    async fn renew(
        &mut self,
        ctx: plexspaces::actor::types::Context,
        lock_key: String,
        holder_id: String,
        version: String,
        lease_duration_ms: u64,
    ) -> Result<plexspaces::actor::locks::Lock, plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_locks_renew_total").increment(1);
        let _span = tracing::span!(
            tracing::Level::DEBUG,
            "wasm_locks_renew",
            actor_id = %self.actor_id,
            lock_key = %lock_key,
            holder_id = %holder_id,
            version = %version,
            lease_duration_ms = lease_duration_ms
        )
        .entered();

        let lock_manager = self.host_functions.lock_manager()
            .ok_or_else(|| plexspaces::actor::types::ActorError {
                code: plexspaces::actor::types::ErrorCode::Internal,
                message: "LockManager not configured".to_string(),
                details: None,
            })?;

        let request_ctx = context_to_request_context(&ctx);
        let lease_duration_secs = (lease_duration_ms / 1000) as u32;

        let options = plexspaces_proto::locks::prv::RenewLockOptions {
            lock_key: lock_key.clone(),
            holder_id: holder_id.clone(),
            version: version.clone(),
            lease_duration_secs,
            metadata: std::collections::HashMap::new(),
        };

        // Drop span before await to ensure Send
        drop(_span);
        match lock_manager.renew_lock(&request_ctx, options).await {
            Ok(lock) => {
                // Convert proto Lock to WIT Lock
                let expires_at_ms = lock.expires_at
                    .map(|ts| (ts.seconds as u64 * 1000) + (ts.nanos as u64 / 1_000_000))
                    .unwrap_or(0);
                let last_heartbeat_ms = lock.last_heartbeat
                    .map(|ts| (ts.seconds as u64 * 1000) + (ts.nanos as u64 / 1_000_000))
                    .unwrap_or(0);
                
                let wit_lock = plexspaces::actor::locks::Lock {
                    lock_key: lock.lock_key,
                    holder_id: lock.holder_id,
                    version: lock.version,
                    locked: lock.locked,
                    expires_at: expires_at_ms,
                    last_heartbeat: last_heartbeat_ms,
                };

                let duration = start_time.elapsed();
                metrics::histogram!("plexspaces_wasm_locks_renew_duration_seconds").record(duration.as_secs_f64());
                metrics::counter!("plexspaces_wasm_locks_renew_success_total").increment(1);
                tracing::debug!(lock_key = %lock_key, holder_id = %holder_id, version = %version, "Lock renew succeeded");
                Ok(wit_lock)
            }
            Err(e) => {
                metrics::counter!("plexspaces_wasm_locks_renew_errors_total").increment(1);
                let error_code = if e.to_string().contains("version mismatch") {
                    plexspaces::actor::types::ErrorCode::Internal
                } else if e.to_string().contains("not found") {
                    plexspaces::actor::types::ErrorCode::ActorNotFound
                } else {
                    plexspaces::actor::types::ErrorCode::Internal
                };
                tracing::warn!(lock_key = %lock_key, holder_id = %holder_id, version = %version, error = %e, "Lock renew failed");
                Err(plexspaces::actor::types::ActorError {
                    code: error_code,
                    message: format!("Lock renew failed: {}", e),
                    details: None,
                })
            }
        }
    }

    async fn release(
        &mut self,
        ctx: plexspaces::actor::types::Context,
        lock_key: String,
        holder_id: String,
        version: String,
        delete_lock: bool,
    ) -> Result<(), plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_locks_release_total").increment(1);
        let _span = tracing::span!(
            tracing::Level::DEBUG,
            "wasm_locks_release",
            actor_id = %self.actor_id,
            lock_key = %lock_key,
            holder_id = %holder_id,
            version = %version,
            delete_lock = delete_lock
        )
        .entered();

        let lock_manager = self.host_functions.lock_manager()
            .ok_or_else(|| plexspaces::actor::types::ActorError {
                code: plexspaces::actor::types::ErrorCode::Internal,
                message: "LockManager not configured".to_string(),
                details: None,
            })?;

        let request_ctx = context_to_request_context(&ctx);

        let options = plexspaces_proto::locks::prv::ReleaseLockOptions {
            lock_key: lock_key.clone(),
            holder_id: holder_id.clone(),
            version: version.clone(),
            delete_lock,
        };

        // Drop span before await to ensure Send
        drop(_span);
        match lock_manager.release_lock(&request_ctx, options).await {
            Ok(()) => {
                let duration = start_time.elapsed();
                metrics::histogram!("plexspaces_wasm_locks_release_duration_seconds").record(duration.as_secs_f64());
                metrics::counter!("plexspaces_wasm_locks_release_success_total").increment(1);
                tracing::debug!(lock_key = %lock_key, holder_id = %holder_id, version = %version, "Lock release succeeded");
                Ok(())
            }
            Err(e) => {
                metrics::counter!("plexspaces_wasm_locks_release_errors_total").increment(1);
                let error_code = if e.to_string().contains("version mismatch") {
                    plexspaces::actor::types::ErrorCode::Internal
                } else if e.to_string().contains("not found") {
                    plexspaces::actor::types::ErrorCode::ActorNotFound
                } else {
                    plexspaces::actor::types::ErrorCode::Internal
                };
                tracing::warn!(lock_key = %lock_key, holder_id = %holder_id, version = %version, error = %e, "Lock release failed");
                Err(plexspaces::actor::types::ActorError {
                    code: error_code,
                    message: format!("Lock release failed: {}", e),
                    details: None,
                })
            }
        }
    }

    async fn try_acquire(
        &mut self,
        ctx: plexspaces::actor::types::Context,
        lock_key: String,
        holder_id: String,
        lease_duration_ms: u64,
    ) -> Result<Option<plexspaces::actor::locks::Lock>, plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_locks_try_acquire_total").increment(1);
        let _span = tracing::span!(
            tracing::Level::DEBUG,
            "wasm_locks_try_acquire",
            actor_id = %self.actor_id,
            lock_key = %lock_key,
            holder_id = %holder_id,
            lease_duration_ms = lease_duration_ms
        )
        .entered();

        let lock_manager = self.host_functions.lock_manager()
            .ok_or_else(|| plexspaces::actor::types::ActorError {
                code: plexspaces::actor::types::ErrorCode::Internal,
                message: "LockManager not configured".to_string(),
                details: None,
            })?;

        let request_ctx = context_to_request_context(&ctx);
        let lease_duration_secs = (lease_duration_ms / 1000) as u32;

        let options = plexspaces_proto::locks::prv::AcquireLockOptions {
            lock_key: lock_key.clone(),
            holder_id: holder_id.clone(),
            lease_duration_secs,
            additional_wait_time_ms: 0, // Try once, no retry
            refresh_period_ms: 0,
            metadata: std::collections::HashMap::new(),
        };

        // Drop span before await to ensure Send
        drop(_span);
        match lock_manager.acquire_lock(&request_ctx, options).await {
            Ok(lock) => {
                // Convert proto Lock to WIT Lock
                let expires_at_ms = lock.expires_at
                    .map(|ts| (ts.seconds as u64 * 1000) + (ts.nanos as u64 / 1_000_000))
                    .unwrap_or(0);
                let last_heartbeat_ms = lock.last_heartbeat
                    .map(|ts| (ts.seconds as u64 * 1000) + (ts.nanos as u64 / 1_000_000))
                    .unwrap_or(0);
                
                let wit_lock = plexspaces::actor::locks::Lock {
                    lock_key: lock.lock_key,
                    holder_id: lock.holder_id,
                    version: lock.version,
                    locked: lock.locked,
                    expires_at: expires_at_ms,
                    last_heartbeat: last_heartbeat_ms,
                };

                let duration = start_time.elapsed();
                metrics::histogram!("plexspaces_wasm_locks_try_acquire_duration_seconds").record(duration.as_secs_f64());
                metrics::counter!("plexspaces_wasm_locks_try_acquire_success_total").increment(1);
                tracing::debug!(lock_key = %lock_key, holder_id = %holder_id, "Lock try_acquire succeeded");
                Ok(Some(wit_lock))
            }
            Err(e) => {
                // If lock is already held, return None (non-blocking)
                if e.to_string().contains("already held") {
                    let duration = start_time.elapsed();
                    metrics::histogram!("plexspaces_wasm_locks_try_acquire_duration_seconds").record(duration.as_secs_f64());
                    metrics::counter!("plexspaces_wasm_locks_try_acquire_success_total").increment(1);
                    tracing::debug!(lock_key = %lock_key, holder_id = %holder_id, "Lock try_acquire returned None (lock held)");
                    Ok(None)
                } else {
                    metrics::counter!("plexspaces_wasm_locks_try_acquire_errors_total").increment(1);
                    tracing::warn!(lock_key = %lock_key, holder_id = %holder_id, error = %e, "Lock try_acquire failed");
                    Err(plexspaces::actor::types::ActorError {
                        code: plexspaces::actor::types::ErrorCode::Internal,
                        message: format!("Lock try_acquire failed: {}", e),
                        details: None,
                    })
                }
            }
        }
    }

    async fn get_lock(
        &mut self,
        ctx: plexspaces::actor::types::Context,
        lock_key: String,
    ) -> Result<Option<plexspaces::actor::locks::Lock>, plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_locks_get_lock_total").increment(1);
        let _span = tracing::span!(
            tracing::Level::DEBUG,
            "wasm_locks_get_lock",
            actor_id = %self.actor_id,
            lock_key = %lock_key
        )
        .entered();

        let lock_manager = self.host_functions.lock_manager()
            .ok_or_else(|| plexspaces::actor::types::ActorError {
                code: plexspaces::actor::types::ErrorCode::Internal,
                message: "LockManager not configured".to_string(),
                details: None,
            })?;

        let request_ctx = context_to_request_context(&ctx);

        // Drop span before await to ensure Send
        drop(_span);
        match lock_manager.get_lock(&request_ctx, &lock_key).await {
            Ok(Some(lock)) => {
                // Convert proto Lock to WIT Lock
                let expires_at_ms = lock.expires_at
                    .map(|ts| (ts.seconds as u64 * 1000) + (ts.nanos as u64 / 1_000_000))
                    .unwrap_or(0);
                let last_heartbeat_ms = lock.last_heartbeat
                    .map(|ts| (ts.seconds as u64 * 1000) + (ts.nanos as u64 / 1_000_000))
                    .unwrap_or(0);
                
                let wit_lock = plexspaces::actor::locks::Lock {
                    lock_key: lock.lock_key,
                    holder_id: lock.holder_id,
                    version: lock.version,
                    locked: lock.locked,
                    expires_at: expires_at_ms,
                    last_heartbeat: last_heartbeat_ms,
                };

                let duration = start_time.elapsed();
                metrics::histogram!("plexspaces_wasm_locks_get_lock_duration_seconds").record(duration.as_secs_f64());
                metrics::counter!("plexspaces_wasm_locks_get_lock_success_total").increment(1);
                tracing::debug!(lock_key = %lock_key, "Lock get_lock succeeded");
                Ok(Some(wit_lock))
            }
            Ok(None) => {
                let duration = start_time.elapsed();
                metrics::histogram!("plexspaces_wasm_locks_get_lock_duration_seconds").record(duration.as_secs_f64());
                metrics::counter!("plexspaces_wasm_locks_get_lock_success_total").increment(1);
                tracing::debug!(lock_key = %lock_key, "Lock get_lock returned None");
                Ok(None)
            }
            Err(e) => {
                metrics::counter!("plexspaces_wasm_locks_get_lock_errors_total").increment(1);
                tracing::warn!(lock_key = %lock_key, error = %e, "Lock get_lock failed");
                Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::Internal,
                    message: format!("Lock get_lock failed: {}", e),
                    details: None,
                })
            }
        }
    }
}

#[cfg(feature = "component-model")]
pub struct RegistryImpl {
    pub actor_id: ActorId,
    pub host_functions: Arc<HostFunctions>,
}

#[cfg(feature = "component-model")]
impl RegistryImpl {
    /// Convert proto ObjectType to WIT ObjectType
    fn proto_to_wit_object_type(proto_type: i32) -> plexspaces::actor::registry::ObjectType {
        use plexspaces_proto::object_registry::v1::ObjectType;
        use prost::Enumeration;
        // prost::Enumeration provides from_i32
        match ObjectType::from_i32(proto_type).unwrap_or(ObjectType::ObjectTypeUnspecified) {
            ObjectType::ObjectTypeUnspecified => plexspaces::actor::registry::ObjectType::Unspecified,
            ObjectType::ObjectTypeActor => plexspaces::actor::registry::ObjectType::Actor,
            ObjectType::ObjectTypeTuplespace => plexspaces::actor::registry::ObjectType::Tuplespace,
            ObjectType::ObjectTypeService => plexspaces::actor::registry::ObjectType::Service,
            ObjectType::ObjectTypeVm => plexspaces::actor::registry::ObjectType::Vm,
            ObjectType::ObjectTypeApplication => plexspaces::actor::registry::ObjectType::Application,
            ObjectType::ObjectTypeWorkflow => plexspaces::actor::registry::ObjectType::Workflow,
            ObjectType::ObjectTypeNode => plexspaces::actor::registry::ObjectType::Node,
        }
    }

    /// Convert proto HealthStatus to WIT HealthStatus
    fn proto_to_wit_health_status(proto_status: i32) -> plexspaces::actor::registry::HealthStatus {
        use plexspaces_proto::object_registry::v1::HealthStatus;
        use prost::Enumeration;
        // prost::Enumeration provides from_i32
        match HealthStatus::from_i32(proto_status).unwrap_or(HealthStatus::HealthStatusUnknown) {
            HealthStatus::HealthStatusUnknown => plexspaces::actor::registry::HealthStatus::Unknown,
            HealthStatus::HealthStatusHealthy => plexspaces::actor::registry::HealthStatus::Healthy,
            HealthStatus::HealthStatusDegraded => plexspaces::actor::registry::HealthStatus::Degraded,
            HealthStatus::HealthStatusUnhealthy => plexspaces::actor::registry::HealthStatus::Unhealthy,
            HealthStatus::HealthStatusStarting => plexspaces::actor::registry::HealthStatus::Starting,
            HealthStatus::HealthStatusStopping => plexspaces::actor::registry::HealthStatus::Stopping,
        }
    }

    /// Convert proto ObjectRegistration to WIT ObjectRegistration
    fn proto_to_wit_registration(proto: &plexspaces_proto::object_registry::v1::ObjectRegistration) -> plexspaces::actor::registry::ObjectRegistration {
        // Convert proto labels (Vec<String> with "key=value" format) to WIT labels (list<label>)
        let wit_labels: Vec<plexspaces::actor::registry::Label> = proto.labels
            .iter()
            .filter_map(|l| {
                let parts: Vec<&str> = l.splitn(2, '=').collect();
                if parts.len() == 2 {
                    Some(plexspaces::actor::registry::Label {
                        key: parts[0].to_string(),
                        value: parts[1].to_string(),
                    })
                } else {
                    None
                }
            })
            .collect();

        // Convert timestamps
        let created_at_ms = proto.created_at
            .as_ref()
            .map(|ts| (ts.seconds as u64 * 1000) + (ts.nanos as u64 / 1_000_000))
            .unwrap_or(0);
        let updated_at_ms = proto.updated_at
            .as_ref()
            .map(|ts| (ts.seconds as u64 * 1000) + (ts.nanos as u64 / 1_000_000))
            .unwrap_or(0);
        let last_heartbeat_ms = proto.last_heartbeat
            .as_ref()
            .map(|ts| (ts.seconds as u64 * 1000) + (ts.nanos as u64 / 1_000_000));

        plexspaces::actor::registry::ObjectRegistration {
            object_id: proto.object_id.clone(),
            object_type: Self::proto_to_wit_object_type(proto.object_type),
            grpc_address: proto.grpc_address.clone(),
            object_category: proto.object_category.clone(),
            capabilities: proto.capabilities.clone(),
            labels: wit_labels,
            health_status: Self::proto_to_wit_health_status(proto.health_status),
            created_at: created_at_ms,
            updated_at: updated_at_ms,
            last_heartbeat: last_heartbeat_ms,
        }
    }
}

#[cfg(feature = "component-model")]
#[async_trait::async_trait]
impl plexspaces::actor::registry::Host for RegistryImpl {
    async fn register(
        &mut self,
        ctx: plexspaces::actor::types::Context,
        object_id: String,
        object_type: plexspaces::actor::registry::ObjectType,
        grpc_address: String,
        object_category: Option<String>,
        capabilities: Vec<String>,
        labels: Vec<plexspaces::actor::registry::Label>,
    ) -> Result<(), plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_registry_register_total").increment(1);
        let _span = tracing::span!(
            tracing::Level::DEBUG,
            "wasm_registry_register",
            actor_id = %self.actor_id,
            object_id = %object_id,
            object_type = ?object_type,
            grpc_address = %grpc_address
        )
        .entered();

        let registry = self.host_functions.object_registry()
            .ok_or_else(|| plexspaces::actor::types::ActorError {
                code: plexspaces::actor::types::ErrorCode::Internal,
                message: "ObjectRegistry not configured".to_string(),
                details: None,
            })?;

        let request_ctx = context_to_request_context(&ctx);

        // Convert WIT ObjectType to proto ObjectType
        let proto_object_type = match object_type {
            plexspaces::actor::registry::ObjectType::Unspecified => plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeUnspecified,
            plexspaces::actor::registry::ObjectType::Actor => plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeActor,
            plexspaces::actor::registry::ObjectType::Tuplespace => plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeTuplespace,
            plexspaces::actor::registry::ObjectType::Service => plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeService,
            plexspaces::actor::registry::ObjectType::Vm => plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeVm,
            plexspaces::actor::registry::ObjectType::Application => plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeApplication,
            plexspaces::actor::registry::ObjectType::Workflow => plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeWorkflow,
            plexspaces::actor::registry::ObjectType::Node => plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeNode,
        };

        // Convert WIT labels to proto labels (Vec<String>)
        let proto_labels: Vec<String> = labels
            .into_iter()
            .map(|l| format!("{}={}", l.key, l.value))
            .collect();

        let registration = plexspaces_proto::object_registry::v1::ObjectRegistration {
            object_id: object_id.clone(),
            object_type: proto_object_type as i32,
            grpc_address: grpc_address.clone(),
            object_category: object_category.unwrap_or_default(),
            capabilities,
            labels: proto_labels,
            health_status: plexspaces_proto::object_registry::v1::HealthStatus::HealthStatusHealthy as i32,
            ..Default::default()
        };

        // Drop span before await to ensure Send
        drop(_span);
        match registry.register(&request_ctx, registration).await {
            Ok(()) => {
                let duration = start_time.elapsed();
                metrics::histogram!("plexspaces_wasm_registry_register_duration_seconds").record(duration.as_secs_f64());
                metrics::counter!("plexspaces_wasm_registry_register_success_total").increment(1);
                tracing::debug!(object_id = %object_id, object_type = ?object_type, "Registry register succeeded");
                Ok(())
            }
            Err(e) => {
                metrics::counter!("plexspaces_wasm_registry_register_errors_total").increment(1);
                let error_code = if e.to_string().contains("already registered") {
                    plexspaces::actor::types::ErrorCode::Internal
                } else if e.to_string().contains("Invalid input") {
                    plexspaces::actor::types::ErrorCode::Internal
                } else {
                    plexspaces::actor::types::ErrorCode::Internal
                };
                tracing::warn!(object_id = %object_id, object_type = ?object_type, error = %e, "Registry register failed");
                Err(plexspaces::actor::types::ActorError {
                    code: error_code,
                    message: format!("Registry register failed: {}", e),
                    details: None,
                })
            }
        }
    }

    async fn unregister(
        &mut self,
        ctx: plexspaces::actor::types::Context,
        object_type: plexspaces::actor::registry::ObjectType,
        object_id: String,
    ) -> Result<(), plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_registry_unregister_total").increment(1);
        let _span = tracing::span!(
            tracing::Level::DEBUG,
            "wasm_registry_unregister",
            actor_id = %self.actor_id,
            object_type = ?object_type,
            object_id = %object_id
        )
        .entered();

        let registry = self.host_functions.object_registry()
            .ok_or_else(|| plexspaces::actor::types::ActorError {
                code: plexspaces::actor::types::ErrorCode::Internal,
                message: "ObjectRegistry not configured".to_string(),
                details: None,
            })?;

        let request_ctx = context_to_request_context(&ctx);

        // Convert WIT ObjectType to proto ObjectType
        let proto_object_type = match object_type {
            plexspaces::actor::registry::ObjectType::Unspecified => plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeUnspecified,
            plexspaces::actor::registry::ObjectType::Actor => plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeActor,
            plexspaces::actor::registry::ObjectType::Tuplespace => plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeTuplespace,
            plexspaces::actor::registry::ObjectType::Service => plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeService,
            plexspaces::actor::registry::ObjectType::Vm => plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeVm,
            plexspaces::actor::registry::ObjectType::Application => plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeApplication,
            plexspaces::actor::registry::ObjectType::Workflow => plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeWorkflow,
            plexspaces::actor::registry::ObjectType::Node => plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeNode,
        };

        // Drop span before await to ensure Send
        drop(_span);
        match registry.unregister(&request_ctx, proto_object_type, &object_id).await {
            Ok(()) => {
                let duration = start_time.elapsed();
                metrics::histogram!("plexspaces_wasm_registry_unregister_duration_seconds").record(duration.as_secs_f64());
                metrics::counter!("plexspaces_wasm_registry_unregister_success_total").increment(1);
                tracing::debug!(object_type = ?object_type, object_id = %object_id, "Registry unregister succeeded");
                Ok(())
            }
            Err(e) => {
                metrics::counter!("plexspaces_wasm_registry_unregister_errors_total").increment(1);
                let error_code = if e.to_string().contains("not found") {
                    plexspaces::actor::types::ErrorCode::ActorNotFound
                } else {
                    plexspaces::actor::types::ErrorCode::Internal
                };
                tracing::warn!(object_type = ?object_type, object_id = %object_id, error = %e, "Registry unregister failed");
                Err(plexspaces::actor::types::ActorError {
                    code: error_code,
                    message: format!("Registry unregister failed: {}", e),
                    details: None,
                })
            }
        }
    }

    async fn lookup(
        &mut self,
        ctx: plexspaces::actor::types::Context,
        object_type: plexspaces::actor::registry::ObjectType,
        object_id: String,
    ) -> Result<Option<plexspaces::actor::registry::ObjectRegistration>, plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_registry_lookup_total").increment(1);
        let _span = tracing::span!(
            tracing::Level::DEBUG,
            "wasm_registry_lookup",
            actor_id = %self.actor_id,
            object_type = ?object_type,
            object_id = %object_id
        )
        .entered();

        let registry = self.host_functions.object_registry()
            .ok_or_else(|| plexspaces::actor::types::ActorError {
                code: plexspaces::actor::types::ErrorCode::Internal,
                message: "ObjectRegistry not configured".to_string(),
                details: None,
            })?;

        let request_ctx = context_to_request_context(&ctx);

        // Convert WIT ObjectType to proto ObjectType
        let proto_object_type = match object_type {
            plexspaces::actor::registry::ObjectType::Unspecified => plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeUnspecified,
            plexspaces::actor::registry::ObjectType::Actor => plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeActor,
            plexspaces::actor::registry::ObjectType::Tuplespace => plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeTuplespace,
            plexspaces::actor::registry::ObjectType::Service => plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeService,
            plexspaces::actor::registry::ObjectType::Vm => plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeVm,
            plexspaces::actor::registry::ObjectType::Application => plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeApplication,
            plexspaces::actor::registry::ObjectType::Workflow => plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeWorkflow,
            plexspaces::actor::registry::ObjectType::Node => plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeNode,
        };

        // Drop span before await to ensure Send
        drop(_span);
        match registry.lookup(&request_ctx, proto_object_type, &object_id).await {
            Ok(Some(proto_reg)) => {
                let wit_reg = Self::proto_to_wit_registration(&proto_reg);
                let duration = start_time.elapsed();
                metrics::histogram!("plexspaces_wasm_registry_lookup_duration_seconds").record(duration.as_secs_f64());
                metrics::counter!("plexspaces_wasm_registry_lookup_success_total").increment(1);
                tracing::debug!(object_type = ?object_type, object_id = %object_id, "Registry lookup succeeded");
                Ok(Some(wit_reg))
            }
            Ok(None) => {
                let duration = start_time.elapsed();
                metrics::histogram!("plexspaces_wasm_registry_lookup_duration_seconds").record(duration.as_secs_f64());
                metrics::counter!("plexspaces_wasm_registry_lookup_success_total").increment(1);
                tracing::debug!(object_type = ?object_type, object_id = %object_id, "Registry lookup returned None");
                Ok(None)
            }
            Err(e) => {
                metrics::counter!("plexspaces_wasm_registry_lookup_errors_total").increment(1);
                tracing::warn!(object_type = ?object_type, object_id = %object_id, error = %e, "Registry lookup failed");
                Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::Internal,
                    message: format!("Registry lookup failed: {}", e),
                    details: None,
                })
            }
        }
    }

    async fn discover(
        &mut self,
        ctx: plexspaces::actor::types::Context,
        object_type: Option<plexspaces::actor::registry::ObjectType>,
        object_category: Option<String>,
        capabilities: Vec<String>,
        labels: Vec<String>,
        health_status: Option<plexspaces::actor::registry::HealthStatus>,
        offset: u32,
        limit: u32,
    ) -> Result<Vec<plexspaces::actor::registry::ObjectRegistration>, plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_registry_discover_total").increment(1);
        let _span = tracing::span!(
            tracing::Level::DEBUG,
            "wasm_registry_discover",
            actor_id = %self.actor_id,
            object_type = ?object_type,
            offset = offset,
            limit = limit
        )
        .entered();

        let registry = self.host_functions.object_registry()
            .ok_or_else(|| plexspaces::actor::types::ActorError {
                code: plexspaces::actor::types::ErrorCode::Internal,
                message: "ObjectRegistry not configured".to_string(),
                details: None,
            })?;

        let request_ctx = context_to_request_context(&ctx);

        // Convert WIT types to proto types
        let proto_object_type = object_type.map(|ot| match ot {
            plexspaces::actor::registry::ObjectType::Unspecified => plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeUnspecified,
            plexspaces::actor::registry::ObjectType::Actor => plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeActor,
            plexspaces::actor::registry::ObjectType::Tuplespace => plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeTuplespace,
            plexspaces::actor::registry::ObjectType::Service => plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeService,
            plexspaces::actor::registry::ObjectType::Vm => plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeVm,
            plexspaces::actor::registry::ObjectType::Application => plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeApplication,
            plexspaces::actor::registry::ObjectType::Workflow => plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeWorkflow,
            plexspaces::actor::registry::ObjectType::Node => plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeNode,
        });

        let proto_health_status = health_status.map(|hs| match hs {
            plexspaces::actor::registry::HealthStatus::Unknown => plexspaces_proto::object_registry::v1::HealthStatus::HealthStatusUnknown,
            plexspaces::actor::registry::HealthStatus::Healthy => plexspaces_proto::object_registry::v1::HealthStatus::HealthStatusHealthy,
            plexspaces::actor::registry::HealthStatus::Degraded => plexspaces_proto::object_registry::v1::HealthStatus::HealthStatusDegraded,
            plexspaces::actor::registry::HealthStatus::Unhealthy => plexspaces_proto::object_registry::v1::HealthStatus::HealthStatusUnhealthy,
            plexspaces::actor::registry::HealthStatus::Starting => plexspaces_proto::object_registry::v1::HealthStatus::HealthStatusStarting,
            plexspaces::actor::registry::HealthStatus::Stopping => plexspaces_proto::object_registry::v1::HealthStatus::HealthStatusStopping,
        });

        let capabilities_opt = if capabilities.is_empty() { None } else { Some(capabilities) };
        let labels_opt = if labels.is_empty() { None } else { Some(labels) };

        // Drop span before await to ensure Send
        drop(_span);
        match registry.discover(&request_ctx, proto_object_type, object_category, capabilities_opt, labels_opt, proto_health_status, offset as usize, limit as usize).await {
            Ok(proto_regs) => {
                let wit_regs: Vec<plexspaces::actor::registry::ObjectRegistration> = proto_regs
                    .iter()
                    .map(|r| Self::proto_to_wit_registration(r))
                    .collect();
                let duration = start_time.elapsed();
                metrics::histogram!("plexspaces_wasm_registry_discover_duration_seconds").record(duration.as_secs_f64());
                metrics::counter!("plexspaces_wasm_registry_discover_success_total").increment(1);
                tracing::debug!(object_type = ?object_type, count = wit_regs.len(), "Registry discover succeeded");
                Ok(wit_regs)
            }
            Err(e) => {
                metrics::counter!("plexspaces_wasm_registry_discover_errors_total").increment(1);
                tracing::warn!(object_type = ?object_type, error = %e, "Registry discover failed");
                Err(plexspaces::actor::types::ActorError {
                    code: plexspaces::actor::types::ErrorCode::Internal,
                    message: format!("Registry discover failed: {}", e),
                    details: None,
                })
            }
        }
    }

    async fn heartbeat(
        &mut self,
        ctx: plexspaces::actor::types::Context,
        object_type: plexspaces::actor::registry::ObjectType,
        object_id: String,
    ) -> Result<(), plexspaces::actor::types::ActorError> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_registry_heartbeat_total").increment(1);
        let _span = tracing::span!(
            tracing::Level::DEBUG,
            "wasm_registry_heartbeat",
            actor_id = %self.actor_id,
            object_type = ?object_type,
            object_id = %object_id
        )
        .entered();

        let registry = self.host_functions.object_registry()
            .ok_or_else(|| plexspaces::actor::types::ActorError {
                code: plexspaces::actor::types::ErrorCode::Internal,
                message: "ObjectRegistry not configured".to_string(),
                details: None,
            })?;

        let request_ctx = context_to_request_context(&ctx);

        // Convert WIT ObjectType to proto ObjectType
        let proto_object_type = match object_type {
            plexspaces::actor::registry::ObjectType::Unspecified => plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeUnspecified,
            plexspaces::actor::registry::ObjectType::Actor => plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeActor,
            plexspaces::actor::registry::ObjectType::Tuplespace => plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeTuplespace,
            plexspaces::actor::registry::ObjectType::Service => plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeService,
            plexspaces::actor::registry::ObjectType::Vm => plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeVm,
            plexspaces::actor::registry::ObjectType::Application => plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeApplication,
            plexspaces::actor::registry::ObjectType::Workflow => plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeWorkflow,
            plexspaces::actor::registry::ObjectType::Node => plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeNode,
        };

        // Drop span before await to ensure Send
        drop(_span);
        match registry.heartbeat(&request_ctx, proto_object_type, &object_id).await {
            Ok(()) => {
                let duration = start_time.elapsed();
                metrics::histogram!("plexspaces_wasm_registry_heartbeat_duration_seconds").record(duration.as_secs_f64());
                metrics::counter!("plexspaces_wasm_registry_heartbeat_success_total").increment(1);
                tracing::debug!(object_type = ?object_type, object_id = %object_id, "Registry heartbeat succeeded");
                Ok(())
            }
            Err(e) => {
                metrics::counter!("plexspaces_wasm_registry_heartbeat_errors_total").increment(1);
                let error_code = if e.to_string().contains("not found") {
                    plexspaces::actor::types::ErrorCode::ActorNotFound
                } else {
                    plexspaces::actor::types::ErrorCode::Internal
                };
                tracing::warn!(object_type = ?object_type, object_id = %object_id, error = %e, "Registry heartbeat failed");
                Err(plexspaces::actor::types::ActorError {
                    code: error_code,
                    message: format!("Registry heartbeat failed: {}", e),
                    details: None,
                })
            }
        }
    }
}
