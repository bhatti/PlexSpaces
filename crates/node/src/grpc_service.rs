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

//! gRPC service implementation for remote actor communication

use plexspaces_mailbox::Message;
use plexspaces_proto::{
    ActorServiceClient,
    v1::actor::{
        ActivateActorRequest, ActivateActorResponse, ActorDownNotification,
        CheckActorExistsRequest, CheckActorExistsResponse, CreateActorRequest,
        CreateActorResponse, DeactivateActorRequest, DeleteActorRequest, GetActorRequest,
        GetActorResponse, GetOrActivateActorRequest, GetOrActivateActorResponse,
        InvokeActorRequest, InvokeActorResponse,
        ListActorsRequest,
        ListActorsResponse, Message as ProtoMessage, MigrateActorRequest, MigrateActorResponse,
        MonitorActorRequest, MonitorActorResponse, SendMessageRequest, SendMessageResponse,
        SetActorStateRequest, SetActorStateResponse, SpawnActorRequest,
        SpawnActorResponse, StreamMessageRequest, StreamMessageResponse,
        LinkActorRequest, LinkActorResponse, UnlinkActorRequest, UnlinkActorResponse,
    },
    v1::common::Empty,
};
use std::sync::Arc;
use tonic::{Request, Response, Status};

use crate::{ActorLocation, Node};
use plexspaces_core::{ActorRegistry, ServiceLocator};
use plexspaces_actor::{ActorFactory, actor_factory_impl::ActorFactoryImpl};

/// ActorService gRPC implementation
pub struct ActorServiceImpl {
    node: Arc<Node>,
    /// Health reporter for shutdown checks
    health_reporter: Option<Arc<crate::health_service::PlexSpacesHealthReporter>>,
}

impl ActorServiceImpl {
    /// Create new ActorService implementation
    pub fn new(node: Arc<Node>) -> Self {
        Self {
            node,
            health_reporter: None,
        }
    }
    
    /// Create ActorService with health reporter for shutdown checks
    pub fn with_health_reporter(
        node: Arc<Node>,
        health_reporter: Arc<crate::health_service::PlexSpacesHealthReporter>,
    ) -> Self {
        Self {
            node,
            health_reporter: Some(health_reporter),
        }
    }
    
    /// Check if service should accept requests (not shutting down)
    async fn check_accepting_requests(&self) -> Result<(), Status> {
        if let Some(ref reporter) = self.health_reporter {
            if reporter.is_shutting_down().await {
                return Err(Status::unavailable("Service is shutting down and not accepting new requests"));
            }
        }
        Ok(())
    }
}

