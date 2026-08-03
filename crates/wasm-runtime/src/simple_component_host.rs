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

//! Actor-world PlexSpaces host function bindings for deployable polyglot WASM components.
//!
//! This module implements the `actor-world` WIT interface using protobuf bytes
//! and result-based errors at the guest/host boundary.
//!
//! ## KeyValue WIT compatibility
//! - WIT (plexspaces:actor actor-world): `host.kv-get(key) -> result<payload, actor-error>`,
//!   `host.kv-put(key, value) -> result<_, actor-error>`.
//! - Full plexspaces-actor WIT uses `keyvalue.get(ctx, key) -> result<option<payload>, actor-error>`.
//! - Simple host builds `RequestContext` with tenant_id="" and namespace=actor_id for key scoping.
//! - Node must pass keyvalue_store when instantiating; otherwise kv-get/kv-put return an actor error.
//!
//! See: archived_docs/python-wasm-component-guide.md for details

#[cfg(feature = "component-model")]
use crate::HostFunctions;
use plexspaces_actor::{
    ActorId, LockManager, RequestContext, RequestContextExt, TupleSpaceProvider,
};
use plexspaces_blob::BlobService;
use plexspaces_locks::{AcquireLockOptions, RenewLockOptions};
use plexspaces_proto::actor::v1::{
    AllReduceShardGroupRequest, BarrierShardGroupRequest, BroadcastShardGroupRequest,
    BulkUpdateShardGroupRequest, CreateShardGroupRequest, MapShardGroupRequest,
    ReduceShardGroupRequest, ScatterGatherRequest, SpawnActorsRequest,
};
use plexspaces_proto::application::v1::{ApplicationMetrics, GetApplicationStatusResponse};
use plexspaces_proto::locks::prv::Lock as ProtoLock;
use plexspaces_proto::pool::v1::{ActorHandle as PoolActorHandle, PoolMetrics as ProtoPoolMetrics};
use plexspaces_proto::tuplespace::v1::{
    ReadRequest, ReadResponse, Tuple as ProtoTuple, WriteRequest,
};
use plexspaces_proto::wasm::v1::{HttpFetchRequest, HttpFetchResponse};
use plexspaces_tuplespace::{
    proto_template_to_pattern, proto_tuple_to_tuple, tuple_to_proto_tuple,
};
use std::collections::HashMap;
use std::sync::Arc;

// Generate bindings from the unified `plexspaces:actor` WIT package.
// The bindgen macro generates types like ActorWorld for the "actor-world" world
#[cfg(feature = "component-model")]
wasmtime::component::bindgen!({
    world: "actor-world",
    path: "../../wit/plexspaces-actor",
    async: true,
});

// Note: The bindgen macro generates types directly in this module:
// - ActorWorld: Main instantiation type (available as ActorWorld in this module)
// - plexspaces::actor::*: Interface types
// ActorWorld is used via crate::simple_component_host::ActorWorld

/// Host implementation for `actor-world` WASM actors.
///
/// Implements the WIT `host` interface for `plexspaces:actor/actor-world`.
/// All host functions delegate to the framework's `HostFunctions` service gateway,
/// ensuring the WIT/SDK layer is a thin decorator over the core framework.
#[cfg(feature = "component-model")]
pub struct SimpleHostImpl {
    /// Actor ID of the component instance
    pub actor_id: ActorId,
    /// Tenant ID for this actor instance (from deployment context)
    pub tenant_id: String,
    /// Host functions implementation (gateway to framework services)
    pub host_functions: Arc<HostFunctions>,
    /// TupleSpace provider for ts_write/read/take
    pub tuplespace_provider: Option<Arc<dyn TupleSpaceProvider>>,
    /// Lock manager for distributed locks
    pub lock_manager: Option<Arc<dyn LockManager + Send + Sync>>,
    /// Blob service for object storage
    pub blob_service: Option<Arc<BlobService>>,
}

#[cfg(feature = "component-model")]
impl SimpleHostImpl {
    pub fn new(
        actor_id: ActorId,
        host_functions: Arc<HostFunctions>,
        tuplespace_provider: Option<Arc<dyn TupleSpaceProvider>>,
    ) -> Self {
        let lock_manager = host_functions.lock_manager().cloned();
        let blob_service = host_functions.blob_service().cloned();
        let tenant_id = host_functions.tenant_id.clone();
        Self {
            actor_id,
            tenant_id,
            host_functions,
            tuplespace_provider,
            lock_manager,
            blob_service,
        }
    }

    pub fn with_services(
        actor_id: ActorId,
        host_functions: Arc<HostFunctions>,
        tuplespace_provider: Option<Arc<dyn TupleSpaceProvider>>,
        lock_manager: Option<Arc<dyn LockManager + Send + Sync>>,
        blob_service: Option<Arc<BlobService>>,
    ) -> Self {
        let tenant_id = host_functions.tenant_id.clone();
        Self {
            actor_id,
            tenant_id,
            host_functions,
            tuplespace_provider,
            lock_manager,
            blob_service,
        }
    }

    /// Build a RequestContext for process group operations.
    /// The namespace comes directly from the validated structured actor ID.
    fn pg_context(&self) -> RequestContext {
        RequestContext::new_without_auth(
            self.tenant_id.clone(),
            self.actor_id.namespace().to_string(),
        )
    }

    /// Build a RequestContext from the actor's runtime tenant/namespace context.
    /// Used by host functions that previously accepted tenant-id/namespace as WIT params.
    fn make_context(&self) -> RequestContext {
        RequestContext::new_without_auth(
            self.tenant_id.clone(),
            self.actor_id.namespace().to_string(),
        )
    }

    fn decode_proto<M>(payload: &[u8], type_name: &str) -> Result<M, String>
    where
        M: prost::Message + Default,
    {
        M::decode(payload).map_err(|err| format!("invalid {} protobuf: {}", type_name, err))
    }

    fn encode_proto<M>(message: &M) -> Vec<u8>
    where
        M: prost::Message,
    {
        message.encode_to_vec()
    }

    fn decode_tuple_request(payload: &[u8]) -> Result<ProtoTuple, String> {
        let request = Self::decode_proto::<WriteRequest>(payload, "tuplespace WriteRequest")?;
        request
            .tuples
            .into_iter()
            .next()
            .ok_or_else(|| "invalid tuplespace WriteRequest: missing tuple".to_string())
    }

    fn decode_template_request(payload: &[u8]) -> Result<ProtoTuple, String> {
        Self::decode_proto::<ReadRequest>(payload, "tuplespace ReadRequest")?
            .template
            .ok_or_else(|| "invalid tuplespace ReadRequest: missing template".to_string())
    }

    fn encode_read_response(tuples: Vec<ProtoTuple>) -> Vec<u8> {
        Self::encode_proto(&ReadResponse {
            request_id: ulid::Ulid::new().to_string(),
            tuples,
            has_more: false,
        })
    }
}

/// Logging and time — host-logging interface
#[cfg(feature = "component-model")]
#[async_trait::async_trait]
impl plexspaces::actor::host_logging::Host for SimpleHostImpl {
    /// Log a message
    async fn log(&mut self, level: String, message: String) {
        metrics::counter!("plexspaces_wasm_simple_log_total").increment(1);

        match level.to_lowercase().as_str() {
            "trace" => {
                if tracing::enabled!(tracing::Level::TRACE) {
                    tracing::trace!(actor_id = %self.actor_id, "[WASM] {}", message);
                }
            }
            "debug" => {
                if tracing::enabled!(tracing::Level::DEBUG) {
                    tracing::debug!(actor_id = %self.actor_id, "[WASM] {}", message);
                }
            }
            "info" => tracing::info!(actor_id = %self.actor_id, "[WASM] {}", message),
            "warn" | "warning" => tracing::warn!(actor_id = %self.actor_id, "[WASM] {}", message),
            "error" => tracing::error!(actor_id = %self.actor_id, "[WASM] {}", message),
            _ => tracing::info!(actor_id = %self.actor_id, level = %level, "[WASM] {}", message),
        }
    }

    /// Get current timestamp in milliseconds
    async fn now_ms(&mut self) -> u64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }
}

/// Actor messaging, lifecycle, linking, timers, process groups — host-actor interface
#[cfg(feature = "component-model")]
#[async_trait::async_trait]
impl plexspaces::actor::host_actor::Host for SimpleHostImpl {
    /// Send a message to another actor (fire-and-forget at the WIT boundary).
    async fn send(&mut self, to: String, msg_type: String, payload: Vec<u8>) -> Result<(), String> {
        if self
            .host_functions
            .is_replaying
            .load(std::sync::atomic::Ordering::Acquire)
        {
            tracing::debug!(
                actor_id = %self.actor_id, to = %to, msg_type = %msg_type,
                "send: suppressed during journal replay"
            );
            return Ok(());
        }
        metrics::counter!("plexspaces_wasm_simple_send_total").increment(1);
        let self_id = self.actor_id.to_string();

        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!(
                actor_id = %self_id,
                to = %to,
                msg_type = %msg_type,
                payload_len = payload.len(),
                "actor-world send"
            );
        }

        let start = std::time::Instant::now();
        let result = self
            .host_functions
            .send_message(&self_id, &to, &msg_type, &payload)
            .await;
        metrics::histogram!("plexspaces_wasm_simple_send_duration_seconds")
            .record(start.elapsed().as_secs_f64());

