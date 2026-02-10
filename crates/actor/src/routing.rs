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

//! Unified Routing Module
//!
//! ## Purpose
//! Generic routing helpers that can be used by both ActorRef and ActorService.
//! All routing logic is centralized here to avoid duplication and ensure consistency.
//!
//! ## Design Principles
//! - **Generic functions**: Not tied to specific instances (ActorRef, ActorService)
//! - **RequestContext required**: All routing functions take RequestContext for proper tenant/namespace isolation
//! - **Return Futures**: All async operations return Futures for parallel operations (map/reduce)
//! - **No cyclic dependencies**: Routing module doesn't depend on ActorRef or ActorService

use std::pin::Pin;
use std::sync::Arc;
use std::future::Future;
use std::time::Duration;
use std::io::Write;
use async_trait::async_trait;

use plexspaces_core::{ServiceLocator as ServiceLocatorTrait, RequestContext, MessageSender, ReplyWaiter, ReplyWaiterError};
use plexspaces_proto::common::v1::Message;
use plexspaces_proto::actor::v1::{actor_service_client::ActorServiceClient, SendMessageRequest};
use prost_types;
use ulid::Ulid;

use crate::ActorRef;
use crate::ActorRefError;

/// Extract node_id from actor ID (format: "actor_name@node_id" or just "actor_name")
///
/// ## Returns
/// Tuple of (actor_name, node_id). If no @node_id is present, returns (actor_id, None).
pub fn extract_node_id(actor_id: &str) -> (String, Option<String>) {
    if let Some((name, node)) = actor_id.split_once('@') {
        (name.to_string(), Some(node.to_string()))
    } else {
        (actor_id.to_string(), None)
    }
}

/// Determine if an actor is local by comparing node_id from actor_id with local_node_id.
///
/// ## Purpose
/// Unified locality determination that doesn't rely on ActorRefInner::Local/Remote variants.
/// Instead, determines locality by:
/// 1. Extracting node_id from actor_id string
/// 2. Comparing with local_node_id from NodeConfig (primary source)
/// 3. Fallback to ActorRegistry.local_node_id() for testing
/// 4. Also checking if actor exists locally (for actors registered with "remote-looking" IDs)
///
/// ## Arguments
/// * `actor_id` - Actor ID to check (format: "actor_name@node_id" or just "actor_name")
/// * `service_locator` - ServiceLocator to access NodeConfig and ActorRegistry
///
/// ## Returns
/// `true` if actor is local, `false` if remote
pub async fn is_actor_local(
    actor_id: &str,
    service_locator: &Arc<dyn ServiceLocatorTrait>,
) -> bool {
    // Extract node_id from actor_id
    let (_, node_id_opt) = extract_node_id(actor_id);
    
    // Get local_node_id from NodeConfig (primary source, always available in production)
    let local_node_id = if let Some(node_config) = service_locator.get_node_config().await {
        Some(node_config.id)
    } else if let Some(registry) = service_locator.actor_registry().await {
        // Fallback to ActorRegistry for testing (when NodeConfig not registered)
        Some(registry.local_node_id().to_string())
    } else {
        None
    };
    
    if let Some(local_id) = local_node_id {
        // Check if node_id matches local_node_id
        if let Some(node_id) = node_id_opt {
            if node_id == local_id {
                return true;
            }
        }
        
        // Also check if actor exists locally (for actors registered with "remote-looking" IDs)
        if let Some(registry) = service_locator.actor_registry().await {
            if registry.lookup_actor(&actor_id.to_string()).await.is_some() {
                return true;
            }
        }
    }
    
    false
}