#[tonic::async_trait]
impl plexspaces_proto::v1::actor::actor_service_server::ActorService for ActorServiceImpl {
    async fn create_actor(
        &self,
        request: Request<CreateActorRequest>,
    ) -> Result<Response<CreateActorResponse>, Status> {
        // Check if service is accepting requests
        self.check_accepting_requests().await?;
        
        let req = request.into_inner();
        
        // Validate actor_type
        if req.actor_type.is_empty() {
            return Err(Status::invalid_argument("Missing actor_type"));
        }
        
        // Use spawn_actor logic but for local node
        let actor_id = format!("{}@{}", ulid::Ulid::new(), self.node.id().as_str());
        
        // Create actor based on actor_type
        // Future: Use actor registry to look up actor factory by type
        use plexspaces_actor::Actor;
        use plexspaces_core::Actor as ActorTrait;
        use plexspaces_mailbox::{mailbox_config_default, Mailbox};
        
        // Create a simple test behavior that logs messages
        struct SimpleBehavior;
        
        #[async_trait::async_trait]
        impl ActorTrait for SimpleBehavior {
            async fn handle_message(
                &mut self,
                _ctx: &plexspaces_core::ActorContext,
                _msg: plexspaces_mailbox::Message,
            ) -> Result<(), plexspaces_core::BehaviorError> {
                Ok(())
            }
            
            fn behavior_type(&self) -> plexspaces_core::BehaviorType {
                plexspaces_core::BehaviorType::GenServer
            }
        }
        
        let mailbox = Mailbox::new(mailbox_config_default(), format!("{}:mailbox", actor_id))
            .await
            .map_err(|e| Status::internal(format!("Failed to create mailbox: {}", e)))?;
        let behavior = Box::new(SimpleBehavior);
        
        // Extract namespace from labels if present, otherwise use "default"
        let namespace = req.labels.get("namespace")
            .cloned()
            .unwrap_or_else(|| "default".to_string());
        
        let actor = Actor::new(
            actor_id.clone(),
            behavior,
            mailbox,
            namespace,
            Some(self.node.id().as_str().to_string()),
        );
        
        // Spawn actor using ActorFactory
        let actor_factory: Arc<ActorFactoryImpl> = self.node.service_locator().get_service().await
            .ok_or_else(|| Status::internal("ActorFactory not found in ServiceLocator"))?;
        
        let _message_sender = actor_factory.spawn_built_actor(Arc::new(actor), None, None, None).await
            .map_err(|e| Status::internal(format!("Failed to spawn actor: {}", e)))?;
        
        // Record metrics
        metrics::counter!("plexspaces_node_actors_created_via_grpc_total",
            "node_id" => self.node.id().as_str().to_string(),
            "actor_type" => req.actor_type.clone()
        ).increment(1);
        tracing::info!(actor_id = %actor_id, actor_type = %req.actor_type, node_id = %self.node.id().as_str(), "Actor created via gRPC");
        
        // Build proto Actor message for response
        use plexspaces_proto::v1::actor::{Actor as ProtoActor, ActorState};
        
        // Convert labels HashMap to Metadata proto
        use plexspaces_proto::common::v1::Metadata;
        let metadata = if req.labels.is_empty() {
            None
        } else {
            Some(Metadata {
                create_time: None,
                update_time: None,
                created_by: String::new(),
                updated_by: String::new(),
                labels: req.labels,
                annotations: std::collections::HashMap::new(),
            })
        };
        
        let proto_actor = ProtoActor {
            actor_id: actor_id.clone(),
            actor_type: req.actor_type.clone(),
            state: ActorState::ActorStateActive as i32,
            node_id: self.node.id().as_str().to_string(),
            vm_id: String::new(),
            actor_state: req.initial_state,
            metadata,
            config: req.config,
            metrics: None,
            facets: vec![],
            isolation: None,
            actor_state_schema_version: 0,
            error_message: String::new(),
        };
        
        Ok(Response::new(CreateActorResponse {
            actor: Some(proto_actor),
        }))
    }

    async fn spawn_actor(
        &self,
        request: Request<SpawnActorRequest>,
    ) -> Result<Response<SpawnActorResponse>, Status> {
        let req = request.into_inner();

        // Validate actor_type
        if req.actor_type.is_empty() {
            return Err(Status::invalid_argument("Missing actor_type"));
        }

        // Determine actor ID: client-specified or server-generated
        // Target node is implicit from gRPC endpoint (this node)
        let node_id = self.node.id().as_str();
        let actor_id = if !req.actor_id.is_empty() {
            // Client-specified ID (for virtual actors)
            // Format: "{actor_type}/{key}" or "{actor_type}@{key}"
            // Ensure it includes node suffix for consistency
            if req.actor_id.contains('@') {
                req.actor_id.clone()
            } else {
                format!("{}@{}", req.actor_id, node_id)
            }
        } else {
            // Server-generated ID (ULID)
            format!("{}@{}", ulid::Ulid::new(), node_id)
        };

        // Create actor based on actor_type
        // Phase 1: Simple implementation - spawn simple actor as placeholder
        // Future: Use actor registry to look up actor factory by type
        use plexspaces_actor::Actor;
        use plexspaces_core::Actor as ActorTrait;
        use plexspaces_mailbox::{Mailbox, MailboxConfig};

        // Create a simple test behavior that logs messages
        struct SimpleBehavior;

        #[async_trait::async_trait]
        impl ActorTrait for SimpleBehavior {
            async fn handle_message(
                &mut self,
                _ctx: &plexspaces_core::ActorContext,
                _msg: plexspaces_mailbox::Message,
            ) -> Result<(), plexspaces_core::BehaviorError> {
                Ok(())
            }

            fn behavior_type(&self) -> plexspaces_core::BehaviorType {
                plexspaces_core::BehaviorType::GenServer
            }
        }

        use plexspaces_mailbox::mailbox_config_default;
        let mailbox = Mailbox::new(mailbox_config_default(), format!("{}:mailbox", actor_id))
            .await
            .map_err(|e| tonic::Status::internal(format!("Failed to create mailbox: {}", e)))?;
        let behavior = Box::new(SimpleBehavior);

        let actor = Actor::new(
            actor_id.clone(),
            behavior,
            mailbox,
            "default".to_string(), // namespace
            None, // node_id - will be set by ActorFactory
        );

        // Spawn actor using ActorFactory
        let actor_factory: Arc<ActorFactoryImpl> = self.node.service_locator().get_service().await
            .ok_or_else(|| Status::internal("ActorFactory not found in ServiceLocator"))?;
        
        let _message_sender = actor_factory.spawn_built_actor(Arc::new(actor), None, None, None).await
            .map_err(|e| Status::internal(format!("Failed to spawn actor: {}", e)))?;

        // Build proto Actor message for response
        use plexspaces_proto::v1::actor::{Actor as ProtoActor, ActorState};

        let proto_actor = ProtoActor {
            actor_id: actor_id.clone(),
            actor_type: req.actor_type.clone(),
            state: ActorState::ActorStateActive as i32,
            node_id: node_id.to_string(),
            vm_id: String::new(),
            actor_state: req.initial_state.clone(),
            metadata: None,
            config: req.config,
            metrics: None,
            facets: vec![],
            isolation: None,
            actor_state_schema_version: 0,
            error_message: String::new(),
        };

        // Return response with ActorRef format "actor_id@node_id"
        Ok(Response::new(SpawnActorResponse {
            actor_ref: actor_id.clone(),
            actor: Some(proto_actor),
        }))
    }

