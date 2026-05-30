// SPDX-License-Identifier: AGPL-3.0-or-later
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

use crate::core::{
    ActorContext, ActorFactory, ActorId, ActorRegistry, ExitReason,
    MessageSender, RequestContext, RequestContextExt, Service,
    ServiceLocator as ServiceLocatorTrait, VirtualActorManager,
};
use crate::{ActorInstance, ActorRef};
use async_trait::async_trait;
use plexspaces_common::ServiceNameExt;
use plexspaces_mailbox::{Mailbox, MailboxConfig};
use plexspaces_proto::actor::v1::ActorVisibility;
use plexspaces_proto::ActorLifecycleEvent;
use prost_types::Timestamp;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;

/// Single DEBUG line after facets attach; includes mailbox stats when durability is present.
async fn debug_log_attached_facets(actor: &ActorInstance, actor_id: &ActorId) {
    if !tracing::enabled!(tracing::Level::DEBUG) {
        return;
    }
    let attached = actor.facets().read().await.list_facets();
    if attached.is_empty() {
        return;
    }
    let facet_str = attached.join(", ");
    if attached.iter().any(|t| t == "durability") {
        let st = actor.mailbox().get_stats().await;
        tracing::debug!(
            actor_id = %actor_id,
            facets = %facet_str,
            mailbox_backend = %st.backend_type,
            mailbox_is_durable = st.is_durable,
            mailbox_size = st.total_size(),
            "Attached facets"
        );
    } else {
        tracing::debug!(
            actor_id = %actor_id,
            facets = %facet_str,
            "Attached facets"
        );
    }
}

