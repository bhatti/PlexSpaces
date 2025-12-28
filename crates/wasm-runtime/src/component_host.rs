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
use plexspaces_core::ActorId;
use std::sync::Arc;
use wasmtime::Result as WasmtimeResult;

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
}

#[cfg(feature = "component-model")]
#[async_trait::async_trait]
impl plexspaces::actor::messaging::Host for MessagingImpl {
    async fn tell(
        &mut self,
        to: plexspaces::actor::types::ActorId,
        _msg_type: String,
        payload: plexspaces::actor::types::Payload,
    ) -> Result<plexspaces::actor::types::MessageId, plexspaces::actor::types::ActorError> {
        let message_str = String::from_utf8_lossy(&payload).to_string();
        match self.host_functions.send_message(&self.actor_id, &to, &message_str).await {
            Ok(_) => Ok(ulid::Ulid::new().to_string()),
            Err(e) => Err(plexspaces::actor::types::ActorError {
                code: plexspaces::actor::types::ErrorCode::Internal,
                message: e,
                details: None,
            }),
        }
    }

    async fn ask(
        &mut self,
        _to: plexspaces::actor::types::ActorId,
        _msg_type: String,
        _payload: plexspaces::actor::types::Payload,
        _timeout_ms: u64,
    ) -> Result<plexspaces::actor::types::Payload, plexspaces::actor::types::ActorError> {
        // TODO: Implement ask/request-reply pattern
        Err(plexspaces::actor::types::ActorError {
            code: plexspaces::actor::types::ErrorCode::NotImplemented,
            message: "ask not yet implemented".to_string(),
            details: None,
        })
    }

    async fn reply(
        &mut self,
        _correlation_id: plexspaces::actor::types::CorrelationId,
        _payload: plexspaces::actor::types::Payload,
    ) -> Result<(), plexspaces::actor::types::ActorError> {
        // TODO: Implement reply
        Err(plexspaces::actor::types::ActorError {
            code: plexspaces::actor::types::ErrorCode::NotImplemented,
            message: "reply not yet implemented".to_string(),
            details: None,
        })
    }

    async fn forward(
        &mut self,
        _to: plexspaces::actor::types::ActorId,
        _msg_type: String,
        _payload: plexspaces::actor::types::Payload,
        _original_sender: plexspaces::actor::types::ActorId,
        _correlation_id: Option<plexspaces::actor::types::CorrelationId>,
    ) -> Result<plexspaces::actor::types::MessageId, plexspaces::actor::types::ActorError> {
        // TODO: Implement forward
        Err(plexspaces::actor::types::ActorError {
            code: plexspaces::actor::types::ErrorCode::NotImplemented,
            message: "forward not yet implemented".to_string(),
            details: None,
        })
    }

    async fn spawn(
        &mut self,
        _module_ref: String,
        _initial_state: plexspaces::actor::types::Payload,
        _options: plexspaces::actor::types::SpawnOptions,
    ) -> Result<plexspaces::actor::types::ActorId, plexspaces::actor::types::ActorError> {
        // TODO: Implement spawn
        Err(plexspaces::actor::types::ActorError {
            code: plexspaces::actor::types::ErrorCode::NotImplemented,
            message: "spawn not yet implemented".to_string(),
            details: None,
        })
    }

    async fn stop(
        &mut self,
        _actor_id: plexspaces::actor::types::ActorId,
        _timeout_ms: u64,
    ) -> Result<(), plexspaces::actor::types::ActorError> {
        // TODO: Implement stop
        Err(plexspaces::actor::types::ActorError {
            code: plexspaces::actor::types::ErrorCode::NotImplemented,
            message: "stop not yet implemented".to_string(),
            details: None,
        })
    }

    async fn link(
        &mut self,
        _actor_id: plexspaces::actor::types::ActorId,
    ) -> Result<(), plexspaces::actor::types::ActorError> {
        // TODO: Implement link
        Err(plexspaces::actor::types::ActorError {
            code: plexspaces::actor::types::ErrorCode::NotImplemented,
            message: "link not yet implemented".to_string(),
            details: None,
        })
    }