    async fn get_actor(
        &self,
        request: Request<GetActorRequest>,
    ) -> Result<Response<GetActorResponse>, Status> {
        // Check if service is accepting requests
        self.check_accepting_requests().await?;
        
        let req = request.into_inner();
        let actor_id = req.actor_id;
        
        // Use ActorRegistry directly to lookup routing info
        let actor_registry: Arc<ActorRegistry> = self.node.service_locator().get_service().await
            .ok_or_else(|| Status::internal("ActorRegistry not found in ServiceLocator"))?;
        
        let routing = actor_registry.lookup_routing(&actor_id).await
            .map_err(|e| Status::not_found(format!("Actor not found: {}", e)))?;
        
        let location = if let Some(routing_info) = routing {
            if routing_info.is_local {
                ActorLocation::Local(actor_id.clone())
            } else {
                ActorLocation::Remote(crate::NodeId::from(routing_info.node_id))
            }
        } else {
            return Err(Status::not_found(format!("Actor not found: {}", actor_id)));
        };
        
        // Record metrics
        metrics::counter!("plexspaces_node_actor_lookups_total",
            "node_id" => self.node.id().as_str().to_string(),
            "location" => match &location {
                ActorLocation::Local(_) => "local",
                ActorLocation::Remote(_) => "remote",
            }
        ).increment(1);
        tracing::debug!(actor_id = %actor_id, node_id = %self.node.id().as_str(), "Actor lookup via gRPC");
        
        // Build proto Actor message
        use plexspaces_proto::v1::actor::{Actor as ProtoActor, ActorState};
        
        match location {
            ActorLocation::Local(_) => {
                // Check if virtual actor using VirtualActorManager
                let virtual_actor_manager: Arc<plexspaces_core::VirtualActorManager> = self.node.service_locator().get_service().await
                    .ok_or_else(|| Status::internal("VirtualActorManager not found"))?;
                let is_virtual = virtual_actor_manager.is_virtual(&actor_id).await;
                let actor_type = if is_virtual { "virtual" } else { "standard" }.to_string();
                
                // Get actor config via Node's public accessor method
                let config = self.node.get_actor_config(&actor_id).await;
                
                let proto_actor = ProtoActor {
                    actor_id: actor_id.clone(),
                    actor_type,
                    state: ActorState::ActorStateActive as i32,
                    node_id: self.node.id().as_str().to_string(),
                    vm_id: String::new(),
                    actor_state: vec![],
                    metadata: None,
                    config,
                    metrics: None,
                    facets: vec![],
                    isolation: None,
                    actor_state_schema_version: 0,
                    error_message: String::new(),
                };
                
                Ok(Response::new(GetActorResponse {
                    actor: Some(proto_actor),
                }))
            }
            ActorLocation::Remote(node_id) => {
                // Actor is on remote node - return basic info
                let proto_actor = ProtoActor {
                    actor_id: actor_id.clone(),
                    actor_type: "remote".to_string(),
                    state: ActorState::ActorStateActive as i32,
                    node_id: node_id.as_str().to_string(),
                    vm_id: String::new(),
                    actor_state: vec![],
                    metadata: None,
                    config: None,
                    metrics: None,
                    facets: vec![],
                    isolation: None,
                    actor_state_schema_version: 0,
                    error_message: String::new(),
                };
                
                Ok(Response::new(GetActorResponse {
                    actor: Some(proto_actor),
                }))
            }
        }
    }

