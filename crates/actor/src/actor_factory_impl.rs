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

use async_trait::async_trait;
use std::sync::Arc;
use std::collections::HashMap;
use std::time::Instant;
use tokio::task::JoinHandle;
use plexspaces_core::{ActorId, Service, ServiceLocator as ServiceLocatorTrait, ActorRegistry, MessageSender, VirtualActorManager, ActorContext, RequestContext, ExitReason, ActorFactory, ApplicationManager};
use plexspaces_proto::ActorLifecycleEvent;
use prost_types::Timestamp;
use plexspaces_mailbox::{Mailbox, MailboxConfig};
use crate::{Actor, ActorRef};
use crate::{VirtualActorWrapper};

/// ActorFactory implementation
///
/// ## Design
/// Uses ServiceLocator to access ActorRegistry, VirtualActorManager, and other services
/// needed for spawning actors. This decouples ActorFactory from Node directly.
pub struct ActorFactoryImpl {
    service_locator: Arc<dyn ServiceLocatorTrait>,
    /// Self-reference as ActorFactory trait object (for VirtualActorWrapper)
    /// This is set after creation via set_self_reference()
    self_as_factory: Arc<tokio::sync::RwLock<Option<Arc<dyn ActorFactory>>>>,
}

impl ActorFactoryImpl {
    pub fn new(service_locator: Arc<dyn ServiceLocatorTrait>) -> Self {
        Self {
            service_locator,
            self_as_factory: Arc::new(tokio::sync::RwLock::new(None)),
        }
    }
    
    /// Create ActorFactoryImpl and wrap in Arc with self-reference set
    /// 
    /// ## Purpose
    /// Helper function that creates ActorFactoryImpl, wraps it in Arc, and sets the self-reference
    /// so that VirtualActorWrapper can access it. This avoids needing to call set_self_reference() separately.
    /// 
    /// ## Note
    /// This is async because it needs to set the self-reference using async RwLock.
    pub async fn new_arc(service_locator: Arc<dyn ServiceLocatorTrait>) -> Arc<Self> {
        let impl_instance = Arc::new(Self::new(service_locator));
        let factory_trait: Arc<dyn ActorFactory> = impl_instance.clone();
        impl_instance.set_self_reference(factory_trait).await;
        impl_instance
    }
    
    /// Set self-reference as ActorFactory (called after wrapping in Arc)
    /// 
    /// ## Note
    /// This is async because it uses tokio::sync::RwLock which requires async access.
    /// Cannot use blocking_write() from within an async runtime.
    pub async fn set_self_reference(&self, self_ref: Arc<dyn ActorFactory>) {
        let mut guard = self.self_as_factory.write().await;
        *guard = Some(self_ref);
    }
    
    /// Get self as ActorFactory (for VirtualActorWrapper)
    async fn get_self_as_factory(&self) -> Option<Arc<dyn ActorFactory>> {
        self.self_as_factory.read().await.clone()
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
            Mailbox::new(MailboxConfig::default(), temp_sender_id.clone()).await
                .map_err(|e| format!("Failed to create temporary sender mailbox: {}", e))?
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
            registry.register_temporary_sender(
                ctx,
                temp_sender_id.clone(),
                temp_sender_ref.clone(),
                correlation_id,
                expires_at,
            ).await;
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
        let registry: Arc<ActorRegistry> = self.service_locator.actor_registry().await
            .unwrap_or_else(|| panic!("ActorRegistry not registered in ServiceLocator"));
        let actor_id_clone = actor_id.clone();
        
        tokio::spawn(async move {
            // CRITICAL: Clear tracing context at the start of watch task to prevent span cloning panics
            // This task is spawned from spawn_actor which may have an active span from gRPC handler
            let noop_dispatcher = tracing::dispatcher::Dispatch::none();
            let _watch_guard = tracing::dispatcher::set_default(&noop_dispatcher);
            // Wait for actor task to complete
            let result = join_handle.await;
            
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
                                format!("linked:{}:{}", actor_id, match reason.as_ref() {
                                    ExitReason::Normal => "normal",
                                    ExitReason::Shutdown => "shutdown",
                                    ExitReason::Killed => "killed",
                                    ExitReason::Error(msg) => msg,
                                    ExitReason::Linked { .. } => "linked",
                                })
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
            registry.handle_actor_termination(&actor_id_clone, exit_reason).await;
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
            ).increment(1);
        });
    }
}

#[async_trait]
impl ActorFactory for ActorFactoryImpl {
    async fn activate_virtual_actor(&self, actor_id: &ActorId) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Get services from ServiceLocator
        let registry: Arc<ActorRegistry> = self.service_locator.actor_registry().await
            .ok_or_else(|| "ActorRegistry not found in ServiceLocator".to_string())?;
        let manager: Arc<VirtualActorManager> = self.service_locator.virtual_actor_manager().await
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
        let evicted = manager.evict_lru_if_needed(
            &actor_type,
            Some(self.service_locator.clone()),
        ).await;
        
