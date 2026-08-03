// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! Supervisor tree operations — impl ActorService (actor_context trait) for ActorServiceImpl.

use std::sync::Arc;

use async_trait::async_trait;
use tonic::Status;

use plexspaces_actor::{RequestContext, RequestContextExt};
use plexspaces_proto::actor::v1::{
    DemonitorActorRequest, LinkActorRequest, MonitorActorRequest, UnlinkActorRequest,
};
use plexspaces_proto::common::v1::Message;
use plexspaces_proto::ActorServiceClient;

use super::ActorServiceImpl;

/// Implement ActorService trait from core (for ActorContext)
#[async_trait]
impl plexspaces_actor::actor_context::ActorService for ActorServiceImpl {
    async fn spawn_actor(
        &self,
        ctx: &RequestContext,
        spec: &plexspaces_proto::actor::v1::ActorSpawnSpec,
    ) -> Result<plexspaces_service_traits::ActorRef, Box<dyn std::error::Error + Send + Sync>> {
        let actor_ref_impl = self
            .spawn_actor_local_from_spec(ctx, spec)
            .await
            .map_err(|e| format!("Failed to spawn actor: {}", e))?;
        plexspaces_service_traits::ActorRef::new(actor_ref_impl.id().clone())
            .map_err(|e| format!("Failed to create ActorRef: {}", e).into())
    }

    async fn send(
        &self,
        ctx: &RequestContext,
        actor_id: &str,
        message: Message,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        self.send_message(actor_id, message, Some(ctx)).await
    }

    async fn send_and_wait(
        &self,
        ctx: &RequestContext,
        actor_id: &str,
        message: Message,
        timeout: Option<std::time::Duration>,
    ) -> Result<Message, Box<dyn std::error::Error + Send + Sync>> {
        self.send_message_and_wait(actor_id, message, timeout, ctx.clone())
            .await
    }