        match result {
            Ok(()) => {
                metrics::counter!("plexspaces_wasm_simple_send_success_total").increment(1);
                Ok(())
            }
            Err(e) => {
                metrics::counter!("plexspaces_wasm_simple_send_errors_total").increment(1);
                tracing::warn!(
                    from = %self_id,
                    to = %to,
                    msg_type = %msg_type,
                    error = %e,
                    "actor-world send failed"
                );
                Err(e)
            }
        }
    }

    /// Send request and wait for response. Delegates to HostFunctions::ask().
    async fn ask(
        &mut self,
        to: String,
        msg_type: String,
        payload: Vec<u8>,
        timeout_ms: u64,
    ) -> Result<Vec<u8>, String> {
        if self
            .host_functions
            .is_replaying
            .load(std::sync::atomic::Ordering::Acquire)
        {
            tracing::debug!(
                actor_id = %self.actor_id, to = %to, msg_type = %msg_type,
                "ask: suppressed during journal replay"
            );
            return Ok(vec![]);
        }
        let self_id = self.actor_id.to_string();
        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!(
                actor_id = %self_id, to = %to, msg_type = %msg_type,
                timeout_ms = timeout_ms, "simple actor ask"
            );
        }
        match self
            .host_functions
            .ask(&self_id, &to, &msg_type, payload, timeout_ms)
            .await
        {
            Ok(response_bytes) => Ok(response_bytes),
            Err(e) => Err(e.to_string()),
        }
    }

    // ========================================================================
    // Actor Identity
    // ========================================================================

    /// Get own actor ID. Returns the ActorId assigned during actor registration.
    async fn self_id(&mut self) -> String {
        self.actor_id.to_string()
    }

    // ========================================================================
    // Actor Lifecycle: spawn, stop
    // ========================================================================

    /// Spawn a new actor. Delegates to HostFunctions::spawn_actor() which calls
    /// ActorServiceMessageSender → ActorService → ActorFactory::spawn_actor().
    ///
    /// role: discriminator used only when multiple actors share the same module_ref.
    ///       Pass empty string when the module_ref is unique.
    /// args: key-value init arguments forwarded to the new actor's Init() payload.
    ///
    /// Returns the canonical spawned actor ID on success (use this ID for subsequent Ask calls).
    async fn spawn(
        &mut self,
        module_ref: String,
        actor_name: String,
        role: String,
        args_json: String,
    ) -> Result<String, String> {
        metrics::counter!("plexspaces_wasm_spawn_total").increment(1);
        let self_id = self.actor_id.to_string();
        let requested_name = if actor_name.is_empty() {
            None
        } else {
            Some(actor_name.clone())
        };
        let args: Vec<(String, String)> = if args_json.is_empty() || args_json == "{}" {
            vec![]
        } else {
            match serde_json::from_str::<std::collections::HashMap<String, String>>(&args_json) {
                Ok(map) => map.into_iter().collect(),
                Err(_) => vec![],
            }
        };
        tracing::debug!(
            actor_id = %self_id, module_ref = %module_ref,
            actor_name = %actor_name, role = %role, "simple actor spawn"
        );
        match self
            .host_functions
            .spawn_actor(&self_id, &module_ref, role, args, requested_name)
            .await
        {
            Ok(spawned_id) => {
                metrics::counter!("plexspaces_wasm_spawn_success_total").increment(1);
                tracing::debug!(
                    actor_id = %self_id, spawned_id = %spawned_id,
                    "simple actor spawn success"
                );
                Ok(spawned_id)
            }
            Err(e) => {
                metrics::counter!("plexspaces_wasm_spawn_errors_total").increment(1);
                Err(e.to_string())
            }
        }
    }

    /// Stop an actor. Delegates to HostFunctions::stop_actor().
    async fn stop(&mut self, actor_id: String) -> Result<(), String> {
        let self_id = self.actor_id.to_string();
        match self
            .host_functions
            // 5s default: long enough for graceful shutdown, short enough to avoid hanging undeploy.
            .stop_actor(&self_id, &actor_id, 5000)
            .await
        {
            Ok(()) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }

    // ========================================================================
    // Actor Linking & Monitoring (Erlang/OTP patterns)
    // ========================================================================

    /// Bidirectional link
    async fn link(&mut self, actor_id: String) -> Result<(), String> {
        let self_id = self.actor_id.to_string();
        match self
            .host_functions
            .link_actor(&self_id, &self_id, &actor_id)
            .await
        {
            Ok(()) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }

    /// Remove bidirectional link
    async fn unlink(&mut self, actor_id: String) -> Result<(), String> {
        let self_id = self.actor_id.to_string();
        match self
            .host_functions
            .unlink_actor(&self_id, &self_id, &actor_id)
            .await
        {
            Ok(()) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }

    /// Unidirectional monitor — returns monitor reference as string
    async fn monitor(&mut self, actor_id: String) -> Result<String, String> {
        let self_id = self.actor_id.to_string();
        match self.host_functions.monitor_actor(&self_id, &actor_id).await {
            Ok(monitor_ref) => Ok(monitor_ref.to_string()),
            Err(e) => Err(e.to_string()),
        }
    }

    /// Cancel a monitor
    async fn demonitor(&mut self, monitor_ref: String) -> Result<(), String> {
        let self_id = self.actor_id.to_string();
        let ref_id: u64 = match monitor_ref.parse() {
            Ok(id) => id,
            Err(_) => return Err(format!("invalid monitor ref: {}", monitor_ref)),
        };
        match self
            .host_functions
            .demonitor_actor(&self_id, "", ref_id)
            .await
        {
            Ok(()) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }

    // ========================================================================
    // Timers (Delayed Messaging)
    // ========================================================================

    /// Send a message to self after a delay.
    /// Spawns a tracked background task that delivers the message after delay_ms.
    /// Returns a timer-id for observability. The JoinHandle is stored in host_functions.timer_handles
    /// so it can be joined/aborted when the actor stops (cleanup via drop or explicit stop).
    async fn send_after(
        &mut self,
        delay_ms: u64,
        msg_type: String,
        payload: Vec<u8>,
    ) -> Result<String, String> {
        // During journal replay, suppress timer scheduling to prevent reentrance traps.
        // The WASM component model forbids re-entering a component instance; timers that
        // fire during replay would attempt to deliver messages while the instance is busy.
        if self
            .host_functions
            .is_replaying
            .load(std::sync::atomic::Ordering::Acquire)
        {
            let suppressed_id = format!("timer-replay-suppressed-{}", ulid::Ulid::new());
            tracing::debug!(
                actor_id = %self.actor_id, timer_id = %suppressed_id,
                delay_ms = delay_ms, msg_type = %msg_type,
                "send_after: suppressed during journal replay"
            );
            return Ok(suppressed_id);
        }
        metrics::counter!("plexspaces_wasm_send_after_total").increment(1);
        let host_functions = self.host_functions.clone();
        let self_id = self.actor_id.to_string();
        let from = self_id.clone();

        // Generate unique timer ID using actor ID + ULID
        let timer_id = format!("timer-{}-{}", self_id, ulid::Ulid::new());
        let timer_id_for_task = timer_id.clone();
        let payload_bytes = payload.clone();

        tracing::debug!(
            actor_id = %self_id, timer_id = %timer_id,
            delay_ms = delay_ms, msg_type = %msg_type,
            "send_after: scheduling delayed message"
        );

        // Spawn tracked background task.
        // The handle is stored in host_functions.timer_handles (shared across re-instantiations)
        // so undeploy can bulk-cancel all pending timers for this actor.
        let timer_handles = self.host_functions.timer_handles.clone();
        let handle = tokio::task::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
            if let Err(e) = host_functions
                .send_message(&from, &from, &msg_type, &payload_bytes)
                .await
            {
                tracing::warn!(
                    actor_id = %from, timer_id = %timer_id_for_task,
                    error = %e, "send_after: delivery failed"
                );
                metrics::counter!("plexspaces_wasm_send_after_errors_total").increment(1);
            } else {
                tracing::debug!(
                    actor_id = %from, timer_id = %timer_id_for_task,
                    "send_after: message delivered"
                );
            }
        });

        if let Ok(mut handles) = timer_handles.lock() {
            handles.retain(|h| !h.is_finished());
            handles.push(handle);
        }

        Ok(timer_id)
    }

    // ========================================================================
    // Process Groups
    // ========================================================================

    /// Join a named process group (auto-creates the group if it doesn't exist, like Erlang pg:join/2)
    async fn pg_join(&mut self, group_name: String) -> Result<(), String> {
        let self_id = self.actor_id.clone();
        let ctx = self.pg_context();
        match self.host_functions.process_group_registry() {
            Some(registry) => {
                match registry
                    .join_group(&ctx, &group_name, &self_id, vec![])
                    .await
                {
                    Ok(()) => Ok(()),
                    Err(plexspaces_actor::process_groups::ProcessGroupError::GroupNotFound(_)) => {
                        // Auto-create group and retry join (Erlang pg semantics)
                        if let Err(e) = registry.create_group(&ctx, &group_name).await {
                            // Ignore GroupAlreadyExists (race condition with another actor)
                            if !matches!(
                                e,
                                plexspaces_actor::process_groups::ProcessGroupError::GroupAlreadyExists(_)
                            ) {
                                return Err(format!("Failed to create group: {}", e));
                            }
                        }
                        match registry
                            .join_group(&ctx, &group_name, &self_id, vec![])
                            .await
                        {
                            Ok(()) => Ok(()),
                            Err(e) => Err(e.to_string()),
                        }
                    }
                    Err(e) => Err(e.to_string()),
                }
            }
            None => Err("ProcessGroupRegistry not configured".to_string()),
        }
    }

    /// Leave a named process group
    async fn pg_leave(&mut self, group_name: String) -> Result<(), String> {
        let self_id = self.actor_id.clone();
        let ctx = self.pg_context();
        match self.host_functions.process_group_registry() {
            Some(registry) => match registry.leave_group(&ctx, &group_name, &self_id).await {
                Ok(()) => Ok(()),
                Err(e) => Err(e.to_string()),
            },
            None => Err("ProcessGroupRegistry not configured".to_string()),
        }
    }

    /// List members of a process group
    async fn pg_members(&mut self, group_name: String) -> Result<Vec<String>, String> {
        let ctx = self.pg_context();
        match self.host_functions.process_group_registry() {
            Some(registry) => match registry.get_members(&ctx, &group_name).await {
                Ok(members) => {
                    let ids: Vec<String> = members.iter().map(|m| m.to_string()).collect();
                    Ok(ids)
                }
                Err(e) => Err(e.to_string()),
            },
            None => Err("ProcessGroupRegistry not configured".to_string()),
        }
    }

    /// Broadcast message to all other members of a process group (sender excluded).
    /// Uses msg_type for routing; payload can be data-only.
    async fn pg_broadcast(
        &mut self,
        group_name: String,
        msg_type: String,
        payload: Vec<u8>,
    ) -> Result<(), String> {
        let self_id = self.actor_id.to_string();
        let ctx = self.pg_context();
        let registry = match self.host_functions.process_group_registry() {
            Some(r) => r,
            None => return Err("ProcessGroupRegistry not configured".to_string()),
        };
        let message_type = if msg_type.is_empty() {
            "cast".to_string()
        } else {
            msg_type
        };
        let members = registry
            .get_members(&ctx, &group_name)
            .await
            .map_err(|e| e.to_string())?;
        for member in &members {
            let member_str = member.to_string();
            if member_str == self_id {
                continue;
            }
            if let Err(e) = self
                .host_functions
                .send_message(&self_id, &member_str, &message_type, &payload)
                .await
            {
                tracing::warn!(
                    actor_id = %self_id,
                    group = %group_name,
                    target = %member_str,
                    error = %e,
                    "pg_broadcast: failed to send to member"
                );
            }
        }
        Ok(())
    }
}

/// Key-value storage and durable alarms — host-kv interface
#[cfg(feature = "component-model")]
#[async_trait::async_trait]
impl plexspaces::actor::host_kv::Host for SimpleHostImpl {
    /// Key-value get (string-only). Returns value or empty if not found.
    /// WIT: actor-world host.kv-get(key) -> string. Context uses tenant_id="", namespace=actor_id for key scoping.
    async fn kv_get(&mut self, key: String) -> Result<Vec<u8>, String> {
        if tracing::enabled!(tracing::Level::TRACE) {
            tracing::trace!(actor_id = %self.actor_id, key = %key, "wasm kv_get entry");
        }
        let ctx = RequestContext::new_without_auth(String::new(), self.actor_id.to_string());
        match self.host_functions.get_keyvalue(&ctx, &key).await {
            Ok(Some(bytes)) => {
                if tracing::enabled!(tracing::Level::TRACE) {
                    tracing::trace!(actor_id = %self.actor_id, key = %key, value_len = bytes.len(), "wasm kv_get ok");
                }
                Ok(bytes)
            }
            Ok(None) => {
                if tracing::enabled!(tracing::Level::TRACE) {
                    tracing::trace!(actor_id = %self.actor_id, key = %key, "wasm kv_get none");
                }
                Ok(Vec::new())
            }
            Err(e) => {
                tracing::warn!(actor_id = %self.actor_id, key = %key, error = %e, "wasm kv_get failed");
                Err(e.to_string())
            }
        }
    }

    /// Key-value put (string-only). Returns empty on success.
    /// Values are stored as UTF-8 bytes so kv_store remains human-readable for actor keys
    /// (object-registry uses the same table with protobuf for its entries).
    async fn kv_put(&mut self, key: String, value: Vec<u8>) -> Result<(), String> {
        if tracing::enabled!(tracing::Level::TRACE) {
            tracing::trace!(actor_id = %self.actor_id, key = %key, value_len = value.len(), "wasm kv_put entry");
        }
        let ctx = RequestContext::new_without_auth(String::new(), self.actor_id.to_string());
        match self.host_functions.put_keyvalue(&ctx, &key, value).await {
            Ok(()) => {
                if tracing::enabled!(tracing::Level::TRACE) {
                    tracing::trace!(actor_id = %self.actor_id, key = %key, "wasm kv_put ok");
                }
                Ok(())
            }
            Err(e) => {
                tracing::warn!(actor_id = %self.actor_id, key = %key, error = %e, "wasm kv_put failed");
                Err(e.to_string())
            }
        }
    }

    // ========================================================================
    // Extended Key-Value Operations
    // ========================================================================

    /// Key-value delete
    async fn kv_delete(&mut self, key: String) -> Result<(), String> {
        let ctx = RequestContext::new_without_auth(String::new(), self.actor_id.to_string());
        match self.host_functions.delete_keyvalue(&ctx, &key).await {
            Ok(()) => Ok(()),
            Err(e) => {
                tracing::warn!(actor_id = %self.actor_id, key = %key, error = %e, "wasm kv_delete failed");
                Err(e.to_string())
            }
        }
    }

    /// Key-value list keys with prefix
    async fn kv_list(&mut self, prefix: String) -> Result<Vec<String>, String> {
        let ctx = RequestContext::new_without_auth(String::new(), self.actor_id.to_string());
        match self.host_functions.list_keyvalue(&ctx, &prefix).await {
            Ok(keys) => Ok(keys),
            Err(e) => {
                tracing::warn!(actor_id = %self.actor_id, prefix = %prefix, error = %e, "wasm kv_list failed");
                Err(e.to_string())
            }
        }
    }

    async fn kv_put_with_ttl(
        &mut self,
        key: String,
        value: Vec<u8>,
        ttl_seconds: u64,
    ) -> Result<(), String> {
        let ctx = RequestContext::new_without_auth(String::new(), self.actor_id.to_string());
        self.host_functions.put_keyvalue_with_ttl(&ctx, &key, value, ttl_seconds).await
            .map_err(|e| { tracing::warn!(actor_id = %self.actor_id, key = %key, error = %e, "wasm kv_put_with_ttl failed"); e })
    }

    async fn kv_get_ttl(&mut self, key: String) -> Result<u64, String> {
        let ctx = RequestContext::new_without_auth(String::new(), self.actor_id.to_string());
        self.host_functions.get_keyvalue_ttl(&ctx, &key).await
            .map_err(|e| { tracing::warn!(actor_id = %self.actor_id, key = %key, error = %e, "wasm kv_get_ttl failed"); e })
    }

    /// Compare-and-swap: empty expected bytes means "key must not exist".
    async fn kv_cas(
        &mut self,
        key: String,
        expected: Vec<u8>,
        new_value: Vec<u8>,
    ) -> Result<bool, String> {
        let ctx = RequestContext::new_without_auth(String::new(), self.actor_id.to_string());
        let expected_opt = if expected.is_empty() {
            None
        } else {
            Some(expected)
        };
        self.host_functions.cas_keyvalue(&ctx, &key, expected_opt, new_value).await
            .map_err(|e| { tracing::warn!(actor_id = %self.actor_id, key = %key, error = %e, "wasm kv_cas failed"); e })
    }

    async fn kv_increment(&mut self, key: String, delta: i64) -> Result<i64, String> {
        let ctx = RequestContext::new_without_auth(String::new(), self.actor_id.to_string());
        self.host_functions.increment_keyvalue(&ctx, &key, delta).await
            .map_err(|e| { tracing::warn!(actor_id = %self.actor_id, key = %key, error = %e, "wasm kv_increment failed"); e })
    }

    /// Batch read. keys-json payload is a JSON array of key strings.
    /// Returns a JSON array of base64-encoded values in the same order; null for missing keys.
    async fn kv_multi_get(&mut self, keys_json: Vec<u8>) -> Result<Vec<u8>, String> {
        let ctx = RequestContext::new_without_auth(String::new(), self.actor_id.to_string());
        let keys: Vec<String> = serde_json::from_slice(&keys_json)
            .map_err(|e| format!("kv_multi_get: invalid keys JSON: {}", e))?;
        let key_refs: Vec<&str> = keys.iter().map(String::as_str).collect();
        let values = self
            .host_functions
            .multi_get_keyvalue(&ctx, &key_refs)
            .await
            .map_err(|e| {
                tracing::warn!(actor_id = %self.actor_id, error = %e, "wasm kv_multi_get failed");
                e
            })?;
        let result: Vec<Option<String>> = values
            .into_iter()
            .map(|v| {
                v.map(|b| base64::Engine::encode(&base64::engine::general_purpose::STANDARD, b))
            })
            .collect();
        serde_json::to_vec(&result)
            .map_err(|e| format!("kv_multi_get: response serialize failed: {}", e))
    }

    /// Batch write. entries-json payload is a JSON object mapping key strings to base64-encoded values.
    async fn kv_multi_put(&mut self, entries_json: Vec<u8>) -> Result<(), String> {
        let ctx = RequestContext::new_without_auth(String::new(), self.actor_id.to_string());
        let map: std::collections::HashMap<String, String> = serde_json::from_slice(&entries_json)
            .map_err(|e| format!("kv_multi_put: invalid entries JSON: {}", e))?;
        let pairs: Vec<(String, Vec<u8>)> = map
            .into_iter()
            .map(|(k, v64)| {
                let k_err = k.clone();
                base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &v64)
                    .map(|v| (k, v))
                    .map_err(|e| format!("kv_multi_put: base64 decode for key {:?}: {}", k_err, e))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let pair_refs: Vec<(&str, Vec<u8>)> =
            pairs.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
        self.host_functions
            .multi_put_keyvalue(&ctx, &pair_refs)
            .await
            .map_err(|e| {
                tracing::warn!(actor_id = %self.actor_id, error = %e, "wasm kv_multi_put failed");
                e
            })
    }

    // ========================================================================
    // Durable Alarms (Cloudflare DO setAlarm equivalent)
    //
    // Alarms are built on top of the existing two-layer mechanism:
    // 1. JournalStorage::register_reminder (same storage used by ReminderFacet) for
    //    persistence across actor deactivation/restart.
    // 2. send_after (same in-process timer used by host.send-after) for immediate
    //    scheduling within the current process lifetime.
    //
    // On actor re-activation the application layer re-arms the send_after from storage.
    // WASM actors receive "__alarm__" as the message type (not "ReminderFired") so the
    // SDK alarm handler is a simple case in the actor's handle() switch/dispatcher.
    // ========================================================================

    async fn alarm_set(&mut self, timestamp_ms: u64) -> Result<(), String> {
        let actor_id = self.actor_id.to_string();
        // Persist to JournalStorage so the alarm survives actor deactivation/restart.
        self.host_functions
            .alarm_set(&actor_id, timestamp_ms)
            .await?;
        // Also schedule an in-process send_after so the actor fires without waiting for
        // a scanner (reuses the existing send_after infrastructure, no duplication).
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let delay_ms = timestamp_ms.saturating_sub(now_ms);
        let host_functions = self.host_functions.clone();
        let hf_alarm_id = actor_id.clone();
        let timer_handles = self.host_functions.timer_handles.clone();
        let handle = tokio::task::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
            // CAS-delete: atomically remove the alarm only if it still matches our timestamp.
            // Returns false if alarm was superseded by a newer alarm_set call.
            match host_functions
                .alarm_delete_if_matches(&hf_alarm_id, timestamp_ms)
                .await
            {
                Ok(true) => { /* we own this alarm, proceed with delivery */ }
                Ok(false) => {
                    tracing::debug!(actor_id = %hf_alarm_id, timestamp_ms, "alarm superseded, skipping delivery");
                    return;
                }
                Err(e) => {
                    tracing::warn!(actor_id = %hf_alarm_id, error = %e, "alarm_delete_if_matches failed, skipping delivery");
                    return;
                }
            }
            if let Err(e) = host_functions
                .send_message(&hf_alarm_id, &hf_alarm_id, "__alarm__", &[])
                .await
            {
                tracing::warn!(actor_id = %hf_alarm_id, error = %e, "alarm delivery failed");
            }
        });
        if let Ok(mut handles) = timer_handles.lock() {
            handles.retain(|h| !h.is_finished());
            handles.push(handle);
        }
        Ok(())
    }

    async fn alarm_get(&mut self) -> Result<u64, String> {
        self.host_functions.alarm_get(self.actor_id.as_ref()).await
    }

    async fn alarm_delete(&mut self) -> Result<(), String> {
        // Remove from JournalStorage. The in-flight send_after task checks alarm_get before
        // firing, so the alarm will not be delivered after this call.
        self.host_functions
            .alarm_delete(self.actor_id.as_ref())
            .await
    }
}