    async fn unlink(
        &mut self,
        _actor_id: plexspaces::actor::types::ActorId,
    ) -> Result<(), plexspaces::actor::types::ActorError> {
        // TODO: Implement unlink
        Err(plexspaces::actor::types::ActorError {
            code: plexspaces::actor::types::ErrorCode::NotImplemented,
            message: "unlink not yet implemented".to_string(),
            details: None,
        })
    }

    async fn monitor(
        &mut self,
        _actor_id: plexspaces::actor::types::ActorId,
    ) -> Result<u64, plexspaces::actor::types::ActorError> {
        // TODO: Implement monitor
        Err(plexspaces::actor::types::ActorError {
            code: plexspaces::actor::types::ErrorCode::NotImplemented,
            message: "monitor not yet implemented".to_string(),
            details: None,
        })
    }

    async fn demonitor(
        &mut self,
        _monitor_ref: u64,
    ) -> Result<(), plexspaces::actor::types::ActorError> {
        // TODO: Implement demonitor
        Err(plexspaces::actor::types::ActorError {
            code: plexspaces::actor::types::ErrorCode::NotImplemented,
            message: "demonitor not yet implemented".to_string(),
            details: None,
        })
    }

    async fn self_id(&mut self) -> plexspaces::actor::types::ActorId {
        self.actor_id.clone()
    }

    async fn parent_id(&mut self) -> Option<plexspaces::actor::types::ActorId> {
        // TODO: Implement parent_id
        None
    }

    async fn now(&mut self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    }

    async fn sleep(&mut self, duration_ms: u64) {
        tokio::time::sleep(tokio::time::Duration::from_millis(duration_ms)).await;
    }

    async fn send_after(
        &mut self,
        _delay_ms: u64,
        _msg_type: String,
        _payload: plexspaces::actor::types::Payload,
    ) -> Result<u64, plexspaces::actor::types::ActorError> {
        // TODO: Implement send_after
        Err(plexspaces::actor::types::ActorError {
            code: plexspaces::actor::types::ErrorCode::NotImplemented,
            message: "send_after not yet implemented".to_string(),
            details: None,
        })
    }

    async fn cancel_timer(
        &mut self,
        _timer_id: u64,
    ) -> Result<(), plexspaces::actor::types::ActorError> {
        // TODO: Implement cancel_timer
        Err(plexspaces::actor::types::ActorError {
            code: plexspaces::actor::types::ErrorCode::NotImplemented,
            message: "cancel_timer not yet implemented".to_string(),
            details: None,
        })
    }
}

#[cfg(feature = "component-model")]
pub struct TuplespaceImpl;

#[cfg(feature = "component-model")]
#[async_trait::async_trait]
impl plexspaces::actor::tuplespace::Host for TuplespaceImpl {
    async fn write(
        &mut self,
        _tuple_data: plexspaces::actor::types::TupleData,
    ) -> Result<(), plexspaces::actor::types::ActorError> {
        // TODO: Implement tuplespace write
        tracing::warn!("Tuplespace write not yet implemented for components");
        Ok(())
    }

    async fn write_with_ttl(
        &mut self,
        _tuple_data: plexspaces::actor::types::TupleData,
        _ttl_ms: u64,
    ) -> Result<(), plexspaces::actor::types::ActorError> {
        // TODO: Implement tuplespace write_with_ttl
        tracing::warn!("Tuplespace write_with_ttl not yet implemented for components");
        Ok(())
    }

    async fn read(
        &mut self,
        _pattern_data: plexspaces::actor::types::Pattern,
    ) -> Result<Option<plexspaces::actor::types::TupleData>, plexspaces::actor::types::ActorError> {
        // TODO: Implement tuplespace read
        tracing::warn!("Tuplespace read not yet implemented for components");
        Ok(None)
    }

