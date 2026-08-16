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

//! WebSocket session registry.
//!
//! # Purpose
//! Tracks every active WebSocket connection by `node_id`. The `WsTransportClient`
//! queries this registry to decide whether to route through an active WS session
//! or fall back to gRPC.
//!
//! # Concurrency
//! All state lives behind a single `RwLock<HashMap>`. Reads (get_sender,
//! is_connected, list_thin_nodes) hold the read lock briefly to clone the
//! channel/flag. Writes (register, unregister, update_heartbeat) hold the write
//! lock only while mutating the map.
//!
//! # No Global State
//! `WsRegistry` is constructed in `Node::start()` and injected into all consumers
//! via `Arc<WsRegistry>`. There are no thread-locals or statics.

use std::time::Instant;

use dashmap::DashMap;
use plexspaces_proto::node::v1::NodeRole;
use plexspaces_proto::transport::ws::v1::WsFrame;
use tokio::sync::mpsc;

// ─────────────────────────────────────────────────────────────────────────────
// WsSession
// ─────────────────────────────────────────────────────────────────────────────

/// A single active WebSocket connection from a remote node.
///
/// # Fields
/// - `node_id`: canonical node ID of the remote node (from the registration frame)
/// - `sender`: write half of the channel drained by the WS connection writer task
/// - `role`: `NODE_ROLE_THIN` (outbound-only) or `NODE_ROLE_FULL` (also has gRPC)
/// - `tenant_id`: tenant extracted from the JWT on upgrade
/// - `connected_at`: monotonic timestamp of the upgrade
/// - `last_heartbeat`: updated on each heartbeat frame (used for stale-session GC)
pub struct WsSession {
    /// The remote node's identifier.
    pub node_id: String,
    /// Channel for sending frames to the remote node's WebSocket connection.
    pub sender: mpsc::Sender<WsFrame>,
    /// Role of the remote node in the cluster.
    pub role: NodeRole,
    /// Tenant ID extracted from the JWT on upgrade.
    pub tenant_id: String,
    /// Monotonic timestamp of the WebSocket upgrade.
    pub connected_at: Instant,
    /// Updated on each heartbeat frame; used for stale-session GC.
    pub last_heartbeat: Instant,
}

// ─────────────────────────────────────────────────────────────────────────────
// WsRegistry
// ─────────────────────────────────────────────────────────────────────────────

/// Registry of active WebSocket sessions, keyed by `node_id`.
///
/// # Purpose
/// `WsActorTransportClient` checks `is_connected(node_id)` before choosing
/// between WS and gRPC routing. The sender retrieved via `get_sender` is used to
/// enqueue outbound frames for an active session.
///
/// # Concurrency
/// Uses `DashMap` (16 shards by default) instead of a single `RwLock<HashMap>`.
/// Each shard is independently locked, so heartbeats from 20k thin nodes/s do
/// not contend on a single lock.  All operations are O(1) with shard-level
/// locking only, and methods remain `async` for API stability even though no
/// await points are needed.
///
/// # Lifecycle
/// - Created once in `Node::start()` and shared via `Arc<WsRegistry>`.
/// - `register()` is called by the WS upgrade handler after a successful handshake.
/// - `unregister()` is called when the socket is closed (normal or abnormal).
/// - `update_heartbeat()` is called when a heartbeat frame arrives.
pub struct WsRegistry {
    sessions: DashMap<String, WsSession>,
}

impl WsRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            sessions: DashMap::new(),
        }
    }

    /// Register a new WebSocket session.
    ///
    /// If a session already exists for `session.node_id`, it is replaced
    /// (e.g., reconnect after a transient disconnect).
    pub async fn register(&self, session: WsSession) {
        self.sessions.insert(session.node_id.clone(), session);
    }

    /// Remove the session for `node_id`. No-op if not present.
    pub async fn unregister(&self, node_id: &str) {
        self.sessions.remove(node_id);
    }

    /// Return the outbound sender for `node_id`, or `None` if not connected.
    pub async fn get_sender(&self, node_id: &str) -> Option<mpsc::Sender<WsFrame>> {
        self.sessions.get(node_id).map(|s| s.sender.clone())
    }

    /// Return `true` if `node_id` has an active WS session.
    pub async fn is_connected(&self, node_id: &str) -> bool {
        self.sessions.contains_key(node_id)
    }

    /// Return all node IDs connected with `NODE_ROLE_THIN`.
    pub async fn list_thin_nodes(&self) -> Vec<String> {
        self.sessions
            .iter()
            .filter(|e| e.value().role == NodeRole::NodeRoleThin)
            .map(|e| e.key().clone())
            .collect()
    }

    /// Return all connected node IDs regardless of role.
    pub async fn list_all_nodes(&self) -> Vec<String> {
        self.sessions.iter().map(|e| e.key().clone()).collect()
    }

    /// Update the heartbeat timestamp for `node_id`. No-op if not registered.
    pub async fn update_heartbeat(&self, node_id: &str) {
        if let Some(mut session) = self.sessions.get_mut(node_id) {
            session.last_heartbeat = Instant::now();
        }
    }

    /// Update heartbeat and return the sender in a single shard-lock operation.
    ///
    /// With DashMap, `get_mut` holds only the shard lock for `node_id`, so
    /// this is still a single atomic operation — no double-lock risk.
    pub async fn update_heartbeat_and_get_sender(
        &self,
        node_id: &str,
    ) -> Option<mpsc::Sender<WsFrame>> {
        self.sessions.get_mut(node_id).map(|mut s| {
            s.last_heartbeat = Instant::now();
            s.sender.clone()
        })
    }

    /// Return the current session count.
    pub async fn session_count(&self) -> usize {
        self.sessions.len()
    }
}

