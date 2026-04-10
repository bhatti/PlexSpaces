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

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use plexspaces_core::{
    ActorId, ReplyWaiter, ReplyWaiterError, RequestContext, ServiceLocator as ServiceLocatorTrait,
};
use plexspaces_proto::actor::v1::{
    actor_service_client::ActorServiceClient, AskReplyRequest, SendMessageRequest,
};
use plexspaces_proto::common::v1::Message;
use prost_types;

use crate::ActorRefError;

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
/// ## Scope Behavior
/// The fallback registry lookup is intentionally conservative. If the same actor id exists in
/// multiple scopes, `ActorRegistry::lookup_actor()` returns `None` rather than selecting an
/// arbitrary actor, so locality detection fails closed in ambiguous cases.
///
/// ## Arguments
/// * `actor_id` - Canonical target actor ID
/// * `service_locator` - ServiceLocator to access NodeConfig and ActorRegistry
///
/// ## Returns
/// `true` if actor is local, `false` if remote
pub async fn is_actor_local(
    actor_id: &ActorId,
    service_locator: &Arc<dyn ServiceLocatorTrait>,
) -> bool {
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
        if actor_id.node_id() == local_id {
            return true;
        }

        // Also check if actor exists locally (for actors registered with "remote-looking" IDs).
        // This remains a conservative fallback: ambiguous cross-scope ids fail closed.
        if let Some(registry) = service_locator.actor_registry().await {
            if registry.lookup_actor(actor_id).await.is_some() {
                return true;
            }
        }
    }

    false
}

fn parse_target_actor_id(actor_id: &str) -> Result<ActorId, ActorRefError> {
    ActorId::from_canonical(actor_id).map_err(|e| {
        ActorRefError::SendFailed(format!("Invalid canonical ActorId '{}': {}", actor_id, e))
    })
}

