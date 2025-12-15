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
use tokio::task::JoinHandle;
use plexspaces_core::{ActorId, Service, ServiceLocator, ActorRegistry, MessageSender, VirtualActorManager, FacetManager, ActorContext};
use plexspaces_proto::ActorLifecycleEvent;
use prost_types::Timestamp;
use crate::{ActorFactory, Actor, ActorRef};
use crate::{RegularActorWrapper, VirtualActorWrapper};

/// ActorFactory implementation
///
/// ## Design
/// Uses ServiceLocator to access ActorRegistry, VirtualActorManager, and other services
/// needed for spawning actors. This decouples ActorFactory from Node directly.
pub struct ActorFactoryImpl {
    service_locator: Arc<ServiceLocator>,
}

impl ActorFactoryImpl {
    pub fn new(service_locator: Arc<ServiceLocator>) -> Self {
        Self { service_locator }
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
    
    /// Setup facets (TimerFacet, ReminderFacet, etc.) after actor spawn
    ///
    /// ## Note
    /// This requires journaling crate for TimerFacet, which actor crate doesn't depend on.
    /// So we'll skip facet setup here - it should be done by Node or a separate facet setup service.
    /// TODO: Create a FacetSetupService that can be called from Node or ActorFactory
    async fn setup_facets(
        &self,
        _actor_id: &ActorId,
        _actor_ref: &ActorRef,
        _node_id: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Facet setup requires journaling crate (TimerFacet, ReminderFacet)
        // ActorFactoryImpl doesn't depend on journaling crate to avoid circular dependencies
        // Facet setup should be done by Node or a separate FacetSetupService
        // For now, skip facet setup here
        Ok(())
    }
    
    /// Watch actor termination and handle cleanup
    async fn watch_actor_termination(
        &self,
        actor_id: ActorId,
        join_handle: JoinHandle<()>,
    ) {
        let registry: Arc<ActorRegistry> = self.service_locator.get_service().await
            .unwrap_or_else(|| panic!("ActorRegistry not registered in ServiceLocator"));
        let actor_id_clone = actor_id.clone();
        
        tokio::spawn(async move {
            // Wait for actor task to complete
            let result = join_handle.await;
            
            // Determine termination reason and create lifecycle event
            let (reason, lifecycle_event) = match result {
                Ok(_) => {
                    // Graceful shutdown
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
            
            // Notify all monitors (Erlang-style DOWN messages)
            registry.notify_actor_down(&actor_id_clone, &reason).await;
            
            // Unregister actor
            registry.unregister_with_cleanup(&actor_id_clone).await;
        });
    }
}

#[async_trait]
impl ActorFactory for ActorFactoryImpl {
    async fn activate_virtual_actor(&self, actor_id: &ActorId) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Get services from ServiceLocator
        let registry: Arc<ActorRegistry> = self.service_locator.get_service().await
            .ok_or_else(|| "ActorRegistry not found in ServiceLocator".to_string())?;
        let manager: Arc<VirtualActorManager> = self.service_locator.get_service().await
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
            return Ok(()); // Already active
        }
        
        // Get actor instance (for lazy virtual actors)
        if let Some(actor_any) = manager.get_actor_instance(&actor_id).await {
            // Actor instance exists - spawn it
            // Downcast to Actor
            let actor_arc = actor_any.downcast::<Actor>()
                .map_err(|_| "Failed to downcast actor instance to Actor")?;
            
            // Remove from actor_instances before spawning (to allow unwrapping)
            manager.remove_actor_instance(&actor_id).await;
            
            // Spawn the actor
            let _actor_ref = self.spawn_built_actor(actor_arc, None, None, None).await?;
            
            // Process pending messages
            let pending_messages = manager.take_pending_messages(&actor_id).await;
            // TODO: Send pending messages to actor
            // For now, messages are queued and will be processed when actor starts
            
            return Ok(());
        }
        
        Err(format!("Virtual actor {} not found - cannot activate", actor_id).into())
    }
    
    async fn spawn_actor(
        &self,
        actor_id: &ActorId,
        actor_type: &str,
        initial_state: Vec<u8>,
        config: Option<plexspaces_proto::v1::actor::ActorConfig>,
        labels: HashMap<String, String>,
    ) -> Result<Arc<dyn MessageSender>, Box<dyn std::error::Error + Send + Sync>> {
        use crate::ActorBuilder;
        use plexspaces_core::{Actor as ActorTrait, BehaviorType};
        use async_trait::async_trait;
        
        // Create a simple default behavior (same logic as create_spawn_callback)
        struct SimpleBehavior {
            actor_type: String,
        }
        #[async_trait]
        impl ActorTrait for SimpleBehavior {
            async fn handle_message(
                &mut self,
                _ctx: &plexspaces_core::ActorContext,
                _msg: plexspaces_mailbox::Message,
            ) -> Result<(), plexspaces_core::BehaviorError> {
                Ok(())
            }
            fn behavior_type(&self) -> BehaviorType {
                BehaviorType::Custom(self.actor_type.clone())
            }
        }
        let behavior: Box<dyn ActorTrait> = Box::new(SimpleBehavior { 
            actor_type: actor_type.to_string() 
        });

        // Create Actor using ActorBuilder
        let mut builder = ActorBuilder::new(behavior)
            .with_id(actor_id.clone());
        
        // Apply config if provided
        if let Some(cfg) = config {
            builder = builder.with_config(Some(cfg));
        }
        
        // Extract namespace from labels if present
        if let Some(namespace) = labels.get("namespace") {
            builder = builder.with_namespace(namespace.clone());
        }

        // Build actor
        let actor = builder.build().await;
        
        // Extract tenant_id from labels or use "default"
        let tenant_id = labels.get("tenant_id")
            .cloned()
            .unwrap_or_else(|| "default".to_string());
        
        // Spawn the built actor with type information
        self.spawn_built_actor(Arc::new(actor), Some(actor_type.to_string()), Some(tenant_id), Some("default".to_string())).await
    }
    
    async fn spawn_built_actor(
        &self,
        actor: Arc<Actor>,
        actor_type: Option<String>,
        tenant_id: Option<String>,
        namespace: Option<String>,
    ) -> Result<Arc<dyn MessageSender>, Box<dyn std::error::Error + Send + Sync>> {
        // Unwrap the Arc to get the Actor
        let mut actor = Arc::try_unwrap(actor)
            .map_err(|_| "Actor Arc has multiple references - cannot unwrap")?;
        
        // Get services from ServiceLocator
        let registry: Arc<ActorRegistry> = self.service_locator.get_service().await
            .ok_or_else(|| "ActorRegistry not found in ServiceLocator".to_string())?;
        let manager: Arc<VirtualActorManager> = self.service_locator.get_service().await
            .ok_or_else(|| "VirtualActorManager not found in ServiceLocator".to_string())?;
        let facet_manager: Arc<FacetManager> = self.service_locator.get_service().await
            .ok_or_else(|| "FacetManager not found in ServiceLocator".to_string())?;
        
        // Normalize actor ID
        let local_node_id = registry.local_node_id();
        let mut actor_id = actor.id().clone();
        actor_id = self.normalize_actor_id(&actor_id, local_node_id);
        let actor_namespace = namespace.as_ref().map(|s| s.clone()).unwrap_or_else(|| actor.context().namespace.clone());
        
        // Extract actor config from context (if available)
        let actor_config = actor.context().config.clone();
        
        // Create ActorContext (actor_id is no longer stored in context)
        let actor_context = ActorContext::new(
            local_node_id.to_string(),
            actor_namespace.clone(),
            self.service_locator.clone(),
            actor_config.clone(),
        );
        
        // Update actor with full context
        actor = actor.set_context(Arc::new(actor_context));
        
        // Update metrics
        metrics::gauge!("plexspaces_node_active_actors",
            "node_id" => local_node_id.to_string()
        ).increment(1.0);
        
        metrics::counter!("plexspaces_node_actors_spawned_total",
            "node_id" => local_node_id.to_string(),
            "namespace" => actor_namespace.clone()
        ).increment(1);
        
        tracing::info!(actor_id = %actor_id, node_id = %local_node_id, namespace = %actor_namespace, "Actor spawned");
        
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
        
        if is_virtual {
            // Virtual actor handling
            let actor_facets = actor.facets();
            let facets_guard = actor_facets.read().await;
            let virtual_facet_arc = facets_guard.get_facet("virtual_actor")
                .ok_or_else(|| format!("VirtualActorFacet not found in actor facets"))?;
            
            // Extract VirtualActorFacet to check activation strategy
            let virtual_facet_guard = virtual_facet_arc.read().await;
            use std::any::Any;
            use plexspaces_journaling::VirtualActorFacet;
            let virtual_facet = virtual_facet_guard.as_any().downcast_ref::<VirtualActorFacet>()
                .ok_or_else(|| format!("Failed to downcast to VirtualActorFacet"))?;
            
            // Check activation strategy
            let activation_strategy = virtual_facet.get_activation_strategy().await;
            let should_activate_eagerly = matches!(activation_strategy, plexspaces_journaling::ActivationStrategy::Eager);
            
            // Create new facet for registration
            drop(virtual_facet_guard);
            drop(facets_guard);
            
            let facet_config = serde_json::json!({
                "idle_timeout": "5m",
                "activation_strategy": match activation_strategy {
                    plexspaces_journaling::ActivationStrategy::Eager => "eager",
                    plexspaces_journaling::ActivationStrategy::Prewarm => "prewarm",
                    _ => "lazy"
                }
            });
            let virtual_facet_for_reg = VirtualActorFacet::new(facet_config);
            
            // Register as virtual actor
            let facet_box = Arc::new(tokio::sync::RwLock::new(
                Box::new(virtual_facet_for_reg) as Box<dyn std::any::Any + Send + Sync>
            ));
            manager.register(actor_id.clone(), facet_box).await
                .map_err(|e| format!("Failed to register virtual actor: {}", e))?;
            
            // Get mailbox (for creating ActorRef)
            let mailbox = actor.mailbox().clone();
            
            // Register VirtualActorWrapper (MessageSender - mailbox is internal)
            let virtual_wrapper = Arc::new(VirtualActorWrapper::new(
                actor_id.clone(),
                self.service_locator.clone(),
            ));
            let effective_namespace = namespace.as_ref().map(|s| s.clone()).unwrap_or_else(|| actor_namespace.clone());
            registry.register_actor(actor_id.clone(), virtual_wrapper, actor_type.clone(), tenant_id.clone(), Some(effective_namespace)).await;
            
            // Create ActorRef
            let actor_ref = ActorRef::local(
                actor_id.clone(),
                mailbox.clone(),
                self.service_locator.clone(),
            );
            
            // Register actor with config
            registry.register_actor_with_config(actor_id.clone(), actor_config.clone()).await
                .map_err(|e| format!("Failed to register actor config: {}", e))?;
            
            // Handle eager vs lazy activation
            if should_activate_eagerly {
                tracing::debug!(actor_id = %actor_id, "Virtual actor with eager activation - starting immediately");
                
                // Start the actor
                let join_handle = actor.start().await
                    .map_err(|e| format!("Failed to start actor: {}", e))?;
                
                // Wrap in Arc after starting
                let actor_arc = Arc::new(actor);
                
                // Mark as activated
                manager.mark_activated(&actor_id).await
                    .map_err(|e| format!("Failed to mark actor as activated: {}", e))?;
                
                // Store actor instance
                {
                    let mut actor_instances = registry.actor_instances().write().await;
                    actor_instances.insert(actor_id.clone(), actor_arc.clone() as Arc<dyn std::any::Any + Send + Sync>);
                }
                
                // Store facets
                let facets_clone = actor_arc.facets();
                facet_manager.store_facets(actor_id.clone(), facets_clone).await;
                
                // Watch termination
                self.watch_actor_termination(actor_id.clone(), join_handle).await;
                
                tracing::debug!(actor_id = %actor_id, "Virtual actor started with eager activation");
            } else {
                // Lazy activation - store actor Arc but don't start
                tracing::debug!(actor_id = %actor_id, "Virtual actor with lazy activation - will activate on first message");
                
                let actor_arc = Arc::new(actor);
                {
                    let mut actor_instances = registry.actor_instances().write().await;
                    actor_instances.insert(actor_id.clone(), actor_arc as Arc<dyn std::any::Any + Send + Sync>);
                }
                // Note: Facets NOT stored here - will be stored after activation
            }
            
            return Ok(Arc::new(RegularActorWrapper::new(
                actor_id.clone(),
                mailbox.clone(),
                self.service_locator.clone(),
            )));
        }
        
        // Normal actor - start immediately
        // Store facets
        let facets_clone = actor.facets().clone();
        facet_manager.store_facets(actor_id.clone(), facets_clone).await;
        
        // Get mailbox (for creating ActorRef)
        let mailbox = actor.mailbox().clone();
        
        // Register RegularActorWrapper (MessageSender - mailbox is internal)
        let regular_wrapper = Arc::new(RegularActorWrapper::new(
            actor_id.clone(),
            mailbox.clone(),
            self.service_locator.clone(),
        ));
        let effective_namespace = namespace.as_ref().map(|s| s.clone()).unwrap_or_else(|| actor_namespace.clone());
        registry.register_actor(actor_id.clone(), regular_wrapper, actor_type, tenant_id, Some(effective_namespace)).await;
        
        // Start actor
        let join_handle = actor.start().await
            .map_err(|e| format!("Failed to start actor: {}", e))?;
        
        // Store actor in Arc after starting
        let actor_arc = Arc::new(actor);
        
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
        
        // Create ActorRef
        let actor_ref = ActorRef::local(
            actor_id.clone(),
            mailbox.clone(),
            self.service_locator.clone(),
        );
        
        // Setup facets (skipped - requires journaling crate)
        // TODO: Call FacetSetupService if available
        
        // Register actor with config
        registry.register_actor_with_config(actor_id.clone(), actor_config.clone()).await;
        
        // Store actor instance
        {
            let mut actor_instances = registry.actor_instances().write().await;
            actor_instances.insert(actor_id.clone(), actor_arc.clone() as Arc<dyn std::any::Any + Send + Sync>);
        }
        
        // Watch termination
        self.watch_actor_termination(actor_id.clone(), join_handle).await;
        
        Ok(Arc::new(RegularActorWrapper::new(
            actor_id.clone(),
            mailbox.clone(),
            self.service_locator.clone(),
        )))
    }
}

// Implement Service trait so ActorFactoryImpl can be registered in ServiceLocator
impl Service for ActorFactoryImpl {}