    async fn create_shard_group(
        &self,
        ctx: &RequestContext,
        req: plexspaces_proto::actor::v1::CreateShardGroupRequest,
    ) -> Result<
        plexspaces_proto::actor::v1::CreateShardGroupResponse,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        self.check_accepting_requests()
            .await
            .map_err(|e| format!("Service not accepting requests: {}", e))?;
        self.create_shard_group_internal(ctx, req).await
    }

    async fn bulk_update_shard_group(
        &self,
        ctx: &RequestContext,
        req: plexspaces_proto::actor::v1::BulkUpdateShardGroupRequest,
    ) -> Result<
        plexspaces_proto::actor::v1::BulkUpdateShardGroupResponse,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        self.check_accepting_requests()
            .await
            .map_err(|e| format!("Service not accepting requests: {}", e))?;
        self.bulk_update_shard_group_internal(ctx, req).await
    }

    async fn map_shard_group(
        &self,
        ctx: &RequestContext,
        req: plexspaces_proto::actor::v1::MapShardGroupRequest,
    ) -> Result<
        plexspaces_proto::actor::v1::MapShardGroupResponse,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        self.check_accepting_requests()
            .await
            .map_err(|e| format!("Service not accepting requests: {}", e))?;
        self.map_shard_group_internal(ctx, req).await
    }

    async fn scatter_gather(
        &self,
        ctx: &RequestContext,
        req: plexspaces_proto::actor::v1::ScatterGatherRequest,
    ) -> Result<
        plexspaces_proto::actor::v1::ScatterGatherResponse,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        self.check_accepting_requests()
            .await
            .map_err(|e| format!("Service not accepting requests: {}", e))?;
        self.scatter_gather_internal(ctx, req).await
    }

    async fn broadcast_shard_group(
        &self,
        ctx: &RequestContext,
        req: plexspaces_proto::actor::v1::BroadcastShardGroupRequest,
    ) -> Result<
        plexspaces_proto::actor::v1::BroadcastShardGroupResponse,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        self.check_accepting_requests()
            .await
            .map_err(|e| format!("Service not accepting requests: {}", e))?;
        self.broadcast_shard_group_internal(ctx, req).await
    }

    async fn reduce_shard_group(
        &self,
        ctx: &RequestContext,
        req: plexspaces_proto::actor::v1::ReduceShardGroupRequest,
    ) -> Result<
        plexspaces_proto::actor::v1::ReduceShardGroupResponse,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        self.check_accepting_requests()
            .await
            .map_err(|e| format!("Service not accepting requests: {}", e))?;
        self.reduce_shard_group_internal(ctx, req).await
    }

    async fn all_reduce_shard_group(
        &self,
        ctx: &RequestContext,
        req: plexspaces_proto::actor::v1::AllReduceShardGroupRequest,
    ) -> Result<
        plexspaces_proto::actor::v1::AllReduceShardGroupResponse,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        self.check_accepting_requests()
            .await
            .map_err(|e| format!("Service not accepting requests: {}", e))?;
        self.all_reduce_shard_group_internal(ctx, req).await
    }

    async fn barrier_shard_group(
        &self,
        ctx: &RequestContext,
        req: plexspaces_proto::actor::v1::BarrierShardGroupRequest,
    ) -> Result<
        plexspaces_proto::actor::v1::BarrierShardGroupResponse,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        self.check_accepting_requests()
            .await
            .map_err(|e| format!("Service not accepting requests: {}", e))?;
        self.barrier_shard_group_internal(ctx, req).await
    }

    async fn spawn_actors(
        &self,
        ctx: &RequestContext,
        req: plexspaces_proto::actor::v1::SpawnActorsRequest,
    ) -> Result<
        plexspaces_proto::actor::v1::SpawnActorsResponse,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        self.check_accepting_requests()
            .await
            .map_err(|e| format!("Service not accepting requests: {}", e))?;

        let request_id = req.request_id.clone();
        let mut results = Vec::new();
        for spawn_req in req.requests {
            let mut base_spec = match spawn_req.spec {
                Some(s) => s,
                None => {
                    results.push(plexspaces_proto::actor::v1::SpawnActorResult {
                        success: false,
                        error: "spec is required".to_string(),
                        response: None,
                    });
                    continue;
                }
            };
            if !spawn_req.namespace.is_empty() {
                base_spec.namespace = spawn_req.namespace.clone();
            }
            let namespace = if base_spec.namespace.is_empty() {
                ctx.namespace().to_string()
            } else {
                base_spec.namespace.clone()
            };

            let (actor_type, name_prefix) = match base_spec.identity.as_ref() {
                Some(id) if !id.actor_type.is_empty() => (id.actor_type.clone(), id.name.clone()),
                _ => {
                    results.push(plexspaces_proto::actor::v1::SpawnActorResult {
                        success: false,
                        error: "spec.identity.actor_type is required".to_string(),
                        response: None,
                    });
                    continue;
                }
            };

            let count = if spawn_req.instances_count <= 1 {
                1
            } else {
                spawn_req.instances_count
            };

            for i in 0..count {
                let actor_name = if name_prefix.is_empty() {
                    ulid::Ulid::new().to_string()
                } else if count > 1 {
                    format!("{}-{}", name_prefix, i)
                } else {
                    name_prefix.clone()
                };

                let actor_id = match self.build_canonical_actor_id(
                    &actor_name,
                    &actor_type,
                    &namespace,
                    &self.local_node_id,
                ) {
                    Ok(id) => id,
                    Err(e) => {
                        results.push(plexspaces_proto::actor::v1::SpawnActorResult {
                            success: false,
                            error: e.to_string(),
                            response: None,
                        });
                        continue;
                    }
                };

                let mut spec_instance = base_spec.clone();
                spec_instance.identity = Some(plexspaces_proto::common::v1::ActorIdentity {
                    name: actor_name,
                    actor_type: actor_type.clone(),
                });
                spec_instance.namespace = namespace.clone();
                spec_instance.tenant_id = ctx.tenant_id().to_string();

                match self.spawn_actor_local_from_spec(ctx, &spec_instance).await {
                    Ok(_actor_ref) => {
                        results.push(plexspaces_proto::actor::v1::SpawnActorResult {
                            success: true,
                            error: String::new(),
                            response: Some(plexspaces_proto::actor::v1::SpawnActorResponse {
                                request_id: ulid::Ulid::new().to_string(),
                                actor_ref: actor_id.to_string(),
                                actor: None,
                            }),
                        });
                    }
                    Err(e) => {
                        results.push(plexspaces_proto::actor::v1::SpawnActorResult {
                            success: false,
                            error: e.to_string(),
                            response: None,
                        });
                    }
                }
            }
        }
        Ok(plexspaces_proto::actor::v1::SpawnActorsResponse {
            request_id,
            results,
        })
    }

    async fn monitor_actor(
        &self,
        ctx: &RequestContext,
        actor_id: &str,
        supervisor_id: &str,
        supervisor_callback: &str,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let target = self.parse_canonical_actor_id(actor_id).map_err(
            |e: Status| -> Box<dyn std::error::Error + Send + Sync> { e.to_string().into() },
        )?;
        let node_id = target.node_id();
        let sl: Arc<dyn plexspaces_actor::ServiceLocator> = self.service_locator.clone();
        let channel = sl
            .get_actor_service_client(node_id)
            .await
            .map_err(|e: Box<dyn std::error::Error + Send + Sync>| e)?;
        let mut client = ActorServiceClient::new(channel);
        let mut req = tonic::Request::new(MonitorActorRequest {
            request_id: ulid::Ulid::new().to_string(),
            actor_id: actor_id.to_string(),
            supervisor_id: supervisor_id.to_string(),
            supervisor_callback: supervisor_callback.to_string(),
        });
        plexspaces_actor::apply_request_context_to_grpc_metadata(ctx, req.metadata_mut());
        let resp = client.monitor_actor(req).await.map_err(
            |e: tonic::Status| -> Box<dyn std::error::Error + Send + Sync> {
                format!("monitor_actor failed: {}", e.message()).into()
            },
        )?;
        Ok(resp.into_inner().monitor_ref)
    }

    async fn demonitor_actor(
        &self,
        ctx: &RequestContext,
        actor_id: &str,
        supervisor_id: &str,
        monitor_ref: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let target = self.parse_canonical_actor_id(actor_id).map_err(
            |e: Status| -> Box<dyn std::error::Error + Send + Sync> { e.to_string().into() },
        )?;
        let node_id = target.node_id();
        let sl: Arc<dyn plexspaces_actor::ServiceLocator> = self.service_locator.clone();
        let channel = sl
            .get_actor_service_client(node_id)
            .await
            .map_err(|e: Box<dyn std::error::Error + Send + Sync>| e)?;
        let mut client = ActorServiceClient::new(channel);
        let mut req = tonic::Request::new(DemonitorActorRequest {
            request_id: ulid::Ulid::new().to_string(),
            actor_id: actor_id.to_string(),
            supervisor_id: supervisor_id.to_string(),
            monitor_ref: monitor_ref.to_string(),
        });
        plexspaces_actor::apply_request_context_to_grpc_metadata(ctx, req.metadata_mut());
        client.demonitor_actor(req).await.map_err(
            |e: tonic::Status| -> Box<dyn std::error::Error + Send + Sync> {
                format!("demonitor_actor failed: {}", e.message()).into()
            },
        )?;
        Ok(())
    }

    async fn link_actor(
        &self,
        ctx: &RequestContext,
        actor_id: &str,
        linked_actor_id: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let target = self.parse_canonical_actor_id(actor_id).map_err(
            |e: Status| -> Box<dyn std::error::Error + Send + Sync> { e.to_string().into() },
        )?;
        let node_id = target.node_id();
        let sl: Arc<dyn plexspaces_actor::ServiceLocator> = self.service_locator.clone();
        let channel = sl
            .get_actor_service_client(node_id)
            .await
            .map_err(|e: Box<dyn std::error::Error + Send + Sync>| e)?;
        let mut client = ActorServiceClient::new(channel);
        let mut req = tonic::Request::new(LinkActorRequest {
            request_id: ulid::Ulid::new().to_string(),
            actor_id: actor_id.to_string(),
            linked_actor_id: linked_actor_id.to_string(),
        });
        plexspaces_actor::apply_request_context_to_grpc_metadata(ctx, req.metadata_mut());
        client.link_actor(req).await.map_err(
            |e: tonic::Status| -> Box<dyn std::error::Error + Send + Sync> {
                format!("link_actor failed: {}", e.message()).into()
            },
        )?;
        Ok(())
    }

    async fn unlink_actor(
        &self,
        ctx: &RequestContext,
        actor_id: &str,
        linked_actor_id: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let target = self.parse_canonical_actor_id(actor_id).map_err(
            |e: Status| -> Box<dyn std::error::Error + Send + Sync> { e.to_string().into() },
        )?;
        let node_id = target.node_id();
        let sl: Arc<dyn plexspaces_actor::ServiceLocator> = self.service_locator.clone();
        let channel = sl
            .get_actor_service_client(node_id)
            .await
            .map_err(|e: Box<dyn std::error::Error + Send + Sync>| e)?;
        let mut client = ActorServiceClient::new(channel);
        let mut req = tonic::Request::new(UnlinkActorRequest {
            request_id: ulid::Ulid::new().to_string(),
            actor_id: actor_id.to_string(),
            linked_actor_id: linked_actor_id.to_string(),
        });
        plexspaces_actor::apply_request_context_to_grpc_metadata(ctx, req.metadata_mut());
        client.unlink_actor(req).await.map_err(
            |e: tonic::Status| -> Box<dyn std::error::Error + Send + Sync> {
                format!("unlink_actor failed: {}", e.message()).into()
            },
        )?;
        Ok(())
    }
}