/// Generic ask helper for shared temporary-sender fanout operations.
/// Returns a Future for parallel operations (map/reduce).
///
/// ## Purpose
/// `ActorRegistry::ask()` is the standard local ask path for one request/one temporary sender.
/// This helper is reserved for shard-group fanout paths that intentionally reuse one temporary
/// sender while tracking one `ReplyWaiter` per correlation id.
///
/// Caller is responsible for creating and cleaning up the shared temporary sender. This helper
/// owns per-correlation waiter registration and removal.
///
/// ## Arguments
/// * `ctx` - RequestContext with tenant_id and namespace (required for proper isolation) - FIRST PARAMETER
/// * `service_locator` - ServiceLocator for accessing registries
/// * `target_actor_id` - Target actor ID
/// * `message` - Message to send (will be modified with sender_id and correlation_id)
/// * `temp_sender_id` - Temporary sender ID in canonical ActorId string form
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

        tracing::debug!(
            message_id = %message_id,
            sender_id = %temp_sender_id,
            recipient_id = %target_actor_id,
            correlation_id = %correlation_id,
            "ask_helper: routing request to target, waiting for reply on temp sender"
        );

        let waiter = ReplyWaiter::new();
        let waiter_registry = service_locator
            .reply_waiter_registry()
            .await
            .ok_or_else(|| {
                ActorRefError::SendFailed("ReplyWaiterRegistry not available".to_string())
            })?;
        waiter_registry
            .register(correlation_id.clone(), waiter.clone())
            .await;

        let registry = service_locator
            .actor_registry()
            .await
            .ok_or_else(|| ActorRefError::SendFailed("ActorRegistry not available".to_string()))?;
        let target_actor_id = parse_target_actor_id(&target_actor_id)?;
        let is_local = match registry
            .lookup_actor_in_scope(ctx.tenant_id(), ctx.namespace(), &target_actor_id)
            .await
        {
            Some(_) => true,
            None => {
                target_actor_id.node_id() == registry.local_node_id()
            }
        };

        if !is_local {
            waiter_registry.remove(&correlation_id).await;
            match route_remote(
                ctx,
                service_locator,
                target_actor_id.node_id().to_string(),
                target_actor_id.to_string(),
                message,
                true,
                Some(timeout),
            )
            .await
            {
                Ok((_, Some(reply))) => return Ok(reply),
                Ok((_, None)) => {
                    return Err(ActorRefError::SendFailed(
                        "Remote ask returned no reply".to_string(),
                    ))
                }
                Err(e) => return Err(e),
            }
        }

        if let Err(e) = registry.tell(&target_actor_id, message).await {
            waiter_registry.remove(&correlation_id).await;
            return Err(match e {
                plexspaces_core::ActorRegistryError::ActorNotFound(id) => {
                    ActorRefError::ActorNotFound(id.into())
                }
                plexspaces_core::ActorRegistryError::Timeout => ActorRefError::Timeout,
                other => ActorRefError::SendFailed(other.to_string()),
            });
        }

        let result = waiter.wait(timeout).await;
        waiter_registry.remove(&correlation_id).await;
        match &result {
            Ok(reply) => {
                tracing::debug!(
                    request_id = %message_id,
                    reply_id = %reply.id,
                    temp_sender = %temp_sender_id,
                    correlation_id = %correlation_id,
                    "ask_helper: reply received from target"
                );
            }
            Err(e) => {
                tracing::debug!(
                    request_id = %message_id,
                    temp_sender = %temp_sender_id,
                    correlation_id = %correlation_id,
                    error = %e,
                    "ask_helper: wait for reply failed"
                );
            }
        }
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
/// * `actor_id` - Target actor ID in canonical format `name//actor_type::namespace@node_id`
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
        let actor_id = parse_target_actor_id(&actor_id)?;

        let actor_registry = service_locator
            .actor_registry()
            .await
            .ok_or_else(|| ActorRefError::SendFailed("ActorRegistry not available".to_string()))?;

        // OBSERVABILITY: Track duration
        let duration = start.elapsed();
        metrics::histogram!("plexspaces_routing_local_route_duration_seconds")
            .record(duration.as_secs_f64());

        if wait_for_response {
            let timeout_duration = timeout.unwrap_or(Duration::from_secs(5));
            let result = actor_registry
                .ask(&ctx, &actor_id, message, timeout_duration)
                .await
                .map_err(|e| match e {
                    plexspaces_core::ActorRegistryError::ActorNotFound(id) => {
                        ActorRefError::ActorNotFound(id.into())
                    }
                    plexspaces_core::ActorRegistryError::Timeout => ActorRefError::Timeout,
                    other => ActorRefError::SendFailed(other.to_string()),
                });

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
            let result = actor_registry
                .tell(&actor_id, message)
                .await
                .map_err(|e| match e {
                    plexspaces_core::ActorRegistryError::ActorNotFound(id) => {
                        ActorRefError::ActorNotFound(id.into())
                    }
                    plexspaces_core::ActorRegistryError::Timeout => ActorRefError::Timeout,
                    other => ActorRefError::SendFailed(other.to_string()),
                });

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
    actor_id: String,
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
        let channel = service_locator
            .get_actor_service_client(&node_id)
            .await
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

        let method = if wait_for_response { "GET" } else { "POST" }.to_string();

        // Forward to remote ActorService using explicit tell vs ask RPCs.
        let response = match if wait_for_response {
            client
                .ask_reply(tonic::Request::new(AskReplyRequest {
                    namespace: ctx.namespace().to_string(),
                    actor_type: actor_id.clone(),
                    http_method: method,
                    payload: proto_message.payload.clone(),
                    headers: proto_message.headers.clone(),
                    query_params: Default::default(),
                    path: proto_message.uri_path.clone(),
                    subpath: String::new(),
                    sender_id: proto_message.sender_id.clone(),
                    message_type: proto_message.message_type.clone(),
                    correlation_id: proto_message.correlation_id.clone(),
                    reply_to: proto_message.reply_to.clone(),
                    message_id: proto_message.id.clone(),
                    timeout: proto_timeout,
                }))
                .await
                .map(|resp| {
                    let inner = resp.into_inner();
                    (inner.actor_id, inner.payload, inner.headers, String::new())
                })
        } else {
            client
                .send_message(tonic::Request::new(SendMessageRequest {
                    namespace: ctx.namespace().to_string(),
                    actor_type: actor_id.clone(),
                    http_method: method,
                    payload: proto_message.payload.clone(),
                    headers: proto_message.headers.clone(),
                    query_params: Default::default(),
                    path: proto_message.uri_path.clone(),
                    subpath: String::new(),
                    sender_id: proto_message.sender_id.clone(),
                    message_type: proto_message.message_type.clone(),
                    correlation_id: proto_message.correlation_id.clone(),
                    reply_to: proto_message.reply_to.clone(),
                    message_id: proto_message.id.clone(),
                }))
                .await
                .map(|resp| {
                    let inner = resp.into_inner();
                    (
                        inner.actor_id,
                        Vec::new(),
                        Default::default(),
                        inner.message_id,
                    )
                })
        } {
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
                return Err(ActorRefError::SendFailed(format!(
                    "Remote call to {} failed: {}",
                    node_id, e
                )));
            }
        };

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

        if wait_for_response {
            let (resolved_actor_id, payload, headers, _) = response;
            let correlation_id = proto_message.id.clone();
            let reply_message = Message {
                id: format!("res-{}", ulid::Ulid::new()),
                sender_id: resolved_actor_id,
                receiver_id: proto_message.sender_id,
                message_type: "call".to_string(),
                payload,
                headers,
                correlation_id,
                ..Default::default()
            };
            Ok((proto_message.id, Some(reply_message)))
        } else {
            let (_resolved_actor_id, _payload, _headers, message_id) = response;
            Ok((message_id, None))
        }
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
/// * `actor_id` - Target actor ID in canonical format `name//actor_type::namespace@node_id`
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
        let target_actor_id = parse_target_actor_id(&actor_id)?;

        // Get local node ID
        let local_node_id = if let Some(node_config) = service_locator.get_node_config().await {
            node_config.id
        } else if let Some(registry) = service_locator.actor_registry().await {
            registry.local_node_id().to_string()
        } else {
            return Err(ActorRefError::SendFailed(
                "Cannot determine local node ID".to_string(),
            ));
        };

        // Determine routing: local if node_id matches OR actor exists locally
        let is_local = is_actor_local(&target_actor_id, &service_locator).await;

        // OBSERVABILITY: Track routing decision
        metrics::counter!("plexspaces_routing_route_total",
            "actor_id" => target_actor_id.to_string(),
            "node_id" => target_actor_id.node_id().to_string(),
            "local" => if is_local { "true" } else { "false" }
        )
        .increment(1);

        if is_local {
            route_local(
                ctx,
                service_locator,
                target_actor_id.to_string(),
                message,
                wait_for_response,
                timeout,
            )
            .await
        } else {
            // REMOTE ROUTING: Use route_remote
            route_remote(
                ctx,
                service_locator,
                target_actor_id.node_id().to_string(),
                target_actor_id.to_string(),
                message,
                wait_for_response,
                timeout,
            )
            .await
        }
    })
}