async fn actor_behavior_kind(actor: &ActorInstance) -> Option<String> {
    let behavior = actor.behavior().read().await;
    Some(match behavior.behavior_kind() {
        crate::core::BehaviorType::GenServer => "GenServer".to_string(),
        crate::core::BehaviorType::GenEvent => "GenEvent".to_string(),
        crate::core::BehaviorType::GenStateMachine => "GenStateMachine".to_string(),
        crate::core::BehaviorType::Workflow => "Workflow".to_string(),
        crate::core::BehaviorType::Custom(value) => value,
    })
}

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
    /// Create a new `ActorFactoryImpl` backed by the given service locator.
    pub fn new(service_locator: Arc<dyn ServiceLocatorTrait>) -> Self {
        Self {
            service_locator,
            stopping_actors: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    /// Create a new `ActorFactoryImpl` wrapped in an `Arc`.
    pub async fn new_arc(service_locator: Arc<dyn ServiceLocatorTrait>) -> Arc<Self> {
        Arc::new(Self::new(service_locator))
    }

    async fn start_registered_local_actor(
        &self,
        mut actor: ActorInstance,
        actor_id: &ActorId,
        ctx: &RequestContext,
        registry: &Arc<ActorRegistry>,
        facet_manager: &Arc<plexspaces_facet::FacetManager>,
        spawn_visibility: ActorVisibility,
    ) -> Result<(Arc<ActorInstance>, ActorRef), Box<dyn std::error::Error + Send + Sync>> {
        let facets_clone = actor.facets().clone();
        let mailbox = actor.mailbox().clone();

        let join_handle = actor.start().await.map_err(|e| {
            tracing::warn!(
                actor_id = %actor_id,
                error = %e,
                "Actor start() failed (init() error) - actor not registered"
            );
            format!("Failed to start actor: {}", e)
        })?;

        let state_after_start = actor.state().await;
        if state_after_start != crate::ActorState::Active {
            return Err(format!(
                "Actor {} did not reach Active state after start(), current state: {:?}",
                actor_id, state_after_start
            )
            .into());
        }

        let actor_arc = Arc::new(actor);
        let exit_reason_arc = actor_arc.exit_reason();

        facet_manager
            .store_facets(actor_id.to_string(), facets_clone)
            .await;
        actor_arc
            .register_started(registry, spawn_visibility.clone())
            .await;

        let actor_ref = ActorRef::local(
            actor_id.clone(),
            ctx.tenant_id().to_string(),
            ctx.namespace().to_string(),
            mailbox,
            self.service_locator.clone(),
            spawn_visibility,
        );

        registry
            .publish_lifecycle_event(ActorLifecycleEvent {
                actor_id: actor_id.to_string(),
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

        self.watch_actor_termination(actor_id.clone(), join_handle, exit_reason_arc)
            .await;

        Ok((actor_arc, actor_ref))
    }

    async fn mark_actor_stopping(&self, actor_id: &ActorId) {
        self.stopping_actors.write().await.insert(actor_id.clone());
    }

    async fn take_actor_stopping(&self, actor_id: &ActorId) -> bool {
        self.stopping_actors.write().await.remove(actor_id)
    }

    /// Create temporary sender ActorRef for ask() pattern
    ///
    /// ## Purpose
    /// Creates a temporary sender ActorRef that routes replies to ReplyWaiter.
    /// This is used by the ask() pattern to collect replies asynchronously.
    ///
    /// ## Arguments
    /// * `ctx` - RequestContext with proper tenant/namespace (first parameter)
    /// * `temp_sender_id` - Temporary sender ID in canonical ActorId string form
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
    pub async fn create_temporary_sender_impl(
        &self,
        ctx: &RequestContext,
        temp_sender_id: ActorId,
        correlation_id: String,
        expires_at: Instant,
    ) -> Result<Arc<dyn MessageSender>, Box<dyn std::error::Error + Send + Sync>> {
        // Create mailbox (never used - tell() routes to ReplyWaiter before mailbox)
        let dummy_mailbox = Arc::new(
            Mailbox::new(MailboxConfig::default(), temp_sender_id.to_string())
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
            ActorVisibility::ActorVisibilityPublic,
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
                if tracing::enabled!(tracing::Level::TRACE) {
                    tracing::trace!(
                        actor_id = %actor_id_clone,
                        "Skipping watcher cleanup for actor handled by explicit stop_actor"
                    );
                }
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
                } else if tracing::enabled!(tracing::Level::DEBUG) {
                    tracing::debug!(
                        actor_id = %actor_id_clone,
                        "No stored exit reason (normal termination)"
                    );
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
                            actor_id: actor_id_clone.to_string(),
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
                            actor_id: actor_id_clone.to_string(),
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
                        actor_id: actor_id_clone.to_string(),
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
                        actor_id: actor_id_clone.to_string(),
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
                        actor_id: actor_id_clone.to_string(),
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
                    reason.parse().unwrap_or(ExitReason::Normal)
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
                "actor_id" => actor_id_clone.to_string(),
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

        let local_node_id = registry.local_node_id();
        if !actor_id.is_on_node(local_node_id) {
            return Err(format!(
                "Virtual actor '{}' targets node '{}' but activation only occurs on local node '{}'",
                actor_id,
                actor_id.node_id(),
                local_node_id
            )
            .into());
        }
        let actor_id = actor_id.clone();

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
        let actor_type = if !actor_id.actor_type().is_empty() {
            actor_id.actor_type().to_string()
        } else if let Some(metadata) = manager.get_metadata(&actor_id).await {
            metadata.actor_type().to_string()
        } else {
            return Err("Cannot determine actor_type for LRU eviction".into());
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
                // Fall back to type-level metadata.
                // VirtualActorManager.virtual_actor_types is keyed by actor_type (e.g. "inference_worker"),
                // which is the second segment of the canonical ActorId: {name}//{actor_type}::{ns}@{node}.
                // This is distinct from behavior_kind ("GenServer", "GenEvent") for observability; BehaviorRegistry is keyed by actor_type slugs (e.g. gen_server).
                let actor_type = actor_id.actor_type().to_string();

                manager.get_virtual_actor_type(&actor_type).await
                    .ok_or_else(|| format!(
                        "Virtual actor {} not found - cannot activate. Actor was suspended but metadata is missing from VirtualActorManager. Tried instance-level (actor_id) and type-level (actor_type: {}) lookups.",
                        actor_id, actor_type
                    ))?
            };

            // Extract actor_type for BehaviorRegistry lookup and WasmActorBehavior construction.
            // actor_type is the second segment of the canonical ActorId:
            //   "inference_worker_a//inference_worker::ns@node" → actor_type() == "inference_worker"
            // This is distinct from behavior_kind ("GenServer", …) which is the OTP model for logging.
            // actor_id.actor_type() is authoritative; metadata.actor_type() is the fallback.
            let actor_type = {
                let id_type = actor_id.actor_type().to_string();
                if !id_type.is_empty() {
                    id_type
                } else {
                    metadata.actor_type().to_string()
                }
            };
            let config = metadata.spec.config.clone();
            let tenant_id = metadata.spec.tenant_id.clone();
            let namespace = metadata.spec.namespace.clone();
            // Compute initial_state via wasm_init_payload (uses spec.args, injects actor_id).
            let _initial_state = crate::core::wasm_init_payload(&metadata.spec, &actor_id);
            let labels = metadata.spec.labels.clone();
            let tenant_id_clone = tenant_id.clone();

            // Create context for spawn_actor
            let ctx = RequestContext::new_without_auth(tenant_id_clone, namespace.clone());

            // CRITICAL: For virtual actors, we need to recreate the VirtualActorFacet
            // spawn_built_actor only detects virtual actors if they already have the facet attached
            // Since we're rebuilding a suspended actor, we need to recreate the facet
            // Get the facet from VirtualActorManager metadata to recreate it
            // For type-level registration, use facet_config; for instance-level, use stored facet
            let mut facets_to_attach: Vec<Box<dyn plexspaces_facet::Facet>> = vec![];

            if let Some(facet_config) = metadata.facet_config() {
                // Recreate all non-virtual facets from canonical stored config.
                // This must work for both type-level metadata and instance-level metadata,
                // because explicit stop removes live facet storage from FacetManager.
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
                    tracing::debug!(
                        actor_id = %actor_id,
                        "FacetRegistry not available - non-virtual facets will not be recreated"
                    );
                }
            }
            // Default (no stored facet_config): VirtualActorFacet is added below.

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
                // Look up the configured idle_timeout from type-level metadata so resurrection
                // honors whatever value was set in annotations or app-config.toml, not just the default.
                let idle_timeout_str = {
                    let configured: Option<String> =
                        if let Some(va_mgr) = self.service_locator.virtual_actor_manager().await {
                            va_mgr
                                .get_virtual_actor_type(&actor_type)
                                .await
                                .and_then(|meta| meta.facet_config())
                                .and_then(|fc: serde_json::Value| {
                                    fc.get("virtual_actor")
                                        .and_then(|v| v.get("idle_timeout"))
                                        .and_then(|v| v.as_str())
                                        .map(|s| s.to_string())
                                })
                        } else {
                            None
                        };
                    configured.unwrap_or_else(|| {
                        format_duration(Duration::from_secs(DEFAULT_IDLE_TIMEOUT_SECONDS))
                    })
                };
                // Use to_config_str with the enum variant so this never drifts from the canonical string.
                use plexspaces_common::ActivationStrategy;
                let eager_config = serde_json::json!({
                    "idle_timeout": idle_timeout_str,
                    "activation_strategy": plexspaces_journaling::to_config_str(&ActivationStrategy::ActivationStrategyEager)
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
            let rebuild_spec = {
                use crate::core::ActorSpawnSpec;
                use plexspaces_proto::common::v1::ActorIdentity;
                // identity.name must be the instance name ("session-1") so spawn_actor
                // builds the correct ActorId. spec.role carries the declaration name
                // (e.g. "worker") for BehaviorRegistry multi-spec dispatch via wasm_init_payload.
                ActorSpawnSpec {
                    identity: Some(ActorIdentity {
                        name: actor_id.name().to_string(),
                        actor_type: actor_type.clone(),
                    }),
                    role: metadata.spec.role.clone(),
                    namespace: namespace.clone(),
                    tenant_id: tenant_id.clone(),
                    visibility: metadata.spec.visibility,
                    behavior_kind: String::new(),
                    args: metadata.spec.args.clone(),
                    facets: vec![],
                    config: config.clone(),
                    labels: labels.clone(),
                    ..Default::default()
                }
            };
            let actor_ref = match self
                .spawn_actor(
                    &ctx,
                    &rebuild_spec,
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
                for (message, pending_ctx) in pending_messages {
                    if let Err(e) = actor_ref.tell(&pending_ctx, message).await {
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
        spawn_spec: &plexspaces_proto::actor::v1::ActorSpawnSpec,
        facets: Vec<Box<dyn plexspaces_facet::Facet>>,
    ) -> Result<Arc<dyn MessageSender>, Box<dyn std::error::Error + Send + Sync>> {
        use crate::core::{behavior_factory::BehaviorFactory, Actor as ActorTrait};

        let actor_type = spawn_spec
            .identity
            .as_ref()
            .map(|id| id.actor_type.as_str())
            .unwrap_or("");

        // If actor name is empty, assign a ULID so every actor has a stable unique identity.
        let actor_name_raw = spawn_spec
            .identity
            .as_ref()
            .map(|id| id.name.as_str())
            .unwrap_or("");
        let actor_name: std::borrow::Cow<str> = if actor_name_raw.is_empty() {
            std::borrow::Cow::Owned(ulid::Ulid::new().to_string())
        } else {
            std::borrow::Cow::Borrowed(actor_name_raw)
        };

        // Namespace: prefer spawn_spec.namespace, fall back to ctx.namespace()
        let namespace = if spawn_spec.namespace.is_empty() {
            ctx.namespace().to_string()
        } else {
            spawn_spec.namespace.clone()
        };

        let registry: Arc<ActorRegistry> = self
            .service_locator
            .actor_registry()
            .await
            .ok_or_else(|| "ActorRegistry not found in ServiceLocator".to_string())?;
        let local_node_id = registry.local_node_id();

        // Build ActorId directly from spec identity + local node_id
        let actor_id =
            crate::core::ActorId::new(actor_name.as_ref(), actor_type, &namespace, local_node_id)
                .map_err(|e| format!("Failed to build ActorId from spec: {}", e))?;

        // Derive init payload from spec (deterministic; no stale state)
        let initial_state = crate::core::wasm_init_payload(spawn_spec, &actor_id);

        // Create behavior via BehaviorRegistry using actor_type + init payload
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
                    "No BehaviorRegistry registered in ServiceLocator. Cannot create behavior for actor_type '{}'.",
                    actor_type
                ).into());
            }
        };

        // Build actor directly from spec fields — no ActorBuilder intermediate.
        // Actor::new creates a stub context; spawn_built_actor_impl replaces it with the real one.
        use plexspaces_mailbox::{mailbox_config_default, Mailbox};
        let mailbox_config = spawn_spec
            .config
            .as_ref()
            .map(|c| {
                let mut cfg = mailbox_config_default();
                if c.max_mailbox_size > 0 {
                    cfg.capacity = c.max_mailbox_size;
                }
                cfg
            })
            .unwrap_or_else(mailbox_config_default);
        let mailbox = Mailbox::new(mailbox_config, format!("mailbox_{}", actor_id))
            .await
            .map_err(|e| format!("Failed to create mailbox: {}", e))?;

        let tenant_id = if spawn_spec.tenant_id.is_empty() {
            ctx.tenant_id().to_string()
        } else {
            spawn_spec.tenant_id.clone()
        };

        let mut actor = crate::ActorInstance::new(
            actor_id.clone(),
            behavior,
            mailbox,
            tenant_id,
            namespace.clone(),
            Some(local_node_id.to_string()),
        );

        // Apply ActorConfig if present (wraps actor with a config-carrying context)
        if let Some(cfg) = spawn_spec.config.clone() {
            use crate::core::ActorContext;
            use crate::TestServiceLocatorStub;
            let sl: Arc<dyn crate::core::ServiceLocator> = Arc::new(TestServiceLocatorStub::new());
            let ctx_with_cfg = Arc::new(ActorContext::new(
                local_node_id.to_string(),
                ctx.tenant_id().to_string(),
                namespace.clone(),
                sl,
                Some(cfg),
            ));
            actor = actor.set_context(ctx_with_cfg);
        }

        // Capture facet flags before facets are consumed by the attachment loop.
        let has_virtual = facets.iter().any(|f| f.facet_type() == "virtual_actor");
        let has_durability = facets.iter().any(|f| f.facet_type() == "durability");
        let should_register =
            spawn_spec.register_in_object_registry || (has_virtual && has_durability);
        let enforce_unique = spawn_spec.enforce_unique_placement;

        // Attach runtime facets
        let num_facets = facets.len();
        for facet in facets {
            actor
                .attach_facet(facet)
                .await
                .map_err(|e| format!("Failed to attach facet: {}", e))?;
        }
        if num_facets > 0 {
            debug_log_attached_facets(&actor, &actor_id).await;
        }

        let labels: HashMap<String, String> = spawn_spec.labels.clone();
        let actor_ref = self
            .spawn_built_actor_impl(
                ctx,
                Arc::new(actor),
                actor_type.to_string(),
                initial_state,
                labels,
            )
            .await?;

        if should_register {
            self.maybe_register_actor_in_object_registry(
                ctx,
                &actor_id,
                actor_type,
                &namespace,
                enforce_unique,
            )
            .await?;
        }

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
        temp_sender_id: ActorId,
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
        actor: Arc<ActorInstance>,
        actor_type: String,
        _initial_state: Vec<u8>,
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
            behavior_type.actor_type_slug().into_owned()
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

        // Normalize actor ID to use the real local node_id.
        // Actors built without an explicit node_id get a placeholder ("local") from
        // ActorBuilder. Normalize to the actual node_id at spawn time so every
        // registered actor has a fully-qualified canonical identity.
        let local_node_id = registry.local_node_id();
        let raw_actor_id = actor.id().clone();
        let actor_id = if raw_actor_id.node_id() != local_node_id {
            // Normalize: replace any non-matching node_id with the real local node_id.
            // This covers "local" default and any other mismatch (which is an error in
            // production but acceptable for tests that build actors before knowing node_id).
            raw_actor_id
                .with_node_id(local_node_id)
                .map_err(|e| format!("Failed to normalize actor_id to local node: {}", e))?
        } else {
            raw_actor_id
        };

        let actor_namespace = ctx.namespace().to_string();
        let actor_tenant_id = ctx.tenant_id().to_string();

        // Extract actor config from context (if available)
        let actor_config = actor.context().config.clone();

        let self_ref = crate::core::ActorRef::new(actor_id.clone())
            .map_err(|e| format!("Failed to construct actor self_ref: {}", e))?;

        // Create ActorContext (actor_id is no longer stored in context)
        let actor_context = ActorContext::new(
            local_node_id.to_string(),
            actor_tenant_id.clone(),
            actor_namespace.clone(),
            self.service_locator.clone(),
            actor_config.clone(),
        )
        .with_self_ref(self_ref);

        // Update actor with full context
        actor = actor.set_context(Arc::new(actor_context));

        // Update actor's canonical ID to the normalized one.
        // The actor struct carries its own id (used by register_started → registry lookup).
        // Without this, register_started registers under the placeholder node_id ("unassigned"),
        // while the returned ActorRef carries the real node_id — causing lookup mismatches.
        actor = actor.with_normalized_id(actor_id.clone());

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
                actor_id: actor_id.to_string(),
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
                actor_id: actor_id.to_string(),
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
        if is_virtual {
            // Virtual actor handling
            let actor_facets = actor.facets();
            let facets_guard = actor_facets.read().await;
            let virtual_facet_arc = facets_guard
                .get_facet("virtual_actor")
                .ok_or("VirtualActorFacet not found in actor facets")?;

            // Extract VirtualActorFacet to check activation strategy
            let virtual_facet_guard = virtual_facet_arc.read().await;

            use plexspaces_journaling::VirtualActorFacet;
            let virtual_facet = virtual_facet_guard
                .as_any()
                .downcast_ref::<VirtualActorFacet>()
                .ok_or("Failed to downcast to VirtualActorFacet")?;

            // Check activation strategy
            let activation_strategy = virtual_facet.get_activation_strategy().await;
            let should_activate_eagerly = matches!(
                activation_strategy,
                plexspaces_journaling::ActivationStrategy::ActivationStrategyEager
                    | plexspaces_journaling::ActivationStrategy::ActivationStrategyPrewarm
            );
            let activation_strategy_clone = activation_strategy.clone();
            let activation_strategy_opt = Some(activation_strategy);

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
            let behavior_kind = actor_behavior_kind(&actor).await;

            // Register as virtual actor (only if instance not already registered)
            // Store metadata in VirtualActorManager (source of truth for virtual actors)
            // CRITICAL: Check if instance is registered (not just type), because is_virtual()
            // returns true for type-level registration even if instance is not registered
            let instance_metadata = manager.get_metadata(&actor_id).await;
            let needs_runtime_binding = instance_metadata
                .as_ref()
                .map(|metadata| metadata.facet.is_none())
                .unwrap_or(true);
            if needs_runtime_binding {
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
                let _activation_strategy = {
                    // Use the actor_type parameter directly (not parsed from actor_id) because
                    // actor_id may use "name@node" format which differs from the registered type name.
                    let type_strategy = manager
                        .get_virtual_actor_type(&actor_type)
                        .await
                        .map(|m| m.activation_strategy());
                    // Fall back to facet strategy, then default to lazy.
                    type_strategy.unwrap_or_else(|| {
                        activation_strategy_opt
                            .clone()
                            .unwrap_or(ActivationStrategy::ActivationStrategyLazy)
                    })
                };

                // Register with full metadata (ActorSpawnSpec) including labels and args.
                // This metadata persists across suspension and is used to rebuild actors.
                // Inherit args from type-level spec so wasm_init_payload always has them.
                // Fall back to instance_metadata (primed from named_virtual_actor_definitions)
                // when the actor_type is only in named_virtual_actor_definitions (not virtual_actor_types).
                let existing_spec = manager
                    .get_virtual_actor_type(&actor_type)
                    .await
                    .map(|m| m.spec)
                    .or_else(|| instance_metadata.as_ref().map(|m| m.spec.clone()));

                // Capture all non-virtual facets attached to this actor as proto Facets.
                // This ensures that facets like TimerFacet survive suspension/reactivation
                // even when the actor was not registered via a type-level spec.
                let actor_proto_facets: Vec<plexspaces_proto::common::v1::Facet> = {
                    let facets_arc = actor.facets().clone();
                    let facets_guard = facets_arc.read().await;
                    let all = facets_guard.get_all_facets();
                    let meta = facets_guard.get_metadata();
                    let mut result = Vec::new();
                    for facet_arc in all {
                        let f = facet_arc.read().await;
                        let ft = f.facet_type().to_string();
                        if ft == "virtual_actor" || ft == "durability" {
                            continue;
                        }
                        let config: std::collections::HashMap<String, String> = {
                            let json_cfg = if let Some(fm) = meta.get(ft.as_str()) {
                                fm.config.clone()
                            } else {
                                f.get_config()
                            };
                            if let serde_json::Value::Object(map) = json_cfg {
                                map.into_iter()
                                    .map(|(k, v)| {
                                        let s = match v {
                                            serde_json::Value::String(s) => s,
                                            other => other.to_string(),
                                        };
                                        (k, s)
                                    })
                                    .collect()
                            } else {
                                std::collections::HashMap::new()
                            }
                        };
                        result.push(plexspaces_proto::common::v1::Facet {
                            r#type: ft,
                            config,
                            priority: 0,
                            state: std::collections::HashMap::new(),
                            metadata: None,
                        });
                    }
                    result
                };

                // Prefer actor's own attached facets; fall back to type-level spec facets.
                let effective_facets = if !actor_proto_facets.is_empty() {
                    actor_proto_facets
                } else {
                    existing_spec
                        .as_ref()
                        .map(|s| s.facets.clone())
                        .unwrap_or_default()
                };

                use crate::core::ActorSpawnSpec;
                use plexspaces_proto::common::v1::ActorIdentity;
                // Use the spec name from the type/definition-level spec (e.g. "ephemeral")
                // so wasm_init_payload generates the role that BehaviorRegistry can match.
                // For type-level actors the definition name equals actor_type; for named virtual
                // actors (name != actor_type) the instance name (e.g. "cart-1") differs from the
                // role registered in the TOML ("ephemeral").
                let spec_name = existing_spec
                    .as_ref()
                    .and_then(|s| s.identity.as_ref())
                    .map(|id| id.name.clone())
                    .filter(|n| !n.is_empty())
                    .unwrap_or_else(|| actor_id.name().to_string());
                let spec = ActorSpawnSpec {
                    identity: Some(ActorIdentity {
                        name: spec_name,
                        actor_type: actor_type.clone(),
                    }),
                    role: existing_spec
                        .as_ref()
                        .map(|s| s.role.clone())
                        .unwrap_or_default(),
                    namespace: ctx.namespace().to_string(),
                    tenant_id: ctx.tenant_id().to_string(),
                    visibility: existing_spec
                        .as_ref()
                        .map(|s| s.visibility)
                        .unwrap_or_default(),
                    behavior_kind: behavior_kind.unwrap_or_default(),
                    args: existing_spec
                        .as_ref()
                        .map(|s| s.args.clone())
                        .unwrap_or_default(),
                    facets: effective_facets,
                    labels: labels.clone(),
                    config: actor_config.clone(),
                    ..Default::default()
                };
                manager
                    .register(actor_id.clone(), facet_box, spec)
                    .await
                    .map_err(|e| format!("Failed to register virtual actor: {}", e))?;
            } else {
                // Instance already registered — update facet binding and config while
                // preserving all other spec fields (args, facets, behavior_kind) from the
                // existing instance spec so nothing is lost on reactivation.
                use plexspaces_journaling::virtual_actor_facet_to_lifecycle_facet;
                let lifecycle_facet_update =
                    virtual_actor_facet_to_lifecycle_facet(virtual_facet_for_reg);
                let facet_box_update = Arc::new(tokio::sync::RwLock::new(lifecycle_facet_update));
                // Build a minimal spec whose empty fields will be filled in by the merge
                // logic inside register(), which falls back to the existing instance spec
                // then the type-level spec.
                use crate::core::ActorSpawnSpec;
                use plexspaces_proto::common::v1::ActorIdentity;
                let spec_update = ActorSpawnSpec {
                    identity: Some(ActorIdentity {
                        name: actor_id.name().to_string(),
                        actor_type: actor_type.clone(),
                    }),
                    role: instance_metadata
                        .as_ref()
                        .map(|m| m.spec.role.clone())
                        .unwrap_or_default(),
                    namespace: ctx.namespace().to_string(),
                    tenant_id: ctx.tenant_id().to_string(),
                    visibility: instance_metadata
                        .as_ref()
                        .map(|m| m.spec.visibility)
                        .unwrap_or_default(),
                    behavior_kind: String::new(), // merge picks up from existing instance/type spec
                    args: Default::default(),     // merge picks up from existing instance/type spec
                    facets: Default::default(),   // merge picks up from existing instance/type spec
                    labels: labels.clone(),
                    config: actor_config.clone(),
                    ..Default::default()
                };
                manager
                    .register(actor_id.clone(), facet_box_update, spec_update)
                    .await
                    .map_err(|e| format!("Failed to update virtual actor metadata: {}", e))?;
            }

            let spawn_vis_i32 = manager
                .get_metadata(&actor_id)
                .await
                .map(|m| m.spec.visibility)
                .unwrap_or(ActorVisibility::ActorVisibilityPublic as i32);
            let spawn_visibility = ActorVisibility::try_from(spawn_vis_i32)
                .unwrap_or(ActorVisibility::ActorVisibilityPublic);

            // Handle eager vs lazy activation
            if should_activate_eagerly {
                if tracing::enabled!(tracing::Level::DEBUG) {
                    tracing::debug!(actor_id = %actor_id, "Virtual actor with eager activation - starting immediately");
                }
                let (_actor_arc, actor_ref) = self
                    .start_registered_local_actor(
                        actor,
                        &actor_id,
                        &ctx,
                        &registry,
                        &facet_manager,
                        spawn_visibility,
                    )
                    .await?;

                // Mark as activated
                manager
                    .mark_activated(&actor_id)
                    .await
                    .map_err(|e| format!("Failed to mark actor as activated: {}", e))?;

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
                    for (message, pending_ctx) in pending_messages {
                        if let Err(e) = actor_ref.tell(&pending_ctx, message).await {
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
                let actor_ref = ActorRef::local(
                    actor_id.clone(),
                    ctx.tenant_id().to_string(),
                    ctx.namespace().to_string(),
                    actor.mailbox().clone(),
                    self.service_locator.clone(),
                    spawn_visibility,
                );

                // Lazy activation keeps only metadata in the registry and virtual manager.
                // The running sender is created on first local ask/tell or explicit activation.
                drop(actor); // Arc<Actor> dropped; metadata in VirtualActorManager is the rebuild source

                registry
                    .register_virtual_actor_index(&ctx, actor_id.clone(), actor_type.clone())
                    .await;

                return Ok(actor_ref);
            }
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

        let spawn_vis_i32 = manager
            .get_metadata(&actor_id)
            .await
            .map(|m| m.spec.visibility)
            .unwrap_or(ActorVisibility::ActorVisibilityPublic as i32);
        let spawn_vis = ActorVisibility::try_from(spawn_vis_i32)
            .unwrap_or(ActorVisibility::ActorVisibilityPublic);

        let (_actor_arc, actor_ref) = self
            .start_registered_local_actor(
                actor,
                &actor_id,
                &ctx,
                &registry,
                &facet_manager,
                spawn_vis,
            )
            .await?;

        Ok(actor_ref)
    }
}

impl ActorFactoryImpl {
    fn validate_actor_scope(
        ctx: &RequestContext,
        actor_id: &ActorId,
        actor_tenant_id: &str,
        actor_namespace: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let caller_tenant = ctx.tenant_id();
        if !caller_tenant.is_empty() && caller_tenant != actor_tenant_id {
            return Err(format!(
                "Tenant isolation violation: caller tenant '{}' cannot access actor '{}' owned by tenant '{}'",
                caller_tenant, actor_id, actor_tenant_id
            )
            .into());
        }

        let caller_namespace = ctx.namespace();
        if !caller_namespace.is_empty() && caller_namespace != actor_namespace {
            return Err(format!(
                "Namespace isolation violation: caller namespace '{}' cannot access actor '{}' in namespace '{}'",
                caller_namespace, actor_id, actor_namespace
            )
            .into());
        }

        Ok(())
    }

    async fn resolve_actor_stop_scope(
        registry: &Arc<ActorRegistry>,
        actor_id: &ActorId,
        ctx: &RequestContext,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        if let Some(sender) = registry.lookup_actor(actor_id).await {
            let actor_tenant_id = sender.tenant_id().unwrap_or_default().to_string();
            let actor_namespace = sender.namespace().unwrap_or_default().to_string();
            Self::validate_actor_scope(ctx, actor_id, &actor_tenant_id, &actor_namespace)?;
            return Ok(actor_namespace);
        }

        if let Some((actor_tenant_id, actor_namespace)) =
            registry.get_actor_metadata(actor_id).await
        {
            Self::validate_actor_scope(ctx, actor_id, &actor_tenant_id, &actor_namespace)?;
            return Ok(actor_namespace);
        }

        if !ctx.tenant_id().is_empty() || !ctx.namespace().is_empty() {
            return Err(format!(
                "Actor '{}' not found or metadata missing - cannot verify tenant isolation",
                actor_id
            )
            .into());
        }

        Ok(String::new())
    }

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

        let namespace = if let Some(sender) = registry
            .lookup_actor_in_scope(ctx.tenant_id(), ctx.namespace(), actor_id)
            .await
        {
            let actor_tenant_id = sender.tenant_id().unwrap_or_default().to_string();
            let actor_namespace = sender.namespace().unwrap_or_default().to_string();
            Self::validate_actor_scope(ctx, actor_id, &actor_tenant_id, &actor_namespace)?;
            actor_namespace
        } else {
            Self::resolve_actor_stop_scope(&registry, actor_id, ctx).await?
        };

        let is_local = actor_id.node_id() == local_node_id;
        if !is_local {
            return Err(format!("Actor not found or not local: {}", actor_id).into());
        }

        // OBSERVABILITY: Log actor stop attempt
        if tracing::enabled!(tracing::Level::TRACE) {
            tracing::trace!(
                actor_id = %actor_id,
                node_id = %local_node_id,
                namespace = %namespace,
                "Stopping actor"
            );
        }

        let is_virtual = if let Some(manager) = &virtual_actor_manager {
            manager.is_virtual(actor_id).await
        } else {
            false
        };

        let instance = registry.get_actor_instance(actor_id).await;
        let had_instance = instance.is_some();
        if had_instance {
            self.mark_actor_stopping(actor_id).await;
        }

        // CRITICAL: Get actor instance and stop it BEFORE unregistering
        // This ensures the message loop is stopped before we remove the instance
        // Production-grade: Use stop_from_arc() which properly stops the message loop
        if let Some(instance) = instance {
            if let Err(e) = instance.stop_actor().await {
                tracing::warn!(
                    actor_id = %actor_id,
                    error = %e,
                    "Failed to stop actor (continuing with unregister)"
                );
            }
        }

        // Emit Deactivating event before unregistration
        registry
            .publish_lifecycle_event(ActorLifecycleEvent {
                actor_id: actor_id.to_string(),
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

        registry
            .handle_actor_termination(actor_id, ExitReason::Shutdown)
            .await;

        registry
            .unregister_with_cleanup(actor_id)
            .await
            .map_err(|e| format!("Failed to unregister actor: {}", e))?;

        // Best-effort unregister from object registry (not all actors are registered there).
        self.maybe_unregister_actor_from_object_registry(
            &RequestContext::new_without_auth(
                ctx.tenant_id().to_string(),
                namespace.clone(),
            ),
            actor_id,
        )
        .await;

        if is_virtual {
            let manager = virtual_actor_manager
                .ok_or_else(|| "VirtualActorManager not found in ServiceLocator".to_string())?;
            if let Ok(facet_arc) = manager.get_facet(actor_id).await {
                let facet_guard = facet_arc.write().await;
                facet_guard.mark_deactivated().await;
            }
            manager.remove_from_active_tracking(actor_id).await;
        }
        if !had_instance {
            self.take_actor_stopping(actor_id).await;
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

        // Emit Deactivated event after unregistration
        registry
            .publish_lifecycle_event(ActorLifecycleEvent {
                actor_id: actor_id.to_string(),
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
        if tracing::enabled!(tracing::Level::TRACE) {
            tracing::trace!(
                actor_id = %actor_id,
                node_id = %local_node_id,
                namespace = %namespace,
                "Actor stopped successfully"
            );
        }

        Ok(())
    }

    // ========================================================================
    // Object registry integration helpers
    // ========================================================================

    /// Register the actor in the object registry if the service is available.
    ///
    /// Silently skips when no object registry is configured.  Returns `Err` only
    /// when `enforce_unique=true` and another live instance holds the same alias.
    async fn maybe_register_actor_in_object_registry(
        &self,
        ctx: &RequestContext,
        actor_id: &ActorId,
        actor_type: &str,
        namespace: &str,
        enforce_unique: bool,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let Some(obj_registry) = self.service_locator.get_object_registry().await else {
            return Ok(());
        };

        let grpc_address = self
            .service_locator
            .get_node_config()
            .await
            .map(|c| c.grpc_address)
            .unwrap_or_default();

        let reg_ctx =
            RequestContext::new_without_auth(ctx.tenant_id().to_string(), namespace.to_string());

        use crate::actor_context::RegisterResult;
        use crate::object_registry_helpers::{register_actor, RegisterActorParams};
        match register_actor(
            &obj_registry,
            &reg_ctx,
            RegisterActorParams {
                actor_id: actor_id.as_ref(),
                actor_type,
                actor_name: actor_id.name(),
                node_id: actor_id.node_id(),
                grpc_address: &grpc_address,
                enforce_unique,
            },
        )
        .await?
        {
            RegisterResult::AlreadyExists {
                object_id,
                grpc_address: existing_addr,
            } => Err(format!(
                "Placement conflict: actor '{}' already active on '{}' (object_id={})",
                actor_id, existing_addr, object_id
            )
            .into()),
            RegisterResult::Registered => Ok(()),
        }
    }

    /// Unregister the actor from the object registry if the service is available.
    ///
    /// Always best-effort — errors are logged and swallowed so they never
    /// interrupt the stop sequence.
    async fn maybe_unregister_actor_from_object_registry(
        &self,
        ctx: &RequestContext,
        actor_id: &ActorId,
    ) {
        let Some(obj_registry) = self.service_locator.get_object_registry().await else {
            return;
        };

        use crate::object_registry_helpers::unregister_actor;
        if let Err(e) = unregister_actor(&obj_registry, ctx, actor_id.as_ref()).await {
            let msg = e.to_string().to_lowercase();
            if msg.contains("not found") || msg.contains("does not exist") {
                tracing::debug!(actor_id = %actor_id, "Actor not in object registry, skip unregister");
            } else {
                tracing::warn!(
                    actor_id = %actor_id,
                    error = %e,
                    "Failed to unregister actor from object registry"
                );
            }
        }
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
        B: crate::core::Actor + Send + 'static,
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
        B: crate::core::Actor + Send + 'static,
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
        B: crate::core::Actor + Send + 'static,
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
        B: crate::core::Actor + Send + 'static,
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
        B: crate::core::Actor + Send + 'static,
    {
        use crate::ActorBuilder;

        let actor_id: ActorId = actor_id.into();

        // Get behavior type for logging/tracking
        let behavior_type = behavior.behavior_type();
        let actor_type = behavior_type.actor_type_slug();

        // Build actor with the provided behavior
        let actor = ActorBuilder::new(Box::new(behavior))
            .with_name(actor_id.name().to_string())
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
        if num_facets > 0 {
            debug_log_attached_facets(&actor, &actor_id).await;
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

// Implement Service trait so ActorFactoryImpl can be registered in ServiceLocator
impl Service for ActorFactoryImpl {
    fn service_name(&self) -> String {
        crate::core::ServiceName::ServiceNameActorFactoryImpl
            .as_str()
            .to_string()
    }
}
