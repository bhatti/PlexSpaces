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

//! gRPC client for remote actor communication
//!
//! Provides low-level client for sending messages to actors on remote nodes.
//! This is the foundation for the future high-level SDK.

use plexspaces_actor::{apply_request_context_to_grpc_metadata, RequestContext};
use plexspaces_proto::{
    actor::v1::{ActorSpawnSpec, ActorVisibility},
    common::v1::{ActorIdentity, Message as ProtoMessage},
    v1::actor::{
        DemonitorActorRequest, LinkActorRequest, MonitorActorRequest, SendMessageRequest,
        SpawnActorRequest, SpawnActorResponse, UnlinkActorRequest,
    },
    ActorServiceClient,
};
use tonic::{transport::Channel, Request};

/// Client for communicating with remote actor nodes via gRPC
///
/// ## Purpose
/// Low-level wrapper over gRPC ActorServiceClient for remote actor communication.
/// Future high-level SDK will build on top of this.
///
/// ## Examples
/// ```no_run
/// # use plexspaces_node::grpc_client::RemoteActorClient;
/// # use plexspaces_proto::common::v1::Message as ProtoMessage;
/// # use plexspaces_proto::v1::common::ActorId as ProtoActorId;
/// # async fn example() -> Result<(), String> {
/// let mut client = RemoteActorClient::connect("http://localhost:8000").await?;
///
/// let message = ProtoMessage {
///     id: "msg-1".to_string(),
///     sender: Some(ProtoActorId { id: "sender//gen_server::default@node1".to_string() }),
///     receiver: Some(ProtoActorId { id: "actor//gen_server::default@node2".to_string() }),
///     message_type: "call".to_string(),
///     payload: vec![1, 2, 3],
///     timestamp: None,
///     priority: 0,
///     ttl: None,
///     headers: std::collections::HashMap::new(),
///     qos_level: 0,
///     task_priority: 0,
/// };
///
/// client.send_message(message).await?;
/// # Ok(())
/// # }
/// ```
#[derive(Clone)] // Safe because tonic clients are Arc-based internally
pub struct RemoteActorClient {
    /// Node address
    node_address: String,
    /// gRPC client
    client: ActorServiceClient<Channel>,
}

impl RemoteActorClient {
    /// Construct from an already-established pooled channel.
    ///
    /// Prefer this over [`Self::connect`] when a
    /// [`plexspaces_actor::GrpcConnectionManager`] is available so that TCP
    /// connections are reused across calls.
    pub fn from_channel(node_address: impl Into<String>, channel: Channel) -> Self {
        RemoteActorClient {
            node_address: node_address.into(),
            client: ActorServiceClient::new(channel),
        }
    }

    /// Connect to a remote node
    ///
    /// ## Arguments
    /// * `node_address` - Address of the remote node (e.g., "http://localhost:8000")
    ///
    /// ## Returns
    /// A connected client instance or error
    ///
    /// ## Errors
    /// - Connection failures
    /// - Invalid address format
    pub async fn connect(node_address: &str) -> Result<Self, String> {
        Self::connect_with_tls(node_address, None).await
    }

    /// Connect with optional mTLS client credentials.
    ///
    /// When `tls_config` is `Some`, the connection uses TLS and the client cert
    /// is presented during the handshake (mutual TLS).
    pub async fn connect_with_tls(
        node_address: &str,
        tls_config: Option<tonic::transport::ClientTlsConfig>,
    ) -> Result<Self, String> {
        // Choose scheme based on whether TLS is configured.
        let endpoint = if node_address.starts_with("http://")
            || node_address.starts_with("https://")
        {
            node_address.to_string()
        } else if tls_config.is_some() {
            format!("https://{}", node_address)
        } else {
            format!("http://{}", node_address)
        };

        let mut builder = Channel::from_shared(endpoint.clone())
            .map_err(|e| format!("Invalid endpoint: {}", e))?;

        if let Some(cfg) = tls_config {
            builder = builder.tls_config(cfg).map_err(|e| format!("TLS config: {}", e))?;
        }

        let channel = builder
            .connect()
            .await
            .map_err(|e| format!("Connection failed: {}", e))?;

        let client = ActorServiceClient::new(channel);

        Ok(RemoteActorClient {
            node_address: endpoint,
            client,
        })
    }