/// Generic ask helper: register waiter, set message sender/correlation, lookup target, send via tell(), wait for reply.
/// Returns a Future for parallel operations (map/reduce).
///
/// ## Purpose
/// Unified ask pattern implementation that can be used by both ActorRef and ActorService.
/// Caller is responsible for creating temp sender and calling cleanup_ask_resources after (success or error).
///
/// ## Arguments
/// * `ctx` - RequestContext with tenant_id and namespace (required for proper isolation) - FIRST PARAMETER
/// * `service_locator` - ServiceLocator for accessing registries
/// * `target_actor_id` - Target actor ID
/// * `message` - Message to send (will be modified with sender_id and correlation_id)
/// * `temp_sender_id` - Temporary sender ID (format: "ask-{correlation_id}@{node_id}")
/// * `correlation_id` - Correlation ID for reply matching
/// * `timeout` - Timeout for waiting for reply
///
/// ## Returns
/// Future that resolves to Result<Message, ActorRefError>
pub fn ask_helper(
    ctx: RequestContext,
    service_locator: Arc<dyn ServiceLocatorTrait>,
    target_actor_id: String,
    mut message: Message,
    temp_sender_id: String,
    correlation_id: String,
    timeout: Duration,
) -> Pin<Box<dyn Future<Output = Result<Message, ActorRefError>> + Send>> {
    Box::pin(async move {
        // Clone message_id before message is moved
        let message_id = message.id.clone();
        
        message.sender_id = temp_sender_id.clone();
        message.correlation_id = correlation_id.clone();
        if message.receiver_id.is_empty() {
            message.receiver_id = target_actor_id.clone();
        }


        let waiter = ReplyWaiter::new();
        let waiter_registry = service_locator
            .reply_waiter_registry()
            .await
            .ok_or_else(|| ActorRefError::SendFailed("ReplyWaiterRegistry not available".to_string()))?;
        waiter_registry.register(correlation_id.clone(), waiter.clone()).await;

        let registry = service_locator
            .actor_registry()
            .await
            .ok_or_else(|| ActorRefError::SendFailed("ActorRegistry not available".to_string()))?;
        let sender = match registry.lookup_actor(&target_actor_id).await {
            Some(s) => s,
            None => {
                waiter_registry.remove(&correlation_id).await;
                return Err(ActorRefError::ActorNotFound(target_actor_id.clone()));
            }
        };

        if let Err(e) = sender.tell(message).await {
            waiter_registry.remove(&correlation_id).await;
            return Err(ActorRefError::SendFailed(format!("Failed to send message: {}", e)));
        }

        let result = waiter.wait(timeout).await;
        result.map_err(|e| match e {
            ReplyWaiterError::Timeout => ActorRefError::Timeout,
            _ => ActorRefError::SendFailed(format!("Reply waiter error: {}", e)),
        })
    })
}

/// Route message to local actor (generic helper).
/// Returns a Future for parallel operations.
///
/// ## Purpose
/// Unified local routing that can be used by both ActorRef and ActorService.
/// Uses ActorRef::tell() and ActorRef::ask() instead of direct mailbox access.
///
/// ## Arguments
/// * `ctx` - RequestContext with tenant_id and namespace (required for proper isolation) - FIRST PARAMETER
/// * `service_locator` - ServiceLocator for accessing registries
/// * `actor_id` - Target actor ID (format: "actor_name@node_id" or just "actor_name")
/// * `message` - Message to send
/// * `wait_for_response` - Whether to wait for reply (ask vs tell)
/// * `timeout` - Optional timeout for request-reply
///
/// ## Returns
/// Future that resolves to Result<(message_id, Option<reply_message>), ActorRefError>
pub fn route_local(
    ctx: RequestContext,
    service_locator: Arc<dyn ServiceLocatorTrait>,
    actor_id: String,
    mut message: Message,
    wait_for_response: bool,
    timeout: Option<Duration>,
) -> Pin<Box<dyn Future<Output = Result<(String, Option<Message>), ActorRefError>> + Send>> {
    Box::pin(async move {
        let start = std::time::Instant::now();
        let message_id = message.id.clone();

        // Look up MessageSender from ActorRegistry
        let actor_registry = service_locator
            .actor_registry()
            .await
            .ok_or_else(|| ActorRefError::SendFailed("ActorRegistry not available".to_string()))?;
        
        // Try constructed actor_id first, then original receiver_id
        let sender = if let Some(s) = actor_registry.lookup_actor(&actor_id).await {
            s
        } else if message.receiver_id != actor_id {
            // Try lookup with original receiver ID (may have different node_id)
            actor_registry.lookup_actor(&message.receiver_id).await
                .ok_or_else(|| ActorRefError::ActorNotFound(actor_id.clone()))?
        } else {
            return Err(ActorRefError::ActorNotFound(actor_id.clone()));
        };

        // OBSERVABILITY: Track duration
        let duration = start.elapsed();
        metrics::histogram!("plexspaces_routing_local_route_duration_seconds")
            .record(duration.as_secs_f64());

        if wait_for_response {
            // ASK PATTERN: Use ask_helper for unified routing
            let timeout_duration = timeout.unwrap_or(Duration::from_secs(5));
            
            // Generate unique correlation_id and temp_sender_id
            let correlation_id = Ulid::new().to_string();
            
            // Get local node ID for temp_sender_id
            let local_node_id = if let Some(node_config) = service_locator.get_node_config().await {
                node_config.id
            } else if let Some(registry) = service_locator.actor_registry().await {
                registry.local_node_id().to_string()
            } else {
                return Err(ActorRefError::SendFailed("Cannot determine local node ID".to_string()));
            };
            
            use plexspaces_core::TEMP_SENDER_PREFIX;
            let temp_sender_id = format!("{}-{}@{}", TEMP_SENDER_PREFIX, correlation_id, local_node_id);
            let expires_at = std::time::Instant::now() + (timeout_duration * 2);
            
            // Create temporary sender via ActorFactory
            if let Some(factory) = service_locator.get_actor_factory().await {
                let create_result = factory
                    .create_temporary_sender(&ctx, temp_sender_id.clone(), correlation_id.clone(), expires_at)
                    .await;
                create_result
                    .map_err(|e| ActorRefError::SendFailed(format!("Failed to create temporary sender: {}", e)))?;
            } else {
                return Err(ActorRefError::SendFailed("ActorFactory not found in ServiceLocator".to_string()));
            }
            
            // Use ask_helper for unified routing (returns Future)
            let result = ask_helper(
                ctx,
                service_locator.clone(),
                actor_id.clone(),
                message,
                temp_sender_id.clone(),
                correlation_id.clone(),
                timeout_duration,
            ).await;
            
            // Cleanup temporary sender
            if let Some(registry) = service_locator.actor_registry().await {
                registry.remove_temporary_sender(&temp_sender_id).await;
            }
            
            // Update metrics
            match &result {
                Ok(_) => {
                    if let Some(accessor) = service_locator.get_node_metrics_accessor().await {
                        accessor.increment_messages_routed().await;
                        accessor.increment_local_deliveries().await;
                    }
                    metrics::counter!("plexspaces_routing_local_route_success_total",
                        "pattern" => "ask"
                    )
                    .increment(1);
                }
                Err(e) => {
                    if let Some(accessor) = service_locator.get_node_metrics_accessor().await {
                        accessor.increment_messages_routed().await;
                        accessor.increment_failed_deliveries().await;
                    }
                    let error_type = match e {
                        ActorRefError::Timeout => "timeout",
                        ActorRefError::ActorNotFound(_) => "not_found",
                        _ => "other",
                    };
                    metrics::counter!("plexspaces_routing_local_route_error_total",
                        "pattern" => "ask",
                        "error" => error_type
                    )
                    .increment(1);
                }
            }
            
            result.map(|reply| (message_id, Some(reply)))
        } else {
            // TELL PATTERN: Use MessageSender::tell() directly
            let result = sender.tell(message).await
                .map_err(|e| ActorRefError::SendFailed(format!("Failed to send message: {}", e)));
            
            // Update metrics
            if let Some(accessor) = service_locator.get_node_metrics_accessor().await {
                accessor.increment_messages_routed().await;
                if result.is_ok() {
                    accessor.increment_local_deliveries().await;
                } else {
                    accessor.increment_failed_deliveries().await;
                }
            }
            
            metrics::counter!("plexspaces_routing_local_route_success_total",
                "pattern" => "tell"
            )
            .increment(1);

            result.map(|_| (message_id, None))
        }
    })
}

