// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Leader-worker helpers for multi-node: first node is leader, splits work across workers
// on same or other nodes. Prefer virtual actors: they are created lazily on first message
// receive, so the leader just sends to the canonical actor ID with no explicit ensure or spawn.
// Use spawn_actor_on_node only for non-virtual workers.

use anyhow::{Context, Result};
use std::sync::Arc;
use tonic::Request;

use plexspaces_core::{RequestContext, ServiceLocator};

/// List node IDs that can run workers (peers + self). Excludes none; filter by cluster if needed.
///
/// Use from the **leader** node to get the set of nodes for distributing work. After
/// [ConnectNodes](crate::NodeClient::connect_nodes), the registry contains peer nodes.
///
/// ## Returns
/// Vector of node_id strings (node registrations from the registry). Include the local
/// node if you want the leader to also run workers locally.
pub async fn list_worker_node_ids(
    ctx: &RequestContext,
    service_locator: Arc<dyn ServiceLocator>,
    cluster: Option<&str>,
    page_size: u32,
) -> Result<Vec<String>> {
    let registry = service_locator
        .get_node_registry()
        .await
        .ok_or_else(|| anyhow::anyhow!("NodeRegistry not registered"))?;
    let (nodes, _next_token) = registry
        .list_nodes(ctx, cluster, page_size, "")
        .await
        .map_err(|e| anyhow::anyhow!("list_nodes failed: {}", e))?;
    Ok(nodes
        .into_iter()
        .map(|n| n.node_id)
        .collect::<Vec<String>>())
}

/// Spawn a (non-virtual) actor on a specific node by calling that node's SpawnActor.
///
/// Use when you need an explicit worker on a remote node and are not using virtual
/// actors. The **leader** calls this, then sends work to the returned actor ref.
///
/// ## Arguments
/// - `node_id`: Target node ID (must be in registry after ConnectNodes).
/// - `actor_type`: Pre-deployed actor type on the target node (e.g. `"worker"`).
/// - `actor_name`: Optional logical actor name; if empty, the server generates one.
///
/// ## Returns
/// The canonical actor ref string for messaging.
#[cfg(feature = "grpc")]
pub async fn spawn_actor_on_node(
    _ctx: &RequestContext,
    service_locator: Arc<dyn ServiceLocator>,
    node_id: &str,
    actor_type: &str,
    actor_name: String,
    initial_state: Vec<u8>,
    config: Option<plexspaces_proto::actor::v1::ActorConfig>,
    labels: std::collections::HashMap<String, String>,
) -> Result<String> {
    use plexspaces_proto::actor::v1::{
        actor_service_client::ActorServiceClient, ActorSpawnSpec, SpawnActorRequest,
        SpawnActorResponse,
    };
    use plexspaces_proto::common::v1::ActorIdentity;

    let channel = service_locator
        .get_actor_service_client(node_id)
        .await
        .map_err(|e| anyhow::anyhow!("get_actor_service_client failed: {}", e))?;
    let mut client = ActorServiceClient::new(channel);

    let (role_opt, args) = plexspaces_core::legacy_spawn_init_json_to_role_and_args(&initial_state);
    let spec = ActorSpawnSpec {
        identity: Some(ActorIdentity {
            name: actor_name,
            actor_type: actor_type.to_string(),
        }),
        role: role_opt.unwrap_or_default(),
        namespace: String::new(),
        tenant_id: String::new(),
        visibility: 0,
        behavior_kind: String::new(),
        args,
        facets: vec![],
        config,
        labels,
    };

    let req = SpawnActorRequest {
        spec: Some(spec),
        namespace: String::new(),
        instances_count: 1,
    };

    let resp: SpawnActorResponse = client
        .spawn_actor(Request::new(req))
        .await
        .context("SpawnActor RPC failed")?
        .into_inner();

    if resp.actor_ref.is_empty() {
        anyhow::bail!("SpawnActor returned empty actor_ref")
    }
    Ok(resp.actor_ref)
}