impl Default for WsRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// Each method below delegates to the identically-named inherent method.
// Rust resolves `self.method()` to the inherent impl first (not this trait),
// so there is no recursion. Adding a default method to WsRegistryTrait with
// the same name as an inherent method would silently shadow it — avoid that.
#[async_trait::async_trait]
impl plexspaces_actor::WsRegistryTrait for WsRegistry {
    async fn list_thin_nodes(&self) -> Vec<String> {
        self.list_thin_nodes().await
    }

    async fn list_all_nodes(&self) -> Vec<String> {
        self.list_all_nodes().await
    }

    async fn is_connected(&self, node_id: &str) -> bool {
        self.is_connected(node_id).await
    }

    async fn session_count(&self) -> usize {
        self.session_count().await
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use plexspaces_proto::node::v1::NodeRole;
    use tokio::sync::mpsc;

    fn make_session(node_id: &str, role: NodeRole) -> (WsSession, mpsc::Receiver<WsFrame>) {
        let (tx, rx) = mpsc::channel(16);
        let session = WsSession {
            node_id: node_id.to_string(),
            sender: tx,
            role,
            tenant_id: "tenant-1".to_string(),
            connected_at: Instant::now(),
            last_heartbeat: Instant::now(),
        };
        (session, rx)
    }

    #[tokio::test]
    async fn test_register_and_is_connected() {
        let registry = WsRegistry::new();
        let (session, _rx) = make_session("node-a", NodeRole::NodeRoleThin);

        assert!(!registry.is_connected("node-a").await);
        registry.register(session).await;
        assert!(registry.is_connected("node-a").await);
    }

    #[tokio::test]
    async fn test_unregister_removes_session() {
        let registry = WsRegistry::new();
        let (session, _rx) = make_session("node-b", NodeRole::NodeRoleFull);

        registry.register(session).await;
        assert!(registry.is_connected("node-b").await);

        registry.unregister("node-b").await;
        assert!(!registry.is_connected("node-b").await);
    }

    #[tokio::test]
    async fn test_get_sender_returns_channel() {
        let registry = WsRegistry::new();
        let (session, _rx) = make_session("node-c", NodeRole::NodeRoleThin);

        registry.register(session).await;
        let sender = registry.get_sender("node-c").await;
        assert!(sender.is_some(), "Expected sender for registered node");

        let sender = registry.get_sender("node-nonexistent").await;
        assert!(sender.is_none(), "Expected None for unregistered node");
    }

    #[tokio::test]
    async fn test_list_thin_nodes_filters_by_role() {
        let registry = WsRegistry::new();
        let (thin1, _) = make_session("thin-1", NodeRole::NodeRoleThin);
        let (thin2, _) = make_session("thin-2", NodeRole::NodeRoleThin);
        let (full1, _) = make_session("full-1", NodeRole::NodeRoleFull);

        registry.register(thin1).await;
        registry.register(thin2).await;
        registry.register(full1).await;

        let mut thin_nodes = registry.list_thin_nodes().await;
        thin_nodes.sort();
        assert_eq!(thin_nodes, vec!["thin-1", "thin-2"]);
    }

    #[tokio::test]
    async fn test_list_all_nodes() {
        let registry = WsRegistry::new();
        let (s1, _) = make_session("node-1", NodeRole::NodeRoleThin);
        let (s2, _) = make_session("node-2", NodeRole::NodeRoleFull);

        registry.register(s1).await;
        registry.register(s2).await;

        let mut all = registry.list_all_nodes().await;
        all.sort();
        assert_eq!(all, vec!["node-1", "node-2"]);
    }

    #[tokio::test]
    async fn test_update_heartbeat_updates_timestamp() {
        let registry = WsRegistry::new();
        let (session, _rx) = make_session("node-d", NodeRole::NodeRoleThin);

        registry.register(session).await;

        // Should not panic for registered node
        registry.update_heartbeat("node-d").await;
        // No-op for unregistered node — should not panic
        registry.update_heartbeat("no-such-node").await;
    }

    #[tokio::test]
    async fn test_reconnect_replaces_session() {
        let registry = WsRegistry::new();
        let (session1, _rx1) = make_session("node-e", NodeRole::NodeRoleThin);
        let (session2, _rx2) = make_session("node-e", NodeRole::NodeRoleFull);

        registry.register(session1).await;
        assert_eq!(registry.session_count().await, 1);

        // Reconnect with different role — should replace
        registry.register(session2).await;
        assert_eq!(registry.session_count().await, 1);

        // The thin_nodes list should be empty since the reconnect used FULL role
        let thin = registry.list_thin_nodes().await;
        assert!(thin.is_empty(), "Reconnect replaced thin session with full");
    }

    #[tokio::test]
    async fn test_concurrent_access() {
        use std::sync::Arc;
        let registry = Arc::new(WsRegistry::default());
        let mut handles = Vec::new();

        for i in 0..8usize {
            let reg = registry.clone();
            let handle = tokio::spawn(async move {
                let node_id = format!("node-{}", i);
                let (session, _rx) = make_session(&node_id, NodeRole::NodeRoleThin);
                reg.register(session).await;
                reg.is_connected(&node_id).await
            });
            handles.push(handle);
        }

        for handle in handles {
            let connected = handle.await.unwrap();
            assert!(connected);
        }

        assert_eq!(registry.session_count().await, 8);
    }
}