/// Route message to remote actor via gRPC (generic helper).
/// Returns a Future for parallel operations.
///
/// ## Purpose
/// Unified remote routing that can be used by both ActorRef and ActorService.
///
/// ## Arguments
/// * `ctx` - RequestContext with tenant_id and namespace (required for proper isolation) - FIRST PARAMETER
/// * `service_locator` - ServiceLocator for accessing gRPC clients
/// * `node_id` - Target node ID
/// * `actor_id` - Target actor ID (for logging)
/// * `message` - Message to send
/// * `wait_for_response` - Whether to wait for reply (ask vs tell)
/// * `timeout` - Optional timeout for request-reply
///
/// ## Returns
/// Future that resolves to Result<(message_id, Option<reply_message>), ActorRefError>
pub fn route_remote(
    ctx: RequestContext,
    service_locator: Arc<dyn ServiceLocatorTrait>,
    node_id: String,
    _actor_id: String,
    message: Message,
    wait_for_response: bool,
    timeout: Option<Duration>,
) -> Pin<Box<dyn Future<Output = Result<(String, Option<Message>), ActorRefError>> + Send>> {
    Box::pin(async move {
        let start = std::time::Instant::now();
        let message_id = message.id.clone();

        // OBSERVABILITY: Track remote routing
        metrics::counter!("plexspaces_routing_remote_route_total",
            "target_node" => node_id.clone()
        )
        .increment(1);

        // Get ActorServiceClient using ServiceLocator helper (handles ObjectRegistry lookup and connection pooling)
        let channel = service_locator.get_actor_service_client(&node_id).await
            .map_err(|e| {
                ActorRefError::SendFailed(format!("Failed to get ActorServiceClient: {}", e))
            })?;
        
        let mut client = ActorServiceClient::new(channel);

        // Convert message to proto
        let proto_message = message.clone();

        // Convert timeout to proto Duration
        let proto_timeout = timeout.map(|d| prost_types::Duration {
            seconds: d.as_secs() as i64,
            nanos: d.subsec_nanos() as i32,
        });

        // Create SendMessage request
        let request = tonic::Request::new(SendMessageRequest {
            message: Some(proto_message),
            wait_for_response,
            timeout: proto_timeout,
        });

        // Forward to remote ActorService
        let response = match client.send_message(request).await {
            Ok(r) => r,
            Err(e) => {
                // Update Node metrics on failure
                if let Some(accessor) = service_locator.get_node_metrics_accessor().await {
                    accessor.increment_messages_routed().await;
                    accessor.increment_failed_deliveries().await;
                }
                metrics::counter!("plexspaces_routing_remote_route_error_total",
                    "target_node" => node_id.clone(),
                    "error" => e.code().to_string()
                )
                .increment(1);
                
                // Map timeout error
                if e.code() == tonic::Code::DeadlineExceeded {
                    return Err(ActorRefError::Timeout);
                }
                return Err(ActorRefError::SendFailed(format!("Remote call to {} failed: {}", node_id, e)));
            }
        };

        let response_inner = response.into_inner();

        // OBSERVABILITY: Track duration
        let duration = start.elapsed();
        metrics::histogram!("plexspaces_routing_remote_route_duration_seconds")
            .record(duration.as_secs_f64());

        metrics::counter!("plexspaces_routing_remote_route_success_total",
            "target_node" => node_id.clone()
        )
        .increment(1);

        // Update Node metrics on success
        if let Some(accessor) = service_locator.get_node_metrics_accessor().await {
            accessor.increment_messages_routed().await;
            accessor.increment_remote_deliveries().await;
        }

        // Convert response back to internal Message if present
        let reply_message = response_inner.response;

        Ok((response_inner.message_id, reply_message))
    })
}