    async fn read_blocking(
        &mut self,
        _pattern_data: plexspaces::actor::types::Pattern,
        _timeout_ms: u64,
    ) -> Result<Option<plexspaces::actor::types::TupleData>, plexspaces::actor::types::ActorError> {
        // TODO: Implement tuplespace read_blocking
        tracing::warn!("Tuplespace read_blocking not yet implemented for components");
        Ok(None)
    }

    async fn read_all(
        &mut self,
        _pattern_data: plexspaces::actor::types::Pattern,
        _limit: u32,
    ) -> Result<Vec<plexspaces::actor::types::TupleData>, plexspaces::actor::types::ActorError> {
        // TODO: Implement tuplespace read_all
        tracing::warn!("Tuplespace read_all not yet implemented for components");
        Ok(vec![])
    }

    async fn take(
        &mut self,
        _pattern_data: plexspaces::actor::types::Pattern,
    ) -> Result<Option<plexspaces::actor::types::TupleData>, plexspaces::actor::types::ActorError> {
        // TODO: Implement tuplespace take
        tracing::warn!("Tuplespace take not yet implemented for components");
        Ok(None)
    }

    async fn take_blocking(
        &mut self,
        _pattern_data: plexspaces::actor::types::Pattern,
        _timeout_ms: u64,
    ) -> Result<Option<plexspaces::actor::types::TupleData>, plexspaces::actor::types::ActorError> {
        // TODO: Implement tuplespace take_blocking
        tracing::warn!("Tuplespace take_blocking not yet implemented for components");
        Ok(None)
    }

    async fn count(
        &mut self,
        _pattern_data: plexspaces::actor::types::Pattern,
    ) -> Result<u64, plexspaces::actor::types::ActorError> {
        // TODO: Implement tuplespace count
        tracing::warn!("Tuplespace count not yet implemented for components");
        Ok(0)
    }

    async fn subscribe(
        &mut self,
        _pattern_data: plexspaces::actor::types::Pattern,
    ) -> Result<u64, plexspaces::actor::types::ActorError> {
        // TODO: Implement tuplespace subscribe
        Err(plexspaces::actor::types::ActorError {
            code: plexspaces::actor::types::ErrorCode::NotImplemented,
            message: "subscribe not yet implemented".to_string(),
            details: None,
        })
    }

    async fn unsubscribe(
        &mut self,
        _subscription_id: u64,
    ) -> Result<(), plexspaces::actor::types::ActorError> {
        // TODO: Implement tuplespace unsubscribe
        Err(plexspaces::actor::types::ActorError {
            code: plexspaces::actor::types::ErrorCode::NotImplemented,
            message: "unsubscribe not yet implemented".to_string(),
            details: None,
        })
    }

    async fn compare_and_swap(
        &mut self,
        _pattern_data: plexspaces::actor::types::Pattern,
        _expected: plexspaces::actor::types::TupleData,
        _new_tuple: plexspaces::actor::types::TupleData,
    ) -> Result<bool, plexspaces::actor::types::ActorError> {
        // TODO: Implement tuplespace compare_and_swap
        Err(plexspaces::actor::types::ActorError {
            code: plexspaces::actor::types::ErrorCode::NotImplemented,
            message: "compare_and_swap not yet implemented".to_string(),
            details: None,
        })
    }
}

#[cfg(feature = "component-model")]
pub struct ChannelsImpl {
    pub host_functions: Arc<HostFunctions>,
}

#[cfg(feature = "component-model")]
#[async_trait::async_trait]
impl plexspaces::actor::channels::Host for ChannelsImpl {
    async fn send_to_queue(
        &mut self,
        queue_name: String,
        msg_type: String,
        payload: plexspaces::actor::types::Payload,
    ) -> Result<plexspaces::actor::types::MessageId, plexspaces::actor::types::ActorError> {
        match self.host_functions.send_to_queue(&queue_name, &msg_type, payload).await {
            Ok(message_id) => Ok(message_id),
            Err(e) => Err(plexspaces::actor::types::ActorError {
                code: plexspaces::actor::types::ErrorCode::Internal,
                message: e,
                details: None,
            }),
        }
    }