    async fn list_actors(
        &self,
        _request: Request<ListActorsRequest>,
    ) -> Result<Response<ListActorsResponse>, Status> {
        Err(Status::unimplemented("ListActors not yet implemented"))
    }

    async fn send_message(
        &self,
        request: Request<SendMessageRequest>,
    ) -> Result<Response<SendMessageResponse>, Status> {
        // Check if service is accepting requests
        self.check_accepting_requests().await?;
        
        let req = request.into_inner();

        // Validate: Check for missing message
        let proto_msg = req
            .message
            .ok_or_else(|| Status::invalid_argument("Missing message"))?;

        // Validate: Check for missing receiver_id
        if proto_msg.receiver_id.is_empty() {
            return Err(Status::invalid_argument("Missing receiver_id"));
        }

        // Convert proto message to internal message
        let message = convert_proto_to_internal(&proto_msg)?;

        // Use ActorRegistry to get ActorRef - ActorRef::tell() handles routing and metrics
        let actor_registry: Arc<ActorRegistry> = self.node.service_locator().get_service().await
            .ok_or_else(|| Status::internal("ActorRegistry not found in ServiceLocator"))?;
        
        // Check if actor exists in registry
        let actor_ref = if let Some(_message_sender) = actor_registry.lookup_actor(&proto_msg.receiver_id).await {
            // Actor exists - create ActorRef pointing to local node
            plexspaces_actor::ActorRef::remote(
                proto_msg.receiver_id.clone(),
                self.node.id().as_str().to_string(),
                self.node.service_locator().clone(),
            )
        } else {
            // Check routing for remote actors
            let routing = actor_registry.lookup_routing(&proto_msg.receiver_id).await
                .map_err(|e| Status::not_found(format!("Actor not found: {}", e)))?;
            
            if let Some(routing_info) = routing {
                if routing_info.is_local {
                    return Err(Status::not_found(format!("Actor not found: {}", proto_msg.receiver_id)));
                } else {
                    // Remote actor
                    plexspaces_actor::ActorRef::remote(
                        proto_msg.receiver_id.clone(),
                        routing_info.node_id,
                        self.node.service_locator().clone(),
                    )
                }
            } else {
                return Err(Status::not_found(format!("Actor not found: {}", proto_msg.receiver_id)));
            }
        };
        
        use plexspaces_actor::ActorRefError;
        actor_ref.tell(message).await
            .map_err(|e| {
                // Convert ActorRefError to appropriate gRPC Status
                match e {
                    ActorRefError::ActorNotFound(_) => Status::not_found(e.to_string()),
                    ActorRefError::SendFailed(_) => Status::internal(format!("Failed to send message: {}", e)),
                    ActorRefError::MailboxFull => Status::resource_exhausted(format!("Mailbox full: {}", e)),
                    ActorRefError::ActorTerminated => Status::not_found(format!("Actor terminated: {}", e)),
                    _ => Status::internal(format!("Failed to send message: {}", e)),
                }
            })?;

        // Return success response
        Ok(Response::new(SendMessageResponse {
            message_id: proto_msg.id,
            response: None,
        }))
    }

    async fn set_actor_state(
        &self,
        _request: Request<SetActorStateRequest>,
    ) -> Result<Response<SetActorStateResponse>, Status> {
        Err(Status::unimplemented("SetActorState not yet implemented"))
    }

    async fn migrate_actor(
        &self,
        _request: Request<MigrateActorRequest>,
    ) -> Result<Response<MigrateActorResponse>, Status> {
        Err(Status::unimplemented("MigrateActor not yet implemented"))
    }

    async fn delete_actor(
        &self,
        _request: Request<DeleteActorRequest>,
    ) -> Result<Response<Empty>, Status> {
        Err(Status::unimplemented("DeleteActor not yet implemented"))
    }

    // StreamMessages associated type (required by trait)
    type StreamMessagesStream =
        tokio_stream::wrappers::ReceiverStream<Result<StreamMessageResponse, Status>>;

    async fn stream_messages(
        &self,
        _request: Request<tonic::Streaming<StreamMessageRequest>>,
    ) -> Result<Response<Self::StreamMessagesStream>, Status> {
        Err(Status::unimplemented("StreamMessages not yet implemented"))
    }