/// TupleSpace — host-ts interface
#[cfg(feature = "component-model")]
#[async_trait::async_trait]
impl plexspaces::actor::host_ts::Host for SimpleHostImpl {
    /// TupleSpace write using protobuf `WriteRequest` bytes.
    async fn ts_write(&mut self, tuple_data: Vec<u8>) -> Result<(), String> {
        let provider = match &self.tuplespace_provider {
            Some(p) => p,
            None => {
                tracing::warn!(actor_id = %self.actor_id, "ts_write: TupleSpaceProvider not available");
                return Err("TupleSpaceProvider not available".to_string());
            }
        };
        let proto_tuple = Self::decode_tuple_request(&tuple_data)?;
        let tuple = proto_tuple_to_tuple(&proto_tuple).map_err(|err| err.to_string())?;
        match provider.write(tuple).await {
            Ok(()) => {
                if tracing::enabled!(tracing::Level::TRACE) {
                    tracing::trace!(actor_id = %self.actor_id, "wasm ts_write ok");
                }
                Ok(())
            }
            Err(e) => {
                tracing::warn!(actor_id = %self.actor_id, error = %e, "wasm ts_write failed");
                Err(e.to_string())
            }
        }
    }

    // ========================================================================
    // Extended TupleSpace Operations
    // ========================================================================

    /// TupleSpace read (non-destructive) using protobuf `ReadRequest` bytes.
    async fn ts_read(&mut self, pattern_data: Vec<u8>) -> Result<Vec<u8>, String> {
        let provider = match &self.tuplespace_provider {
            Some(p) => p,
            None => {
                tracing::warn!(actor_id = %self.actor_id, "ts_read: TupleSpaceProvider not available");
                return Err("TupleSpaceProvider not available".to_string());
            }
        };
        let proto_template = Self::decode_template_request(&pattern_data)?;
        let pattern = proto_template_to_pattern(&proto_template).map_err(|err| err.to_string())?;
        match provider.read(&pattern).await {
            Ok(tuples) => {
                if let Some(tuple) = tuples.first() {
                    Ok(Self::encode_read_response(vec![tuple_to_proto_tuple(
                        tuple,
                    )]))
                } else {
                    Ok(Self::encode_read_response(Vec::new()))
                }
            }
            Err(e) => {
                tracing::warn!(actor_id = %self.actor_id, error = %e, "wasm ts_read failed");
                Err(e.to_string())
            }
        }
    }

    /// TupleSpace take (destructive read) using protobuf `ReadRequest` bytes.
    async fn ts_take(&mut self, pattern_data: Vec<u8>) -> Result<Vec<u8>, String> {
        let provider = match &self.tuplespace_provider {
            Some(p) => p,
            None => {
                tracing::warn!(actor_id = %self.actor_id, "ts_take: TupleSpaceProvider not available");
                return Err("TupleSpaceProvider not available".to_string());
            }
        };
        let proto_template = Self::decode_template_request(&pattern_data)?;
        let pattern = proto_template_to_pattern(&proto_template).map_err(|err| err.to_string())?;
        match provider.take(&pattern).await {
            Ok(Some(tuple)) => Ok(Self::encode_read_response(vec![tuple_to_proto_tuple(
                &tuple,
            )])),
            Ok(None) => Ok(Self::encode_read_response(Vec::new())),
            Err(e) => {
                tracing::warn!(actor_id = %self.actor_id, error = %e, "wasm ts_take failed");
                Err(e.to_string())
            }
        }
    }