    async fn send_to_queue_with_options(
        &mut self,
        queue_name: String,
        msg_type: String,
        payload: plexspaces::actor::types::Payload,
        _delay_ms: u64,
        _ttl_ms: u64,
        _headers: Vec<(String, String)>,
    ) -> Result<plexspaces::actor::types::MessageId, plexspaces::actor::types::ActorError> {
        // For now, use basic send_to_queue
        self.send_to_queue(queue_name, msg_type, payload).await
    }

    async fn receive_from_queue(
        &mut self,
        queue_name: String,
        timeout_ms: u64,
    ) -> Result<Option<plexspaces::actor::channels::QueueMessage>, plexspaces::actor::types::ActorError> {
        match self.host_functions.receive_from_queue(&queue_name, timeout_ms).await {
            Ok(Some((msg_type, payload))) => {
                Ok(Some(plexspaces::actor::channels::QueueMessage {
                    id: ulid::Ulid::new().to_string(),
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
            Ok(None) => Ok(None),
            Err(e) => Err(plexspaces::actor::types::ActorError {
                code: plexspaces::actor::types::ErrorCode::Internal,
                message: e,
                details: None,
            }),
        }
    }

    async fn ack(
        &mut self,
        _queue_name: String,
        _message_id: plexspaces::actor::types::MessageId,
    ) -> Result<(), plexspaces::actor::types::ActorError> {
        // TODO: Implement ack
        Err(plexspaces::actor::types::ActorError {
            code: plexspaces::actor::types::ErrorCode::NotImplemented,
            message: "ack not yet implemented".to_string(),
            details: None,
        })
    }

    async fn nack(
        &mut self,
        _queue_name: String,
        _message_id: plexspaces::actor::types::MessageId,
        _requeue: bool,
    ) -> Result<(), plexspaces::actor::types::ActorError> {
        // TODO: Implement nack
        Err(plexspaces::actor::types::ActorError {
            code: plexspaces::actor::types::ErrorCode::NotImplemented,
            message: "nack not yet implemented".to_string(),
            details: None,
        })
    }

    async fn publish_to_topic(
        &mut self,
        topic_name: String,
        msg_type: String,
        payload: plexspaces::actor::types::Payload,
    ) -> Result<plexspaces::actor::types::MessageId, plexspaces::actor::types::ActorError> {
        match self.host_functions.publish_to_topic(&topic_name, &msg_type, payload).await {
            Ok(message_id) => Ok(message_id),
            Err(e) => Err(plexspaces::actor::types::ActorError {
                code: plexspaces::actor::types::ErrorCode::Internal,
                message: e,
                details: None,
            }),
        }
    }

    async fn subscribe_to_topic(
        &mut self,
        _topic_name: String,
        _filter: Option<String>,
    ) -> Result<u64, plexspaces::actor::types::ActorError> {
        // TODO: Implement subscribe_to_topic
        Err(plexspaces::actor::types::ActorError {
            code: plexspaces::actor::types::ErrorCode::NotImplemented,
            message: "subscribe_to_topic not yet implemented".to_string(),
            details: None,
        })
    }

    async fn unsubscribe_from_topic(
        &mut self,
        _subscription_id: u64,
    ) -> Result<(), plexspaces::actor::types::ActorError> {
        // TODO: Implement unsubscribe_from_topic
        Err(plexspaces::actor::types::ActorError {
            code: plexspaces::actor::types::ErrorCode::NotImplemented,
            message: "unsubscribe_from_topic not yet implemented".to_string(),
            details: None,
        })
    }

    async fn create_queue(
        &mut self,
        _queue_name: String,
        _max_size: u32,
        _message_ttl_ms: u64,
    ) -> Result<(), plexspaces::actor::types::ActorError> {
        // TODO: Implement create_queue
        Err(plexspaces::actor::types::ActorError {
            code: plexspaces::actor::types::ErrorCode::NotImplemented,
            message: "create_queue not yet implemented".to_string(),
            details: None,
        })
    }

    async fn delete_queue(
        &mut self,
        _queue_name: String,
    ) -> Result<(), plexspaces::actor::types::ActorError> {
        // TODO: Implement delete_queue
        Err(plexspaces::actor::types::ActorError {
            code: plexspaces::actor::types::ErrorCode::NotImplemented,
            message: "delete_queue not yet implemented".to_string(),
            details: None,
        })
    }

    async fn queue_depth(
        &mut self,
        _queue_name: String,
    ) -> Result<u64, plexspaces::actor::types::ActorError> {
        // TODO: Implement queue_depth
        Err(plexspaces::actor::types::ActorError {
            code: plexspaces::actor::types::ErrorCode::NotImplemented,
            message: "queue_depth not yet implemented".to_string(),
            details: None,
        })
    }
}

#[cfg(feature = "component-model")]
pub struct DurabilityImpl;

#[cfg(feature = "component-model")]
#[async_trait::async_trait]
impl plexspaces::actor::durability::Host for DurabilityImpl {
    async fn persist(
        &mut self,
        _event_type: String,
        _payload: plexspaces::actor::types::Payload,
    ) -> Result<u64, plexspaces::actor::types::ActorError> {
        // TODO: Implement persist
        tracing::warn!("Durability persist not yet implemented for components");
        Ok(0)
    }

    async fn persist_batch(
        &mut self,
        _events: Vec<(String, plexspaces::actor::types::Payload)>,
    ) -> Result<u64, plexspaces::actor::types::ActorError> {
        // TODO: Implement persist_batch
        tracing::warn!("Durability persist_batch not yet implemented for components");
        Ok(0)
    }

    async fn checkpoint(&mut self) -> Result<u64, plexspaces::actor::types::ActorError> {
        // TODO: Implement checkpoint
        tracing::warn!("Durability checkpoint not yet implemented for components");
        Ok(0)
    }

    async fn get_sequence(&mut self) -> Result<u64, plexspaces::actor::types::ActorError> {
        // TODO: Implement get_sequence
        tracing::warn!("Durability get_sequence not yet implemented for components");
        Ok(0)
    }

    async fn get_checkpoint_sequence(&mut self) -> Result<u64, plexspaces::actor::types::ActorError> {
        // TODO: Implement get_checkpoint_sequence
        tracing::warn!("Durability get_checkpoint_sequence not yet implemented for components");
        Ok(0)
    }

    async fn is_replaying(&mut self) -> Result<bool, plexspaces::actor::types::ActorError> {
        Ok(false)
    }

    async fn cache_side_effect(
        &mut self,
        _key: String,
        _result_value: plexspaces::actor::types::Payload,
    ) -> Result<plexspaces::actor::types::Payload, plexspaces::actor::types::ActorError> {
        // TODO: Implement cache_side_effect
        Err(plexspaces::actor::types::ActorError {
            code: plexspaces::actor::types::ErrorCode::NotImplemented,
            message: "cache_side_effect not yet implemented".to_string(),
            details: None,
        })
    }

    async fn read_journal(
        &mut self,
        _from_sequence: u64,
        _to_sequence: u64,
        _limit: u32,
    ) -> Result<Vec<plexspaces::actor::durability::JournalEntry>, plexspaces::actor::types::ActorError> {
        // TODO: Implement read_journal
        Err(plexspaces::actor::types::ActorError {
            code: plexspaces::actor::types::ErrorCode::NotImplemented,
            message: "read_journal not yet implemented".to_string(),
            details: None,
        })
    }

    async fn compact(
        &mut self,
        _up_to_sequence: u64,
    ) -> Result<(), plexspaces::actor::types::ActorError> {
        // TODO: Implement compact
        Err(plexspaces::actor::types::ActorError {
            code: plexspaces::actor::types::ErrorCode::NotImplemented,
            message: "compact not yet implemented".to_string(),
            details: None,
        })
    }
}