    async fn monitor_actor(
        &self,
        request: Request<MonitorActorRequest>,
    ) -> Result<Response<MonitorActorResponse>, Status> {
        // Check if service is accepting requests
        self.check_accepting_requests().await?;
        
        let req = request.into_inner();

        // Extract fields
        let actor_id = req.actor_id;
        let supervisor_id = req.supervisor_id;
        let supervisor_callback = req.supervisor_callback;

        // Verify actor exists locally using ActorRegistry
        let actor_registry: Arc<ActorRegistry> = self.node.service_locator().get_service().await
            .ok_or_else(|| Status::internal("ActorRegistry not found in ServiceLocator"))?;
        
        let routing = actor_registry.lookup_routing(&actor_id).await
            .map_err(|e| Status::not_found(format!("Actor not found: {}", e)))?;
        
        match routing {
            Some(routing_info) if routing_info.is_local => {
                // Actor is local, verify it's actually activated
                if actor_registry.lookup_actor(&actor_id).await.is_none() {
                    return Err(Status::not_found(format!("Actor {} not activated", actor_id)));
                }
            }
            Some(_) => {
                return Err(Status::not_found(format!(
                    "Actor {} not on this node",
                    actor_id
                )));
            }
            None => {
                return Err(Status::not_found(format!("Actor {} not found", actor_id)));
            }
        }

        // Create a channel for termination notifications
        // This channel will send notifications to the supervisor's callback address
        let (tx, mut rx) = tokio::sync::mpsc::channel(10);

        // Register monitoring link with Node
        let monitor_ref = self
            .node
            .monitor(&actor_id, &supervisor_id, tx)
            .await
            .map_err(|e| Status::internal(format!("Failed to establish monitor: {}", e)))?;

        // Spawn background task to forward notifications to supervisor callback
        let _node_clone = self.node.clone();
        tokio::spawn(async move {
            while let Some((terminated_actor_id, reason)) = rx.recv().await {
                // Forward notification to supervisor via gRPC
                if let Err(e) = notify_supervisor_via_grpc(
                    &supervisor_callback,
                    &terminated_actor_id,
                    &supervisor_id,
                    &reason,
                )
                .await
                {
                    eprintln!("Failed to notify supervisor: {}", e);
                }
            }
        });

        Ok(Response::new(MonitorActorResponse { monitor_ref }))
    }

    async fn notify_actor_down(
        &self,
        request: Request<ActorDownNotification>,
    ) -> Result<Response<Empty>, Status> {
        let notif = request.into_inner();

        // This RPC is called by remote nodes to notify this node that
        // a monitored actor has terminated

        // The notification contains:
        // - actor_id: The actor that terminated
        // - supervisor_id: The supervisor on THIS node that was monitoring
        // - reason: Why the actor terminated

        // Forward this to the local Node's notify_actor_down
        // which will trigger any local monitoring callbacks
        self.node
            .notify_actor_down(&notif.actor_id, &notif.reason)
            .await
            .map_err(|e| Status::internal(format!("Failed to notify: {}", e)))?;

        Ok(Response::new(Empty {}))
    }

    async fn link_actor(
        &self,
        request: Request<LinkActorRequest>,
    ) -> Result<Response<LinkActorResponse>, Status> {
        // Check if service is accepting requests
        self.check_accepting_requests().await?;
        
        let req = request.into_inner();

        // Verify both actors exist locally using ActorRegistry
        let actor_registry: Arc<ActorRegistry> = self.node.service_locator().get_service().await
            .ok_or_else(|| Status::internal("ActorRegistry not found in ServiceLocator"))?;
        
        // Check first actor
        let routing1 = actor_registry.lookup_routing(&req.actor_id).await
            .map_err(|e| Status::not_found(format!("Actor {} not found: {}", req.actor_id, e)))?;
        if !routing1.map(|r| r.is_local).unwrap_or(false) || actor_registry.lookup_actor(&req.actor_id).await.is_none() {
            return Err(Status::not_found(format!(
                "Actor {} not found on this node",
                req.actor_id
            )));
        }
        
        // Check second actor
        let routing2 = actor_registry.lookup_routing(&req.linked_actor_id).await
            .map_err(|e| Status::not_found(format!("Actor {} not found: {}", req.linked_actor_id, e)))?;
        if !routing2.map(|r| r.is_local).unwrap_or(false) || actor_registry.lookup_actor(&req.linked_actor_id).await.is_none() {
            return Err(Status::not_found(format!(
                "Actor {} not found on this node",
                req.linked_actor_id
            )));
        }

        // Create bidirectional link
        self.node
            .link(&req.actor_id, &req.linked_actor_id)
            .await
            .map_err(|e| Status::internal(format!("Failed to link actors: {}", e)))?;

        Ok(Response::new(LinkActorResponse { success: true }))
    }

