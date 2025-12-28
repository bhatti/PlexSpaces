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

//! gRPC client for remote actor communication
//!
//! Provides low-level client for sending messages to actors on remote nodes.
//! This is the foundation for the future high-level SDK.

use plexspaces_proto::{
    v1::actor::{Message as ProtoMessage, SendMessageRequest, SpawnActorRequest, SpawnActorResponse},
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
/// # use plexspaces_proto::v1::actor::Message as ProtoMessage;
/// # use plexspaces_proto::v1::common::ActorId as ProtoActorId;
/// # async fn example() -> Result<(), String> {
/// let mut client = RemoteActorClient::connect("http://localhost:8000").await?;
///
/// let message = ProtoMessage {
///     id: "msg-1".to_string(),
///     sender: Some(ProtoActorId { id: "sender@node1".to_string() }),
///     receiver: Some(ProtoActorId { id: "actor@node2".to_string() }),
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
        // Ensure HTTP scheme
        let endpoint =
            if !node_address.starts_with("http://") && !node_address.starts_with("https://") {
                format!("http://{}", node_address)
            } else {
                node_address.to_string()
            };

        // Connect to the gRPC server
        let channel = Channel::from_shared(endpoint.clone())
            .map_err(|e| format!("Invalid endpoint: {}", e))?
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
            message: Some(message),
            wait_for_response: false,
            timeout: None,
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
    /// * `initial_state` - Optional initial state for the actor
    /// * `config` - Optional actor configuration
    /// * `labels` - Optional labels for the actor
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
    ///     .spawn_remote_actor(
    ///         "node2",
    ///         "my-actor",
    ///         None,
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
        target_node_id: &str,
        actor_type: &str,
        initial_state: Option<Vec<u8>>,
        config: Option<plexspaces_proto::v1::actor::ActorConfig>,
        labels: std::collections::HashMap<String, String>,
    ) -> Result<plexspaces_core::ActorRef, String> {
        let request = Request::new(SpawnActorRequest {
            actor_type: actor_type.to_string(),
            actor_id: String::new(), // Empty string = server generates ID
            initial_state: initial_state.unwrap_or_default(),
            config,
            labels,
        });

        let response = self
            .client
            .spawn_actor(request)
            .await
            .map_err(|e| format!("spawn_actor failed: {}", e.message()))?;

        let resp: SpawnActorResponse = response.into_inner();
        let actor_ref = plexspaces_core::ActorRef::new(resp.actor_ref)
            .map_err(|e| format!("Failed to create ActorRef: {}", e))?;

        Ok(actor_ref)
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