    /// TupleSpace read-all matching tuples (non-destructive) using protobuf `ReadRequest` bytes.
    async fn ts_read_all(&mut self, pattern_data: Vec<u8>) -> Result<Vec<u8>, String> {
        let provider = match &self.tuplespace_provider {
            Some(p) => p,
            None => {
                tracing::warn!(actor_id = %self.actor_id, "ts_read_all: TupleSpaceProvider not available");
                return Err("TupleSpaceProvider not available".to_string());
            }
        };
        let proto_template = Self::decode_template_request(&pattern_data)?;
        let pattern = proto_template_to_pattern(&proto_template).map_err(|err| err.to_string())?;
        match provider.read(&pattern).await {
            Ok(tuples) => Ok(Self::encode_read_response(
                tuples.iter().map(tuple_to_proto_tuple).collect(),
            )),
            Err(e) => {
                tracing::warn!(actor_id = %self.actor_id, error = %e, "wasm ts_read_all failed");
                Err(e.to_string())
            }
        }
    }
}

/// Distributed locks — host-locks interface
#[cfg(feature = "component-model")]
#[async_trait::async_trait]
impl plexspaces::actor::host_locks::Host for SimpleHostImpl {
    // ========================================================================
    // Distributed Lock Operations
    // ========================================================================
    // API requires tenant-id, namespace, holder-id for all operations (per WIT).

    /// Acquire a distributed lock and return protobuf-encoded `plexspaces.locks.prv.Lock`.
    async fn lock_acquire(
        &mut self,
        holder_id: String,
        lock_name: String,
        lease_duration_secs: u32,
        timeout_ms: u64,
    ) -> Result<Vec<u8>, String> {
        let lock_manager = match &self.lock_manager {
            Some(lm) => lm,
            None => {
                tracing::warn!(actor_id = %self.actor_id, "lock_acquire: LockManager not available");
                return Err("LockManager not available".to_string());
            }
        };
        metrics::counter!("plexspaces_wasm_lock_ops_total", "op" => "acquire").increment(1);
        let lease_secs = if lease_duration_secs == 0 {
            30
        } else {
            lease_duration_secs
        };
        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!(
                actor_id = %self.actor_id,
                lock_name = %lock_name,
                holder_id = %holder_id,
                "lock_acquire attempt"
            );
        }
        let ctx = self.make_context();
        let options = AcquireLockOptions {
            lock_key: lock_name.clone(),
            holder_id: holder_id.clone(),
            lease_duration_secs: lease_secs,
            additional_wait_time_ms: u32::try_from(timeout_ms).unwrap_or(u32::MAX),
            refresh_period_ms: 100,
            metadata: HashMap::new(),
        };
        match lock_manager.acquire_lock(&ctx, options).await {
            Ok(lock) => {
                tracing::debug!(
                    actor_id = %self.actor_id,
                    lock_name = %lock_name,
                    version = %lock.version,
                    "lock_acquire success"
                );
                Ok(Self::encode_proto(&ProtoLock {
                    lock_key: lock.lock_key,
                    holder_id: lock.holder_id,
                    version: lock.version,
                    expires_at: lock.expires_at,
                    lease_duration_secs: lock.lease_duration_secs,
                    last_heartbeat: lock.last_heartbeat,
                    metadata: lock.metadata,
                    locked: lock.locked,
                }))
            }
            Err(e) => {
                tracing::debug!(
                    actor_id = %self.actor_id,
                    lock_name = %lock_name,
                    error = %e,
                    "lock_acquire failed"
                );
                Err(e.to_string())
            }
        }
    }

    /// Release a distributed lock. Requires lock-id, holder-id, lock-version.
    async fn lock_release(
        &mut self,
        lock_id: String,
        holder_id: String,
        lock_version: String,
    ) -> Result<(), String> {
        let lock_manager = match &self.lock_manager {
            Some(lm) => lm,
            None => {
                tracing::warn!(actor_id = %self.actor_id, "lock_release: LockManager not available");
                return Err("LockManager not available".to_string());
            }
        };
        metrics::counter!("plexspaces_wasm_lock_ops_total", "op" => "release").increment(1);
        let ctx = self.make_context();
        let options = plexspaces_locks::ReleaseLockOptions {
            lock_key: lock_id,
            holder_id,
            version: lock_version,
            delete_lock: false,
        };
        match lock_manager.release_lock(&ctx, options).await {
            Ok(()) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }

    /// Renew lease on a held lock and return protobuf-encoded `plexspaces.locks.prv.Lock`.
    async fn lock_renew(
        &mut self,
        lock_id: String,
        holder_id: String,
        lock_version: String,
        lease_duration_secs: u32,
    ) -> Result<Vec<u8>, String> {
        let lock_manager = match &self.lock_manager {
            Some(lm) => lm,
            None => {
                tracing::warn!(actor_id = %self.actor_id, "lock_renew: LockManager not available");
                return Err("LockManager not available".to_string());
            }
        };
        metrics::counter!("plexspaces_wasm_lock_ops_total", "op" => "renew").increment(1);
        let ctx = self.make_context();
        let options = RenewLockOptions {
            lock_key: lock_id.clone(),
            holder_id,
            version: lock_version.clone(),
            lease_duration_secs,
            metadata: HashMap::new(),
        };
        match lock_manager.renew_lock(&ctx, options).await {
            Ok(renewed) => {
                tracing::debug!(
                    actor_id = %self.actor_id,
                    lock_id = %lock_id,
                    new_version = %renewed.version,
                    "lock_renew success"
                );
                Ok(Self::encode_proto(&ProtoLock {
                    lock_key: renewed.lock_key,
                    holder_id: renewed.holder_id,
                    version: renewed.version,
                    expires_at: renewed.expires_at,
                    lease_duration_secs: renewed.lease_duration_secs,
                    last_heartbeat: renewed.last_heartbeat,
                    metadata: renewed.metadata,
                    locked: renewed.locked,
                }))
            }
            Err(e) => {
                tracing::debug!(
                    actor_id = %self.actor_id,
                    lock_id = %lock_id,
                    error = %e,
                    "lock_renew failed"
                );
                Err(e.to_string())
            }
        }
    }
}

/// Blob storage — host-blob interface
#[cfg(feature = "component-model")]
#[async_trait::async_trait]
impl plexspaces::actor::host_blob::Host for SimpleHostImpl {
    // ========================================================================
    // Blob Storage Operations
    // ========================================================================

    /// Upload blob data (base64-encoded).
    async fn blob_upload(
        &mut self,
        blob_id: String,
        data: Vec<u8>,
        content_type: String,
    ) -> Result<String, String> {
        let blob_service = match &self.blob_service {
            Some(bs) => bs,
            None => {
                tracing::error!(actor_id = %self.actor_id, "blob_upload: BlobService not available");
                return Err("BlobService not available".to_string());
            }
        };
        metrics::counter!("plexspaces_wasm_blob_ops_total", "op" => "upload").increment(1);
        let ctx = RequestContext::new_without_auth(String::new(), self.actor_id.to_string());
        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!(actor_id = %self.actor_id, name = %blob_id, data_len = data.len(), content_type = %content_type, "blob_upload: starting");
        }
        match blob_service
            .upload_blob(
                &ctx,
                plexspaces_blob::UploadBlobParams {
                    name: blob_id.clone(),
                    data,
                    content_type: Some(content_type.clone()),
                    ..Default::default()
                },
            )
            .await
        {
            Ok(metadata) => {
                if tracing::enabled!(tracing::Level::DEBUG) {
                    tracing::debug!(actor_id = %self.actor_id, name = %blob_id, internal_blob_id = %metadata.blob_id, "blob_upload: success");
                }
                Ok(metadata.blob_id)
            }
            Err(e) => {
                tracing::warn!(actor_id = %self.actor_id, name = %blob_id, error = %e, "blob_upload: failed");
                Err(e.to_string())
            }
        }
    }

    /// Download blob data (returns base64-encoded).
    /// Supports both name (path) and blob_id (ULID) lookup.
    /// First tries by name, then falls back to by ID.
    async fn blob_download(&mut self, blob_id: String) -> Result<Vec<u8>, String> {
        let blob_service = match &self.blob_service {
            Some(bs) => bs,
            None => {
                tracing::error!(actor_id = %self.actor_id, "blob_download: BlobService not available");
                return Err("BlobService not available".to_string());
            }
        };
        metrics::counter!("plexspaces_wasm_blob_ops_total", "op" => "download").increment(1);
        let ctx = RequestContext::new_without_auth(String::new(), self.actor_id.to_string());
        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!(actor_id = %self.actor_id, blob_id = %blob_id, "blob_download: starting");
        }

        // First try by name (path) - common pattern for WASM actors
        match blob_service.download_blob_by_name(&ctx, &blob_id).await {
            Ok(data) => {
                if tracing::enabled!(tracing::Level::DEBUG) {
                    tracing::debug!(actor_id = %self.actor_id, blob_id = %blob_id, data_len = data.len(), "blob_download: success (by name)");
                }
                return Ok(data);
            }
            Err(plexspaces_blob::BlobError::NotFound(_)) => {
                // Name not found, try by ID
                if tracing::enabled!(tracing::Level::DEBUG) {
                    tracing::debug!(actor_id = %self.actor_id, blob_id = %blob_id, "blob_download: not found by name, trying by ID");
                }
            }
            Err(e) => {
                tracing::warn!(actor_id = %self.actor_id, blob_id = %blob_id, error = %e, "blob_download: name lookup failed");
                // Continue to try by ID
            }
        }

        // Fall back to by ID (ULID) - for callers who have the internal blob_id
        match blob_service.download_blob(&ctx, &blob_id).await {
            Ok(data) => {
                if tracing::enabled!(tracing::Level::DEBUG) {
                    tracing::debug!(actor_id = %self.actor_id, blob_id = %blob_id, data_len = data.len(), "blob_download: success (by ID)");
                }
                Ok(data)
            }
            Err(plexspaces_blob::BlobError::NotFound(_)) => {
                tracing::debug!(actor_id = %self.actor_id, blob_id = %blob_id, "blob_download: not found");
                Err(format!("Blob not found: {}", blob_id))
            }
            Err(e) => {
                tracing::warn!(actor_id = %self.actor_id, blob_id = %blob_id, error = %e, "blob_download: failed");
                Err(e.to_string())
            }
        }
    }

    /// Delete blob.
    /// Supports both name (path) and blob_id (ULID) lookup.
    /// First tries by name, then falls back to by ID.
    async fn blob_delete(&mut self, blob_id: String) -> Result<(), String> {
        let blob_service = match &self.blob_service {
            Some(bs) => bs,
            None => {
                tracing::error!(actor_id = %self.actor_id, "blob_delete: BlobService not available");
                return Err("BlobService not available".to_string());
            }
        };
        metrics::counter!("plexspaces_wasm_blob_ops_total", "op" => "delete").increment(1);
        let ctx = RequestContext::new_without_auth(String::new(), self.actor_id.to_string());
        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!(actor_id = %self.actor_id, blob_id = %blob_id, "blob_delete: starting");
        }

        // First try by name (path) - common pattern for WASM actors
        match blob_service.delete_blob_by_name(&ctx, &blob_id).await {
            Ok(()) => {
                if tracing::enabled!(tracing::Level::DEBUG) {
                    tracing::debug!(actor_id = %self.actor_id, blob_id = %blob_id, "blob_delete: success (by name)");
                }
                return Ok(());
            }
            Err(plexspaces_blob::BlobError::NotFound(_)) => {
                // Name not found, try by ID
                if tracing::enabled!(tracing::Level::DEBUG) {
                    tracing::debug!(actor_id = %self.actor_id, blob_id = %blob_id, "blob_delete: not found by name, trying by ID");
                }
            }
            Err(e) => {
                tracing::warn!(actor_id = %self.actor_id, blob_id = %blob_id, error = %e, "blob_delete: name lookup failed");
                // Continue to try by ID
            }
        }

        // Fall back to by ID (ULID) - for callers who have the internal blob_id
        match blob_service.delete_blob(&ctx, &blob_id).await {
            Ok(()) => {
                if tracing::enabled!(tracing::Level::DEBUG) {
                    tracing::debug!(actor_id = %self.actor_id, blob_id = %blob_id, "blob_delete: success (by ID)");
                }
                Ok(())
            }
            Err(plexspaces_blob::BlobError::NotFound(_)) => {
                Err(format!("Blob not found: {}", blob_id))
            }
            Err(e) => Err(e.to_string()),
        }
    }

    /// List blobs with prefix.
    /// Returns blob names (paths) since WASM actors use paths as identifiers.
    async fn blob_list(&mut self, prefix: String) -> Result<Vec<String>, String> {
        let blob_service = match &self.blob_service {
            Some(bs) => bs,
            None => {
                tracing::error!(actor_id = %self.actor_id, "blob_list: BlobService not available");
                return Err("BlobService not available".to_string());
            }
        };
        let ctx = RequestContext::new_without_auth(String::new(), self.actor_id.to_string());
        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!(actor_id = %self.actor_id, prefix = %prefix, "blob_list: starting");
        }
        let filters = plexspaces_blob::repository::ListFilters {
            name_prefix: Some(prefix.clone()),
            ..Default::default()
        };
        match blob_service.list_blobs(&ctx, &filters, 100, 1).await {
            Ok((blobs, _total)) => {
                // Return names (paths) instead of blob_ids since WASM actors use paths
                let names: Vec<String> = blobs.iter().map(|b| b.name.clone()).collect();
                if tracing::enabled!(tracing::Level::DEBUG) {
                    tracing::debug!(actor_id = %self.actor_id, prefix = %prefix, count = names.len(), "blob_list: success");
                }
                Ok(names)
            }
            Err(e) => {
                tracing::warn!(actor_id = %self.actor_id, prefix = %prefix, error = %e, "blob_list: failed");
                Err(e.to_string())
            }
        }
    }
}