/// Route message to local or remote actor (unified routing).
/// Returns a Future for parallel operations.
///
/// ## Purpose
/// Main routing function that determines locality and routes accordingly.
/// Can be used by both ActorRef and ActorService.
///
/// ## Arguments
/// * `ctx` - RequestContext with tenant_id and namespace (required for proper isolation) - FIRST PARAMETER
/// * `service_locator` - ServiceLocator for accessing registries and gRPC clients
/// * `actor_id` - Target actor ID (format: "actor_name@node_id" or just "actor_name")
/// * `message` - Message to send
/// * `wait_for_response` - Whether to wait for reply (ask vs tell)
/// * `timeout` - Optional timeout for request-reply
///
/// ## Returns
/// Future that resolves to Result<(message_id, Option<reply_message>), ActorRefError>
pub fn route_message(
    ctx: RequestContext,
    service_locator: Arc<dyn ServiceLocatorTrait>,
    actor_id: String,
    message: Message,
    wait_for_response: bool,
    timeout: Option<Duration>,
) -> Pin<Box<dyn Future<Output = Result<(String, Option<Message>), ActorRefError>> + Send>> {
    Box::pin(async move {
        // Parse actor@node ID (or just actor name, defaults to local node)
        let (actor_name, node_id) = extract_node_id(&actor_id);
        
        // Get local node ID
        let local_node_id = if let Some(node_config) = service_locator.get_node_config().await {
            node_config.id
        } else if let Some(registry) = service_locator.actor_registry().await {
            registry.local_node_id().to_string()
        } else {
            return Err(ActorRefError::SendFailed("Cannot determine local node ID".to_string()));
        };
        
        // Determine routing: local if node_id matches OR actor exists locally
        let is_local = is_actor_local(&actor_id, &service_locator).await;
        
        // OBSERVABILITY: Track routing decision
        metrics::counter!("plexspaces_routing_route_total",
            "actor_id" => actor_id.clone(),
            "node_id" => node_id.clone().unwrap_or_else(|| local_node_id.clone()),
            "local" => if is_local { "true" } else { "false" }
        )
        .increment(1);

        if is_local {
            // LOCAL ROUTING: Use route_local
            let target_actor_id = if let Some(node_id) = node_id {
                format!("{}@{}", actor_name, node_id)
            } else {
                format!("{}@{}", actor_name, local_node_id)
            };
            
            route_local(
                ctx,
                service_locator,
                target_actor_id,
                message,
                wait_for_response,
                timeout,
            ).await
        } else {
            // REMOTE ROUTING: Use route_remote
            let target_node_id = node_id.unwrap_or_else(|| local_node_id);
            route_remote(
                ctx,
                service_locator,
                target_node_id,
                actor_id,
                message,
                wait_for_response,
                timeout,
            ).await
        }
    })
}
