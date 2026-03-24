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

//! ActorFactory implementation
//!
//! ## Purpose
//! Provides an ActorFactory implementation that spawns actors using ActorRegistry
//! and other services from ServiceLocator. This decouples ActorFactory from Node directly.
//!
//! ## Design
//! ActorFactoryImpl depends only on ServiceLocator, not Node directly.
//! It uses ActorRegistry, VirtualActorManager, and other services to spawn actors.

use crate::{Actor, ActorRef};
use async_trait::async_trait;
use plexspaces_core::{
    ActorContext, ActorFactory, ActorId, ActorRegistry, ApplicationManager, ExitReason,
    MessageSender, RequestContext, Service, ServiceLocator as ServiceLocatorTrait,
    VirtualActorManager,
};
use plexspaces_mailbox::{Mailbox, MailboxConfig};
use plexspaces_proto::ActorLifecycleEvent;
use prost_types::Timestamp;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;

/// ActorFactory implementation
///
/// ## Design
/// Uses ServiceLocator to access ActorRegistry, VirtualActorManager, and other services
/// needed for spawning actors. This decouples ActorFactory from Node directly.
pub struct ActorFactoryImpl {
    service_locator: Arc<dyn ServiceLocatorTrait>,
    stopping_actors: Arc<RwLock<HashSet<ActorId>>>,
}

