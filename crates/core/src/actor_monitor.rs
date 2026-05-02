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

//! Monitor and link state management.
//!
//! This module provides:
//! - [`CTRL_MSG_PREFIX`] / [`is_ctrl_message`]: control-message convention (`__` prefix)
//! - [`MonitorLink`]: records a monitoring relationship (who receives `__DOWN__`)
//! - [`ActorMonitor`]: owns all monitor and link state; CRUD operations for both
//! - [`create_down_message`]: builds the `__DOWN__` mailbox message
//! - [`create_exit_message`]: builds the `__EXIT__` mailbox message
//! - [`create_ping_message`] / [`create_pong_message`]: liveness probe messages
//! - [`exit_reason_to_string`]: converts `ExitReason` to header string
//! - [`LocalOnlyActorService`]: no-op ActorService for tests / early startup
//! - [`start_monitor_gc_task`]: background GC that prunes stale remote monitor entries
//!
//! # Design: Erlang-style semantics, message-passing delivery
//!
//! Monitor and link follow **Erlang/OTP** ideas (one-way watch vs bidirectional
//! propagation on abnormal exit), but **all** notifications are ordinary **mailbox
//! messages** (`__DOWN__`, `__EXIT__`) delivered through the same path as other
//! actor traffic — there is no separate OS signal or out-of-band “signal plane”
//! as in some Erlang implementations.
//!
//! A **monitor** is a one-way, unidirectional watch.  When the watched actor
//! terminates for *any* reason (normal, error, killed), a single `__DOWN__`
//! message is delivered to the monitoring actor's mailbox.  There is no default
//! kill behaviour — the monitoring actor decides what to do.
//!
//! Each monitor stores the [`RequestContext`] from the **call that established the
//! monitor** (gRPC metadata + JWT via [`crate::request_context_from_grpc_request`], or
//! an explicit caller context). That context is replayed on outbound delivery so
//! tenant and namespace follow the same design as other cross-node actor traffic.
//!
//! A **link** is bidirectional: when either linked actor terminates with an
//! *error* exit, the other receives an `__EXIT__` message.  Normal / Shutdown
//! exits do **not** propagate over links (Erlang semantics).
//!
//! All state (monitors map, links map) lives here in [`ActorMonitor`].
//! `ActorRegistry` delegates monitor/link CRUD operations to an embedded
//! `ActorMonitor` instance and owns the notification dispatch logic.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

use crate::actor_context::ActorService;
use crate::{ActorId, ActorRef, RequestContext};
use plexspaces_proto::common::v1::Message;
use ulid::Ulid;

// ─── Control-message convention ──────────────────────────────────────────────

/// Message types whose names start with this prefix are **control messages**.
///
/// Control messages (`__DOWN__`, `__EXIT__`, `__PING__`, `__INFO__`, …) are
/// routed through the mailbox ctrl-queue, which is drained before the data
/// queue on every dequeue call.  This guarantees that lifecycle signals are
/// never buried behind application messages regardless of load.
pub const CTRL_MSG_PREFIX: &str = "__";

/// Returns `true` when `message_type` identifies a control message.
///
/// Inline so the hot-path check compiles to a single byte-comparison.
#[inline(always)]
pub fn is_ctrl_message(message_type: &str) -> bool {
    message_type.starts_with(CTRL_MSG_PREFIX)
}

// ─── Data types ──────────────────────────────────────────────────────────────

/// A single monitoring relationship.
///
/// Stored per monitored actor in [`ActorMonitor`].  When the monitored actor
/// terminates, a `__DOWN__` message is delivered to `monitoring_actor_id`.
#[derive(Clone, Debug)]
pub struct MonitorLink {
    /// Unique ULID for this monitor instance — used by `demonitor()` to remove
    /// a specific watch without affecting others on the same target.
    pub monitor_ref: String,
    /// Canonical ID of the actor that receives `__DOWN__` (may be remote).
    pub monitoring_actor_id: ActorId,
    /// Request scope from the monitor-establishing call (JWT tenant, request namespace, etc.).
    pub monitoring_context: RequestContext,
}