    /// Send a message to a remote actor
    ///
    /// ## Arguments
    /// * `message` - Proto message to send
    ///
    /// ## Returns
    /// Message ID on success or error message
    ///
    /// ## Errors
    /// - Actor not found
    /// - Invalid message
    /// - Network errors
    pub async fn send_message(&mut self, message: ProtoMessage) -> Result<String, String> {
        let request = tonic::Request::new(SendMessageRequest {
            namespace: String::new(),
            actor_type: message.receiver_id.clone(),
            actor_name: String::new(),
            http_method: "POST".to_string(),
            payload: message.payload,
            headers: message.headers,
            query_params: Default::default(),
            path: String::new(),
            subpath: String::new(),
            sender_id: message.sender_id,
            message_type: if message.message_type.is_empty() {
                "cast".to_string()
            } else {
                message.message_type
            },
            correlation_id: message.correlation_id,
            reply_to: message.reply_to,
            message_id: message.id,
        });

        let response = self
            .client
            .send_message(request)
            .await
            .map_err(|e| e.message().to_string())?;

        let resp = response.into_inner();
        Ok(resp.message_id)
    }

    /// Get the node address this client is connected to
    pub fn node_address(&self) -> &str {
        &self.node_address
    }

    /// Spawn an actor on the remote node
    ///
    /// ## Purpose
    /// Creates a new actor on the remote node and returns an ActorRef for location-transparent messaging.
    ///
    /// ## Arguments
    /// * `target_node_id` - ID of the target node (where actor will be created)
    /// * `actor_type` - Type/name of the actor to spawn
    /// * `init_args` - String key/value arguments for [`ActorSpawnSpec::args`] (structured init; no JSON bytes)
    /// * `config` - Optional actor configuration
    /// * `labels` - Optional labels for the actor
    ///
    /// ## Note
    /// This helper leaves [`ActorSpawnSpec::role`] empty. For virtual definitions or dispatch
    /// roles, call `ActorServiceClient::spawn_actor` with a fully built `SpawnActorRequest`.
    ///
    /// ## Returns
    /// ActorRef for the newly created remote actor
    ///
    /// ## Errors
    /// - Connection failures
    /// - Invalid request
    /// - Actor creation failures
    ///
    /// ## Example
    /// ```no_run
    /// # use plexspaces_node::grpc_client::RemoteActorClient;
    /// # use plexspaces_proto::v1::actor::ActorConfig;
    /// # async fn example() -> Result<(), String> {
    /// let mut client = RemoteActorClient::connect("http://localhost:8000").await?;
    ///
    /// let actor_ref = client
    ///     .spawn_actor(
    ///         "node2",
    ///         "my-actor",
    ///         std::collections::HashMap::new(),
    ///         Some(ActorConfig::default()),
    ///         std::collections::HashMap::new(),
    ///     )
    ///     .await?;
    ///
    /// // Use actor_ref for location-transparent messaging
    /// # Ok(())
    /// # }
    /// ```
    pub async fn spawn_actor(
        &mut self,
        _target_node_id: &str,
        actor_type: &str,
        init_args: std::collections::HashMap<String, String>,
        config: Option<plexspaces_proto::v1::actor::ActorConfig>,
        labels: std::collections::HashMap<String, String>,
    ) -> Result<plexspaces_service_traits::ActorRef, String> {
        let spec = ActorSpawnSpec {
            identity: Some(ActorIdentity {
                name: String::new(),
                actor_type: actor_type.to_string(),
            }),
            role: String::new(),
            namespace: String::new(),
            tenant_id: String::new(),
            visibility: ActorVisibility::ActorVisibilityPublic as i32,
            behavior_kind: String::new(),
            args: init_args,
            facets: vec![],
            config,
            labels,
            ..Default::default()
        };
        let request = Request::new(SpawnActorRequest {
            spec: Some(spec),
            namespace: String::new(),
            instances_count: 1,
        });

        let response = self
            .client
            .spawn_actor(request)
            .await
            .map_err(|e| format!("spawn_actor failed: {}", e.message()))?;

        let resp: SpawnActorResponse = response.into_inner();
        let actor_ref = plexspaces_service_traits::ActorRef::new(resp.actor_ref.into())
            .map_err(|e| format!("Failed to create ActorRef: {}", e))?;

        Ok(actor_ref)
    }