    async fn invoke_actor(
        &self,
        request: Request<InvokeActorRequest>,
    ) -> Result<Response<InvokeActorResponse>, Status> {
        // Check if service is accepting requests
        self.check_accepting_requests().await?;
        
        // Delegate to actor-service's ActorServiceImpl
        // Create actor-service instance with node's service locator
        let actor_service_impl = plexspaces_actor_service::ActorServiceImpl::new(
            self.node.service_locator().clone(),
            self.node.id().as_str().to_string(),
        );
        
        actor_service_impl.invoke_actor(request).await
    }

    async fn unlink_actor(
        &self,
        request: Request<UnlinkActorRequest>,
    ) -> Result<Response<UnlinkActorResponse>, Status> {
        // Check if service is accepting requests
        self.check_accepting_requests().await?;
        
        let req = request.into_inner();

        // Remove bidirectional link
        self.node
            .unlink(&req.actor_id, &req.linked_actor_id)
            .await
            .map_err(|e| Status::internal(format!("Failed to unlink actors: {}", e)))?;

        Ok(Response::new(UnlinkActorResponse { success: true }))
    }

    // Phase 8.5: Virtual Actor Lifecycle RPCs

    async fn activate_actor(
        &self,
        request: Request<ActivateActorRequest>,
    ) -> Result<Response<ActivateActorResponse>, Status> {
        // Check if service is accepting requests
        self.check_accepting_requests().await?;
        
        let req = request.into_inner();
        let actor_id = req.actor_id;

        // Activate virtual actor using ActorFactory
        let actor_factory: Arc<ActorFactoryImpl> = self.node.service_locator().get_service().await
            .ok_or_else(|| Status::internal("ActorFactory not found in ServiceLocator"))?;
        
        actor_factory.activate_virtual_actor(&actor_id).await
            .map_err(|e| Status::not_found(format!("Failed to activate actor: {}", e)))?;

        // Extract lifecycle state from VirtualActorFacet
        // Convert to proto VirtualActorLifecycle if available
        use plexspaces_proto::v1::actor::VirtualActorLifecycle;
        use plexspaces_proto::prost_types;
        use std::time::{SystemTime, UNIX_EPOCH};
        
        let lifecycle = if let Some(virtual_meta) = self.node.get_virtual_actor_metadata(&actor_id).await {
            let facet_arc = &virtual_meta.facet;
            let facet_guard = facet_arc.read().await;
            // Downcast from Box<dyn Any> to VirtualActorFacet
            let facet = facet_guard
                .downcast_ref::<plexspaces_journaling::VirtualActorFacet>()
                .ok_or_else(|| {
                    tonic::Status::internal("Failed to downcast VirtualActorFacet")
                })?;
            let lifecycle_state = facet.get_lifecycle_state().await;
            drop(facet_guard);
            
            // Get pending message count from node's pending_activations
            let pending_count = self.node.get_pending_activation_count(&actor_id).await;
            
            // Convert VirtualActorLifecycleState to proto VirtualActorLifecycle
            Some(VirtualActorLifecycle {
                last_activated: lifecycle_state.last_activated.map(|t| {
                    prost_types::Timestamp {
                        seconds: t.duration_since(UNIX_EPOCH).unwrap().as_secs() as i64,
                        nanos: 0,
                    }
                }),
                last_accessed: lifecycle_state.last_accessed.map(|t| {
                    prost_types::Timestamp {
                        seconds: t.duration_since(UNIX_EPOCH).unwrap().as_secs() as i64,
                        nanos: 0,
                    }
                }),
                idle_timeout: Some(prost_types::Duration {
                    seconds: lifecycle_state.idle_timeout.as_secs() as i64,
                    nanos: lifecycle_state.idle_timeout.subsec_nanos() as i32,
                }),
                activation_count: lifecycle_state.activation_count,
                is_activating: lifecycle_state.is_activating,
                pending_message_count: pending_count,
            })
        } else {
            None
        };

        // Get actor config if available using public accessor method
        let config = self.node.get_actor_config(&actor_id).await;

        // Build proto Actor message
        use plexspaces_proto::v1::actor::{Actor as ProtoActor, ActorState};
        let proto_actor = ProtoActor {
            actor_id: actor_id.clone(),
            actor_type: "virtual".to_string(),
            state: ActorState::ActorStateActive as i32,
            node_id: self.node.id().as_str().to_string(),
            vm_id: String::new(),
            actor_state: vec![],
            metadata: None,
            config,
            metrics: None,
            facets: vec![],
            isolation: None,
            actor_state_schema_version: 0,
            error_message: String::new(),
        };

        Ok(Response::new(ActivateActorResponse {
            actor: Some(proto_actor),
            lifecycle,
        }))
    }