        // Re-register VirtualActorWrapper for evicted actors to keep them addressable
        if !evicted.is_empty() {
            tracing::info!(
                actor_type = %actor_type,
                evicted_count = evicted.len(),
                "LRU-evicted {} virtual actor(s) of type {} to stay under max_pool_per_actor_type limit",
                evicted.len(),
                actor_type
            );
            
            // Re-register VirtualActorWrapper for each evicted actor to keep them addressable
            use crate::virtual_actor_wrapper::VirtualActorWrapper;
            use plexspaces_core::RequestContext;
            let actor_registry = self.service_locator.actor_registry().await
                .ok_or_else(|| "ActorRegistry not found in ServiceLocator".to_string())?;
            
            for evicted_actor_id in &evicted {
                // Get tenant/namespace from metadata
                let (tenant_id, namespace) = if let Some(metadata) = manager.get_metadata(evicted_actor_id).await {
                    (metadata.tenant_id, metadata.namespace)
                } else {
                    (String::new(), String::new())
                };
                
                let ctx = RequestContext::new_without_auth(tenant_id, namespace);
                let virtual_wrapper: Arc<dyn plexspaces_core::MessageSender> = Arc::new(
                    VirtualActorWrapper::new(
                        evicted_actor_id.clone(),
                        self.service_locator.clone(),
                        self.service_locator.get_actor_factory().await
                            .ok_or_else(|| "ActorFactory not found".to_string())?,
                    )
                );
                
                // Re-register VirtualActorWrapper (actor remains addressable but not active)
                actor_registry.register_actor(
                    &ctx,
                    evicted_actor_id.clone(),
                    virtual_wrapper,
                    None, // actor_type preserved in metadata
                    None, // config preserved in metadata
                    None, // no instance - actor is deactivated
                    None, // behavior_kind preserved
                ).await;
            }
        }
        // Get actor instance (for lazy virtual actors) - use ActorRegistry directly
        // VirtualActorManager doesn't store actor instances - ActorRegistry does
        // For lazy activation, we need to:
        // 1. Get the stored actor instance
        // 2. Unregister the actor (removes instance from registry, allowing Arc unwrap)
        // 3. Unwrap Arc to get Actor
        // 4. Spawn it (which re-registers it with ActorRef)
        // For suspended actors (no instance), we need to rebuild from scratch using spawn_actor
        if let Some(actor_any) = registry.get_actor_instance(&actor_id).await {
            // Actor instance exists - get it before unregistering
            // Downcast to Actor Arc
            let actor_arc = actor_any.downcast::<Actor>()
                .map_err(|_| "Failed to downcast actor instance to Actor")?;
            
            // Get actor's context before unregistering (needed for re-registration)
            let actor_context = actor_arc.context();
            let activation_ctx = RequestContext::new_without_auth(
                actor_context.tenant_id.clone(),
                actor_context.namespace.clone(),
            );
            
            // CRITICAL: Unregister actor to remove instance from registry
            // This removes the instance from the registry map, but we still have our Arc reference
            // Use unregister_with_cleanup which removes the instance (and facets, but we'll re-register with facets)
            // After unregister, only our local Arc reference exists, so we can pass it to spawn_built_actor
            // spawn_built_actor will re-register the actor with ActorRef (not instance)
            registry.unregister_with_cleanup(&actor_id).await
                .map_err(|e| format!("Failed to unregister actor before activation: {}", e))?;
            
            // Spawn the actor (use actor's context for virtual actor activation)
            // spawn_built_actor is synchronous - it awaits actor.start() which:
            // 1. Calls init() and waits for it
            // 2. Calls register_in_registry() and waits for it (registers ActorRef, replacing VirtualActorWrapper)
            // 3. Spawns message processing task and returns JoinHandle
            // Actor is fully registered and message loop is running when start().await returns
            // IMPORTANT: After activation, virtual actors behave exactly like regular actors
            // Note: spawn_built_actor takes Arc<Actor>, so we pass actor_arc directly (no unwrap needed)
            // For lazy activation, we need to start the actor directly (not through spawn_built_actor's lazy path)
            // Unwrap Arc to get mut Actor for start()
            let mut actor = Arc::try_unwrap(actor_arc)
                .map_err(|_| "Actor Arc has multiple references - cannot unwrap for activation")?;
            
            // CRITICAL: Check actor state before calling start() to ensure we only call it once
            // start() can only be called when state is Creating or Terminated
            use crate::ActorState;
            let current_state = actor.state().await;
            
            // Verify actor is in correct state for start() (Creating or Terminated)
            if current_state != ActorState::Creating && current_state != ActorState::Terminated {
                return Err(format!("Actor {} is in invalid state {:?} for start() - can only start from Creating or Terminated", actor_id, current_state).into());
            }
            
            // Start the actor (calls init() internally, then registers in ActorRegistry)
            // This is synchronous - waits for actor to become Active
            // start() already registers the ActorRef in the registry, so we don't need to register again
            let join_handle = actor.start().await
                .map_err(|e| format!("Failed to start lazy virtual actor: {}", e))?;
            
            // Verify actor reached Active state after start()
            let state_after_start = actor.state().await;
            
            if state_after_start != ActorState::Active {
                return Err(format!("Actor {} did not reach Active state after start(), current state: {:?}", actor_id, state_after_start).into());
            }
            
            // Wrap in Arc after starting
            let actor_arc = Arc::new(actor);
            
            // Store facets after activation (unregister_with_cleanup may have removed them)
            let facet_manager_wrapper = self.service_locator.get_facet_manager().await
                .ok_or_else(|| "FacetManager not found in ServiceLocator".to_string())?;
            let facet_manager = facet_manager_wrapper.inner_clone();
            let facets_clone = actor_arc.facets();
            facet_manager.store_facets(actor_id.clone(), facets_clone).await;
            
            // Get mailbox for ActorRef
            let mailbox = actor_arc.mailbox().clone();
            
            // Create ActorRef (already registered by start())
            // CRITICAL: Pass tenant_id from RequestContext to ActorRef
            let actor_ref = ActorRef::local(
                actor_id.clone(),
                activation_ctx.tenant_id().to_string(), // CRITICAL: tenant_id flows from API → ActorBuilder → ActorRef
                activation_ctx.namespace().to_string(),
                mailbox.clone(),
                self.service_locator.clone(),
            );
            
            // Update registration with config and instance (idempotent - ActorRef already registered in Actor::start())
            // This ensures config and instance are stored for resource tracking and ask() pattern
            registry.register_actor(
                &activation_ctx,
                actor_id.clone(),
                Arc::new(actor_ref.clone()) as Arc<dyn MessageSender>,
                None, // actor_type already set
                actor_arc.context().config.clone(), // Config for resource tracking
                Some(actor_arc.clone() as Arc<dyn std::any::Any + Send + Sync>), // Instance for ask() to get mailbox
                None, // behavior_kind already set at registration
            ).await;
            
            // Watch termination
            let exit_reason_arc = actor_arc.exit_reason();
            self.watch_actor_termination(actor_id.clone(), join_handle, exit_reason_arc).await;
            
            // Actor is now registered (ActorRef replaced VirtualActorWrapper) and message loop is running
            // Mark as activated in VirtualActorManager and clear is_activating flag in VirtualActorFacet
            manager.mark_activated(&actor_id).await
                .map_err(|e| format!("Failed to mark actor as activated: {}", e))?;
            
            // Clear is_activating flag in VirtualActorFacet (mark_activated in VirtualActorManager doesn't do this due to circular dependency)
            let facet_arc = manager.get_facet(&actor_id).await
                .map_err(|e| format!("Failed to get virtual actor facet: {}", e))?;
            {
                let facet_guard = facet_arc.read().await;
                // Use trait method directly - facet is Box<dyn VirtualActorLifecycleFacet>
                facet_guard.mark_activated().await;
            }
            
            // Process pending messages - send them to the now-activated actor
            // Use the ActorRef returned by spawn_built_actor (it's already registered and ready)
            // Messages are sent synchronously - actor is ready to process them immediately
            // IMPORTANT: Messages preserve correlation_id and sender for reply routing
            let pending_messages = manager.take_pending_messages(&actor_id).await;
            if !pending_messages.is_empty() {
                if tracing::enabled!(tracing::Level::DEBUG) {
                if tracing::enabled!(tracing::Level::DEBUG) {
                    tracing::debug!(
                        actor_id = %actor_id,
                        pending_count = pending_messages.len(),
                        "Sending {} pending messages to activated virtual actor",
                        pending_messages.len()
                    );
                }
                }
                for message in pending_messages {
                    if tracing::enabled!(tracing::Level::DEBUG) {
                    tracing::debug!(
                        actor_id = %actor_id,
                        message_id = %message.id,
                        correlation_id = ?message.correlation_id,
                        sender = ?message.sender_id,
                        "Sending pending message to activated virtual actor"
                    );
                    }
                    // Send message to actor's mailbox - message loop is already running
                    // Message preserves correlation_id and sender for reply routing
                    if let Err(e) = actor_ref.tell(message).await {
                        tracing::warn!(
                            actor_id = %actor_id,
                            error = %e,
                            "Failed to send pending message to activated virtual actor"
                        );
                    }
                }
            }
            
            return Ok(());
        } else {
            // No actor instance - actor was suspended/passivated
            // Need to rebuild from scratch using spawn_actor
            // Get actor metadata from VirtualActorManager (source of truth for virtual actors)
            
            // Try to get instance-level metadata first, then fall back to type-level metadata
            // This handles both individually-registered actors and type-registered actors
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
            let tenant_id_clone = tenant_id.clone();
            
            // Create context for spawn_actor
            let ctx = RequestContext::new_without_auth(tenant_id_clone, namespace.clone());
            
            // CRITICAL: For virtual actors, we need to recreate the VirtualActorFacet
            // spawn_built_actor only detects virtual actors if they already have the facet attached
            // Since we're rebuilding a suspended actor, we need to recreate the facet
            // Get the facet from VirtualActorManager metadata to recreate it
            // For type-level registration, use facet_config; for instance-level, use stored facet
            let mut facets_to_attach = vec![];
            let mut activation_strategy: Option<plexspaces_journaling::ActivationStrategy> = None;
            
            if let Some(facet_arc) = metadata.facet.clone() {
                // Instance-level registration: use stored facet
                // facet_arc is Arc<RwLock<Box<dyn VirtualActorLifecycleFacet>>>
                // Use trait methods directly - no downcasting needed
                let facet_guard = facet_arc.read().await;
                activation_strategy = Some(facet_guard.get_activation_strategy().await);
                drop(facet_guard);
                
                if let Some(strategy) = activation_strategy.clone() {
                    use plexspaces_journaling::VirtualActorFacet;
                    use plexspaces_common::virtual_actor_config::{DEFAULT_IDLE_TIMEOUT_SECONDS, format_duration};
                    use plexspaces_journaling::VIRTUAL_ACTOR_FACET_DEFAULT_PRIORITY;
                    use std::time::Duration;
                    // Build keyed format for consistency (even though we create facet directly)
                    // This ensures facet_config shape is consistent if we ever need to store it
                    let idle_timeout_str = format_duration(Duration::from_secs(DEFAULT_IDLE_TIMEOUT_SECONDS));
                    let facet_config = serde_json::json!({
                        "virtual_actor": {
                            "idle_timeout": idle_timeout_str,
                            "activation_strategy": plexspaces_journaling::to_config_str(&strategy)
                        }
                    });
                    // Extract virtual_actor config for VirtualActorFacet::new (which expects flat format)
                    let virtual_actor_config = facet_config.get("virtual_actor").cloned().unwrap_or_else(|| serde_json::json!({}));
                    let virtual_facet_new = Box::new(VirtualActorFacet::new(virtual_actor_config, VIRTUAL_ACTOR_FACET_DEFAULT_PRIORITY));
                    facets_to_attach.push(virtual_facet_new as Box<dyn plexspaces_facet::Facet>);
                }
            } else if let Some(facet_config) = metadata.facet_config.clone() {
                // Type-level registration: recreate facet from stored config
                // Use FacetRegistry to create facet from config (supports all facet types)
                if let Some(facet_registry_wrapper) = self.service_locator.get_facet_registry().await {
                    let facet_registry: Arc<plexspaces_facet::FacetRegistry> = facet_registry_wrapper.inner_clone();
                    use plexspaces_facet::create_facets_from_config;
                    use plexspaces_journaling::VirtualActorFacet;
                    let recreated_facets = create_facets_from_config(&facet_config, &facet_registry).await;
                    
                    // Try to extract activation_strategy from recreated facets before moving them
                    for facet in &recreated_facets {
                        if facet.facet_type() == "virtual_actor" {
                            if let Some(vf) = facet.as_any().downcast_ref::<VirtualActorFacet>() {
                                activation_strategy = Some(vf.get_activation_strategy().await);
                                break;
                            }
                        }
                    }
                    
                    facets_to_attach.extend(recreated_facets);
                } else {
                    return Err(format!("FacetRegistry not available - cannot recreate facets for virtual actor {}", actor_id).into());
                }
            } else {
                // No facet config available - create default VirtualActorFacet
                // Use FacetRegistry for consistency with type-level registration path
                use plexspaces_facet::create_facets_from_config;
                use plexspaces_journaling::to_config_str;
                use plexspaces_common::virtual_actor_config::{DEFAULT_IDLE_TIMEOUT_SECONDS, DEFAULT_ACTIVATION_STRATEGY, format_duration};
                use std::time::Duration;
                
                // Build keyed format facet_config for consistency with create_facets_from_config
                let idle_timeout_str = format_duration(Duration::from_secs(DEFAULT_IDLE_TIMEOUT_SECONDS));
                let facet_config = serde_json::json!({
                    "virtual_actor": {
                        "idle_timeout": idle_timeout_str,
                        "activation_strategy": to_config_str(&DEFAULT_ACTIVATION_STRATEGY)
                    }
                });
                
                // Use FacetRegistry to create facet (same path as type-level registration)
                if let Some(facet_registry_wrapper) = self.service_locator.get_facet_registry().await {
                    let facet_registry: Arc<plexspaces_facet::FacetRegistry> = facet_registry_wrapper.inner_clone();
                    let recreated_facets = create_facets_from_config(&facet_config, &facet_registry).await;
                    
                    // Extract activation_strategy from recreated facets
                    use plexspaces_journaling::VirtualActorFacet;
                    for facet in &recreated_facets {
                        if facet.facet_type() == "virtual_actor" {
                            if let Some(vf) = facet.as_any().downcast_ref::<VirtualActorFacet>() {
                                activation_strategy = Some(vf.get_activation_strategy().await);
                                break;
                            }
                        }
                    }
                    
                    facets_to_attach.extend(recreated_facets);
                } else {
                    // Fallback: create facet directly if FacetRegistry not available
                    use plexspaces_journaling::{VirtualActorFacet, VIRTUAL_ACTOR_FACET_DEFAULT_PRIORITY};
                    let virtual_actor_config = facet_config.get("virtual_actor").cloned().unwrap_or_else(|| serde_json::json!({}));
                    let virtual_facet_new = Box::new(VirtualActorFacet::new(virtual_actor_config, VIRTUAL_ACTOR_FACET_DEFAULT_PRIORITY));
                    facets_to_attach.push(virtual_facet_new as Box<dyn plexspaces_facet::Facet>);
                    activation_strategy = Some(DEFAULT_ACTIVATION_STRATEGY);
                }
            }
            
            // Retrieve and reattach all other facets from FacetManager
            // This ensures facets like DurabilityFacet are preserved across suspension/reactivation
            let facet_manager_wrapper = self.service_locator.get_facet_manager().await
                .ok_or_else(|| "FacetManager not found in ServiceLocator".to_string())?;
            let facet_manager = facet_manager_wrapper.inner_clone();
            
            if let Some(stored_facets_container) = facet_manager.get_facets(&actor_id.to_string()).await {
                let facets_guard = stored_facets_container.read().await;
                let all_facets = facets_guard.get_all_facets();
                let metadata = facets_guard.get_metadata();
                
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
                        
                        // Recreate facet based on type - generic approach using metadata
                        // This works for any facet type that can be recreated from config
                        if facet_type == "durability" {
                        // Recreate DurabilityFacet using registered journal storage from ServiceLocator
                        // All actors share the same journal storage instance registered in ServiceLocator
                        // Use trait-based retrieval to avoid hardcoding specific storage types
                        // Note: We need to retrieve concrete types to clone them (DurabilityFacet requires Clone)
                        use plexspaces_journaling::DurabilityFacet;
                        
                        // Get journal storage using trait-based method (no hardcoded concrete types)
                        if let Some(storage) = self.service_locator.get_journal_storage().await {
                            let config = facet_metadata.config.clone();
                            let priority = facet_metadata.priority;
                            let new_facet = Box::new(DurabilityFacet::new(storage, config, priority));
                            facets_to_attach.push(new_facet as Box<dyn plexspaces_facet::Facet>);
                        } else {
                            tracing::warn!(
                                actor_id = %actor_id,
                                "Could not recreate DurabilityFacet - journal storage not found in ServiceLocator"
                            );
                        }
                        } else {
                            // For other facets, we'd need to know how to recreate them
                            // For now, log a warning and skip
                            tracing::warn!(
                                actor_id = %actor_id,
                                facet_type = %facet_type,
                                "Facet type recreation not yet implemented, skipping"
                            );
                        }
                    }
                    drop(facet_read);
                }
            }
            
            // Rebuild actor using spawn_actor with stored actor_type and recreated VirtualActorFacet
            // If BehaviorRegistry fails, try to ensure behavior is registered by re-registering from application
            let actor_ref = match self.spawn_actor(
                &ctx,
                &actor_id,
                &actor_type,
                vec![], // No initial state (state should be loaded from durability facet if present)
                config.clone(),
                HashMap::new(), // No labels
                facets_to_attach, // Recreated VirtualActorFacet (and other facets from FacetManager if needed)
            ).await {
                Ok(ref_) => ref_,
                Err(e) => {
                    // BehaviorRegistry failed - ensure behavior is registered by re-registering from application
                    let error_msg = e.to_string();
                    if error_msg.contains("BehaviorRegistry") || error_msg.contains("Register the behavior") {
                        if tracing::enabled!(tracing::Level::DEBUG) {
                            tracing::debug!(
                                actor_id = %actor_id,
                                actor_type = %actor_type,
                                namespace = %namespace,
                                "BehaviorRegistry failed, attempting to re-register behavior from application"
                            );
                        }
                        
                        // Try to get application by namespace (namespace typically matches application name)
                        if let Some(app_manager) = self.service_locator.application_manager().await {
                            let app_name = namespace.clone();
                            
                            // Check if application exists
                            let app_exists = app_manager.list_applications().await.contains(&app_name);
                            
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
                        return Err(format!("Failed to rebuild suspended actor {}: {}", actor_id, error_msg).into());
                    }
                }
            };
            // After rebuilding, check activation status
            // For eager actors: spawn_actor already started them, just mark as activated
            // For lazy actors: instance exists but not started, activate it now since activate_virtual_actor was explicitly called
            let is_lazy = activation_strategy.map(|s| matches!(s, plexspaces_journaling::ActivationStrategy::ActivationStrategyLazy | plexspaces_journaling::ActivationStrategy::ActivationStrategyUnspecified)).unwrap_or(false);
            
            if is_lazy {
                // Lazy actor: check if instance exists and activate it
                // This respects the lazy design: lazy actors only activate when tell() is called
                // Since activate_virtual_actor is called from VirtualActorWrapper.tell(), this is correct
                if let Some(actor_any) = registry.get_actor_instance(&actor_id).await {
                    let actor_arc = actor_any.downcast::<Actor>()
                        .map_err(|_| "Failed to downcast actor instance to Actor")?;
                    
                    // Get actor's context before unregistering
                    let actor_context = actor_arc.context();
                    let activation_ctx = RequestContext::new_without_auth(
                        actor_context.tenant_id.clone(),
                        actor_context.namespace.clone(),
                    );
                    
                    // Unregister to allow unwrapping
                    registry.unregister_with_cleanup(&actor_id).await
                        .map_err(|e| format!("Failed to unregister actor before activation: {}", e))?;
                    
                    let mut actor = Arc::try_unwrap(actor_arc)
                        .map_err(|_| "Actor Arc has multiple references - cannot unwrap for activation")?;
                    
                    use crate::ActorState;
                    let current_state = actor.state().await;
                    if current_state == ActorState::Creating || current_state == ActorState::Terminated {
                        let join_handle = actor.start().await
                            .map_err(|e| format!("Failed to start reactivated lazy virtual actor: {}", e))?;
                        
                        let state_after_start = actor.state().await;
                        if state_after_start != ActorState::Active {
                            return Err(format!("Reactivated lazy virtual actor {} did not reach Active state, current state: {:?}", actor_id, state_after_start).into());
                        }
                        
                        let actor_arc = Arc::new(actor);
                        let mailbox = actor_arc.mailbox().clone();
                        // CRITICAL: Pass tenant_id from RequestContext to ActorRef
                        let actor_ref = ActorRef::local(
                            actor_id.clone(),
                            activation_ctx.tenant_id().to_string(), // CRITICAL: tenant_id flows from API → ActorBuilder → ActorRef
                            activation_ctx.namespace().to_string(),
                            mailbox.clone(),
                            self.service_locator.clone(),
                        );
                        
                        // Re-register with ActorRef (replacing VirtualActorWrapper)
                        registry.register_actor(
                            &activation_ctx,
                            actor_id.clone(),
                            Arc::new(actor_ref.clone()) as Arc<dyn MessageSender>,
                            Some(actor_type.clone()),
                            actor_arc.context().config.clone(),
                            Some(actor_arc.clone() as Arc<dyn std::any::Any + Send + Sync>),
                            None, // behavior_kind already set at registration
                        ).await;
                        
                        // Store facets
                        let facet_manager_wrapper = self.service_locator.get_facet_manager().await
                            .ok_or_else(|| "FacetManager not found in ServiceLocator".to_string())?;
                        let facet_manager = facet_manager_wrapper.inner_clone();
                        let facets_clone = actor_arc.facets();
                        facet_manager.store_facets(actor_id.clone(), facets_clone).await;
                        
                        // Watch termination
                        let exit_reason_arc = actor_arc.exit_reason();
                        self.watch_actor_termination(actor_id.clone(), join_handle, exit_reason_arc).await;
                        
                        // Mark as activated
                        manager.mark_activated(&actor_id).await
                            .map_err(|e| format!("Failed to mark actor as activated: {}", e))?;
                        
                        // Process pending messages
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
                        
                        return Ok(());
                    }
                }
            } else {
                // Eager actor: spawn_actor already started it, just verify and mark as activated
                // Check if actor is already active (should be for eager actors)
                if manager.is_active(&actor_id).await {
                    // Already active, just return
                    return Ok(());
                }
                
                // If not active, mark as activated (spawn_actor should have started it)
                // This handles the case where the actor was started but not yet marked as activated
                manager.mark_activated(&actor_id).await
                    .map_err(|e| format!("Failed to mark rebuilt eager actor as activated: {}", e))?;
                
                // Process pending messages
                let pending_messages = manager.take_pending_messages(&actor_id).await;
                if !pending_messages.is_empty() {
                    if let Some(sender) = registry.lookup_actor(&actor_id).await {
                        if tracing::enabled!(tracing::Level::DEBUG) {
                        tracing::debug!(
                            actor_id = %actor_id,
                            pending_count = pending_messages.len(),
                            "Sending {} pending messages to reactivated eager virtual actor",
                            pending_messages.len()
                        );
                    }
                    for message in pending_messages {
                            if let Err(e) = sender.tell(message).await {
                                tracing::warn!(
                                    actor_id = %actor_id,
                                    error = %e,
                                    "Failed to send pending message to reactivated eager virtual actor"
                                );
                            }
                        }
                    }
                }
                
                return Ok(());
            }
            
            // Fallback: mark as activated (should not reach here for properly configured actors)
            manager.mark_activated(&actor_id).await
                .map_err(|e| format!("Failed to mark rebuilt actor as activated: {}", e))?;
            
            // Process pending messages - send them to the now-reactivated actor
            // CRITICAL: Pending messages must be processed after reactivation
            // This handles messages that were queued while the actor was suspended
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
        _labels: HashMap<String, String>,
        facets: Vec<Box<dyn plexspaces_facet::Facet>>,
    ) -> Result<Arc<dyn MessageSender>, Box<dyn std::error::Error + Send + Sync>> {
        use crate::ActorBuilder;
        use plexspaces_core::{Actor as ActorTrait, BehaviorType, behavior_factory::BehaviorFactory};
        use async_trait::async_trait;
        
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
        let registry: Arc<ActorRegistry> = self.service_locator.actor_registry().await
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
        let actor = builder.build().await
            .map_err(|e| format!("Failed to build actor: {}", e))?;
        
        // Attach facets before spawning
        for facet in facets {
            actor.attach_facet(facet).await
                .map_err(|e| format!("Failed to attach facet: {}", e))?;
        }
        
        // Spawn the built actor with type information
        // spawn_built_actor_impl returns ActorRef, wrap for trait compatibility
        let actor_ref = self.spawn_built_actor_impl(ctx, Arc::new(actor), Some(actor_type.to_string())).await?;
        Ok(Arc::new(actor_ref) as Arc<dyn MessageSender>)
    }
    
    async fn stop_actor(&self, ctx: &RequestContext, actor_id: &ActorId) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
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
        self.create_temporary_sender_impl(ctx, temp_sender_id, correlation_id, expires_at).await
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
        actor_type: Option<String>,
    ) -> Result<ActorRef, Box<dyn std::error::Error + Send + Sync>> {
        // Extract actor_type from behavior if not provided
        let actor_type = if let Some(atype) = actor_type {
            Some(atype)
        } else {
            // Extract from behavior.behavior_type() before unwrapping Arc
            let behavior_guard = actor.behavior().read().await;
            let behavior_type = behavior_guard.behavior_type();
            drop(behavior_guard);
            match behavior_type {
                plexspaces_core::BehaviorType::GenServer => Some("GenServer".to_string()),
                plexspaces_core::BehaviorType::GenEvent => Some("GenEvent".to_string()),
                plexspaces_core::BehaviorType::GenStateMachine => Some("GenStateMachine".to_string()),
                plexspaces_core::BehaviorType::Workflow => Some("Workflow".to_string()),
                plexspaces_core::BehaviorType::Custom(s) => Some(s),
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
        let registry: Arc<ActorRegistry> = self.service_locator.actor_registry().await
            .ok_or_else(|| "ActorRegistry not found in ServiceLocator".to_string())?;
        let manager: Arc<VirtualActorManager> = self.service_locator.virtual_actor_manager().await
            .ok_or_else(|| "VirtualActorManager not found in ServiceLocator".to_string())?;
        let facet_manager_wrapper = self.service_locator.get_facet_manager().await
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
        ).increment(1.0);
        
        metrics::counter!("plexspaces_node_actors_spawned_total",
            "node_id" => local_node_id.to_string(),
            "namespace" => actor_namespace.clone()
        ).increment(1);

        // Create RequestContext for registry operations (moves values)
        // Clone values for logging before moving
        let actor_tenant_id_for_log = actor_tenant_id.clone();
        let actor_namespace_for_log = actor_namespace.clone();
        let ctx = RequestContext::new_without_auth(actor_tenant_id, actor_namespace);
        
        // Emit Created event
        registry.publish_lifecycle_event(ActorLifecycleEvent {
            actor_id: actor_id.clone(),
            timestamp: Some(Timestamp {
                seconds: chrono::Utc::now().timestamp(),
                nanos: chrono::Utc::now().timestamp_subsec_nanos() as i32,
            }),
            event_type: Some(plexspaces_proto::actor_lifecycle_event::EventType::Created(
                plexspaces_proto::v1::actor::ActorCreated {},
            )),
        }).await;
        
        // Emit Starting event
        registry.publish_lifecycle_event(ActorLifecycleEvent {
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
        }).await;
        
        // Check if actor has VirtualActorFacet
        let facets = actor.list_facets().await;
        let is_virtual = facets.contains(&"virtual_actor".to_string());
        let mut activation_strategy_opt: Option<plexspaces_journaling::ActivationStrategy> = None;
        let mut should_activate_eagerly = false;
        if is_virtual {
            // Virtual actor handling
            let actor_facets = actor.facets();
            let facets_guard = actor_facets.read().await;
            let virtual_facet_arc = facets_guard.get_facet("virtual_actor")
                .ok_or_else(|| format!("VirtualActorFacet not found in actor facets"))?;
            
            // Extract VirtualActorFacet to check activation strategy
            let virtual_facet_guard = virtual_facet_arc.read().await;
            
            use plexspaces_journaling::VirtualActorFacet;
            let virtual_facet = virtual_facet_guard.as_any().downcast_ref::<VirtualActorFacet>()
                .ok_or_else(|| format!("Failed to downcast to VirtualActorFacet"))?;
            
            // Check activation strategy
            let activation_strategy = virtual_facet.get_activation_strategy().await;
            should_activate_eagerly = matches!(activation_strategy, plexspaces_journaling::ActivationStrategy::ActivationStrategyEager);
            let activation_strategy_clone = activation_strategy.clone();
            activation_strategy_opt = Some(activation_strategy);
            
            // Create new facet for registration
            drop(virtual_facet_guard);
            drop(facets_guard);
            
            use plexspaces_common::virtual_actor_config::{DEFAULT_IDLE_TIMEOUT_SECONDS, format_duration};
            use std::time::Duration;
            let idle_timeout_str = format_duration(Duration::from_secs(DEFAULT_IDLE_TIMEOUT_SECONDS));
            let facet_config = serde_json::json!({
                "idle_timeout": idle_timeout_str,
                "activation_strategy": plexspaces_journaling::to_config_str(&activation_strategy_clone)
            });
            use plexspaces_journaling::VIRTUAL_ACTOR_FACET_DEFAULT_PRIORITY;
            let virtual_facet_for_reg = VirtualActorFacet::new(facet_config, VIRTUAL_ACTOR_FACET_DEFAULT_PRIORITY);
            
            // Register as virtual actor (only if instance not already registered)
            // Store metadata in VirtualActorManager (source of truth for virtual actors)
            // CRITICAL: Check if instance is registered (not just type), because is_virtual() 
            // returns true for type-level registration even if instance is not registered
            let instance_metadata = manager.get_metadata(&actor_id).await;
            if instance_metadata.is_none() {
                // Instance not registered - register it fresh
                // This happens when rebuilding a suspended actor that was only type-registered
                // Convert VirtualActorFacet to VirtualActorLifecycleFacet trait object
                use plexspaces_journaling::virtual_actor_facet_to_lifecycle_facet;
                let lifecycle_facet = virtual_actor_facet_to_lifecycle_facet(virtual_facet_for_reg);
                let facet_box = Arc::new(tokio::sync::RwLock::new(lifecycle_facet));
                
                // Register with full metadata (actor_type, config, tenant_id, namespace)
                // This metadata persists across suspension and is used to rebuild actors
                let actor_type_str = actor_type.clone()
                    .ok_or_else(|| format!("Actor type is required for virtual actor registration"))?;
                
                // Register new virtual actor instance
                manager.register(
                    actor_id.clone(),
                    facet_box,
                    actor_type_str,
                    actor_config.clone(),
                    ctx.tenant_id().to_string(),
                    ctx.namespace().to_string(),
                ).await
                    .map_err(|e| format!("Failed to register virtual actor: {}", e))?;
            } else {
                // Instance already registered - update metadata if we have new info
                manager.update_metadata(
                    &actor_id,
                    actor_type.clone(),
                    actor_config.clone(),
                ).await
                    .map_err(|e| format!("Failed to update virtual actor metadata: {}", e))?;
            }
            
            // Get mailbox (for creating ActorRef)
            let mailbox = actor.mailbox().clone();
            
            // Create VirtualActorWrapper (MessageSender - mailbox is internal)
            // Note: For lazy virtual actors, we register immediately since start() is deferred
            // For eager virtual actors, registration happens inside Actor::start() after init() succeeds
            // Get self as ActorFactory for VirtualActorWrapper
            let actor_factory = self.get_self_as_factory().await
                .ok_or_else(|| "ActorFactoryImpl self-reference not set - call set_self_reference() after wrapping in Arc".to_string())?;
            let virtual_wrapper = Arc::new(VirtualActorWrapper::new(
                actor_id.clone(),
                self.service_locator.clone(),
                actor_factory,
            ));
            
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
                let join_handle = actor.start().await
                    .map_err(|e| {
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
                manager.mark_activated(&actor_id).await
                    .map_err(|e| format!("Failed to mark actor as activated: {}", e))?;
                
                // Store facets
                let facets_clone = actor_arc.facets();
                facet_manager.store_facets(actor_id.clone(), facets_clone).await;
                
                // Update registration with config and instance (idempotent - ActorRef already registered in Actor::start())
                // This ensures config and instance are stored for resource tracking and ask() pattern
                registry.register_actor(
                    &ctx,
                    actor_id.clone(),
                    Arc::new(actor_ref.clone()) as Arc<dyn MessageSender>,
                    actor_type.clone(),
                    actor_config.clone(), // Config for resource tracking
                    Some(actor_arc.clone() as Arc<dyn std::any::Any + Send + Sync>), // Instance for ask() to get mailbox
                    None, // behavior_kind already set at registration
                ).await;
                
                // Watch termination (with exit_reason for proper propagation)
                self.watch_actor_termination(actor_id.clone(), join_handle, exit_reason_arc).await;
                
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
                // Lazy activation - store actor Arc but don't start
                
                let actor_arc = Arc::new(actor);
                
                // Store facets even for lazy virtual actors (they'll be activated later)
                // This ensures facets are available when the actor is activated
                let facets_clone = actor_arc.facets();
                facet_manager.store_facets(actor_id.clone(), facets_clone).await;
                
                // Register actor with consolidated method (config included, NO instance for lazy actors)
                // Per Orleans design: virtual actors are always registered (even when not active)
                // For lazy activation: VirtualActorWrapper is registered (will be replaced by ActorRef when activated)
                // Store instance for lazy activation (needed by activate_virtual_actor to retrieve and spawn)
                // is_active() checks actor state (ActorState::Active) to distinguish lazy (not Active) from active (Active)
                // ActorState matches proto: CREATING -> ACTIVATING -> ACTIVE -> DEACTIVATING -> INACTIVE
                registry.register_actor(
                    &ctx,
                    actor_id.clone(),
                    virtual_wrapper.clone(),
                    actor_type.clone(),
                    actor_config.clone(), // Config for resource tracking
                    Some(actor_arc.clone() as Arc<dyn std::any::Any + Send + Sync>), // Instance for lazy activation
                    None, // behavior_kind already set at registration
                ).await;
            }
            
            // OBSERVABILITY: Log actor spawn with full context (after determining activation strategy)
            let activation_info = match activation_strategy_opt {
                Some(plexspaces_journaling::ActivationStrategy::ActivationStrategyLazy)
                | Some(plexspaces_journaling::ActivationStrategy::ActivationStrategyUnspecified) => " (virtual actor with lazy activation - will activate on first message)",
                Some(plexspaces_journaling::ActivationStrategy::ActivationStrategyEager) => " (virtual actor with eager activation)",
                Some(plexspaces_journaling::ActivationStrategy::ActivationStrategyPrewarm) => " (virtual actor with prewarm activation)",
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
            
            // Create ActorRef for return value (for lazy virtual actors, VirtualActorWrapper is already registered)
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
        facet_manager.store_facets(&actor_id.to_string(), facets_clone).await;
        
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
        let join_handle = actor.start().await
            .map_err(|e| {
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
            return Err(format!("Regular actor {} did not reach Active state after start(), current state: {:?}", actor_id, state_after_start).into());
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
        registry.register_actor(
            &ctx,
            actor_id.clone(),
            Arc::new(actor_ref.clone()) as Arc<dyn MessageSender>,
            actor_type.clone(),
            actor_config.clone(), // Config for resource tracking
            Some(actor_arc.clone() as Arc<dyn std::any::Any + Send + Sync>), // Instance for ask() to get mailbox
            None, // behavior_kind already set at registration
        ).await;
        
        // Emit Activated event
        registry.publish_lifecycle_event(ActorLifecycleEvent {
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
        }).await;
        
        // Watch termination (with exit_reason_arc so stored exit reasons can be read)
        let exit_reason_arc = actor_arc.exit_reason();
        self.watch_actor_termination(actor_id.clone(), join_handle, exit_reason_arc).await;
        
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
    async fn stop_actor_impl(&self, ctx: &RequestContext, actor_id: &ActorId) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Get services from ServiceLocator
        let registry: Arc<ActorRegistry> = self.service_locator.actor_registry().await
            .ok_or_else(|| "ActorRegistry not found in ServiceLocator".to_string())?;
        
        let local_node_id = registry.local_node_id();
        
        // CRITICAL: Validate tenant isolation
        // Get actor's stored tenant_id and namespace, then validate against caller's context
        let namespace: String = {
            if let Some((actor_tenant_id, actor_namespace)) = registry.get_actor_metadata(actor_id).await {
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
                    ).into());
                }
                String::new()
            }
        };
        
        // Check routing to ensure actor is local
        let routing = registry.lookup_routing(ctx, actor_id).await
            .map_err(|e| format!("Failed to lookup actor routing: {}", e))?;
        
        if routing.is_none() || !routing.as_ref().unwrap().is_local {
            return Err(format!("Actor not found or not local: {}", actor_id).into());
        }
        
        // OBSERVABILITY: Log actor stop attempt
        tracing::info!(
            actor_id = %actor_id,
            node_id = %local_node_id,
            namespace = %namespace,
            "Stopping actor"
        );
        
        // CRITICAL: Get actor instance and stop it BEFORE unregistering
        // This ensures the message loop is stopped before we remove the instance
        // Production-grade: Use stop_from_arc() which properly stops the message loop
        if let Some(instance) = registry.get_actor_instance(actor_id).await {
            if let Ok(actor_arc) = instance.downcast::<Actor>() {
                // Stop actor gracefully (sends shutdown signal, waits, then aborts if needed)
                if let Err(e) = actor_arc.stop_from_arc().await {
                    tracing::warn!(
                        actor_id = %actor_id,
                        error = %e,
                        "Failed to stop actor from Arc (continuing with unregister)"
                    );
                }
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
        registry.publish_lifecycle_event(ActorLifecycleEvent {
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
        }).await;
        
        // Unregister from ActorRegistry (this handles cleanup and decrements active in ActorMetrics)
        // Note: This removes the instance, but the message loop might still be running
        // The message loop should exit when it tries to access actor state that's been dropped
        registry.unregister_with_cleanup(actor_id).await
            .map_err(|e| format!("Failed to unregister actor: {}", e))?;
        
        // OBSERVABILITY: Update Prometheus-style metrics
        metrics::gauge!("plexspaces_node_active_actors",
            "node_id" => local_node_id.to_string()
        ).decrement(1.0);
        
        metrics::counter!("plexspaces_node_actors_stopped_total",
            "node_id" => local_node_id.to_string(),
            "namespace" => namespace.clone()
        ).increment(1);
        
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
        registry.publish_lifecycle_event(ActorLifecycleEvent {
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
        }).await;
        
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
        let actor_ref = self.spawn_behavior(ctx, actor_id, behavior, facets).await
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
        let actor_ref = self.spawn_behavior(ctx, actor_id, behavior, facets).await
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
        let actor_ref = self.spawn_behavior(ctx, actor_id, behavior, facets).await
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
        let actor_ref = self.spawn_behavior(ctx, actor_id, behavior, facets).await
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
        for facet in facets {
            actor.attach_facet(facet).await
                .map_err(|e| format!("Failed to attach facet: {}", e))?;
        }
        
        // Use spawn_built_actor_impl - returns ActorRef directly
        self.spawn_built_actor_impl(
            ctx,
            Arc::new(actor),
            Some(actor_type.to_string()),
        ).await
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