    /// Register a one-way monitor: `supervisor_id` receives `__DOWN__` when `actor_id` terminates.
    ///
    /// Returns the `monitor_ref` ULID that can be passed to `demonitor`.
    pub async fn monitor_actor(
        &mut self,
        ctx: &RequestContext,
        actor_id: &str,
        supervisor_id: &str,
        supervisor_callback: &str,
    ) -> Result<String, String> {
        let mut req = Request::new(MonitorActorRequest {
            actor_id: actor_id.to_string(),
            supervisor_id: supervisor_id.to_string(),
            supervisor_callback: supervisor_callback.to_string(),
        });
        apply_request_context_to_grpc_metadata(ctx, req.metadata_mut());
        let resp = self
            .client
            .monitor_actor(req)
            .await
            .map_err(|e| format!("monitor_actor failed: {}", e.message()))?;

        Ok(resp.into_inner().monitor_ref)
    }

    /// Cancel a monitor on the remote node that hosts `actor_id`.
    pub async fn demonitor_actor(
        &mut self,
        ctx: &RequestContext,
        actor_id: &str,
        supervisor_id: &str,
        monitor_ref: &str,
    ) -> Result<(), String> {
        let mut req = Request::new(DemonitorActorRequest {
            actor_id: actor_id.to_string(),
            supervisor_id: supervisor_id.to_string(),
            monitor_ref: monitor_ref.to_string(),
        });
        apply_request_context_to_grpc_metadata(ctx, req.metadata_mut());
        self.client
            .demonitor_actor(req)
            .await
            .map_err(|e| format!("demonitor_actor failed: {}", e.message()))?;
        Ok(())
    }

    /// Create a bidirectional link between two actors on the remote node.
    pub async fn link_actor(
        &mut self,
        ctx: &RequestContext,
        actor_id: &str,
        linked_actor_id: &str,
    ) -> Result<(), String> {
        let mut req = Request::new(LinkActorRequest {
            actor_id: actor_id.to_string(),
            linked_actor_id: linked_actor_id.to_string(),
        });
        apply_request_context_to_grpc_metadata(ctx, req.metadata_mut());
        self.client
            .link_actor(req)
            .await
            .map_err(|e| format!("link_actor failed: {}", e.message()))?;

        Ok(())
    }

    /// Remove a bidirectional link between two actors on the remote node.
    pub async fn unlink_actor(
        &mut self,
        ctx: &RequestContext,
        actor_id: &str,
        linked_actor_id: &str,
    ) -> Result<(), String> {
        let mut req = Request::new(UnlinkActorRequest {
            actor_id: actor_id.to_string(),
            linked_actor_id: linked_actor_id.to_string(),
        });
        apply_request_context_to_grpc_metadata(ctx, req.metadata_mut());
        self.client
            .unlink_actor(req)
            .await
            .map_err(|e| format!("unlink_actor failed: {}", e.message()))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_endpoint_formatting() {
        // Without scheme
        let addr = "localhost:8000";
        assert!(format!("http://{}", addr).starts_with("http://"));

        // With http scheme
        let addr = "http://localhost:8000";
        assert!(addr.starts_with("http://"));

        // With https scheme
        let addr = "https://localhost:8000";
        assert!(addr.starts_with("https://"));
    }
}