/// Elastic pool — host-pool interface
#[cfg(feature = "component-model")]
#[async_trait::async_trait]
impl plexspaces::actor::host_pool::Host for SimpleHostImpl {
    // ========================================================================
    // Elastic pool (checkout/checkin)
    // ========================================================================

    async fn pool_checkout(
        &mut self,
        pool_name: String,
        timeout_ms: u64,
    ) -> Result<Vec<u8>, String> {
        let svc = match self.host_functions.elastic_pool_service() {
            Some(s) => s.clone(),
            None => return Err("Elastic pool service not configured".to_string()),
        };
        let timeout = std::time::Duration::from_millis(timeout_ms);
        match svc.checkout(&pool_name, timeout).await {
            Ok(handle) => Ok(Self::encode_proto(&PoolActorHandle {
                actor_id: handle.actor_id,
                pool_name: handle.pool_name,
                checkout_time: handle.checkout_time,
                checkout_id: handle.checkout_id,
                metadata: handle.metadata,
            })),
            Err(e) => Err(e.to_string()),
        }
    }

    async fn pool_checkin(
        &mut self,
        pool_name: String,
        actor_id: String,
        checkout_id: String,
        healthy: bool,
    ) -> Result<(), String> {
        let svc = match self.host_functions.elastic_pool_service() {
            Some(s) => s.clone(),
            None => return Err("Elastic pool service not configured".to_string()),
        };
        match svc
            .checkin(&pool_name, &actor_id, &checkout_id, healthy)
            .await
        {
            Ok(()) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }

    async fn pool_get_metrics(&mut self, pool_name: String) -> Result<Vec<u8>, String> {
        let svc = match self.host_functions.elastic_pool_service() {
            Some(s) => s.clone(),
            None => return Err("Elastic pool service not configured".to_string()),
        };
        match svc.get_metrics(&pool_name).await {
            Ok(metrics) => Ok(Self::encode_proto(&ProtoPoolMetrics {
                name: metrics.name,
                scaling_state: metrics.scaling_state,
                total_actors: metrics.total_actors,
                available_actors: metrics.available_actors,
                busy_actors: metrics.busy_actors,
                idle_actors: metrics.idle_actors,
                failed_actors: metrics.failed_actors,
                waiting_requests: metrics.waiting_requests,
                total_checkouts: metrics.total_checkouts,
                total_checkins: metrics.total_checkins,
                total_timeouts: metrics.total_timeouts,
                current_load: metrics.current_load,
                avg_load_1m: metrics.avg_load_1m,
                avg_load_5m: metrics.avg_load_5m,
                avg_checkout_latency: metrics.avg_checkout_latency,
                p95_checkout_latency: metrics.p95_checkout_latency,
                p99_checkout_latency: metrics.p99_checkout_latency,
                avg_actor_usage_time: metrics.avg_actor_usage_time,
                avg_actor_idle_time: metrics.avg_actor_idle_time,
                circuit_state: metrics.circuit_state,
                last_scale_up: metrics.last_scale_up,
                last_scale_down: metrics.last_scale_down,
                custom_metrics: metrics.custom_metrics,
            })),
            Err(e) => Err(e.to_string()),
        }
    }
}

/// Shard groups and ML collectives — host-shard interface
#[cfg(feature = "component-model")]
#[async_trait::async_trait]
impl plexspaces::actor::host_shard::Host for SimpleHostImpl {
    async fn create_shard_group(&mut self, request_data: Vec<u8>) -> Result<Vec<u8>, String> {
        let req = Self::decode_proto::<CreateShardGroupRequest>(
            &request_data,
            "CreateShardGroupRequest",
        )?;
        let ctx = self.pg_context();
        match self.host_functions.create_shard_group(&ctx, req).await {
            Ok(response) => Ok(Self::encode_proto(&response)),
            Err(err) => Err(err.to_string()),
        }
    }

    async fn bulk_update_shard_group(&mut self, request_data: Vec<u8>) -> Result<Vec<u8>, String> {
        let mut req = Self::decode_proto::<BulkUpdateShardGroupRequest>(
            &request_data,
            "BulkUpdateShardGroupRequest",
        )?;
        for update in req.updates.values_mut() {
            if update.sender_id.is_empty() {
                update.sender_id = self.actor_id.to_string();
            }
        }
        let ctx = self.pg_context();
        match self.host_functions.bulk_update_shard_group(&ctx, req).await {
            Ok(response) => Ok(Self::encode_proto(&response)),
            Err(err) => Err(err.to_string()),
        }
    }

    async fn map_shard_group(&mut self, request_data: Vec<u8>) -> Result<Vec<u8>, String> {
        let mut req =
            Self::decode_proto::<MapShardGroupRequest>(&request_data, "MapShardGroupRequest")?;
        if let Some(map_function) = req.map_function.as_mut() {
            if map_function.sender_id.is_empty() {
                map_function.sender_id = self.actor_id.to_string();
            }
        }
        let ctx = self.pg_context();
        match self.host_functions.map_shard_group(&ctx, req).await {
            Ok(response) => Ok(Self::encode_proto(&response)),
            Err(err) => Err(err.to_string()),
        }
    }

    async fn scatter_gather(&mut self, request_data: Vec<u8>) -> Result<Vec<u8>, String> {
        let mut req =
            Self::decode_proto::<ScatterGatherRequest>(&request_data, "ScatterGatherRequest")?;
        if let Some(query) = req.query.as_mut() {
            if query.sender_id.is_empty() {
                query.sender_id = self.actor_id.to_string();
            }
        }
        let ctx = self.pg_context();
        match self.host_functions.scatter_gather(&ctx, req).await {
            Ok(response) => Ok(Self::encode_proto(&response)),
            Err(err) => Err(err.to_string()),
        }
    }

    async fn broadcast_shard_group(&mut self, request_data: Vec<u8>) -> Result<Vec<u8>, String> {
        let mut req = Self::decode_proto::<BroadcastShardGroupRequest>(
            &request_data,
            "BroadcastShardGroupRequest",
        )?;
        if let Some(message) = req.message.as_mut() {
            if message.sender_id.is_empty() {
                message.sender_id = self.actor_id.to_string();
            }
        }
        let ctx = self.pg_context();
        match self.host_functions.broadcast_shard_group(&ctx, req).await {
            Ok(response) => Ok(Self::encode_proto(&response)),
            Err(err) => Err(err.to_string()),
        }
    }

    async fn reduce_shard_group(&mut self, request_data: Vec<u8>) -> Result<Vec<u8>, String> {
        let mut req = Self::decode_proto::<ReduceShardGroupRequest>(
            &request_data,
            "ReduceShardGroupRequest",
        )?;
        if let Some(map_function) = req.map_function.as_mut() {
            if map_function.sender_id.is_empty() {
                map_function.sender_id = self.actor_id.to_string();
            }
        }
        let ctx = self.pg_context();
        match self.host_functions.reduce_shard_group(&ctx, req).await {
            Ok(response) => Ok(Self::encode_proto(&response)),
            Err(err) => Err(err.to_string()),
        }
    }

    async fn all_reduce_shard_group(&mut self, request_data: Vec<u8>) -> Result<Vec<u8>, String> {
        let mut req = Self::decode_proto::<AllReduceShardGroupRequest>(
            &request_data,
            "AllReduceShardGroupRequest",
        )?;
        if let Some(map_function) = req.map_function.as_mut() {
            if map_function.sender_id.is_empty() {
                map_function.sender_id = self.actor_id.to_string();
            }
        }
        let ctx = self.pg_context();
        match self.host_functions.all_reduce_shard_group(&ctx, req).await {
            Ok(response) => Ok(Self::encode_proto(&response)),
            Err(err) => Err(err.to_string()),
        }
    }

    async fn barrier_shard_group(&mut self, request_data: Vec<u8>) -> Result<Vec<u8>, String> {
        let req = Self::decode_proto::<BarrierShardGroupRequest>(
            &request_data,
            "BarrierShardGroupRequest",
        )?;
        let ctx = self.pg_context();
        match self.host_functions.barrier_shard_group(&ctx, req).await {
            Ok(response) => Ok(Self::encode_proto(&response)),
            Err(err) => Err(err.to_string()),
        }
    }

    async fn spawn_actors(&mut self, request_data: Vec<u8>) -> Result<Vec<u8>, String> {
        let req = Self::decode_proto::<SpawnActorsRequest>(&request_data, "SpawnActorsRequest")?;
        let ctx = self.pg_context();
        match self.host_functions.spawn_actors(&ctx, req).await {
            Ok(response) => Ok(Self::encode_proto(&response)),
            Err(err) => Err(err.to_string()),
        }
    }

    async fn application_metrics_add(
        &mut self,
        application_id: String,
        metrics: Vec<u8>,
    ) -> Result<Vec<u8>, String> {
        let metrics = Self::decode_proto::<ApplicationMetrics>(&metrics, "ApplicationMetrics")?;
        let ctx = self.pg_context();
        match self
            .host_functions
            .merge_application_metrics(&ctx, &application_id, metrics)
            .await
        {
            Ok(metrics) => Ok(Self::encode_proto(&metrics)),
            Err(err) => Err(err.to_string()),
        }
    }

    async fn application_get_metrics(
        &mut self,
        application_id: String,
        node_id: String,
    ) -> Result<Vec<u8>, String> {
        let ctx = self.pg_context();
        match self
            .host_functions
            .get_application_metrics(&ctx, &application_id, &node_id)
            .await
        {
            Ok(metrics) => Ok(Self::encode_proto(&metrics)),
            Err(err) => Err(err.to_string()),
        }
    }