    async fn deactivate_actor(
        &self,
        request: Request<DeactivateActorRequest>,
    ) -> Result<Response<Empty>, Status> {
        // Check if service is accepting requests
        self.check_accepting_requests().await?;
        
        let req = request.into_inner();
        let actor_id = req.actor_id;
        let force = req.force;

        // Deactivate virtual actor using VirtualActorManager
        let manager: Arc<plexspaces_core::VirtualActorManager> = self.node.service_locator().get_service().await
            .ok_or_else(|| Status::internal("VirtualActorManager not found"))?;
        
        let facet_arc = manager.get_facet(&actor_id).await
            .map_err(|e| Status::internal(format!("Failed to get facet: {}", e)))?;
        
        let mut facet_guard = facet_arc.write().await;
        if let Some(facet) = facet_guard.downcast_mut::<plexspaces_journaling::VirtualActorFacet>() {
            facet.mark_deactivated().await;
        }
        drop(facet_guard);
        
        // Unregister actor using ActorRegistry directly
        let actor_registry: Arc<ActorRegistry> = self.node.service_locator().get_service().await
            .ok_or_else(|| Status::internal("ActorRegistry not found in ServiceLocator"))?;
        
        actor_registry.unregister_with_cleanup(&actor_id).await
            .map_err(|e| Status::internal(format!("Failed to deactivate actor: {}", e)))?;

        Ok(Response::new(Empty {}))
    }

    async fn check_actor_exists(
        &self,
        request: Request<CheckActorExistsRequest>,
    ) -> Result<Response<CheckActorExistsResponse>, Status> {
        let req = request.into_inner();
        let actor_id = req.actor_id;

        // Check virtual actor existence using VirtualActorManager
        let manager: Arc<plexspaces_core::VirtualActorManager> = self.node.service_locator().get_service().await
            .ok_or_else(|| Status::internal("VirtualActorManager not found"))?;
        
        let is_virtual = manager.is_virtual(&actor_id).await;
        let is_active = if is_virtual {
            manager.is_active(&actor_id).await
        } else {
            false
        };
        let exists = is_virtual; // Virtual actors are always addressable

        Ok(Response::new(CheckActorExistsResponse {
            exists,
            is_active,
            is_virtual,
        }))
    }