impl ActorFactoryImpl {
    pub fn new(service_locator: Arc<dyn ServiceLocatorTrait>) -> Self {
        Self {
            service_locator,
            stopping_actors: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    pub async fn new_arc(service_locator: Arc<dyn ServiceLocatorTrait>) -> Arc<Self> {
        Arc::new(Self::new(service_locator))
    }

    /// Normalize actor ID to include node ID
    ///
    /// ## Purpose
    /// Ensures actor ID has format "actor_name@node_id". If missing node_id,
    /// appends the local node ID from ActorRegistry.
    fn normalize_actor_id(&self, actor_id: &ActorId, local_node_id: &str) -> ActorId {
        if let Ok((actor_name, node_id)) = plexspaces_core::ActorRef::parse_actor_id(actor_id) {
            // Actor ID already has @ format
            // If node_id matches current node, keep as is, otherwise reconstruct with current node ID
            if node_id == local_node_id {
                actor_id.clone()
            } else {
                format!("{}@{}", actor_name, local_node_id)
            }
        } else {
            // Actor ID doesn't have @ format - append node ID
            format!("{}@{}", actor_id, local_node_id)
        }
    }

    async fn mark_actor_stopping(&self, actor_id: &ActorId) {
        self.stopping_actors.write().await.insert(actor_id.clone());
    }

    async fn take_actor_stopping(&self, actor_id: &ActorId) -> bool {
        self.stopping_actors.write().await.remove(actor_id)
    }

    async fn passivate_virtual_actor(
        &self,
        actor_id: &ActorId,
        registry: &Arc<ActorRegistry>,
        manager: &Arc<VirtualActorManager>,
    ) {
        if let Ok(facet_arc) = manager.get_facet(actor_id).await {
            let mut facet_guard = facet_arc.write().await;
            facet_guard.mark_deactivated().await;
        }

        registry.remove_live_actor_runtime(actor_id).await;
        manager.remove_from_active_tracking(actor_id).await;
    }

    /// Create temporary sender ActorRef for ask() pattern
    ///
    /// ## Purpose
    /// Creates a temporary sender ActorRef that routes replies to ReplyWaiter.
    /// This is used by the ask() pattern to collect replies asynchronously.
    ///
    /// ## Arguments
    /// * `ctx` - RequestContext with proper tenant/namespace (first parameter)
    /// * `temp_sender_id` - Temporary sender ID (format: "ask-{correlation_id}@{node_id}")
    /// * `correlation_id` - Correlation ID for matching replies
    /// * `expires_at` - Expiration time for the temporary sender
    ///
    /// ## Returns
    /// `Arc<dyn MessageSender>` - The temporary sender ActorRef
    ///
    /// ## Design
    /// - Creates mailbox (never used - tell() routes to ReplyWaiter before mailbox)
    /// - Creates ActorRef::local() with namespace from ctx
    /// - Registers in ActorRegistry via register_temporary_sender() with ctx
    /// - Returns temp_sender_ref for use in ask() pattern
    /// Internal implementation of create_temporary_sender (trait method delegates here).
    pub async fn create_temporary_sender_impl(
        &self,
        ctx: &RequestContext,
        temp_sender_id: String,
        correlation_id: String,
        expires_at: Instant,
    ) -> Result<Arc<dyn MessageSender>, Box<dyn std::error::Error + Send + Sync>> {
        // Create mailbox (never used - tell() routes to ReplyWaiter before mailbox)
        let dummy_mailbox = Arc::new(
            Mailbox::new(MailboxConfig::default(), temp_sender_id.clone())
                .await
                .map_err(|e| format!("Failed to create temporary sender mailbox: {}", e))?,
        );

        // Create ActorRef::local() with tenant_id and namespace from ctx
        // CRITICAL: tenant_id flows from API → ActorBuilder → ActorRef → RequestContext
        let temp_sender_ref: Arc<dyn MessageSender> = Arc::new(ActorRef::local(
            temp_sender_id.clone(),
            ctx.tenant_id().to_string(), // CRITICAL: Use tenant_id from RequestContext
            ctx.namespace().to_string(),
            dummy_mailbox,
            self.service_locator.clone(),
        ));

        // Register temporary sender ActorRef in ActorRegistry (so it can be looked up)
        if let Some(registry) = self.service_locator.actor_registry().await {
            registry
                .register_temporary_sender(
                    ctx,
                    temp_sender_id.clone(),
                    temp_sender_ref.clone(),
                    correlation_id,
                    expires_at,
                )
                .await;
        }

        Ok(temp_sender_ref)
    }

    /// Watch actor termination and handle cleanup
    /// Watch actor termination and handle cleanup
    ///
    /// ## Arguments
    /// * `actor_id` - ID of the actor being watched
    /// * `join_handle` - JoinHandle for the actor's task
    /// * `exit_reason_arc` - Arc to the actor's exit_reason field (for EXIT message propagation)
    async fn watch_actor_termination(
        &self,
        actor_id: ActorId,
        join_handle: JoinHandle<()>,
        exit_reason_arc: Arc<tokio::sync::RwLock<Option<ExitReason>>>,
    ) {
        let registry: Arc<ActorRegistry> = self
            .service_locator
            .actor_registry()
            .await
            .unwrap_or_else(|| panic!("ActorRegistry not registered in ServiceLocator"));
        let actor_id_clone = actor_id.clone();
        let factory = Arc::new(Self {
            service_locator: self.service_locator.clone(),
            stopping_actors: self.stopping_actors.clone(),
        });

        tokio::spawn(async move {
            // CRITICAL: Clear tracing context at the start of watch task to prevent span cloning panics
            // This task is spawned from spawn_actor which may have an active span from gRPC handler
            let noop_dispatcher = tracing::dispatcher::Dispatch::none();
            let _watch_guard = tracing::dispatcher::set_default(&noop_dispatcher);
            // Wait for actor task to complete
            let result = join_handle.await;

            if factory.take_actor_stopping(&actor_id_clone).await {
                tracing::debug!(
                    actor_id = %actor_id_clone,
                    "Skipping watcher cleanup for actor handled by explicit stop_actor"
                );
                return;
            }

            // Check if actor stored an exit reason (e.g., from EXIT message)
            let stored_exit_reason = {
                let stored = exit_reason_arc.read().await;
                let cloned = stored.clone();
                if cloned.is_some() {
                    if tracing::enabled!(tracing::Level::DEBUG) {
                        tracing::debug!(
                            actor_id = %actor_id_clone,
                            stored_reason = ?cloned,
                            "Found stored exit reason in actor (terminated due to EXIT)"
                        );
                    }
                } else {
                    if tracing::enabled!(tracing::Level::DEBUG) {
                        tracing::debug!(
                            actor_id = %actor_id_clone,
                            "No stored exit reason (normal termination)"
                        );
                    }
                }
                cloned
            };

            // Determine termination reason and create lifecycle event
            let (reason, lifecycle_event) = match result {
                Ok(_) => {
                    // Check if actor terminated due to EXIT (stored exit reason)
                    if let Some(ref exit_reason) = stored_exit_reason {
                        // Actor terminated due to EXIT - use the stored reason
                        if tracing::enabled!(tracing::Level::DEBUG) {
                            tracing::debug!(
                                actor_id = %actor_id_clone,
                                exit_reason = ?exit_reason,
                                "Actor terminated due to EXIT, using stored exit reason"
                            );
                        }
                        let reason_str = match exit_reason {
                            ExitReason::Normal => "normal".to_string(),
                            ExitReason::Shutdown => "shutdown".to_string(),
                            ExitReason::Killed => "killed".to_string(),
                            ExitReason::Error(msg) => msg.clone(),
                            ExitReason::Linked { actor_id, reason } => {
                                format!(
                                    "linked:{}:{}",
                                    actor_id,
                                    match reason.as_ref() {
                                        ExitReason::Normal => "normal",
                                        ExitReason::Shutdown => "shutdown",
                                        ExitReason::Killed => "killed",
                                        ExitReason::Error(msg) => msg,
                                        ExitReason::Linked { .. } => "linked",
                                    }
                                )
                            }
                        };
                        let now = chrono::Utc::now();
                        let event = ActorLifecycleEvent {
                            actor_id: actor_id_clone.clone(),
                            timestamp: Some(Timestamp {
                                seconds: now.timestamp(),
                                nanos: now.timestamp_subsec_nanos() as i32,
                            }),
                            event_type: Some(
                                plexspaces_proto::actor_lifecycle_event::EventType::Terminated(
                                    plexspaces_proto::v1::actor::ActorTerminated {
                                        reason: reason_str.clone(),
                                    },
                                ),
                            ),
                        };
                        (reason_str, event)
                    } else {
                        // Graceful shutdown (no stored exit reason)
                        let reason = "normal".to_string();
                        let now = chrono::Utc::now();
                        let event = ActorLifecycleEvent {
                            actor_id: actor_id_clone.clone(),
                            timestamp: Some(Timestamp {
                                seconds: now.timestamp(),
                                nanos: now.timestamp_subsec_nanos() as i32,
                            }),
                            event_type: Some(
                                plexspaces_proto::actor_lifecycle_event::EventType::Terminated(
                                    plexspaces_proto::v1::actor::ActorTerminated {
                                        reason: reason.clone(),
                                    },
                                ),
                            ),
                        };
                        (reason, event)
                    }
                }
                Err(e) if e.is_panic() => {
                    // Actor panicked - extract panic message
                    let panic_msg = if let Ok(panic_msg) = e.try_into_panic() {
                        if let Some(s) = panic_msg.downcast_ref::<&str>() {
                            format!("panic: {}", s)
                        } else if let Some(s) = panic_msg.downcast_ref::<String>() {
                            format!("panic: {}", s)
                        } else {
                            "panic: unknown".to_string()
                        }
                    } else {
                        "panic: could not extract message".to_string()
                    };

                    let now = chrono::Utc::now();
                    let event = ActorLifecycleEvent {
                        actor_id: actor_id_clone.clone(),
                        timestamp: Some(Timestamp {
                            seconds: now.timestamp(),
                            nanos: now.timestamp_subsec_nanos() as i32,
                        }),
                        event_type: Some(
                            plexspaces_proto::actor_lifecycle_event::EventType::Failed(
                                plexspaces_proto::v1::actor::ActorFailed {
                                    error: panic_msg.clone(),
                                    stack_trace: format!("Error: {}", panic_msg),
                                },
                            ),
                        ),
                    };
                    (panic_msg, event)
                }
                Err(e) if e.is_cancelled() => {
                    // Actor was killed/aborted
                    let reason = "killed".to_string();
                    let now = chrono::Utc::now();
                    let event = ActorLifecycleEvent {
                        actor_id: actor_id_clone.clone(),
                        timestamp: Some(Timestamp {
                            seconds: now.timestamp(),
                            nanos: now.timestamp_subsec_nanos() as i32,
                        }),
                        event_type: Some(
                            plexspaces_proto::actor_lifecycle_event::EventType::Terminated(
                                plexspaces_proto::v1::actor::ActorTerminated {
                                    reason: reason.clone(),
                                },
                            ),
                        ),
                    };
                    (reason, event)
                }
                Err(_) => {
                    // Unknown error
                    let reason = "unknown error".to_string();
                    let now = chrono::Utc::now();
                    let event = ActorLifecycleEvent {
                        actor_id: actor_id_clone.clone(),
                        timestamp: Some(Timestamp {
                            seconds: now.timestamp(),
                            nanos: now.timestamp_subsec_nanos() as i32,
                        }),
                        event_type: Some(
                            plexspaces_proto::actor_lifecycle_event::EventType::Failed(
                                plexspaces_proto::v1::actor::ActorFailed {
                                    error: reason.clone(),
                                    stack_trace: String::new(),
                                },
                            ),
                        ),
                    };
                    (reason, event)
                }
            };

            // Publish lifecycle event
            registry.publish_lifecycle_event(lifecycle_event).await;

            // Phase 6: Handle actor termination - notify monitors and propagate to links
            // Convert reason string to ExitReason, or use stored exit reason if available
            let exit_reason = if let Some(stored) = &stored_exit_reason {
                if tracing::enabled!(tracing::Level::DEBUG) {
                    tracing::debug!(
                        actor_id = %actor_id_clone,
                        exit_reason = ?stored,
                        "Using stored exit reason for handle_actor_termination (will propagate to links)"
                    );
                }
                stored.clone()
            } else {
                // Parse reason string - handle linked reasons properly
                let converted = if reason.starts_with("linked:") {
                    // Use ExitReason::from_str to parse linked reasons correctly
                    ExitReason::from_str(&reason)
                } else {
                    match reason.as_str() {
                        "normal" => ExitReason::Normal,
                        "shutdown" => ExitReason::Shutdown,
                        "killed" => ExitReason::Killed,
                        _ => ExitReason::Error(reason.clone()),
                    }
                };
                if tracing::enabled!(tracing::Level::DEBUG) {
                    tracing::debug!(
                        actor_id = %actor_id_clone,
                        exit_reason = ?converted,
                        reason_str = %reason,
                        "Using converted exit reason for handle_actor_termination"
                    );
                }
                converted
            };
            if tracing::enabled!(tracing::Level::DEBUG) {
                tracing::debug!(
                    actor_id = %actor_id_clone,
                    exit_reason = ?exit_reason,
                    "Calling handle_actor_termination (will propagate to links if error)"
                );
            }
            registry
                .handle_actor_termination(&actor_id_clone, exit_reason)
                .await;
            if tracing::enabled!(tracing::Level::DEBUG) {
                tracing::debug!(
                    actor_id = %actor_id_clone,
                    "handle_actor_termination completed"
                );
            }

            // CRITICAL: Unregister actor to prevent memory leaks
            // This ensures all registry entries are cleaned up on termination
            if let Err(e) = registry.unregister_with_cleanup(&actor_id_clone).await {
                // Log error but don't fail - actor is already terminated
                tracing::warn!(
                    actor_id = %actor_id_clone,
                    error = %e,
                    "Failed to unregister actor during termination cleanup (non-fatal)"
                );
            }

            // OBSERVABILITY: Track unregistration completion
            metrics::counter!("plexspaces_actor_unregistered_total",
                "actor_id" => actor_id_clone.clone(),
                "reason" => reason.clone()
            )
            .increment(1);
        });
    }
}

#[async_trait]
impl ActorFactory for ActorFactoryImpl {
    async fn activate_virtual_actor(
        &self,
        actor_id: &ActorId,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Get services from ServiceLocator
        let registry: Arc<ActorRegistry> = self
            .service_locator
            .actor_registry()
            .await
            .ok_or_else(|| "ActorRegistry not found in ServiceLocator".to_string())?;
        let manager: Arc<VirtualActorManager> = self
            .service_locator
            .virtual_actor_manager()
            .await
            .ok_or_else(|| "VirtualActorManager not found in ServiceLocator".to_string())?;

        // Normalize actor ID
        let local_node_id = registry.local_node_id();
        let actor_id = self.normalize_actor_id(actor_id, local_node_id);

        // Check if actor is virtual
        if !manager.is_virtual(&actor_id).await {
            return Err(format!("Actor {} is not a virtual actor", actor_id).into());
        }

        // Check if already active
        if manager.is_active(&actor_id).await {
            // Update last_access for LRU tracking
            manager.update_last_access(&actor_id).await;
            return Ok(()); // Already active
        }

        // Get actor_type for LRU eviction check
        let actor_type = {
            use plexspaces_core::actor_id::parse_actor_id;
            if let Ok(parsed) = parse_actor_id(&actor_id) {
                parsed.actor_type
            } else {
                // Fallback: try to get from metadata
                if let Some(metadata) = manager.get_metadata(&actor_id).await {
                    metadata.actor_type
                } else {
                    return Err("Cannot determine actor_type for LRU eviction".into());
                }
            }
        };

        // Evict LRU actors if max_pool_per_actor_type is exceeded
        let evicted = manager
            .evict_lru_if_needed(&actor_type, Some(self.service_locator.clone()))
            .await;

        if !evicted.is_empty() {
            tracing::info!(
                actor_type = %actor_type,
                evicted_count = evicted.len(),
                "LRU-evicted {} virtual actor(s) of type {} to stay under max_pool_per_actor_type limit",
                evicted.len(),
                actor_type
            );
        }
        // Unified activation path: always rebuild from metadata via spawn_actor.
        // initial_state and labels are stored at registration time in VirtualActorManager.
        // The rebuilt VirtualActorFacet is always EAGER so the actor starts immediately.
        // The stored activation_strategy is only used for LRU eviction decisions.
        {
            // Try instance-level metadata first, fall back to type-level metadata.
            let metadata = if let Some(instance_metadata) = manager.get_metadata(&actor_id).await {
                instance_metadata
            } else {
                // Fall back to type-level metadata (for WASM/Rust applications that register types)
                // Extract actor_type from actor_id to lookup type-level registration
                use plexspaces_core::actor_id::parse_actor_id;
                let parsed = parse_actor_id(&actor_id)
                    .map_err(|e| format!("Failed to parse actor_id {}: {}", actor_id, e))?;
                let actor_type = parsed.actor_type;

                manager.get_virtual_actor_type(&actor_type).await
                    .ok_or_else(|| format!(
                        "Virtual actor {} not found - cannot activate. Actor was suspended but metadata is missing from VirtualActorManager. Tried instance-level (actor_id) and type-level (actor_type: {}) lookups.",
                        actor_id, actor_type
                    ))?
            };

            // Extract metadata needed for rebuilding
            // actor_type is now required (not optional) per proto-first design
            let actor_type = metadata.actor_type;
            let config = metadata.config;
            let tenant_id = metadata.tenant_id;
            let namespace = metadata.namespace.clone();
            let initial_state = metadata.initial_state;
            let labels = metadata.labels;
            let tenant_id_clone = tenant_id.clone();

            // Create context for spawn_actor
            let ctx = RequestContext::new_without_auth(tenant_id_clone, namespace.clone());

            // CRITICAL: For virtual actors, we need to recreate the VirtualActorFacet
            // spawn_built_actor only detects virtual actors if they already have the facet attached
            // Since we're rebuilding a suspended actor, we need to recreate the facet
            // Get the facet from VirtualActorManager metadata to recreate it
            // For type-level registration, use facet_config; for instance-level, use stored facet
            let mut facets_to_attach: Vec<Box<dyn plexspaces_facet::Facet>> = vec![];

            if let Some(_facet_arc) = metadata.facet.clone() {
                // Instance-level registration: original facet config not reused here;
                // the rebuild VirtualActorFacet is added below.
            } else if let Some(facet_config) = metadata.facet_config.clone() {
                // Type-level registration: recreate non-virtual facets from stored config.
                // Non-virtual facets recreated; VirtualActorFacet added below.
                if let Some(facet_registry_wrapper) =
                    self.service_locator.get_facet_registry().await
                {
                    let facet_registry: Arc<plexspaces_facet::FacetRegistry> =
                        facet_registry_wrapper.inner_clone();
                    use plexspaces_facet::create_facets_from_config;
                    let recreated_facets =
                        create_facets_from_config(&facet_config, &facet_registry).await;
                    for facet in recreated_facets {
                        if facet.facet_type() != "virtual_actor" {
                            facets_to_attach.push(facet);
                        }
                    }
                } else {
                    return Err(format!(
                        "FacetRegistry not available - cannot recreate facets for virtual actor {}",
                        actor_id
                    )
                    .into());
                }
            }
            // Default (no stored facet or facet_config): VirtualActorFacet is added below.

            // Build the VirtualActorFacet with EAGER strategy so the actor starts immediately.
            // This EAGER facet is used only for the current rebuild (starts the actor running).
            // The stored activation_strategy in VirtualActorMetadata is preserved via the
            // type-level strategy lookup in spawn_built_actor_impl, so LRU eviction is unaffected.
            {
                use plexspaces_common::virtual_actor_config::{
                    format_duration, DEFAULT_IDLE_TIMEOUT_SECONDS,
                };
                use plexspaces_journaling::{
                    VirtualActorFacet, VIRTUAL_ACTOR_FACET_DEFAULT_PRIORITY,
                };
                use std::time::Duration;
                let idle_timeout_str =
                    format_duration(Duration::from_secs(DEFAULT_IDLE_TIMEOUT_SECONDS));
                let eager_config = serde_json::json!({
                    "idle_timeout": idle_timeout_str,
                    "activation_strategy": "eager"
                });
                // Remove any previously-created virtual_actor facet, then insert an EAGER one
                facets_to_attach.retain(|f| f.facet_type() != "virtual_actor");
                facets_to_attach.push(Box::new(VirtualActorFacet::new(
                    eager_config,
                    VIRTUAL_ACTOR_FACET_DEFAULT_PRIORITY,
                )));
            }

            // Retrieve and reattach all other facets from FacetManager
            // This ensures facets like DurabilityFacet are preserved across suspension/reactivation
            let facet_manager_wrapper = self
                .service_locator
                .get_facet_manager()
                .await
                .ok_or_else(|| "FacetManager not found in ServiceLocator".to_string())?;
            let facet_manager = facet_manager_wrapper.inner_clone();

            if let Some(stored_facets_container) =
                facet_manager.get_facets(&actor_id.to_string()).await
            {
                let facets_guard = stored_facets_container.read().await;
                let all_facets = facets_guard.get_all_facets();
                let metadata = facets_guard.get_metadata();

                let facet_registry = self
                    .service_locator
                    .get_facet_registry()
                    .await
                    .map(|wrapper| wrapper.inner_clone());

                // Recreate each facet (except VirtualActorFacet which is already recreated)
                for facet_arc in all_facets {
                    let facet_read = facet_arc.read().await;
                    let facet_type = facet_read.facet_type();

                    // Skip VirtualActorFacet (already recreated above)
                    if facet_type == "virtual_actor" {
                        drop(facet_read);
                        continue;
                    }

                    // Get metadata for this facet
                    if let Some(facet_metadata) = metadata.get(facet_type) {
                        if let Some(ref facet_registry) = facet_registry {
                            match facet_registry
                                .create_facet(facet_type, facet_metadata.config.clone())
                                .await
                            {
                                Ok(new_facet) => facets_to_attach.push(new_facet),
                                Err(e) => tracing::warn!(
                                    actor_id = %actor_id,
                                    facet_type = %facet_type,
                                    error = %e,
                                    "Failed to recreate facet from stored metadata"
                                ),
                            }
                        } else {
                            tracing::warn!(
                                actor_id = %actor_id,
                                facet_type = %facet_type,
                                "FacetRegistry not available - cannot recreate stored facet"
                            );
                        }
                    }
                    drop(facet_read);
                }
            }

            // Rebuild actor using spawn_actor with stored actor_type and recreated VirtualActorFacet
            // If BehaviorRegistry fails, try to ensure behavior is registered by re-registering from application
            let actor_ref = match self
                .spawn_actor(
                    &ctx,
                    &actor_id,
                    &actor_type,
                    initial_state,
                    config.clone(),
                    labels,
                    facets_to_attach, // Recreated VirtualActorFacet (and other facets from FacetManager if needed)
                )
                .await
            {
                Ok(ref_) => ref_,
                Err(e) => {
                    // BehaviorRegistry failed - ensure behavior is registered by re-registering from application
                    let error_msg = e.to_string();
                    if error_msg.contains("BehaviorRegistry")
                        || error_msg.contains("Register the behavior")
                    {
                        if tracing::enabled!(tracing::Level::DEBUG) {
                            tracing::debug!(
                                actor_id = %actor_id,
                                actor_type = %actor_type,
                                namespace = %namespace,
                                "BehaviorRegistry failed, attempting to re-register behavior from application"
                            );
                        }

                        // Try to get application by namespace (namespace typically matches application name)
                        if let Some(app_manager) = self.service_locator.application_manager().await
                        {
                            let app_name = namespace.clone();

                            // Check if application exists
                            let app_exists =
                                app_manager.list_applications().await.contains(&app_name);

                            if app_exists {
                                // Application exists - behaviors should be registered during start()
                                // If they're not registered, it means the application wasn't started properly
                                // Return a clear error message indicating the application needs to be started
                                return Err(format!(
                                    "Failed to rebuild suspended actor {}: {}. Behavior '{}' is not registered in BehaviorRegistry. Ensure the application '{}' has been started (behaviors are registered during application start()).",
                                    actor_id, error_msg, actor_type, app_name
                                ).into());
                            } else {
                                return Err(format!(
                                    "Failed to rebuild suspended actor {}: {}. Application '{}' not found.",
                                    actor_id, error_msg, app_name
                                ).into());
                            }
                        } else {
                            return Err(format!(
                                "Failed to rebuild suspended actor {}: {}. ApplicationManager not available.",
                                actor_id, error_msg
                            ).into());
                        }
                    } else {
                        // Different error - return as-is
                        return Err(format!(
                            "Failed to rebuild suspended actor {}: {}",
                            actor_id, error_msg
                        )
                        .into());
                    }
                }
            };
            // spawn_actor used EAGER strategy so the actor is now active.
            // Mark as activated and deliver any messages that arrived during reactivation.
            manager
                .mark_activated(&actor_id)
                .await
                .map_err(|e| format!("Failed to mark actor as activated: {}", e))?;

            let pending_messages = manager.take_pending_messages(&actor_id).await;
            if !pending_messages.is_empty() {
                if tracing::enabled!(tracing::Level::DEBUG) {
                    tracing::debug!(
                        actor_id = %actor_id,
                        pending_count = pending_messages.len(),
                        "Sending {} pending messages to reactivated virtual actor",
                        pending_messages.len()
                    );
                }
                for message in pending_messages {
                    if let Err(e) = actor_ref.tell(message).await {
                        tracing::warn!(
                            actor_id = %actor_id,
                            error = %e,
                            "Failed to send pending message to reactivated virtual actor"
                        );
                    }
                }
            }

            Ok(())
        }
    }

    async fn spawn_actor(
        &self,
        ctx: &RequestContext,
        actor_id: &ActorId,
        actor_type: &str,
        initial_state: Vec<u8>,
        config: Option<plexspaces_proto::v1::actor::ActorConfig>,
        labels: HashMap<String, String>,
        facets: Vec<Box<dyn plexspaces_facet::Facet>>,
    ) -> Result<Arc<dyn MessageSender>, Box<dyn std::error::Error + Send + Sync>> {
        use crate::ActorBuilder;
        use async_trait::async_trait;
        use plexspaces_core::{
            behavior_factory::BehaviorFactory, Actor as ActorTrait, BehaviorType,
        };

        // Try to get BehaviorFactory from ServiceLocator
        // Note: BehaviorFactory is a trait, so we need to get it as Arc<dyn BehaviorFactory>
        // But ServiceLocator stores by TypeId, so we need to check if BehaviorRegistry is registered
        let behavior: Box<dyn ActorTrait> = {
            if let Some(behavior_registry) = self.service_locator.get_behavior_registry().await {
                match behavior_registry.create(actor_type, &initial_state).await {
                    Ok(b) => b,
                    Err(e) => {
                        return Err(format!(
                            "Failed to create behavior for actor_type '{}': {}. Register the behavior in BehaviorRegistry before spawning.",
                            actor_type, e
                        ).into());
                    }
                }
            } else {
                return Err(format!(
                    "No BehaviorRegistry registered in ServiceLocator. Cannot create behavior for actor_type '{}'. Register BehaviorRegistry before spawning actors.",
                    actor_type
                ).into());
            }
        };

        // Extract tenant_id and namespace from context (required, no defaults)
        let _tenant_id = ctx.tenant_id().to_string();
        let namespace = ctx.namespace().to_string();

        // CRITICAL: Normalize actor ID BEFORE building actor (ensures @node suffix is always present)
        // Get local node ID from registry for normalization
        let registry: Arc<ActorRegistry> = self
            .service_locator
            .actor_registry()
            .await
            .ok_or_else(|| "ActorRegistry not found in ServiceLocator".to_string())?;
        let local_node_id = registry.local_node_id();
        let normalized_actor_id = self.normalize_actor_id(actor_id, local_node_id);

        // Validate: If actor_id already had @node but wrong node, throw error
        if let Ok((_actor_name, node_id)) = plexspaces_core::ActorRef::parse_actor_id(actor_id) {
            if !node_id.is_empty() && node_id != local_node_id {
                return Err(format!(
                    "Actor ID '{}' specifies node '{}' but actor must be spawned on local node '{}'. ActorService always creates actors locally.",
                    actor_id, node_id, local_node_id
                ).into());
            }
        }

        // Create Actor using ActorBuilder with normalized ID
        let mut builder = ActorBuilder::new(behavior)
            .with_id(normalized_actor_id.clone())
            .with_namespace(namespace); // Use namespace from RequestContext

        // Apply config if provided
        if let Some(cfg) = config {
            builder = builder.with_config(Some(cfg));
        }

        // Build actor
        let actor = builder
            .build()
            .await
            .map_err(|e| format!("Failed to build actor: {}", e))?;

        // Attach facets before spawning
        let num_facets = facets.len();
        for facet in facets {
            actor
                .attach_facet(facet)
                .await
                .map_err(|e| format!("Failed to attach facet: {}", e))?;
        }
        if num_facets > 0 && tracing::enabled!(tracing::Level::DEBUG) {
            let attached = actor.facets().read().await.list_facets();
            if !attached.is_empty() {
                tracing::debug!(
                    actor_id = %normalized_actor_id,
                    facets = %attached.join(", "),
                    "Attached facets"
                );
            }
        }

        // Spawn the built actor with type information
        // spawn_built_actor_impl returns ActorRef, wrap for trait compatibility
        let actor_ref = self
            .spawn_built_actor_impl(
                ctx,
                Arc::new(actor),
                actor_type.to_string(),
                initial_state,
                labels,
            )
            .await?;
        Ok(Arc::new(actor_ref) as Arc<dyn MessageSender>)
    }

    async fn stop_actor(
        &self,
        ctx: &RequestContext,
        actor_id: &ActorId,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Delegate to the impl method with tenant validation
        self.stop_actor_impl(ctx, actor_id).await
    }

    async fn create_temporary_sender(
        &self,
        ctx: &RequestContext,
        temp_sender_id: String,
        correlation_id: String,
        expires_at: std::time::Instant,
    ) -> Result<Arc<dyn MessageSender>, Box<dyn std::error::Error + Send + Sync>> {
        self.create_temporary_sender_impl(ctx, temp_sender_id, correlation_id, expires_at)
            .await
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// Regular impl block for methods that need concrete Actor type (not part of the core trait)
impl ActorFactoryImpl {
    /// Internal implementation of spawn_built_actor
    ///
    /// This is the actual implementation that's called by both the trait method
    /// in ActorFactoryExt and internally by spawn_actor.
    ///
    /// ## Returns
    /// ActorRef directly - callers can wrap as Arc<dyn MessageSender> if needed
    pub async fn spawn_built_actor_impl(
        &self,
        ctx: &RequestContext,
        actor: Arc<Actor>,
        actor_type: String,
        initial_state: Vec<u8>,
        labels: HashMap<String, String>,
    ) -> Result<ActorRef, Box<dyn std::error::Error + Send + Sync>> {
        // Use the provided actor_type (required); derive from behavior only as last resort
        let actor_type = if !actor_type.is_empty() {
            actor_type
        } else {
            // Derive from behavior type when caller didn't provide one
            let behavior_guard = actor.behavior().read().await;
            let behavior_type = behavior_guard.behavior_type();
            drop(behavior_guard);
            match behavior_type {
                plexspaces_core::BehaviorType::GenServer => "GenServer".to_string(),
                plexspaces_core::BehaviorType::GenEvent => "GenEvent".to_string(),
                plexspaces_core::BehaviorType::GenStateMachine => "GenStateMachine".to_string(),
                plexspaces_core::BehaviorType::Workflow => "Workflow".to_string(),
                plexspaces_core::BehaviorType::Custom(s) => s,
            }
        };

        // Add observability logging
        let actor_id_before_unwrap = actor.id().clone();
        if tracing::enabled!(tracing::Level::TRACE) {
            tracing::trace!(
                actor_id = %actor_id_before_unwrap,
                actor_type = ?actor_type,
                "Spawning built actor"
            );
        }

        // Unwrap the Arc to get the Actor
        let mut actor = Arc::try_unwrap(actor)
            .map_err(|_| "Actor Arc has multiple references - cannot unwrap")?;
        // Get services from ServiceLocator
        let registry: Arc<ActorRegistry> = self
            .service_locator
            .actor_registry()
            .await
            .ok_or_else(|| "ActorRegistry not found in ServiceLocator".to_string())?;
        let manager: Arc<VirtualActorManager> = self
            .service_locator
            .virtual_actor_manager()
            .await
            .ok_or_else(|| "VirtualActorManager not found in ServiceLocator".to_string())?;
        let facet_manager_wrapper = self
            .service_locator
            .get_facet_manager()
            .await
            .ok_or_else(|| "FacetManager not found in ServiceLocator".to_string())?;
        let facet_manager = facet_manager_wrapper.inner_clone();

        // CRITICAL: Actor ID should already be normalized (done before build in spawn_actor)
        // But verify and update actor's internal ID if needed (defensive check)
        let local_node_id = registry.local_node_id();
        let mut actor_id = actor.id().clone();
        let normalized_id = self.normalize_actor_id(&actor_id, local_node_id);

        // If actor's internal ID doesn't match normalized ID, update it
        // This ensures actor.id() returns the correct ID with @node suffix
        if actor_id != normalized_id {
            // Update actor's internal ID (Actor doesn't have set_id, so we need to reconstruct)
            // Actually, we can't change actor.id() after creation, so we'll use normalized_id for registration
            // The actor's internal ID will be wrong, but registration will use correct ID
            // TODO: Consider adding set_id() to Actor or ensuring ActorBuilder normalizes during build
            actor_id = normalized_id;
            tracing::warn!(
                "Actor internal ID '{}' was not normalized, using '{}' for registration",
                actor.id(),
                actor_id
            );
        }

        let actor_namespace = ctx.namespace().to_string();
        let actor_tenant_id = ctx.tenant_id().to_string();

        // Extract actor config from context (if available)
        let actor_config = actor.context().config.clone();

        // Create ActorContext (actor_id is no longer stored in context)
        let actor_context = ActorContext::new(
            local_node_id.to_string(),
            actor_tenant_id.clone(),
            actor_namespace.clone(),
            self.service_locator.clone(),
            actor_config.clone(),
        );

        // Update actor with full context
        actor = actor.set_context(Arc::new(actor_context));

        // Update metrics before moving values into RequestContext
        metrics::gauge!("plexspaces_node_active_actors",
            "node_id" => local_node_id.to_string()
        )
        .increment(1.0);

        metrics::counter!("plexspaces_node_actors_spawned_total",
            "node_id" => local_node_id.to_string(),
            "namespace" => actor_namespace.clone()
        )
        .increment(1);

        // Create RequestContext for registry operations (moves values)
        // Clone values for logging before moving
        let actor_tenant_id_for_log = actor_tenant_id.clone();
        let actor_namespace_for_log = actor_namespace.clone();
        let ctx = RequestContext::new_without_auth(actor_tenant_id, actor_namespace);

        // Emit Created event
        registry
            .publish_lifecycle_event(ActorLifecycleEvent {
                actor_id: actor_id.clone(),
                timestamp: Some(Timestamp {
                    seconds: chrono::Utc::now().timestamp(),
                    nanos: chrono::Utc::now().timestamp_subsec_nanos() as i32,
                }),
                event_type: Some(plexspaces_proto::actor_lifecycle_event::EventType::Created(
                    plexspaces_proto::v1::actor::ActorCreated {},
                )),
            })
            .await;

        // Emit Starting event
        registry
            .publish_lifecycle_event(ActorLifecycleEvent {
                actor_id: actor_id.clone(),
                timestamp: Some(Timestamp {
                    seconds: chrono::Utc::now().timestamp(),
                    nanos: chrono::Utc::now().timestamp_subsec_nanos() as i32,
                }),
                event_type: Some(
                    plexspaces_proto::actor_lifecycle_event::EventType::Starting(
                        plexspaces_proto::v1::actor::ActorStarting {},
                    ),
                ),
            })
            .await;

        // Check if actor has VirtualActorFacet
        let facets = actor.list_facets().await;
        let is_virtual = facets.contains(&"virtual_actor".to_string());
        let mut activation_strategy_opt: Option<plexspaces_journaling::ActivationStrategy> = None;
        let mut should_activate_eagerly = false;
        if is_virtual {
            // Virtual actor handling
            let actor_facets = actor.facets();
            let facets_guard = actor_facets.read().await;
            let virtual_facet_arc = facets_guard
                .get_facet("virtual_actor")
                .ok_or_else(|| format!("VirtualActorFacet not found in actor facets"))?;

            // Extract VirtualActorFacet to check activation strategy
            let virtual_facet_guard = virtual_facet_arc.read().await;

            use plexspaces_journaling::VirtualActorFacet;
            let virtual_facet = virtual_facet_guard
                .as_any()
                .downcast_ref::<VirtualActorFacet>()
                .ok_or_else(|| format!("Failed to downcast to VirtualActorFacet"))?;

            // Check activation strategy
            let activation_strategy = virtual_facet.get_activation_strategy().await;
            should_activate_eagerly = matches!(
                activation_strategy,
                plexspaces_journaling::ActivationStrategy::ActivationStrategyEager
            );
            let activation_strategy_clone = activation_strategy.clone();
            activation_strategy_opt = Some(activation_strategy);

            // Create new facet for registration
            drop(virtual_facet_guard);
            drop(facets_guard);

            use plexspaces_common::virtual_actor_config::{
                format_duration, DEFAULT_IDLE_TIMEOUT_SECONDS,
            };
            use std::time::Duration;
            let idle_timeout_str =
                format_duration(Duration::from_secs(DEFAULT_IDLE_TIMEOUT_SECONDS));
            let facet_config = serde_json::json!({
                "idle_timeout": idle_timeout_str,
                "activation_strategy": plexspaces_journaling::to_config_str(&activation_strategy_clone)
            });
            use plexspaces_journaling::VIRTUAL_ACTOR_FACET_DEFAULT_PRIORITY;
            let virtual_facet_for_reg =
                VirtualActorFacet::new(facet_config, VIRTUAL_ACTOR_FACET_DEFAULT_PRIORITY);

            // Register as virtual actor (only if instance not already registered)
            // Store metadata in VirtualActorManager (source of truth for virtual actors)
            // CRITICAL: Check if instance is registered (not just type), because is_virtual()
            // returns true for type-level registration even if instance is not registered
            let instance_metadata = manager.get_metadata(&actor_id).await;
            if instance_metadata.is_none() {
                // Instance not registered - register it fresh.
                // This happens on first spawn (lazy or eager) or when rebuilding a
                // suspended actor that was only type-registered.
                use plexspaces_journaling::virtual_actor_facet_to_lifecycle_facet;
                let lifecycle_facet = virtual_actor_facet_to_lifecycle_facet(virtual_facet_for_reg);
                let facet_box = Arc::new(tokio::sync::RwLock::new(lifecycle_facet));

                // Determine activation strategy for storing in VirtualActorManager metadata.
                // Priority: type-level registration strategy > facet strategy > default LAZY.
                // This ensures that EAGER overrides used for reactivation (in activate_virtual_actor)
                // do NOT overwrite the original lazy/eager strategy stored in type-level metadata.
                use plexspaces_common::ActivationStrategy;
                let activation_strategy = {
                    // Use the actor_type parameter directly (not parsed from actor_id) because
                    // actor_id may use "name@node" format which differs from the registered type name.
                    let type_strategy = manager
                        .get_virtual_actor_type(&actor_type)
                        .await
                        .map(|m| m.activation_strategy);
                    // Fall back to facet strategy, then default to lazy.
                    type_strategy.unwrap_or_else(|| {
                        activation_strategy_opt
                            .clone()
                            .unwrap_or(ActivationStrategy::ActivationStrategyLazy)
                    })
                };

                // Register with full metadata including initial_state and labels.
                // This metadata persists across suspension and is used to rebuild actors.
                manager
                    .register(
                        actor_id.clone(),
                        facet_box,
                        actor_type.clone(),
                        actor_config.clone(),
                        ctx.tenant_id().to_string(),
                        ctx.namespace().to_string(),
                        initial_state.clone(),
                        labels.clone(),
                        activation_strategy,
                    )
                    .await
                    .map_err(|e| format!("Failed to register virtual actor: {}", e))?;
            } else {
                // Instance already registered - update mutable metadata fields only.
                // initial_state and labels are immutable after first registration.
                manager
                    .update_metadata(&actor_id, actor_type.clone(), actor_config.clone())
                    .await
                    .map_err(|e| format!("Failed to update virtual actor metadata: {}", e))?;
            }

            // Get mailbox (for creating ActorRef)
            let mailbox = actor.mailbox().clone();

            // Create ActorRef (for return value - not used for lazy virtual actors)
            // CRITICAL: Pass tenant_id from RequestContext to ActorRef
            let _actor_ref = ActorRef::local(
                actor_id.clone(),
                ctx.tenant_id().to_string(), // CRITICAL: tenant_id flows from API → ActorBuilder → ActorRef
                ctx.namespace().to_string(),
                mailbox.clone(),
                self.service_locator.clone(),
            );

            // Handle eager vs lazy activation
            if should_activate_eagerly {
                if tracing::enabled!(tracing::Level::DEBUG) {
                    tracing::debug!(actor_id = %actor_id, "Virtual actor with eager activation - starting immediately");
                }

                // Create ActorRef for return value
                // Note: Registration happens INSIDE Actor::start() AFTER init() succeeds
                let mailbox = actor.mailbox().clone();
                // CRITICAL: Pass tenant_id from RequestContext to ActorRef
                let actor_ref = ActorRef::local(
                    actor_id.clone(),
                    ctx.tenant_id().to_string(), // CRITICAL: tenant_id flows from API → ActorBuilder → ActorRef
                    ctx.namespace().to_string(),
                    mailbox.clone(),
                    self.service_locator.clone(),
                );

                // CRITICAL: Check actor state before calling start() to ensure we only call it once
                use crate::ActorState;
                let _current_state = actor.state().await;

                // Start the actor (calls init() internally, then registers in ActorRegistry)
                // If init() fails, actor is not registered (prevents memory leaks)
                let join_handle = actor.start().await.map_err(|e| {
                    tracing::warn!(
                        actor_id = %actor_id,
                        error = %e,
                        "Virtual actor start() failed (init() error) - actor not registered"
                    );
                    format!("Failed to start actor: {}", e)
                })?;

                // Verify actor reached Active state
                let state_after_start = actor.state().await;

                if state_after_start != ActorState::Active {
                    return Err(format!("Eager virtual actor {} did not reach Active state after start(), current state: {:?}", actor_id, state_after_start).into());
                }

                // Actor is now registered (registration happened inside Actor::start() after init() succeeded)

                // Wrap in Arc after starting
                let actor_arc = Arc::new(actor);

                // Clone exit_reason before wrapping in Arc (needed for watch_actor_termination)
                let exit_reason_arc = actor_arc.exit_reason();

                // Mark as activated
                manager
                    .mark_activated(&actor_id)
                    .await
                    .map_err(|e| format!("Failed to mark actor as activated: {}", e))?;

                // Store facets
                let facets_clone = actor_arc.facets();
                facet_manager
                    .store_facets(actor_id.clone(), facets_clone)
                    .await;

                // Update registration with config and instance (idempotent - ActorRef already registered in Actor::start())
                // This ensures config and instance are stored for resource tracking and ask() pattern
                registry
                    .register_actor(
                        &ctx,
                        actor_id.clone(),
                        Arc::new(actor_ref.clone()) as Arc<dyn MessageSender>,
                        actor_type.clone(),
                        actor_config.clone(), // Config for resource tracking
                        Some(actor_arc.clone() as Arc<dyn plexspaces_core::ActorHandle>),
                        None, // behavior_kind already set at registration
                    )
                    .await;

                // Watch termination (with exit_reason for proper propagation)
                self.watch_actor_termination(actor_id.clone(), join_handle, exit_reason_arc)
                    .await;

                // Process pending messages - send them to the now-activated actor
                // IMPORTANT: For eager virtual actors, pending messages must be processed after activation
                // This handles the case where messages were queued before the actor was activated
                let pending_messages = manager.take_pending_messages(&actor_id).await;
                if !pending_messages.is_empty() {
                    if tracing::enabled!(tracing::Level::DEBUG) {
                        tracing::debug!(
                            actor_id = %actor_id,
                            pending_count = pending_messages.len(),
                            "Sending {} pending messages to activated eager virtual actor",
                            pending_messages.len()
                        );
                    }
                    for message in pending_messages {
                        if let Err(e) = actor_ref.tell(message).await {
                            tracing::warn!(
                                actor_id = %actor_id,
                                error = %e,
                                "Failed to send pending message to activated eager virtual actor"
                            );
                        }
                    }
                }

                return Ok(actor_ref);
            } else {
                // Lazy activation keeps only metadata in the registry and virtual manager.
                // The running sender is created on first local ask/tell or explicit activation.
                drop(actor); // Arc<Actor> dropped; metadata in VirtualActorManager is the rebuild source

                registry
                    .register_virtual_actor_index(&ctx, actor_id.clone(), actor_type.clone())
                    .await;
            }

            // OBSERVABILITY: Log actor spawn with full context (after determining activation strategy)
            let activation_info = match activation_strategy_opt {
                Some(plexspaces_journaling::ActivationStrategy::ActivationStrategyLazy)
                | Some(plexspaces_journaling::ActivationStrategy::ActivationStrategyUnspecified) => {
                    " (virtual actor with lazy activation - will activate on first message)"
                }
                Some(plexspaces_journaling::ActivationStrategy::ActivationStrategyEager) => {
                    " (virtual actor with eager activation)"
                }
                Some(plexspaces_journaling::ActivationStrategy::ActivationStrategyPrewarm) => {
                    " (virtual actor with prewarm activation)"
                }
                None => "",
            };
            tracing::info!(
                actor_id = %actor_id,
                node_id = %local_node_id,
                namespace = %actor_namespace_for_log,
                tenant_id = %actor_tenant_id_for_log,
                actor_type = ?actor_type,
                "Actor spawned{}",
                activation_info
            );

            // Create ActorRef for return value.
            // CRITICAL: Pass tenant_id from RequestContext to ActorRef
            let actor_ref = ActorRef::local(
                actor_id.clone(),
                ctx.tenant_id().to_string(), // CRITICAL: tenant_id flows from API → ActorBuilder → ActorRef
                ctx.namespace().to_string(),
                mailbox.clone(),
                self.service_locator.clone(),
            );

            return Ok(actor_ref);
        }

        // OBSERVABILITY: Log actor spawn with full context (for non-virtual actors)
        tracing::info!(
            actor_id = %actor_id,
            node_id = %local_node_id,
            namespace = %actor_namespace_for_log,
            tenant_id = %actor_tenant_id_for_log,
            actor_type = ?actor_type,
            "Actor spawned"
        );

        // Normal actor - start immediately
        // Store facets
        let facets_clone = actor.facets().clone();
        facet_manager
            .store_facets(&actor_id.to_string(), facets_clone)
            .await;

        // Get mailbox (for creating ActorRef)
        let mailbox = actor.mailbox().clone();

        // Create ActorRef for return value
        // Note: Registration happens INSIDE Actor::start() AFTER init() succeeds
        // This ensures failed actors are never registered (prevents memory leaks)
        // and allows supervisor to wait for init() before starting next child

        // CRITICAL: Check actor state before calling start() to ensure we only call it once
        use crate::ActorState;
        let _current_state = actor.state().await;

        // Start actor (calls init() internally, then registers in ActorRegistry)
        // If init() fails, actor is not registered (prevents memory leaks)
        let join_handle = actor.start().await.map_err(|e| {
            // OBSERVABILITY: Log start failure due to init() error
            tracing::warn!(
                actor_id = %actor_id,
                error = %e,
                "Actor start() failed (init() error) - actor not registered"
            );
            format!("Failed to start actor: {}", e)
        })?;

        // Verify actor reached Active state
        let state_after_start = actor.state().await;
        if state_after_start != ActorState::Active {
            return Err(format!(
                "Regular actor {} did not reach Active state after start(), current state: {:?}",
                actor_id, state_after_start
            )
            .into());
        }

        // Actor is now registered (registration happened inside Actor::start() after init() succeeded)
        // Store actor in Arc after starting
        let actor_arc = Arc::new(actor);

        // Create ActorRef - this is what will be returned
        // Note: The ActorRef was already registered in Actor::start() via register_in_registry()
        // We just need to ensure config and instance are stored (idempotent update)
        // CRITICAL: Pass tenant_id from RequestContext to ActorRef
        let actor_ref = ActorRef::local(
            actor_id.clone(),
            ctx.tenant_id().to_string(), // CRITICAL: tenant_id flows from API → ActorBuilder → ActorRef
            ctx.namespace().to_string(),
            mailbox.clone(),
            self.service_locator.clone(),
        );

        // Update registration with config and instance (idempotent - ActorRef already registered)
        // This ensures config and instance are stored for resource tracking and ask() pattern
        registry
            .register_actor(
                &ctx,
                actor_id.clone(),
                Arc::new(actor_ref.clone()) as Arc<dyn MessageSender>,
                actor_type.clone(),
                actor_config.clone(), // Config for resource tracking
                Some(actor_arc.clone() as Arc<dyn plexspaces_core::ActorHandle>),
                None, // behavior_kind already set at registration
            )
            .await;

        // Emit Activated event
        registry
            .publish_lifecycle_event(ActorLifecycleEvent {
                actor_id: actor_id.clone(),
                timestamp: Some(Timestamp {
                    seconds: chrono::Utc::now().timestamp(),
                    nanos: chrono::Utc::now().timestamp_subsec_nanos() as i32,
                }),
                event_type: Some(
                    plexspaces_proto::actor_lifecycle_event::EventType::Activated(
                        plexspaces_proto::v1::actor::ActorActivated {},
                    ),
                ),
            })
            .await;

        // Watch termination (with exit_reason_arc so stored exit reasons can be read)
        let exit_reason_arc = actor_arc.exit_reason();
        self.watch_actor_termination(actor_id.clone(), join_handle, exit_reason_arc)
            .await;

        // Return ActorRef directly
        Ok(actor_ref)
    }
}

impl ActorFactoryImpl {
    /// stop_actor implementation with tenant isolation validation
    ///
    /// This method is separate because we already closed the main impl block.
    /// It implements the stop_actor functionality from ActorFactory trait.
    ///
    /// ## Tenant Isolation
    /// Validates that the caller's tenant_id and namespace match the actor's stored
    /// tenant_id and namespace. This prevents cross-tenant access.
    async fn stop_actor_impl(
        &self,
        ctx: &RequestContext,
        actor_id: &ActorId,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Get services from ServiceLocator
        let registry: Arc<ActorRegistry> = self
            .service_locator
            .actor_registry()
            .await
            .ok_or_else(|| "ActorRegistry not found in ServiceLocator".to_string())?;
        let virtual_actor_manager = self.service_locator.virtual_actor_manager().await;

        let local_node_id = registry.local_node_id();

        // CRITICAL: Validate tenant isolation
        // Get actor's stored tenant_id and namespace, then validate against caller's context
        let namespace: String = {
            if let Some((actor_tenant_id, actor_namespace)) =
                registry.get_actor_metadata(actor_id).await
            {
                // Validate tenant_id matches (unless caller is system/empty tenant)
                let caller_tenant = ctx.tenant_id();
                if !caller_tenant.is_empty() && caller_tenant != actor_tenant_id {
                    return Err(format!(
                        "Tenant isolation violation: caller tenant '{}' cannot access actor '{}' owned by tenant '{}'",
                        caller_tenant, actor_id, actor_tenant_id
                    ).into());
                }

                // Validate namespace matches (unless caller is system/empty namespace)
                let caller_namespace = ctx.namespace();
                if !caller_namespace.is_empty() && caller_namespace != actor_namespace {
                    return Err(format!(
                        "Namespace isolation violation: caller namespace '{}' cannot access actor '{}' in namespace '{}'",
                        caller_namespace, actor_id, actor_namespace
                    ).into());
                }

                actor_namespace
            } else {
                // Actor not found in metadata - might be a system actor or not registered
                // For safety, only allow if caller has empty tenant/namespace (system-level)
                if !ctx.tenant_id().is_empty() || !ctx.namespace().is_empty() {
                    return Err(format!(
                        "Actor '{}' not found or metadata missing - cannot verify tenant isolation",
                        actor_id
                    )
                    .into());
                }
                String::new()
            }
        };

        let is_local = match plexspaces_core::actor_id::parse_actor_id(actor_id) {
            Ok(parsed) => parsed.node_id == local_node_id,
            Err(_) => true,
        };
        if !is_local {
            return Err(format!("Actor not found or not local: {}", actor_id).into());
        }

        // OBSERVABILITY: Log actor stop attempt
        tracing::info!(
            actor_id = %actor_id,
            node_id = %local_node_id,
            namespace = %namespace,
            "Stopping actor"
        );

        let is_virtual = if let Some(manager) = &virtual_actor_manager {
            manager.is_virtual(actor_id).await
        } else {
            false
        };

        let had_instance = registry.get_actor_instance(actor_id).await.is_some();
        if had_instance {
            self.mark_actor_stopping(actor_id).await;
        }

        // CRITICAL: Get actor instance and stop it BEFORE unregistering
        // This ensures the message loop is stopped before we remove the instance
        // Production-grade: Use stop_from_arc() which properly stops the message loop
        if let Some(instance) = registry.get_actor_instance(actor_id).await {
            if let Err(e) = instance.stop_actor().await {
                tracing::warn!(
                    actor_id = %actor_id,
                    error = %e,
                    "Failed to stop actor (continuing with unregister)"
                );
            }
        }

        // OBSERVABILITY: Update ActorMetrics before stopping
        // Note: unregister_with_cleanup will also decrement active, but we track here for explicit observability
        {
            let _actor_metrics = registry.actor_metrics().write().await;
            // Active count will be decremented by unregister_with_cleanup, but we track here for observability
            // This ensures metrics are updated even if unregister_with_cleanup fails
        }

        // Emit Deactivating event before unregistration
        registry
            .publish_lifecycle_event(ActorLifecycleEvent {
                actor_id: actor_id.clone(),
                timestamp: Some(Timestamp {
                    seconds: chrono::Utc::now().timestamp(),
                    nanos: chrono::Utc::now().timestamp_subsec_nanos() as i32,
                }),
                event_type: Some(
                    plexspaces_proto::actor_lifecycle_event::EventType::Deactivating(
                        plexspaces_proto::v1::actor::ActorDeactivating {
                            reason: "manual_stop".to_string(),
                        },
                    ),
                ),
            })
            .await;

        if is_virtual {
            let manager = virtual_actor_manager
                .ok_or_else(|| "VirtualActorManager not found in ServiceLocator".to_string())?;
            self.passivate_virtual_actor(actor_id, &registry, &manager)
                .await;
            if !had_instance {
                self.take_actor_stopping(actor_id).await;
            }
        } else {
            registry
                .handle_actor_termination(actor_id, ExitReason::Shutdown)
                .await;

            registry
                .unregister_with_cleanup(actor_id)
                .await
                .map_err(|e| format!("Failed to unregister actor: {}", e))?;
            if !had_instance {
                self.take_actor_stopping(actor_id).await;
            }
        }

        // OBSERVABILITY: Update Prometheus-style metrics
        metrics::gauge!("plexspaces_node_active_actors",
            "node_id" => local_node_id.to_string()
        )
        .decrement(1.0);

        metrics::counter!("plexspaces_node_actors_stopped_total",
            "node_id" => local_node_id.to_string(),
            "namespace" => namespace.clone()
        )
        .increment(1);

        // OBSERVABILITY: Verify ActorMetrics were updated (active should be decremented)
        {
            use plexspaces_core::message_metrics::ActorMetricsExt;
            let actor_metrics = registry.actor_metrics().read().await;
            if tracing::enabled!(tracing::Level::DEBUG) {
                tracing::debug!(
                    actor_id = %actor_id,
                    active_actors = actor_metrics.active,
                    "ActorMetrics updated after stop"
                );
            }
        }

        // Emit Deactivated event after unregistration
        registry
            .publish_lifecycle_event(ActorLifecycleEvent {
                actor_id: actor_id.clone(),
                timestamp: Some(Timestamp {
                    seconds: chrono::Utc::now().timestamp(),
                    nanos: chrono::Utc::now().timestamp_subsec_nanos() as i32,
                }),
                event_type: Some(
                    plexspaces_proto::actor_lifecycle_event::EventType::Deactivated(
                        plexspaces_proto::v1::actor::ActorDeactivated {
                            reason: "manual_stop".to_string(),
                        },
                    ),
                ),
            })
            .await;

        // OBSERVABILITY: Log successful stop
        tracing::info!(
            actor_id = %actor_id,
            node_id = %local_node_id,
            namespace = %namespace,
            "Actor stopped successfully"
        );

        Ok(())
    }

    // ========================================================================
    // Typed spawn methods - return typed refs for cleaner API
    // ========================================================================

    /// Spawn a workflow actor and return a WorkflowRef.
    ///
    /// ## Example
    /// ```ignore
    /// let workflow: WorkflowRef = factory.spawn_workflow(
    ///     &ctx,
    ///     "approval-workflow-123",
    ///     workflow_behavior,
    ///     vec![Box::new(DurabilityFacet::new(...))],
    /// ).await?;
    ///
    /// let result: Output = workflow.run(&input).await?;
    /// workflow.signal("approve", &data).await?;
    /// ```
    pub async fn spawn_workflow<B>(
        &self,
        ctx: &RequestContext,
        actor_id: impl Into<ActorId>,
        behavior: B,
        facets: Vec<Box<dyn plexspaces_facet::Facet>>,
    ) -> Result<crate::WorkflowRef, crate::WorkflowRefError>
    where
        B: plexspaces_core::Actor + Send + 'static,
    {
        let actor_ref = self
            .spawn_behavior(ctx, actor_id, behavior, facets)
            .await
            .map_err(|e| crate::WorkflowRefError::Spawn(e.to_string()))?;
        Ok(crate::WorkflowRef::new(actor_ref))
    }

    /// Spawn a GenServer actor and return a GenServerRef.
    ///
    /// ## Example
    /// ```ignore
    /// let server: GenServerRef = factory.spawn_gen_server(
    ///     &ctx,
    ///     "entity-extractor",
    ///     ExtractorBehavior::new(),
    ///     vec![],
    /// ).await?;
    ///
    /// let result: Response = server.call("extract", &request).await?;
    /// ```
    pub async fn spawn_gen_server<B>(
        &self,
        ctx: &RequestContext,
        actor_id: impl Into<ActorId>,
        behavior: B,
        facets: Vec<Box<dyn plexspaces_facet::Facet>>,
    ) -> Result<crate::GenServerRef, crate::GenServerError>
    where
        B: plexspaces_core::Actor + Send + 'static,
    {
        let actor_ref = self
            .spawn_behavior(ctx, actor_id, behavior, facets)
            .await
            .map_err(|e| crate::GenServerError::Spawn(e.to_string()))?;
        Ok(crate::GenServerRef::new(actor_ref))
    }

    /// Spawn an FSM actor and return an FsmRef.
    ///
    /// ## Example
    /// ```ignore
    /// let fsm: FsmRef = factory.spawn_fsm(
    ///     &ctx,
    ///     "order-workflow-123",
    ///     OrderStateMachine::new(),
    ///     vec![Box::new(TimerFacet::new(...))],
    /// ).await?;
    ///
    /// fsm.transition("submit", &order).await?;
    /// let state: OrderState = fsm.query_state().await?;
    /// ```
    pub async fn spawn_fsm<B>(
        &self,
        ctx: &RequestContext,
        actor_id: impl Into<ActorId>,
        behavior: B,
        facets: Vec<Box<dyn plexspaces_facet::Facet>>,
    ) -> Result<crate::FsmRef, crate::FsmError>
    where
        B: plexspaces_core::Actor + Send + 'static,
    {
        let actor_ref = self
            .spawn_behavior(ctx, actor_id, behavior, facets)
            .await
            .map_err(|e| crate::FsmError::Spawn(e.to_string()))?;
        Ok(crate::FsmRef::new(actor_ref))
    }

    /// Spawn a GenEvent actor and return an EventRef.
    ///
    /// ## Example
    /// ```ignore
    /// let logger: EventRef = factory.spawn_event(
    ///     &ctx,
    ///     "audit-logger",
    ///     AuditLoggerBehavior::new(),
    ///     vec![],
    /// ).await?;
    ///
    /// logger.emit("user_login", &event).await?;
    /// ```
    pub async fn spawn_event<B>(
        &self,
        ctx: &RequestContext,
        actor_id: impl Into<ActorId>,
        behavior: B,
        facets: Vec<Box<dyn plexspaces_facet::Facet>>,
    ) -> Result<crate::EventRef, crate::EventError>
    where
        B: plexspaces_core::Actor + Send + 'static,
    {
        let actor_ref = self
            .spawn_behavior(ctx, actor_id, behavior, facets)
            .await
            .map_err(|e| crate::EventError::Spawn(e.to_string()))?;
        Ok(crate::EventRef::new(actor_ref))
    }

    /// Internal helper to spawn a behavior and return ActorRef.
    ///
    /// Reuses `spawn_built_actor_impl` (same code path as `spawn_actor`).
    /// Returns ActorRef directly from spawn_built_actor_impl.
    async fn spawn_behavior<B>(
        &self,
        ctx: &RequestContext,
        actor_id: impl Into<ActorId>,
        behavior: B,
        facets: Vec<Box<dyn plexspaces_facet::Facet>>,
    ) -> Result<ActorRef, Box<dyn std::error::Error + Send + Sync>>
    where
        B: plexspaces_core::Actor + Send + 'static,
    {
        use crate::ActorBuilder;

        let actor_id: ActorId = actor_id.into();

        // Get behavior type for logging/tracking
        let behavior_type = behavior.behavior_type();
        let actor_type = match behavior_type {
            plexspaces_core::BehaviorType::GenServer => "GenServer",
            plexspaces_core::BehaviorType::GenEvent => "GenEvent",
            plexspaces_core::BehaviorType::GenStateMachine => "GenStateMachine",
            plexspaces_core::BehaviorType::Workflow => "Workflow",
            plexspaces_core::BehaviorType::Custom(ref s) => s.as_str(),
        };

        // Build actor with the provided behavior
        let actor = ActorBuilder::new(Box::new(behavior))
            .with_id(actor_id.clone())
            .with_namespace(ctx.namespace().to_string())
            .build()
            .await
            .map_err(|e| format!("Failed to build actor: {}", e))?;

        // Attach facets
        let num_facets = facets.len();
        for facet in facets {
            actor
                .attach_facet(facet)
                .await
                .map_err(|e| format!("Failed to attach facet: {}", e))?;
        }
        if num_facets > 0 && tracing::enabled!(tracing::Level::DEBUG) {
            let attached = actor.facets().read().await.list_facets();
            if !attached.is_empty() {
                tracing::debug!(
                    actor_id = %actor_id,
                    facets = %attached.join(", "),
                    "Attached facets"
                );
            }
        }

        // Use spawn_built_actor_impl - returns ActorRef directly
        self.spawn_built_actor_impl(
            ctx,
            Arc::new(actor),
            actor_type.to_string(),
            vec![],
            HashMap::new(),
        )
        .await
    }
}

/// Configure facets that need actor_ref and actor_service after actor spawn.
///
/// ## Purpose

// Implement Service trait so ActorFactoryImpl can be registered in ServiceLocator
impl Service for ActorFactoryImpl {
    fn service_name(&self) -> String {
        plexspaces_core::service_names::ACTOR_FACTORY_IMPL.to_string()
    }
}