    async fn application_get_status(
        &mut self,
        application_id: String,
        node_id: String,
    ) -> Result<Vec<u8>, String> {
        let ctx = self.pg_context();
        match self
            .host_functions
            .get_application_status(&ctx, &application_id, &node_id)
            .await
        {
            Ok((application, node_address)) => {
                Ok(Self::encode_proto(&GetApplicationStatusResponse {
                    request_id: ulid::Ulid::new().to_string(),
                    application: Some(application),
                    state: None,
                    error: None,
                    node_id,
                    node_address,
                }))
            }
            Err(err) => Err(err.to_string()),
        }
    }
}

/// Outbound HTTP — host-http interface
#[cfg(feature = "component-model")]
#[async_trait::async_trait]
impl plexspaces::actor::host_http::Host for SimpleHostImpl {
    /// Execute outbound HTTP request via named service link.
    /// Delegates to HostFunctions::http_fetch which calls the OutboundHttpClient.
    async fn http_fetch(
        &mut self,
        link_name: String,
        method: String,
        path_and_query: String,
        request: Vec<u8>,
    ) -> Result<Vec<u8>, String> {
        let request = Self::decode_proto::<HttpFetchRequest>(&request, "HttpFetchRequest")?;
        let response = self
            .host_functions
            .http_fetch(&link_name, &method, &path_and_query, request)
            .await?;
        Ok(Self::encode_proto(&HttpFetchResponse {
            request_id: ulid::Ulid::new().to_string(),
            status: response.status,
            headers: response
                .headers
                .into_iter()
                .map(|h| (h.key, h.value))
                .collect(),
            body: response.body,
        }))
    }
}

/// Channels host implementation for `actor-world` WASM actors.
///
/// Delegates to the same `HostFunctions` service gateway used by `ChannelsImpl` in
/// `component_host.rs`. This keeps `actor-world` (Go/TinyGo/Python) on equal footing
/// with native-actor components for channel operations.
#[cfg(feature = "component-model")]
#[async_trait::async_trait]
impl plexspaces::actor::channels::Host for SimpleHostImpl {
    async fn channel_send(
        &mut self,
        _ctx: String,
        channel_name: String,
        msg_type: String,
        payload: plexspaces::actor::types::Payload,
    ) -> Result<plexspaces::actor::types::MessageId, plexspaces::actor::types::ActorError> {
        metrics::counter!("plexspaces_wasm_channel_send_total", "channel" => channel_name.clone())
            .increment(1);
        match self
            .host_functions
            .send_to_queue(&channel_name, &msg_type, payload)
            .await
        {
            Ok(message_id) => Ok(message_id),
            Err(e) => {
                metrics::counter!("plexspaces_wasm_channel_send_errors_total", "channel" => channel_name.clone()).increment(1);
                Err(format!("internal: {}", e))
            }
        }
    }

    async fn channel_send_with_options(
        &mut self,
        ctx: String,
        channel_name: String,
        msg_type: String,
        payload: plexspaces::actor::types::Payload,
        _delay_ms: u64,
        _ttl_ms: u64,
        _headers: String,
    ) -> Result<plexspaces::actor::types::MessageId, plexspaces::actor::types::ActorError> {
        self.channel_send(ctx, channel_name, msg_type, payload)
            .await
    }

    async fn channel_receive(
        &mut self,
        _ctx: String,
        channel_name: String,
        timeout_ms: u64,
    ) -> Result<
        Option<plexspaces::actor::channels::ChannelMessage>,
        plexspaces::actor::types::ActorError,
    > {
        metrics::counter!("plexspaces_wasm_channel_receive_total", "channel" => channel_name.clone()).increment(1);
        match self
            .host_functions
            .receive_from_queue(&channel_name, timeout_ms)
            .await
        {
            Ok(Some((msg_type, payload))) => {
                let message_id = ulid::Ulid::new().to_string();
                Ok(Some(plexspaces::actor::channels::ChannelMessage {
                    id: message_id,
                    msg_type,
                    payload,
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64,
                    delivery_count: 1,
                    headers: vec![],
                }))
            }
            Ok(None) => Ok(None),
            Err(e) => {
                metrics::counter!("plexspaces_wasm_channel_receive_errors_total", "channel" => channel_name.clone()).increment(1);
                Err(format!("internal: {}", e))
            }
        }
    }

    async fn channel_ack(
        &mut self,
        _ctx: String,
        channel_name: String,
        message_id: plexspaces::actor::types::MessageId,
    ) -> Result<(), plexspaces::actor::types::ActorError> {
        metrics::counter!("plexspaces_wasm_channel_ack_total", "channel" => channel_name.clone())
            .increment(1);
        tracing::debug!(channel = %channel_name, message_id = %message_id, "channel_ack (actor-world)");
        Ok(())
    }

    async fn channel_nack(
        &mut self,
        _ctx: String,
        channel_name: String,
        message_id: plexspaces::actor::types::MessageId,
        requeue: bool,
    ) -> Result<(), plexspaces::actor::types::ActorError> {
        metrics::counter!("plexspaces_wasm_channel_nack_total",
            "channel" => channel_name.clone(),
            "requeue" => requeue.to_string()
        )
        .increment(1);
        tracing::debug!(channel = %channel_name, message_id = %message_id, requeue, "channel_nack (actor-world)");
        Ok(())
    }

    async fn channel_publish(
        &mut self,
        _ctx: String,
        channel_name: String,
        msg_type: String,
        payload: plexspaces::actor::types::Payload,
    ) -> Result<plexspaces::actor::types::MessageId, plexspaces::actor::types::ActorError> {
        metrics::counter!("plexspaces_wasm_channel_publish_total", "channel" => channel_name.clone()).increment(1);
        match self
            .host_functions
            .publish_to_topic(&channel_name, &msg_type, payload)
            .await
        {
            Ok(message_id) => Ok(message_id),
            Err(e) => {
                metrics::counter!("plexspaces_wasm_channel_publish_errors_total", "channel" => channel_name.clone()).increment(1);
                Err(format!("internal: {}", e))
            }
        }
    }

    async fn channel_subscribe(
        &mut self,
        _ctx: String,
        channel_name: String,
        _filter: String,
    ) -> Result<String, plexspaces::actor::types::ActorError> {
        metrics::counter!("plexspaces_wasm_channel_subscribe_total", "channel" => channel_name.clone()).increment(1);
        // Push-based pub/sub streaming is not supported for actor-world components.
        // WASM actors receive channel messages via channel_receive (queue polling model).
        Err(
            "not-implemented: use channel_receive for message consumption in actor-world"
                .to_string(),
        )
    }

    async fn channel_unsubscribe(
        &mut self,
        _subscription_id: String,
    ) -> Result<(), plexspaces::actor::types::ActorError> {
        metrics::counter!("plexspaces_wasm_channel_unsubscribe_total").increment(1);
        Err(
            "not-implemented: use channel_receive for message consumption in actor-world"
                .to_string(),
        )
    }

    async fn channel_create(
        &mut self,
        _ctx: String,
        _channel_name: String,
        _max_size: u32,
        _message_ttl_ms: u64,
    ) -> Result<(), plexspaces::actor::types::ActorError> {
        Ok(())
    }

    async fn channel_delete(
        &mut self,
        _ctx: String,
        _channel_name: String,
    ) -> Result<(), plexspaces::actor::types::ActorError> {
        Err("not-implemented: channel_delete is a node-level administrative operation".to_string())
    }