    async fn get_or_activate_actor(
        &self,
        request: Request<GetOrActivateActorRequest>,
    ) -> Result<Response<GetOrActivateActorResponse>, Status> {
        // Check if service is accepting requests
        self.check_accepting_requests().await?;
        
        let req = request.into_inner();
        let actor_id: plexspaces_core::ActorId = req.actor_id.clone();

        // Check if actor exists and is active using ActorRegistry
        let actor_registry: Arc<ActorRegistry> = self.node.service_locator().get_service().await
            .ok_or_else(|| Status::internal("ActorRegistry not found in ServiceLocator"))?;
        
        let routing = actor_registry.lookup_routing(&actor_id).await
            .map_err(|e| Status::not_found(format!("Actor not found: {}", e)))?;
        
        let was_activated = match routing {
            Some(routing_info) if routing_info.is_local => {
                // Actor exists locally - check if it's active
                if actor_registry.lookup_actor(&actor_id).await.is_some() {
                    tracing::debug!(actor_id = %actor_id, "Actor already exists and is active");
                    false
                } else {
                    // Actor registered but not active - need to activate
                    // Use ActorFactory to activate
                    use plexspaces_actor::{ActorFactory, actor_factory_impl::ActorFactoryImpl};
                    let actor_factory: Arc<ActorFactoryImpl> = self.node.service_locator().get_service().await
                        .ok_or_else(|| Status::internal("ActorFactory not found"))?;
                    
                    actor_factory.activate_virtual_actor(&actor_id).await
                        .map_err(|e| Status::internal(format!("Failed to activate actor: {}", e)))?;
                    true
                }
            }
            Some(_) => {
                // Actor exists on remote node - return remote ActorRef
                tracing::debug!(actor_id = %actor_id, "Actor exists on remote node");
                false
            }
            None => {
                // Actor doesn't exist - need to create it
                // Check if actor_type is provided for creation
                if req.actor_type.is_empty() {
                    return Err(Status::invalid_argument(
                        "actor_type is required when creating new actor"
                    ));
                }

                // Create actor using factory pattern
                // For now, create a simple actor - in production, use actor registry to look up factory
                use plexspaces_actor::{Actor, ActorBuilder};
                use plexspaces_core::Actor as ActorTrait;
                use plexspaces_mailbox::mailbox_config_default;

                // Simple behavior for demonstration
                struct SimpleBehavior;

                #[async_trait::async_trait]
                impl ActorTrait for SimpleBehavior {
                    async fn handle_message(
                        &mut self,
                        _ctx: &plexspaces_core::ActorContext,
                        _msg: plexspaces_mailbox::Message,
                    ) -> Result<(), plexspaces_core::BehaviorError> {
                        Ok(())
                    }

                    fn behavior_type(&self) -> plexspaces_core::BehaviorType {
                        plexspaces_core::BehaviorType::GenServer
                    }
                }

                let behavior = Box::new(SimpleBehavior);
                let actor = ActorBuilder::new(behavior)
                    .with_id(actor_id.clone())
                    .build()
                    .await;

                // Spawn actor using ActorFactory
                let actor_factory: Arc<ActorFactoryImpl> = self.node.service_locator().get_service().await
                    .ok_or_else(|| Status::internal("ActorFactory not found in ServiceLocator"))?;
                
                let _message_sender = actor_factory.spawn_built_actor(Arc::new(actor), None, None, None).await
                    .map_err(|e| Status::internal(format!("Failed to spawn actor: {}", e)))?;

                tracing::debug!(actor_id = %actor_id, "Actor created and activated");
                true
            }
        };

        // Build response
        use plexspaces_proto::v1::actor::{Actor as ProtoActor, ActorState};
        let proto_actor = ProtoActor {
            actor_id: req.actor_id.clone(),
            actor_type: if req.actor_type.is_empty() {
                "unknown".to_string()
            } else {
                req.actor_type.clone()
            },
            state: ActorState::ActorStateActive as i32,
            node_id: self.node.id().as_str().to_string(),
            vm_id: String::new(),
            actor_state: req.initial_state.clone(),
            metadata: None,
            config: req.config,
            metrics: None,
            facets: vec![],
            isolation: None,
            actor_state_schema_version: 0,
            error_message: String::new(),
        };

        // Build actor_ref (format: "actor_id@node_id")
        let actor_ref = if req.actor_id.contains('@') {
            req.actor_id.clone()
        } else {
            format!("{}@{}", req.actor_id, self.node.id().as_str())
        };

        Ok(Response::new(GetOrActivateActorResponse {
            actor_ref,
            actor: Some(proto_actor),
            was_activated,
        }))
    }
}

/// Convert proto Message to internal Message
fn convert_proto_to_internal(proto_msg: &ProtoMessage) -> Result<Message, Status> {
    // Extract sender ID (optional - empty string means no sender)
    let sender = if proto_msg.sender_id.is_empty() {
        None
    } else {
        Some(proto_msg.sender_id.clone())
    };

    // Extract receiver ID (required)
    let receiver = if proto_msg.receiver_id.is_empty() {
        return Err(Status::invalid_argument("Missing receiver_id"));
    } else {
        proto_msg.receiver_id.clone()
    };

    // Create internal message with payload
    let mut message = Message::new(proto_msg.payload.clone());

    // Set message fields
    message.id = proto_msg.id.clone();
    message.sender = sender;
    message.receiver = receiver;
    message.message_type = proto_msg.message_type.clone();

    // Copy headers to metadata
    for (key, value) in &proto_msg.headers {
        message.metadata.insert(key.clone(), value.clone());
    }

    Ok(message)
}

/// Notify a supervisor via gRPC that an actor has terminated
///
/// ## Arguments
/// * `supervisor_callback` - gRPC address of supervisor node (e.g., "http://localhost:9001")
/// * `actor_id` - The actor that terminated
/// * `supervisor_id` - The supervisor to notify
/// * `reason` - Why the actor terminated
async fn notify_supervisor_via_grpc(
    supervisor_callback: &str,
    actor_id: &str,
    supervisor_id: &str,
    reason: &str,
) -> Result<(), String> {
    // Connect to supervisor's node
    let channel = tonic::transport::Channel::from_shared(supervisor_callback.to_string())
        .map_err(|e| format!("Invalid supervisor callback: {}", e))?
        .connect()
        .await
        .map_err(|e| format!("Failed to connect to supervisor: {}", e))?;

    let mut client = ActorServiceClient::new(channel);

    // Send notification
    let request = tonic::Request::new(ActorDownNotification {
        actor_id: actor_id.to_string(),
        supervisor_id: supervisor_id.to_string(),
        reason: reason.to_string(),
    });

    client
        .notify_actor_down(request)
        .await
        .map_err(|e| format!("Failed to send notification: {}", e))?;

    Ok(())
}