// ─── ActorMonitor ─────────────────────────────────────────────────────────────

/// Owns all monitor and link state for a node.
///
/// Create one instance per `ActorRegistry`; call the CRUD methods to manage
/// relationships.  The notification dispatch logic lives in `ActorRegistry`
/// which reads/mutates state via the public methods here.
pub struct ActorMonitor {
    /// target_actor_id → Vec of watchers
    monitors: Arc<RwLock<HashMap<ActorId, Vec<MonitorLink>>>>,
    /// actor_id → Vec of bidirectionally linked actor IDs
    links: Arc<RwLock<HashMap<ActorId, Vec<ActorId>>>>,
}

impl ActorMonitor {
    pub fn new() -> Self {
        Self {
            monitors: Arc::new(RwLock::new(HashMap::new())),
            links: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    // ── Monitor CRUD ──────────────────────────────────────────────────────────

    /// Register a one-way watch: `monitor_id` will receive `__DOWN__` when
    /// `target_id` terminates.
    pub async fn add_monitor(
        &self,
        target_id: &ActorId,
        monitor_id: &ActorId,
        monitor_ref: String,
        monitoring_context: RequestContext,
    ) {
        self.monitors
            .write()
            .await
            .entry(target_id.clone())
            .or_default()
            .push(MonitorLink {
                monitor_ref: monitor_ref.clone(),
                monitoring_actor_id: monitor_id.clone(),
                monitoring_context,
            });
        tracing::debug!(
            target = %target_id,
            monitor = %monitor_id,
            monitor_ref = %monitor_ref,
            "Registered monitor"
        );
    }

    /// Remove a specific monitor by its `monitor_ref`.
    pub async fn remove_monitor(&self, target_id: &ActorId, monitor_ref: &str) {
        let mut map = self.monitors.write().await;
        if let Some(links) = map.get_mut(target_id) {
            links.retain(|l| l.monitor_ref != monitor_ref);
            if links.is_empty() {
                map.remove(target_id);
            }
        }
    }

    /// Return all `MonitorLink`s for a target actor (for notification dispatch).
    pub async fn get_monitors(&self, actor_id: &ActorId) -> Vec<MonitorLink> {
        self.monitors
            .read()
            .await
            .get(actor_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Remove all monitor entries where `target_id` is in `actor_ids`.
    /// Used by the stale-monitor GC to prune entries for actors that no longer exist.
    pub async fn remove_monitors_for_actors(&self, actor_ids: &[ActorId]) {
        if actor_ids.is_empty() {
            return;
        }
        let stale_set: HashSet<&ActorId> = actor_ids.iter().collect();
        self.monitors
            .write()
            .await
            .retain(|id, _| !stale_set.contains(id));
    }

    /// Remove all monitor entries where the terminated actor is involved —
    /// both as the **target** being watched and as the **watcher** receiving DOWN messages.
    /// Called on actor termination to prevent stale entries from accumulating.
    pub async fn cleanup_monitors_for_actor(&self, actor_id: &ActorId) {
        let mut map = self.monitors.write().await;
        // Remove entry where this actor is the target (most common path)
        map.remove(actor_id);
        // Remove this actor as a watcher from all other targets' lists
        for watchers in map.values_mut() {
            watchers.retain(|link| &link.monitoring_actor_id != actor_id);
        }
        // Prune now-empty target entries
        map.retain(|_, watchers| !watchers.is_empty());
    }

    /// Return all monitor entries as `(target_id, monitor_ref, monitoring_actor_id)`.
    /// Used by the stale-monitor GC.
    pub async fn all_monitor_entries(&self) -> Vec<(ActorId, String, ActorId)> {
        let map = self.monitors.read().await;
        let mut out = Vec::new();
        for (target, links) in map.iter() {
            for link in links {
                out.push((
                    target.clone(),
                    link.monitor_ref.clone(),
                    link.monitoring_actor_id.clone(),
                ));
            }
        }
        out
    }

    // ── Link CRUD ─────────────────────────────────────────────────────────────

    /// Add a bidirectional link between two actors.
    pub async fn add_link(
        &self,
        actor1_id: &ActorId,
        actor2_id: &ActorId,
    ) -> Result<(), &'static str> {
        if actor1_id == actor2_id {
            return Err("Cannot link actor to itself");
        }
        let mut links = self.links.write().await;
        links
            .entry(actor1_id.clone())
            .or_default()
            .push(actor2_id.clone());
        links
            .entry(actor2_id.clone())
            .or_default()
            .push(actor1_id.clone());
        tracing::debug!(actor1 = %actor1_id, actor2 = %actor2_id, "Linked actors");
        Ok(())
    }

    /// Remove the bidirectional link between two actors.
    pub async fn remove_link(&self, actor1_id: &ActorId, actor2_id: &ActorId) {
        let mut links = self.links.write().await;
        if let Some(v) = links.get_mut(actor1_id) {
            v.retain(|id| id != actor2_id);
            if v.is_empty() {
                links.remove(actor1_id);
            }
        }
        if let Some(v) = links.get_mut(actor2_id) {
            v.retain(|id| id != actor1_id);
            if v.is_empty() {
                links.remove(actor2_id);
            }
        }
        tracing::debug!(actor1 = %actor1_id, actor2 = %actor2_id, "Unlinked actors");
    }

    /// Return all actors linked to `actor_id`.
    pub async fn get_links(&self, actor_id: &ActorId) -> Vec<ActorId> {
        self.links
            .read()
            .await
            .get(actor_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Remove all link entries for a terminated actor.
    pub async fn cleanup_links_for_actor(&self, actor_id: &ActorId) {
        let mut links = self.links.write().await;
        // Remove this actor from all peers' link lists
        for (other, peers) in links.iter_mut() {
            if other != actor_id {
                peers.retain(|id| id != actor_id);
            }
        }
        links.remove(actor_id);
    }
}

impl Default for ActorMonitor {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Message constructors ────────────────────────────────────────────────────

/// Build a `__DOWN__` message (Erlang-style `{'DOWN', Ref, process, Pid, Reason}`).
///
/// Delivered to the monitoring actor's mailbox when the monitored actor terminates.
///
/// | header        | value                               |
/// |---------------|-------------------------------------|
/// | `type`        | `"__DOWN__"`                        |
/// | `down_from`   | canonical ID of the terminated actor|
/// | `monitor_ref` | ULID from the original `monitor()`  |
/// | `down_reason` | exit-reason string                  |
///
/// The payload is a JSON object with the same fields for WASM/SDK actors:
/// `{"down_from": "...", "monitor_ref": "...", "down_reason": "..."}`
pub fn create_down_message(
    terminated_id: &ActorId,
    monitor_ref: &str,
    reason_str: &str,
) -> Message {
    let mut headers = HashMap::new();
    headers.insert("type".to_string(), "__DOWN__".to_string());
    headers.insert("down_from".to_string(), terminated_id.to_string());
    headers.insert("monitor_ref".to_string(), monitor_ref.to_string());
    headers.insert("down_reason".to_string(), reason_str.to_string());
    // JSON payload so WASM/SDK handlers (Python, Go, TypeScript, Rust WASM) can parse
    // the fields by name — headers are authoritative for native Rust actors.
    let payload = format!(
        r#"{{"down_from":{},"monitor_ref":{},"down_reason":{}}}"#,
        serde_json::to_string(&terminated_id.to_string()).unwrap_or_default(),
        serde_json::to_string(&monitor_ref).unwrap_or_default(),
        serde_json::to_string(&reason_str).unwrap_or_default(),
    );
    Message {
        id: Ulid::new().to_string(),
        sender_id: terminated_id.to_string(),
        message_type: "__DOWN__".to_string(),
        payload: payload.into_bytes(),
        headers,
        ..Default::default()
    }
}

/// Build an `__EXIT__` message for bidirectional link propagation.
///
/// | header        | value                              |
/// |---------------|------------------------------------|
/// | `type`        | `"__EXIT__"`                       |
/// | `exit_from`   | canonical ID of the exiting actor  |
/// | `exit_reason` | exit-reason string                 |
///
/// The payload is a JSON object with the same fields for WASM/SDK actors:
/// `{"exit_from": "...", "exit_reason": "..."}`
pub fn create_exit_message(from: String, reason_str: &str) -> Message {
    let mut headers = HashMap::new();
    headers.insert("type".to_string(), "__EXIT__".to_string());
    headers.insert("exit_from".to_string(), from.clone());
    headers.insert("exit_reason".to_string(), reason_str.to_string());
    // JSON payload so WASM/SDK handlers (Python, Go, TypeScript, Rust WASM) can parse
    // the fields by name — headers are authoritative for native Rust actors.
    let payload = format!(
        r#"{{"exit_from":{},"exit_reason":{}}}"#,
        serde_json::to_string(&from).unwrap_or_default(),
        serde_json::to_string(&reason_str).unwrap_or_default(),
    );
    Message {
        id: Ulid::new().to_string(),
        sender_id: from,
        message_type: "__EXIT__".to_string(),
        payload: payload.into_bytes(),
        headers,
        ..Default::default()
    }
}

/// Convert an [`crate::ExitReason`] to the canonical string form used in
/// `__DOWN__` / `__EXIT__` message headers.
pub fn exit_reason_to_string(reason: &crate::ExitReason) -> String {
    match reason {
        crate::ExitReason::Normal => "normal".to_string(),
        crate::ExitReason::Shutdown => "shutdown".to_string(),
        crate::ExitReason::Killed => "killed".to_string(),
        crate::ExitReason::Error(msg) => msg.clone(),
        crate::ExitReason::Linked {
            actor_id: linked_id,
            reason: linked_reason,
        } => {
            let inner = match linked_reason.as_ref() {
                crate::ExitReason::Normal => "normal",
                crate::ExitReason::Shutdown => "shutdown",
                crate::ExitReason::Killed => "killed",
                crate::ExitReason::Error(msg) => msg,
                crate::ExitReason::Linked { .. } => "linked",
            };
            format!("linked:{}:{}", linked_id, inner)
        }
    }
}

/// Build a `__PING__` control message directed at `target_id`.
///
/// The receiving actor's message loop automatically sends a `__PONG__` reply
/// (same `correlation_id`, sender/receiver swapped) without involving actor
/// code.  Callers use the `ask` pattern with a short timeout to measure
/// liveness.
///
/// | field            | value                  |
/// |------------------|------------------------|
/// | `message_type`   | `"__PING__"`           |
/// | `sender_id`      | `from_id`              |
/// | `receiver_id`    | `target_id`            |
/// | `correlation_id` | fresh ULID             |
pub fn create_ping_message(from_id: &str, target_id: &str) -> Message {
    let correlation_id = Ulid::new().to_string();
    let mut headers = HashMap::new();
    headers.insert("type".to_string(), "__PING__".to_string());
    Message {
        id: Ulid::new().to_string(),
        sender_id: from_id.to_string(),
        receiver_id: target_id.to_string(),
        message_type: "__PING__".to_string(),
        correlation_id: correlation_id.clone(),
        headers,
        ..Default::default()
    }
}

/// Build the `__PONG__` reply for a received `__PING__` message.
///
/// Mirrors `sender_id`/`receiver_id` and preserves `correlation_id` so the
/// original caller's `ask` future can match the reply.
pub fn create_pong_message(ping: &Message) -> Message {
    let mut headers = HashMap::new();
    headers.insert("type".to_string(), "__PONG__".to_string());
    Message {
        id: Ulid::new().to_string(),
        sender_id: ping.receiver_id.clone(),
        receiver_id: ping.sender_id.clone(),
        message_type: "__PONG__".to_string(),
        correlation_id: ping.correlation_id.clone(),
        headers,
        ..Default::default()
    }
}

// ─── LocalOnlyActorService ────────────────────────────────────────────────────

/// No-op `ActorService` used when no real service is available (tests, early startup).
///
/// All methods that require network access return an error with a clear message.
/// This allows `ActorRegistry` to hold a non-optional `Arc<dyn ActorService>` and
/// still be constructed in test contexts without a live gRPC service.
pub struct LocalOnlyActorService;

#[async_trait::async_trait]
impl ActorService for LocalOnlyActorService {
    async fn spawn_actor(
        &self,
        _ctx: &RequestContext,
        spec: &plexspaces_proto::actor::v1::ActorSpawnSpec,
    ) -> Result<ActorRef, Box<dyn std::error::Error + Send + Sync>> {
        let label = spec
            .identity
            .as_ref()
            .map(|i| i.name.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("(no name)");
        Err(format!(
            "LocalOnlyActorService: remote spawn not supported (actor '{}'). Inject a real ActorService.",
            label
        )
        .into())
    }

    async fn send(
        &self,
        _ctx: &RequestContext,
        actor_id: &str,
        _message: Message,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        Err(format!(
            "LocalOnlyActorService: remote send not supported (actor '{}'). Inject a real ActorService.",
            actor_id
        )
        .into())
    }
}

// ─── Stale monitor GC ────────────────────────────────────────────────────────

/// Spawn a background task that periodically removes monitor entries for actors
/// that no longer exist on remote nodes.
///
/// The task is fire-and-forget — it runs until the process exits.
///
/// # Algorithm
/// 1. Every `interval_secs` seconds, collect all monitor entries.
/// 2. Group monitored `target_id`s by their remote node_id (skip local actors).
/// 3. For each remote node, call `GetActorStates` RPC with that batch.
/// 4. Remove entries where the actor is not ACTIVE.
pub fn start_monitor_gc_task(
    monitor: Arc<ActorMonitor>,
    local_node_id: String,
    service_locator: Arc<dyn crate::ServiceLocator>,
    interval_secs: u64,
) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));
        loop {
            interval.tick().await;
            run_gc_pass(monitor.as_ref(), &local_node_id, service_locator.as_ref()).await;
        }
    });
}

async fn run_gc_pass(
    monitor: &ActorMonitor,
    local_node_id: &str,
    service_locator: &dyn crate::ServiceLocator,
) {
    use plexspaces_proto::v1::actor::{ActorState, GetActorStatesRequest};
    use plexspaces_proto::ActorServiceClient;

    let entries = monitor.all_monitor_entries().await;
    if entries.is_empty() {
        return;
    }

    // Group target actor IDs by their remote node_id.
    let mut by_node: HashMap<String, Vec<ActorId>> = HashMap::new();
    for (target_id, _monitor_ref, _monitoring_id) in &entries {
        if target_id.is_on_node(local_node_id) {
            continue; // local actors are pruned at termination
        }
        let node_id = target_id.node_id().to_string();
        by_node.entry(node_id).or_default().push(target_id.clone());
    }

    if by_node.is_empty() {
        return;
    }

    let node_registry = match service_locator.get_node_registry().await {
        Some(r) => r,
        None => {
            tracing::debug!("Monitor GC: NodeRegistry not available, skipping");
            return;
        }
    };
    let ctx = service_locator
        .request_context_for_system_operations()
        .await;

    for (node_id, actor_ids) in by_node {
        let node_address = match node_registry.lookup_node(&ctx, &node_id).await {
            Ok(Some(reg)) => reg.node_address,
            _ => {
                tracing::debug!(node_id = %node_id, "Monitor GC: node not found, skipping");
                continue;
            }
        };

        let channel = match tonic::transport::Channel::from_shared(node_address)
            .map_err(|e| e.to_string())
        {
            Ok(b) => match b.connect().await {
                Ok(ch) => ch,
                Err(e) => {
                    tracing::debug!(node_id = %node_id, error = %e, "Monitor GC: connect failed");
                    continue;
                }
            },
            Err(e) => {
                tracing::debug!(node_id = %node_id, error = %e, "Monitor GC: bad address");
                continue;
            }
        };

        let mut client = ActorServiceClient::new(channel);
        let resp = match client
            .get_actor_states(tonic::Request::new(GetActorStatesRequest {
                actor_ids: actor_ids.iter().map(|id| id.to_string()).collect(),
            }))
            .await
        {
            Ok(r) => r.into_inner(),
            Err(e) => {
                tracing::debug!(node_id = %node_id, error = %e, "Monitor GC: RPC failed");
                continue;
            }
        };

        let stale: Vec<ActorId> = actor_ids
            .into_iter()
            .filter(|id| {
                let raw = resp
                    .states
                    .get(&id.to_string())
                    .copied()
                    .unwrap_or(ActorState::ActorStateUnspecified as i32);
                raw != ActorState::ActorStateActive as i32
            })
            .collect();

        if !stale.is_empty() {
            tracing::info!(
                node_id = %node_id,
                stale_count = stale.len(),
                "Monitor GC: removing stale monitor entries"
            );
            monitor.remove_monitors_for_actors(&stale).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_ctrl_message_detects_double_underscore_prefix() {
        assert!(is_ctrl_message("__DOWN__"));
        assert!(is_ctrl_message("__EXIT__"));
        assert!(is_ctrl_message("__PING__"));
        assert!(is_ctrl_message("__PONG__"));
        assert!(is_ctrl_message("__INFO__"));
        assert!(is_ctrl_message("__CUSTOM__"));
    }

    #[test]
    fn test_is_ctrl_message_rejects_data_messages() {
        assert!(!is_ctrl_message("call"));
        assert!(!is_ctrl_message("cast"));
        assert!(!is_ctrl_message("timer"));
        assert!(!is_ctrl_message(""));
        assert!(!is_ctrl_message("_single_underscore"));
    }

    #[test]
    fn test_create_ping_message_fields() {
        let ping = create_ping_message("sender@node1", "target@node2");
        assert_eq!(ping.message_type, "__PING__");
        assert_eq!(ping.sender_id, "sender@node1");
        assert_eq!(ping.receiver_id, "target@node2");
        assert!(!ping.correlation_id.is_empty(), "correlation_id must be set");
        assert_eq!(ping.headers.get("type").map(String::as_str), Some("__PING__"));
    }

    #[test]
    fn test_create_pong_message_mirrors_ping() {
        let ping = create_ping_message("sender@node1", "target@node2");
        let pong = create_pong_message(&ping);

        assert_eq!(pong.message_type, "__PONG__");
        assert_eq!(pong.sender_id, ping.receiver_id);
        assert_eq!(pong.receiver_id, ping.sender_id);
        assert_eq!(pong.correlation_id, ping.correlation_id, "correlation_id must be preserved");
        assert_eq!(pong.headers.get("type").map(String::as_str), Some("__PONG__"));
    }

    #[test]
    fn test_ping_and_pong_are_ctrl_messages() {
        let ping = create_ping_message("a", "b");
        let pong = create_pong_message(&ping);
        assert!(is_ctrl_message(&ping.message_type));
        assert!(is_ctrl_message(&pong.message_type));
    }

    #[test]
    fn test_ctrl_msg_prefix_constant() {
        assert_eq!(CTRL_MSG_PREFIX, "__");
        assert!("__DOWN__".starts_with(CTRL_MSG_PREFIX));
    }
}