    async fn channel_depth(
        &mut self,
        _ctx: String,
        _channel_name: String,
    ) -> Result<u64, plexspaces::actor::types::ActorError> {
        Ok(0)
    }
}

/// Check if a WASM component uses the actor-world interface by examining its imports.
#[cfg(feature = "component-model")]
pub fn is_simple_actor_component(component: &wasmtime::component::Component) -> bool {
    let component_type = component.component_type();
    for (name, _) in component_type.imports(component.engine()) {
        let n = name.to_string();
        // actor-world components import one of the namespaced host interfaces.
        // Match any package version so WIT bumps do not route TS/Python/Go through native-actor.
        if n.starts_with("plexspaces:actor/host-logging@")
            || n.starts_with("plexspaces:actor/host-actor@")
            || n.starts_with("plexspaces:actor/host-kv@")
            || n.starts_with("plexspaces:actor/host-ts@")
            || n.starts_with("plexspaces:actor/host-locks@")
            || n.starts_with("plexspaces:actor/host-blob@")
            || n.starts_with("plexspaces:actor/host-pool@")
            || n.starts_with("plexspaces:actor/host-shard@")
            || n.starts_with("plexspaces:actor/host-http@")
        {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::simple_component_host::plexspaces::actor::host_actor::Host as HostActorTrait;
    use crate::simple_component_host::plexspaces::actor::host_http::Host as HostHttpTrait;
    use crate::simple_component_host::plexspaces::actor::host_shard::Host as HostShardTrait;
    use async_trait::async_trait;
    use plexspaces_actor::{OutboundHttpClient, OutboundHttpRequest, OutboundHttpResponse};
    use plexspaces_proto::actor::v1::{
        BroadcastShardGroupRequest, CollectiveReduction, CreateShardGroupRequest,
        CreateShardGroupResponse, DataParallelConfig, MapShardGroupRequest, MapShardGroupResponse,
        NodePlacement, NodePlacementStrategy, ReduceShardGroupRequest, ScatterGatherRequest,
        ScatterGatherResponse, ScatterGatherStats, ShardGroup, ShardQueryResponse,
        SpawnActorRequest, SpawnActorsRequest, SpawnActorsResponse,
    };
    use plexspaces_proto::application::v1::{
        ApplicationInfo, ApplicationMetrics, GetApplicationStatusResponse,
    };
    use plexspaces_proto::common::v1::Message;
    use plexspaces_proto::wasm::v1::{HttpFetchRequest, HttpFetchResponse};
    use prost::Message as _;
    use std::sync::Arc;

    #[test]
    fn test_simple_actor_module_compiles() {
        // This test ensures the module compiles correctly
        // Actual functional tests require WASM components
    }

    struct MockMessageSender;

    struct MockOutboundHttpClient;

    #[async_trait]
    impl crate::MessageSender for MockMessageSender {
        async fn send_message(
            &self,
            _from: &str,
            _to: &str,
            _message_type: &str,
            _message: &[u8],
        ) -> Result<(), String> {
            Ok(())
        }

        async fn ask(
            &self,
            _from: &str,
            _to: &str,
            _message_type: &str,
            _payload: Vec<u8>,
            _timeout_ms: u64,
        ) -> Result<Vec<u8>, String> {
            Ok(Vec::new())
        }

        async fn spawn_actor(
            &self,
            _from: &str,
            _module_ref: &str,
            _role: String,
            _args: Vec<(String, String)>,
            _actor_name: Option<String>,
        ) -> Result<String, String> {
            Ok("worker-1".to_string())
        }

        async fn stop_actor(
            &self,
            _from: &str,
            _actor_id: &str,
            _timeout_ms: u64,
        ) -> Result<(), String> {
            Ok(())
        }

        async fn link_actor(
            &self,
            _from: &str,
            _actor_id: &str,
            _linked_actor_id: &str,
        ) -> Result<(), String> {
            Ok(())
        }

        async fn unlink_actor(
            &self,
            _from: &str,
            _actor_id: &str,
            _linked_actor_id: &str,
        ) -> Result<(), String> {
            Ok(())
        }

        async fn monitor_actor(&self, _from: &str, _actor_id: &str) -> Result<u64, String> {
            Ok(1)
        }

        async fn demonitor_actor(
            &self,
            _from: &str,
            _actor_id: &str,
            _monitor_ref: u64,
        ) -> Result<(), String> {
            Ok(())
        }

        async fn create_shard_group(
            &self,
            _ctx: &RequestContext,
            req: CreateShardGroupRequest,
        ) -> Result<CreateShardGroupResponse, String> {
            Ok(CreateShardGroupResponse {
                request_id: req.request_id.clone(),
                group: Some(ShardGroup {
                    config: req.config,
                    actor_type: req.actor_type,
                    shard_actor_ids: vec![
                        "worker-0@node-a".to_string(),
                        "worker-1@node-b".to_string(),
                    ],
                    state: 0,
                    created_at: None,
                    metadata: HashMap::new(),
                    rebalance_status: None,
                }),
            })
        }

        async fn map_shard_group(
            &self,
            _ctx: &RequestContext,
            _req: MapShardGroupRequest,
        ) -> Result<MapShardGroupResponse, String> {
            Ok(MapShardGroupResponse {
                request_id: ulid::Ulid::new().to_string(),
                shard_results: vec![ShardQueryResponse {
                    request_id: ulid::Ulid::new().to_string(),
                    shard_id: 0,
                    shard_actor_id: "worker-0@node-a".to_string(),
                    response: Some(Message {
                        payload: br#"{"samples_processed":128,"gradient_checksum":42}"#.to_vec(),
                        ..Default::default()
                    }),
                    latency: None,
                    success: true,
                    error: String::new(),
                }],
                stats: Some(ScatterGatherStats {
                    shards_queried: 1,
                    shards_responded: 1,
                    shards_failed: 0,
                    max_latency: None,
                }),
            })
        }

        async fn scatter_gather(
            &self,
            _ctx: &RequestContext,
            _req: ScatterGatherRequest,
        ) -> Result<ScatterGatherResponse, String> {
            Ok(ScatterGatherResponse {
                request_id: ulid::Ulid::new().to_string(),
                result: None,
                shard_responses: vec![ShardQueryResponse {
                    request_id: ulid::Ulid::new().to_string(),
                    shard_id: 0,
                    shard_actor_id: "worker-0@node-a".to_string(),
                    response: Some(Message {
                        payload:
                            br#"{"max_diff":0.5,"compute_time_ms":3,"coordination_time_ms":1}"#
                                .to_vec(),
                        ..Default::default()
                    }),
                    latency: None,
                    success: true,
                    error: String::new(),
                }],
                stats: None,
            })
        }

        async fn broadcast_shard_group(
            &self,
            _ctx: &RequestContext,
            _req: BroadcastShardGroupRequest,
        ) -> Result<plexspaces_proto::actor::v1::BroadcastShardGroupResponse, String> {
            Ok(plexspaces_proto::actor::v1::BroadcastShardGroupResponse {
                request_id: ulid::Ulid::new().to_string(),
                shard_responses: vec![ShardQueryResponse {
                    request_id: ulid::Ulid::new().to_string(),
                    shard_id: 0,
                    shard_actor_id: "worker-0@node-a".to_string(),
                    response: Some(Message {
                        payload: br#"{"ack":true}"#.to_vec(),
                        ..Default::default()
                    }),
                    latency: None,
                    success: true,
                    error: String::new(),
                }],
                stats: Some(ScatterGatherStats {
                    shards_queried: 1,
                    shards_responded: 1,
                    shards_failed: 0,
                    max_latency: None,
                }),
            })
        }

        async fn reduce_shard_group(
            &self,
            _ctx: &RequestContext,
            _req: ReduceShardGroupRequest,
        ) -> Result<plexspaces_proto::actor::v1::ReduceShardGroupResponse, String> {
            Ok(plexspaces_proto::actor::v1::ReduceShardGroupResponse {
                request_id: ulid::Ulid::new().to_string(),
                result: Some(Message {
                    payload: br#"{"sum":42}"#.to_vec(),
                    ..Default::default()
                }),
                shard_responses: vec![ShardQueryResponse {
                    request_id: ulid::Ulid::new().to_string(),
                    shard_id: 0,
                    shard_actor_id: "worker-0@node-a".to_string(),
                    response: Some(Message {
                        payload: br#"{"value":21}"#.to_vec(),
                        ..Default::default()
                    }),
                    latency: None,
                    success: true,
                    error: String::new(),
                }],
                stats: Some(ScatterGatherStats {
                    shards_queried: 1,
                    shards_responded: 1,
                    shards_failed: 0,
                    max_latency: None,
                }),
            })
        }

        async fn all_reduce_shard_group(
            &self,
            _ctx: &RequestContext,
            _req: AllReduceShardGroupRequest,
        ) -> Result<plexspaces_proto::actor::v1::AllReduceShardGroupResponse, String> {
            Ok(plexspaces_proto::actor::v1::AllReduceShardGroupResponse {
                request_id: ulid::Ulid::new().to_string(),
                result: Some(Message {
                    payload: br#"{"sum":42}"#.to_vec(),
                    ..Default::default()
                }),
                shard_responses: vec![ShardQueryResponse {
                    request_id: ulid::Ulid::new().to_string(),
                    shard_id: 0,
                    shard_actor_id: "worker-0@node-a".to_string(),
                    response: Some(Message {
                        payload: br#"{"ack":true}"#.to_vec(),
                        ..Default::default()
                    }),
                    latency: None,
                    success: true,
                    error: String::new(),
                }],
                stats: Some(ScatterGatherStats {
                    shards_queried: 1,
                    shards_responded: 1,
                    shards_failed: 0,
                    max_latency: None,
                }),
            })
        }

        async fn barrier_shard_group(
            &self,
            _ctx: &RequestContext,
            _req: BarrierShardGroupRequest,
        ) -> Result<plexspaces_proto::actor::v1::BarrierShardGroupResponse, String> {
            Ok(plexspaces_proto::actor::v1::BarrierShardGroupResponse {
                request_id: ulid::Ulid::new().to_string(),
                shard_responses: vec![ShardQueryResponse {
                    request_id: ulid::Ulid::new().to_string(),
                    shard_id: 0,
                    shard_actor_id: "worker-0@node-a".to_string(),
                    response: Some(Message {
                        payload: br#"{"barrier":"ready"}"#.to_vec(),
                        ..Default::default()
                    }),
                    latency: None,
                    success: true,
                    error: String::new(),
                }],
                stats: Some(ScatterGatherStats {
                    shards_queried: 1,
                    shards_responded: 1,
                    shards_failed: 0,
                    max_latency: None,
                }),
            })
        }

        async fn spawn_actors(
            &self,
            _ctx: &RequestContext,
            req: SpawnActorsRequest,
        ) -> Result<plexspaces_proto::actor::v1::SpawnActorsResponse, String> {
            Ok(plexspaces_proto::actor::v1::SpawnActorsResponse {
                request_id: ulid::Ulid::new().to_string(),
                results: req
                    .requests
                    .into_iter()
                    .map(|request| {
                        let spec = request.spec.as_ref();
                        let identity = spec.and_then(|s| s.identity.as_ref());
                        let actor_type = identity.map(|i| i.actor_type.clone()).unwrap_or_default();
                        let actor_name = identity.map(|i| i.name.clone()).unwrap_or_default();
                        let effective_name = if actor_name.is_empty() {
                            actor_type.clone()
                        } else {
                            actor_name.clone()
                        };
                        let namespace = if !request.namespace.is_empty() {
                            request.namespace.clone()
                        } else {
                            spec.map(|s| s.namespace.clone()).unwrap_or_default()
                        };
                        plexspaces_proto::actor::v1::SpawnActorResult {
                            success: true,
                            error: String::new(),
                            response: Some(plexspaces_proto::actor::v1::SpawnActorResponse {
                                request_id: ulid::Ulid::new().to_string(),
                                actor_ref: format!("{}@node-a", effective_name),
                                actor: Some(plexspaces_proto::actor::v1::Actor {
                                    actor_id: effective_name.clone(),
                                    actor_type,
                                    namespace,
                                    ..Default::default()
                                }),
                            }),
                        }
                    })
                    .collect(),
            })
        }

        async fn merge_application_metrics(
            &self,
            _ctx: &RequestContext,
            _application_id: &str,
            metrics: ApplicationMetrics,
        ) -> Result<ApplicationMetrics, String> {
            Ok(metrics)
        }

        async fn get_application_metrics(
            &self,
            _ctx: &RequestContext,
            _application_id: &str,
            _node_id: &str,
        ) -> Result<ApplicationMetrics, String> {
            Ok(ApplicationMetrics {
                actor_counts: HashMap::from([("worker".to_string(), 2)]),
                message_count: 5,
                counter_metrics: HashMap::from([("tuple_operations".to_string(), 11)]),
                ..Default::default()
            })
        }

        async fn get_application_status(
            &self,
            _ctx: &RequestContext,
            application_id: &str,
            _node_id: &str,
        ) -> Result<(ApplicationInfo, String), String> {
            Ok((
                ApplicationInfo {
                    application_id: application_id.to_string(),
                    name: application_id.to_string(),
                    tenant_id: String::new(),
                    version: "1.0.0".to_string(),
                    status: 2,
                    deployed_at: None,
                    metrics: Some(ApplicationMetrics {
                        actor_counts: HashMap::from([
                            ("total".to_string(), 4),
                            ("worker".to_string(), 3),
                        ]),
                        supervisor_count: 1,
                        uptime_seconds: 42,
                        message_count: 9,
                        error_count: 0,
                        counter_metrics: HashMap::from([("tuple_operations".to_string(), 7)]),
                        latency_totals_ms: HashMap::from([("compute".to_string(), 21)]),
                        latency_max_ms: HashMap::from([("compute".to_string(), 9)]),
                        latency_samples: HashMap::from([("compute".to_string(), 3)]),
                    }),
                },
                "http://127.0.0.1:8092".to_string(),
            ))
        }
    }

    #[async_trait]
    impl OutboundHttpClient for MockOutboundHttpClient {
        async fn execute(
            &self,
            link_name: &str,
            request: OutboundHttpRequest,
        ) -> Result<OutboundHttpResponse, plexspaces_actor::OutboundHttpClientError> {
            use plexspaces_actor::HttpHeader;
            Ok(OutboundHttpResponse {
                request_id: request.request_id.clone(),
                status: 200,
                headers: vec![
                    HttpHeader {
                        key: "x-link".to_string(),
                        value: link_name.to_string(),
                    },
                    HttpHeader {
                        key: "x-method".to_string(),
                        value: request.method,
                    },
                ],
                body: request.body,
            })
        }
    }

    #[tokio::test]
    async fn create_shard_group_serializes_group_response() {
        let host_functions = Arc::new(HostFunctions::with_message_sender(Arc::new(
            MockMessageSender,
        )));
        let mut host = SimpleHostImpl::new(
            ActorId::new("leader", "test", "default", "node-a").unwrap(),
            host_functions,
            None,
        );
        let response = decode_proto_response::<CreateShardGroupResponse>(
            host.create_shard_group(
                CreateShardGroupRequest {
                    config: Some(DataParallelConfig {
                        group_id: "heat-group".to_string(),
                        shard_count: 2,
                        placement: Some(NodePlacement {
                            strategy: NodePlacementStrategy::NodePlacementStrategyFromRegistry
                                as i32,
                            ..Default::default()
                        }),
                        ..Default::default()
                    }),
                    actor_type: "worker".to_string(),
                    ..Default::default()
                }
                .encode_to_vec(),
            )
            .await,
        );
        let group = response.group.expect("group should be set");
        assert_eq!(
            group.config.expect("config should be set").group_id,
            "heat-group"
        );
        assert_eq!(group.shard_actor_ids[0], "worker-0@node-a");
    }

    #[tokio::test]
    async fn scatter_gather_serializes_shard_payloads() {
        let host_functions = Arc::new(HostFunctions::with_message_sender(Arc::new(
            MockMessageSender,
        )));
        let mut host = SimpleHostImpl::new(
            ActorId::new("leader", "test", "default", "node-a").unwrap(),
            host_functions,
            None,
        );
        let response = decode_proto_response::<ScatterGatherResponse>(
            host.scatter_gather(
                ScatterGatherRequest {
                    group_id: "heat-group".to_string(),
                    query: Some(Message {
                        message_type: "compute".to_string(),
                        payload: b"compute".to_vec(),
                        ..Default::default()
                    }),
                    ..Default::default()
                }
                .encode_to_vec(),
            )
            .await,
        );
        let shard = response
            .shard_responses
            .first()
            .expect("one shard response expected");
        assert_eq!(
            shard.response.as_ref().expect("message expected").payload,
            br#"{"max_diff":0.5,"compute_time_ms":3,"coordination_time_ms":1}"#
        );
    }

    #[tokio::test]
    async fn map_shard_group_serializes_results() {
        let host_functions = Arc::new(HostFunctions::with_message_sender(Arc::new(
            MockMessageSender,
        )));
        let mut host = SimpleHostImpl::new(
            ActorId::new("leader", "test", "default", "node-a").unwrap(),
            host_functions,
            None,
        );
        let response = decode_proto_response::<MapShardGroupResponse>(
            host.map_shard_group(
                MapShardGroupRequest {
                    group_id: "heat-group".to_string(),
                    map_function: Some(Message {
                        message_type: "compute_gradient".to_string(),
                        payload: b"gradient".to_vec(),
                        ..Default::default()
                    }),
                    ..Default::default()
                }
                .encode_to_vec(),
            )
            .await,
        );
        let shard = response
            .shard_results
            .first()
            .expect("one shard result expected");
        assert_eq!(
            shard.response.as_ref().expect("message expected").payload,
            br#"{"samples_processed":128,"gradient_checksum":42}"#
        );
    }

    #[tokio::test]
    async fn application_metrics_add_serializes_metrics_response() {
        let host_functions = Arc::new(HostFunctions::with_message_sender(Arc::new(
            MockMessageSender,
        )));
        let mut host = SimpleHostImpl::new(
            ActorId::new("leader", "test", "default", "node-a").unwrap(),
            host_functions,
            None,
        );
        let response = decode_proto_response::<ApplicationMetrics>(
            host.application_metrics_add(
                "heat-diffusion-rust".to_string(),
                ApplicationMetrics {
                    actor_counts: HashMap::from([("worker".to_string(), 2)]),
                    message_count: 5,
                    counter_metrics: HashMap::from([("tuple_operations".to_string(), 11)]),
                    ..Default::default()
                }
                .encode_to_vec(),
            )
            .await,
        );
        assert_eq!(response.actor_counts.get("worker"), Some(&2));
        assert_eq!(response.counter_metrics.get("tuple_operations"), Some(&11));
    }

    #[tokio::test]
    async fn application_get_status_serializes_node_and_metrics() {
        let host_functions = Arc::new(HostFunctions::with_message_sender(Arc::new(
            MockMessageSender,
        )));
        let mut host = SimpleHostImpl::new(
            ActorId::new("leader", "test", "default", "node-a").unwrap(),
            host_functions,
            None,
        );
        let response = decode_proto_response::<GetApplicationStatusResponse>(
            host.application_get_status(
                "heat-diffusion-rust".to_string(),
                "test-node-8092".to_string(),
            )
            .await,
        );
        assert_eq!(response.node_id, "test-node-8092");
        assert_eq!(response.node_address, "http://127.0.0.1:8092");
        let metrics = response
            .application
            .expect("application expected")
            .metrics
            .expect("metrics expected");
        assert_eq!(metrics.counter_metrics.get("tuple_operations"), Some(&7));
    }

    #[tokio::test]
    async fn application_get_metrics_serializes_metrics() {
        let host_functions = Arc::new(HostFunctions::with_message_sender(Arc::new(
            MockMessageSender,
        )));
        let mut host = SimpleHostImpl::new(
            ActorId::new("leader", "test", "default", "node-a").unwrap(),
            host_functions,
            None,
        );
        let response = decode_proto_response::<ApplicationMetrics>(
            host.application_get_metrics(
                "heat-diffusion-rust".to_string(),
                "test-node-8092".to_string(),
            )
            .await,
        );
        assert_eq!(response.actor_counts.get("worker"), Some(&2));
        assert_eq!(response.counter_metrics.get("tuple_operations"), Some(&11));
    }

    #[tokio::test]
    async fn broadcast_shard_group_serializes_response() {
        let host_functions = Arc::new(HostFunctions::with_message_sender(Arc::new(
            MockMessageSender,
        )));
        let mut host = SimpleHostImpl::new(
            ActorId::new("leader", "test", "default", "node-a").unwrap(),
            host_functions,
            None,
        );
        let response =
            decode_proto_response::<plexspaces_proto::actor::v1::BroadcastShardGroupResponse>(
                host.broadcast_shard_group(
                    BroadcastShardGroupRequest {
                        group_id: "mpi-group".to_string(),
                        message: Some(Message {
                            message_type: "broadcast".to_string(),
                            payload: b"broadcast".to_vec(),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }
                    .encode_to_vec(),
                )
                .await,
            );
        let shard = response
            .shard_responses
            .first()
            .expect("one shard response expected");
        assert_eq!(
            shard.response.as_ref().expect("message expected").payload,
            br#"{"ack":true}"#
        );
        assert_eq!(response.stats.expect("stats expected").shards_responded, 1);
    }

    #[tokio::test]
    async fn reduce_shard_group_serializes_response() {
        let host_functions = Arc::new(HostFunctions::with_message_sender(Arc::new(
            MockMessageSender,
        )));
        let mut host = SimpleHostImpl::new(
            ActorId::new("leader", "test", "default", "node-a").unwrap(),
            host_functions,
            None,
        );
        let response = decode_proto_response::<plexspaces_proto::actor::v1::ReduceShardGroupResponse>(
            host.reduce_shard_group(
                ReduceShardGroupRequest {
                    group_id: "mpi-group".to_string(),
                    map_function: Some(Message {
                        message_type: "local-sum".to_string(),
                        payload: b"sum".to_vec(),
                        ..Default::default()
                    }),
                    reduction: CollectiveReduction::CollectiveReductionSum as i32,
                    ..Default::default()
                }
                .encode_to_vec(),
            )
            .await,
        );
        assert_eq!(
            response.result.expect("result expected").payload,
            br#"{"sum":42}"#
        );
        assert_eq!(
            response
                .shard_responses
                .first()
                .expect("one shard response expected")
                .response
                .as_ref()
                .expect("message expected")
                .payload,
            br#"{"value":21}"#
        );
    }

    #[tokio::test]
    async fn spawn_actors_serializes_results() {
        let host_functions = Arc::new(HostFunctions::with_message_sender(Arc::new(
            MockMessageSender,
        )));
        let mut host = SimpleHostImpl::new(
            ActorId::new("leader", "test", "default", "node-a").unwrap(),
            host_functions,
            None,
        );
        let response = decode_proto_response::<SpawnActorsResponse>(
            host.spawn_actors(
                SpawnActorsRequest {
                    request_id: ulid::Ulid::new().to_string(),
                    requests: vec![SpawnActorRequest {
                        request_id: ulid::Ulid::new().to_string(),
                        spec: Some(plexspaces_proto::actor::v1::ActorSpawnSpec {
                            identity: Some(plexspaces_proto::common::v1::ActorIdentity {
                                name: "worker-0".to_string(),
                                actor_type: "worker".to_string(),
                            }),
                            role: String::new(),
                            namespace: String::new(),
                            behavior_kind: String::new(),
                            args: std::collections::HashMap::from([(
                                "rank".to_string(),
                                "rank-0".to_string(),
                            )]),
                            ..Default::default()
                        }),
                        namespace: "mpi-app".to_string(),
                        instances_count: 1,
                    }],
                }
                .encode_to_vec(),
            )
            .await,
        );
        let result = response.results.first().expect("one spawn result expected");
        assert!(result.success);
        assert_eq!(
            result
                .response
                .as_ref()
                .expect("spawn response expected")
                .actor_ref,
            "worker-0@node-a"
        );
    }

    #[tokio::test]
    async fn http_fetch_serializes_response() {
        let host_functions = Arc::new(HostFunctions::with_all_services(
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(Arc::new(MockOutboundHttpClient)),
            None, // shared_timer_pool
        ));
        let mut host = SimpleHostImpl::new(
            ActorId::new("leader", "test", "default", "node-a").unwrap(),
            host_functions,
            None,
        );
        let response = decode_proto_response::<HttpFetchResponse>(
            host.http_fetch(
                "weather-api".to_string(),
                "POST".to_string(),
                "/v1/current".to_string(),
                HttpFetchRequest {
                    request_id: ulid::Ulid::new().to_string(),
                    headers: HashMap::from([("x-test".to_string(), "1".to_string())]),
                    body: b"request-body".to_vec(),
                }
                .encode_to_vec(),
            )
            .await,
        );
        assert_eq!(response.status, 200);
        assert_eq!(
            response.headers.get("x-link"),
            Some(&"weather-api".to_string())
        );
        assert_eq!(response.headers.get("x-method"), Some(&"POST".to_string()));
        assert_eq!(response.body, b"request-body".to_vec());
    }

    fn decode_proto_response<M>(response: Result<Vec<u8>, String>) -> M
    where
        M: prost::Message + Default,
    {
        M::decode(response.expect("host call should succeed").as_slice())
            .expect("response should be valid protobuf")
    }

    #[cfg(feature = "component-model")]
    #[tokio::test]
    async fn test_send_after_suppressed_during_replay() {
        use plexspaces::actor::host_actor::Host;

        let hf = Arc::new(HostFunctions::new());
        hf.is_replaying
            .store(true, std::sync::atomic::Ordering::Release);

        let mut host = SimpleHostImpl::new(
            ActorId::new("test-actor", "test-type", "test-ns", "node-1").unwrap(),
            hf.clone(),
            None,
        );

        let result = host
            .send_after(1000, "timer_fire".to_string(), b"payload".to_vec())
            .await;

        assert!(result.is_ok());
        let timer_id = result.unwrap();
        assert!(
            timer_id.contains("replay-suppressed"),
            "Timer ID should indicate suppression: {}",
            timer_id
        );

        // No timer handle should have been spawned
        let handles = hf.timer_handles.lock().unwrap();
        assert_eq!(
            handles.len(),
            0,
            "No timers should be spawned during replay"
        );
    }

    #[cfg(feature = "component-model")]
    #[tokio::test]
    async fn test_send_suppressed_during_replay() {
        use plexspaces::actor::host_actor::Host;

        let sender = Arc::new(MockMessageSender) as Arc<dyn crate::MessageSender>;
        let hf = Arc::new(HostFunctions::with_message_sender(sender));
        hf.is_replaying
            .store(true, std::sync::atomic::Ordering::Release);

        let mut host = SimpleHostImpl::new(
            ActorId::new("test-actor", "test-type", "test-ns", "node-1").unwrap(),
            hf.clone(),
            None,
        );

        let result = host
            .send(
                "other-actor".to_string(),
                "event".to_string(),
                b"data".to_vec(),
            )
            .await;

        assert!(
            result.is_ok(),
            "send should succeed (suppressed) during replay"
        );
    }

    #[cfg(feature = "component-model")]
    #[tokio::test]
    async fn test_send_after_works_when_not_replaying() {
        use plexspaces::actor::host_actor::Host;

        let sender = Arc::new(MockMessageSender) as Arc<dyn crate::MessageSender>;
        let hf = Arc::new(HostFunctions::with_message_sender(sender));

        let mut host = SimpleHostImpl::new(
            ActorId::new("test-actor", "test-type", "test-ns", "node-1").unwrap(),
            hf.clone(),
            None,
        );

        let result = host
            .send_after(50, "timer_fire".to_string(), b"payload".to_vec())
            .await;

        assert!(result.is_ok());
        let timer_id = result.unwrap();
        assert!(
            !timer_id.contains("replay-suppressed"),
            "Timer should be real when not replaying: {}",
            timer_id
        );

        // A timer handle should have been spawned
        let handles = hf.timer_handles.lock().unwrap();
        assert_eq!(handles.len(), 1, "One timer should be spawned");
    }
}
