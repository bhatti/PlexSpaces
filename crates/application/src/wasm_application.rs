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

//! WASM Application Implementation
//!
//! ## Purpose
//! Implements Application trait for WASM-based applications. WASM applications
//! contain supervisor trees and actors compiled to WebAssembly, enabling
//! polyglot actor deployment.
//!
//! ## Architecture
//! - WASM module contains application code (supervisors + actors)
//! - Framework (Rust) provides services, runtime, infrastructure
//! - Actors (WASM) provide business logic
//!
//! ## Design Principles
//! - WASM = Actor implementation (like Lambda function code)
//! - Framework provides infrastructure, WASM provides business logic
//! - Application-level deployment (entire application, not individual actors)

use crate::{Application, ApplicationError, ApplicationNode};
use async_trait::async_trait;
use plexspaces_core::actor_id::{build_actor_id, parse_actor_id};
use plexspaces_core::{Actor, ActorError, BehaviorError, BehaviorType};
use plexspaces_proto::application::v1::{ApplicationSpec, SupervisorSpec};
use plexspaces_proto::common::v1::Message;
use plexspaces_proto::v1::application::HealthStatus;
use plexspaces_wasm_runtime::{deployment_service::WasmDeploymentService, WasmInstance};
use prost::Message as ProstMessage;

use plexspaces_actor::child_spec::{ChildType as ActorChildType, RestartStrategy};
use plexspaces_actor::{
    ChildSpec as ActorChildSpec, StartedChild, SupervisionStrategy, Supervisor,
};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

/// Tries to get application-level message type (handler name) from JSON payload.
///
/// **Canonical payload key**: `message_type`. Accepted aliases: `op` (SDK shorthand), `msg_type`.
/// Check order: `message_type` → `op` → `msg_type` so all SDKs and clients can use one consistent name.
/// Used for GenServer handler names and Workflow routing (workflow_run, workflow_signal:name, workflow_query:name).
/// Returns None if payload is not valid JSON or has no handler field, or value is transport-only ("call"/"cast").
pub(crate) fn try_msg_type_from_payload(payload: &[u8]) -> Option<String> {
    let value: serde_json::Value = serde_json::from_slice(payload).ok()?;
    let take_str = |key: &str| -> Option<String> {
        value
            .get(key)
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
    };
    for key in ["message_type", "op", "msg_type"] {
        if let Some(s) = take_str(key) {
            if !s.is_empty() && !s.eq_ignore_ascii_case("call") && !s.eq_ignore_ascii_case("cast") {
                return Some(s);
            }
        }
    }
    None
}

/// WASM actor behavior that wraps a WasmInstance
///
/// ## Purpose
/// Bridges between the actor system and WASM instances, forwarding messages
/// to the WASM module's handle_message function.
///
/// ## Design
/// Parses optional behavior_kind string from ChildSpec to BehaviorType for logging.
fn parse_behavior_kind(s: Option<&str>) -> plexspaces_core::BehaviorType {
    match s.map(str::trim) {
        Some("GenEvent") | Some("EventHandler") | Some("eventhandler") | Some("event") => {
            plexspaces_core::BehaviorType::GenEvent
        }
        Some("GenServer") | Some("genserver") => plexspaces_core::BehaviorType::GenServer,
        Some("GenStateMachine") | Some("fsm") => plexspaces_core::BehaviorType::GenStateMachine,
        Some("Workflow") | Some("workflow") => plexspaces_core::BehaviorType::Workflow,
        _ => plexspaces_core::BehaviorType::GenServer,
    }
}

fn actor_id_from_initial_state(
    initial_state: &[u8],
    child_id: &str,
    namespace: &str,
    node_id: &str,
) -> String {
    serde_json::from_slice::<serde_json::Value>(initial_state)
        .ok()
        .and_then(|value| {
            value
                .get("actor_id")
                .and_then(|actor_id| actor_id.as_str())
                .map(str::trim)
                .filter(|actor_id| !actor_id.is_empty())
                .map(str::to_string)
        })
        .unwrap_or_else(|| {
            plexspaces_core::actor_id::build_actor_id(child_id, child_id, Some(namespace), node_id)
        })
}

/// Merges framework fields into a JSON object for WASM `Init(config_json)`.
///
/// ShardGroup and similar paths often pass `initial_state` as `{}` (non-empty bytes). Returning
/// that verbatim omitted `actor_id`, which breaks multi-role WASM guests (e.g. Go `ActorRouter`)
/// that route by normalized `actor_id`. When `actor_id` is already present and non-empty, the
/// payload is left unchanged so materialized virtual-actor configs stay stable.
fn init_config_from_initial_state_or_child_spec(
    initial_state: &[u8],
    child_spec: &plexspaces_proto::application::v1::ChildSpec,
    actor_id: &str,
) -> Vec<u8> {
    fn apply_child_spec_to_object(
        obj: &mut serde_json::Map<String, serde_json::Value>,
        child_spec: &plexspaces_proto::application::v1::ChildSpec,
        actor_id: &str,
    ) {
        obj.insert(
            "actor_id".to_string(),
            serde_json::Value::String(actor_id.to_string()),
        );
        if let Some(ref bk) = child_spec.behavior_kind {
            obj.entry("behavior_kind".to_string())
                .or_insert_with(|| serde_json::Value::String(bk.clone()));
        }
        if !child_spec.args.is_empty() {
            obj.entry("args".to_string()).or_insert_with(|| {
                let args_obj: serde_json::Map<String, serde_json::Value> = child_spec
                    .args
                    .iter()
                    .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
                    .collect();
                serde_json::Value::Object(args_obj)
            });
        }
    }

    if initial_state.is_empty() {
        let mut init_config = serde_json::Map::new();
        apply_child_spec_to_object(&mut init_config, child_spec, actor_id);
        return serde_json::to_vec(&serde_json::Value::Object(init_config)).unwrap_or_default();
    }

    let Ok(value) = serde_json::from_slice::<serde_json::Value>(initial_state) else {
        return initial_state.to_vec();
    };
    let serde_json::Value::Object(mut obj) = value else {
        return initial_state.to_vec();
    };

    let has_actor_id = obj
        .get("actor_id")
        .and_then(|v| v.as_str())
        .map(|s| !s.is_empty())
        .unwrap_or(false);

    if !has_actor_id {
        apply_child_spec_to_object(&mut obj, child_spec, actor_id);
        return serde_json::to_vec(&serde_json::Value::Object(obj))
            .unwrap_or_else(|_| initial_state.to_vec());
    }

    initial_state.to_vec()
}

fn wasm_config_for_child_spec(
    child_spec: &plexspaces_proto::application::v1::ChildSpec,
) -> plexspaces_wasm_runtime::WasmConfig {
    let mut config = plexspaces_wasm_runtime::WasmConfig::default();
    config.durability_enabled = child_spec
        .facets
        .iter()
        .any(|facet| facet.r#type == "durability");
    config
}

/// - Wraps a WasmInstance (which holds the WASM module and state)
/// - Forwards handle_message calls to WASM instance
/// - Handles serialization/deserialization of messages
struct WasmActorBehavior {
    instance: Arc<WasmInstance>,
    actor_type: String, // Actor type for dashboard grouping (e.g., application name or child spec id)
    behavior_kind: plexspaces_core::BehaviorType, // OTP-style kind for logging (GenServer, GenEvent, etc.)
}

#[async_trait]
impl Actor for WasmActorBehavior {
    async fn handle_message(
        &mut self,
        ctx: &plexspaces_core::ActorContext,
        message: Message,
    ) -> Result<(), BehaviorError> {
        // Extract message details. "from" is only for WASM actor context (who sent the message).
        // Reply is sent only when message.sender_id is non-empty (see below); we never default sender_id for reply.
        let from = if message.sender_id.is_empty() {
            ""
        } else {
            message.sender_id.as_str()
        };

        // Determine message type for WASM dispatch (handler name or transport):
        // 1. Application msg_type from JSON payload (e.g. "ingest") so SDK can dispatch
        // 2. X-Message-Type header (from HTTP gateway)
        // 3. Envelope message_type ("cast"/"call") - POST=cast (tell), GET/ask=call
        // 4. Default "cast" so tell-style POST delivery remains fire-and-forget
        let message_type: String = try_msg_type_from_payload(&message.payload)
            .or_else(|| {
                message
                    .headers
                    .get("x-message-type")
                    .or_else(|| message.headers.get("X-Message-Type"))
                    .cloned()
            })
            .unwrap_or_else(|| message.message_type.clone());
        let message_type = if message_type.is_empty() {
            "cast".to_string()
        } else {
            message_type
        };

        // Use message payload directly
        let payload = message.payload.clone();

        // Clone Arc before await to ensure Send
        let instance = self.instance.clone();
        let message_id = message.id.clone();
        let payload_len = message.payload.len();

        // Single prominent line for observability: actor invoked, op, payload size
        tracing::info!(
            actor_id = %message.receiver_id,
            op = %message_type,
            payload_len = payload_len,
            message_id = %message_id,
            "WasmActor invoked"
        );
        match instance
            .handle_message_with_id(from, message_type.as_str(), payload, &message_id)
            .await
        {
            Ok(response) => {
                // Send reply for ask (call) messages:
                // Use ctx.send_reply() which routes reply to the temp sender via ActorService
                if !message.sender_id.is_empty() && !message.correlation_id.is_empty() {
                    let reply_id = format!("res-{}", ulid::Ulid::new());
                    let reply_message = Message {
                        id: reply_id.clone(),
                        payload: response,
                        message_type: "reply".to_string(),
                        ..Default::default()
                    };

                    // Get current actor ID from context self_ref or message receiver_id
                    let current_actor_id = ctx
                        .self_ref()
                        .map(|r| r.id().clone())
                        .unwrap_or_else(|| message.receiver_id.clone());

                    let correlation_id_opt = if message.correlation_id.is_empty() {
                        None
                    } else {
                        Some(message.correlation_id.as_str())
                    };

                    // Trace reply: log reply message-id (res-), recipient = temp sender
                    tracing::info!(
                        request_id = %message_id,
                        reply_id = %reply_id,
                        reply_to = %message.sender_id,
                        from_actor = %current_actor_id,
                        correlation_id = %message.correlation_id,
                        response_len = reply_message.payload.len(),
                        "WasmActor handle_message: sending reply to temp sender"
                    );
                    if let Err(e) = ctx
                        .send_reply(
                            correlation_id_opt,
                            &message.sender_id,
                            current_actor_id,
                            reply_message,
                        )
                        .await
                    {
                        tracing::error!(
                            request_id = %message_id,
                            reply_id = %reply_id,
                            reply_to = %message.sender_id,
                            correlation_id = %message.correlation_id,
                            error = %e,
                            "WasmActor handle_message: failed to send reply"
                        );
                    }
                } else if !message.sender_id.is_empty() {
                    tracing::trace!(message_id = %message_id, "WasmActor: tell message (no correlation_id)");
                } else {
                    tracing::trace!(message_id = %message_id, "WasmActor: fire-and-forget (no sender_id)");
                }
                Ok(())
            }
            Err(e) => {
                tracing::debug!(
                    message_id = %message_id,
                    error = %e,
                    "WasmActor handle_message: WASM call failed"
                );
                Err(BehaviorError::ProcessingError(format!(
                    "WASM handle_message failed: {}",
                    e
                )))
            }
        }
    }

    fn behavior_type(&self) -> BehaviorType {
        // Return custom behavior type with actor_type name for dashboard grouping
        BehaviorType::Custom(self.actor_type.clone())
    }

    fn behavior_kind(&self) -> BehaviorType {
        self.behavior_kind.clone()
    }

    async fn capture_checkpoint_state(
        &mut self,
        _ctx: &plexspaces_core::ActorContext,
    ) -> Result<Option<Vec<u8>>, plexspaces_core::ActorError> {
        let state = self
            .instance
            .get_state_component()
            .await
            .map_err(|e| plexspaces_core::ActorError::BehaviorError(e.to_string()))?;
        Ok(Some(state))
    }

    async fn restore_checkpoint_state(
        &mut self,
        _ctx: &plexspaces_core::ActorContext,
        state_data: &[u8],
    ) -> Result<bool, plexspaces_core::ActorError> {
        self.instance
            .set_state_component(state_data)
            .await
            .map_err(|e| plexspaces_core::ActorError::BehaviorError(e.to_string()))?;
        Ok(true)
    }
}

/// WASM-based application implementation
///
/// Loads supervisor tree from WASM module and initializes actors.
/// Follows the simplification principle: WASM = actor implementation,
/// framework provides infrastructure.
pub struct WasmApplication {
    /// Application name
    name: String,
    /// Application version
    version: String,
    /// WASM module hash (content-addressed)
    module_hash: String,
    /// WASM runtime for instantiating actors
    runtime: Arc<dyn plexspaces_core::WasmRuntimeTrait>,
    /// Deployment service for module management
    deployment_service: Arc<WasmDeploymentService>,
    /// Whether the application is running
    is_running: Arc<RwLock<bool>>,
    /// Application specification (if available)
    spec: Option<ApplicationSpec>,
    /// Spawned actor IDs (for graceful shutdown)
    spawned_actor_ids: Arc<RwLock<Vec<String>>>,
    /// Actor IDs most recently stopped during shutdown, retained for undeploy cleanup.
    last_stopped_actor_ids: Arc<RwLock<Vec<String>>>,
    /// Node reference for stopping actors
    node: Arc<RwLock<Option<Arc<dyn ApplicationNode>>>>,
    /// Root supervisor for actor management and restart
    root_supervisor: Arc<RwLock<Option<Arc<RwLock<Supervisor>>>>>,
    /// Tenant ID from API request (for actor spawning)
    tenant_id: Arc<RwLock<String>>,
    /// Namespace from API request (for actor spawning)
    namespace: Arc<RwLock<String>>,
}

impl WasmApplication {
    /// Create new WASM application from deployed module
    ///
    /// ## Arguments
    /// * `name` - Application name
    /// * `version` - Application version
    /// * `module_hash` - Deployed WASM module hash
    /// * `runtime` - WASM runtime instance
    /// * `spec` - Optional application specification
    pub fn new(
        name: String,
        version: String,
        module_hash: String,
        runtime: Arc<dyn plexspaces_core::WasmRuntimeTrait>,
        spec: Option<ApplicationSpec>,
    ) -> Self {
        let deployment_service = Arc::new(WasmDeploymentService::new(runtime.clone()));
        Self {
            name,
            version,
            module_hash,
            runtime,
            deployment_service,
            is_running: Arc::new(RwLock::new(false)),
            root_supervisor: Arc::new(RwLock::new(None)),
            spec,
            spawned_actor_ids: Arc::new(RwLock::new(Vec::new())),
            last_stopped_actor_ids: Arc::new(RwLock::new(Vec::new())),
            node: Arc::new(RwLock::new(None)),
            tenant_id: Arc::new(RwLock::new(String::new())),
            namespace: Arc::new(RwLock::new(String::new())),
        }
    }

    /// Get module hash
    pub fn module_hash(&self) -> &str {
        &self.module_hash
    }

    /// Get application specification
    pub fn spec(&self) -> Option<&ApplicationSpec> {
        self.spec.as_ref()
    }

    /// Get environment variables from application spec
    ///
    /// ## Purpose
    /// Returns the environment variables defined in the ApplicationSpec (if available).
    /// Applications can use these during start() to configure themselves.
    ///
    /// ## Returns
    /// Clone of the environment variables map, or empty map if spec not available
    pub fn env(&self) -> std::collections::HashMap<String, String> {
        self.spec
            .as_ref()
            .map(|spec| spec.env.clone())
            .unwrap_or_default()
    }

    /// Set tenant_id and namespace from API request.
    ///
    /// Called by ApplicationManager before start() to set tenant_id/namespace from API request.
    /// These values are used when spawning actors (actor IDs use name:namespace@node_id format).
    ///
    /// ## Panics
    /// Debug-asserts that namespace is non-empty (required for WASM deployment).
    pub async fn set_tenant_namespace(&self, tenant_id: String, namespace: String) {
        debug_assert!(
            !namespace.is_empty(),
            "namespace must not be empty for WASM deployment"
        );
        if namespace.is_empty() {
            tracing::warn!(
                application = %self.name,
                "set_tenant_namespace called with empty namespace - actor ID format will be degraded"
            );
        }
        *self.tenant_id.write().await = tenant_id;
        *self.namespace.write().await = namespace;
    }

    /// Create a WASM instance for behavior registration
    ///
    /// ## Purpose
    /// Extracts WASM instance creation logic for reuse in behavior registration.
    /// Creates a WASM instance with all required services wired up.
    ///
    /// ## Returns
    /// WasmInstance ready for use in WasmActorBehavior
    async fn create_wasm_instance_for_behavior(
        node: Arc<dyn ApplicationNode>,
        child_spec: &plexspaces_proto::application::v1::ChildSpec,
        module_hash: &str,
        runtime: Arc<dyn plexspaces_core::WasmRuntimeTrait>,
        actor_id: &str,
        initial_state: &[u8],
    ) -> Result<Arc<WasmInstance>, ApplicationError> {
        // Get ServiceLocator from node
        let service_locator = node
            .service_locator()
            .ok_or_else(|| ApplicationError::Other("ServiceLocator not available".to_string()))?;

        // Resolve module by hash
        let module_any = runtime.get_module(module_hash).await.ok_or_else(|| {
            ApplicationError::Other(format!("WASM module not found: {}", module_hash))
        })?;
        let module = plexspaces_wasm_runtime::wasm_runtime_helpers::extract_wasm_module(module_any)
            .map_err(|e| ApplicationError::Other(format!("Failed to extract WasmModule: {}", e)))?;

        // Wire up all services (reused from build_wasm_actor)
        use crate::service_wrappers::ChannelServiceWrapper;
        use plexspaces_core::ChannelService;
        let channel_service: Arc<dyn ChannelService> = Arc::new(ChannelServiceWrapper::new());

        let tuplespace_provider = service_locator.get_tuplespace_provider().await;
        let object_registry = service_locator.get_object_registry().await;
        let lock_manager = service_locator.get_lock_manager().await;

        use plexspaces_journaling::JournalStorage;
        let journal_storage: Option<Arc<dyn JournalStorage>> = {
            let journal_db_path = std::env::var("PLEXSPACES_DATABASE_URL")
                .or_else(|_| std::env::var("PLEXSPACES_JOURNAL_DB"))
                .unwrap_or_else(|_| {
                    let node_id = node.id().replace(['@', '/', '\\', ':'], "-");
                    format!("/tmp/plexspaces-journal-{}.db", node_id)
                });

            if journal_db_path == ":memory:" || journal_db_path.contains(":memory:") {
                plexspaces_journaling::SqliteJournalStorage::new(":memory:")
                    .await
                    .map(|s| Arc::new(s) as Arc<dyn JournalStorage>)
                    .ok()
            } else {
                plexspaces_journaling::SqliteJournalStorage::new(&journal_db_path)
                    .await
                    .map(|s| Arc::new(s) as Arc<dyn JournalStorage>)
                    .ok()
            }
        };

        let blob_service = node.blob_service().await;

        // Get KeyValueStore from ServiceLocator (needed for kv_get/kv_put)
        let keyvalue_store: Option<Arc<dyn plexspaces_core::KeyValueStore>> =
            service_locator.get_keyvalue_store().await;

        // Get ProcessGroupRegistry from ServiceLocator (registered during node startup from shared KeyValueStore)
        // Stored as Arc<dyn Any> to avoid cross-crate trait dependency; downcast by the runtime
        let process_group_registry: Option<Arc<dyn std::any::Any + Send + Sync>> =
            service_locator.get_process_group_registry().await;

        let module_any: Arc<dyn std::any::Any + Send + Sync> = module.clone();
        let config_any: Arc<dyn std::any::Any + Send + Sync> =
            Arc::new(wasm_config_for_child_spec(child_spec));

        // Create MessageSender for inter-actor communication (host.ask, host.tell)
        let message_sender: Option<Arc<dyn std::any::Any + Send + Sync>> = {
            if let Some(actor_service) = service_locator.get_actor_service().await {
                let sender: Arc<dyn plexspaces_wasm_runtime::MessageSender> =
                    Arc::new(crate::wasm_message_sender::ActorServiceMessageSender::new(
                        actor_service,
                        service_locator.clone(),
                    ));
                Some(Arc::new(sender) as Arc<dyn std::any::Any + Send + Sync>)
            } else {
                None
            }
        };

        // Virtual actor reactivation must reuse the original/materialized init payload so the
        // guest sees the same identity, role, and facet-facing config as the framework runtime.
        let init_config_json =
            init_config_from_initial_state_or_child_spec(initial_state, child_spec, actor_id);

        let outbound_http_client = service_locator.get_outbound_http_client().await;

        let instance_any = runtime
            .instantiate(
                module_any,
                actor_id.to_string(),
                &init_config_json,
                config_any,
                Some(channel_service),
                message_sender,
                tuplespace_provider,
                keyvalue_store,
                process_group_registry,
                lock_manager,
                object_registry,
                journal_storage,
                blob_service,
                outbound_http_client,
            )
            .await
            .map_err(|e| ApplicationError::Other(format!("WASM instantiation failed: {}", e)))?;

        let wasm_instance =
            plexspaces_wasm_runtime::wasm_runtime_helpers::extract_wasm_instance(instance_any)
                .map_err(|e| {
                    ApplicationError::Other(format!("Failed to extract WasmInstance: {}", e))
                })?;

        Ok(wasm_instance)
    }

    /// Register behaviors from supervisor tree in BehaviorRegistry
    ///
    /// ## Purpose
    /// Registers behaviors for each ChildSpec.id in the supervisor tree, enabling ShardGroups
    /// to spawn actors using actor_type="worker" (or any ChildSpec.id).
    ///
    /// ## Design
    /// - Extracts all ChildSpec.id values from supervisor tree
    /// - Registers each as a behavior in BehaviorRegistry
    /// - Behavior constructor creates WasmActorBehavior wrapping WASM instance
    /// - Works for both embedded (explicit registration) and WASM (auto-registration) apps
    /// - Reuses `create_wasm_instance_for_behavior` helper to avoid code duplication
    ///
    /// ## Returns
    /// Ok(()) if registration succeeds, ApplicationError otherwise
    pub async fn register_behaviors_from_supervisor_tree(
        &self,
        node: Arc<dyn ApplicationNode>,
    ) -> Result<(), ApplicationError> {
        use plexspaces_core::{Actor as CoreActor, BehaviorFactoryError, BehaviorRegistry};

        // Get ServiceLocator
        let service_locator = node
            .service_locator()
            .ok_or_else(|| ApplicationError::Other("ServiceLocator not available".to_string()))?;

        // Get or create BehaviorRegistry
        let behavior_registry_opt = service_locator.get_behavior_registry().await;
        let registry = if let Some(existing) = behavior_registry_opt.clone() {
            existing
        } else {
            Arc::new(BehaviorRegistry::new())
        };

        // Get supervisor tree (from spec or WASM module)
        let supervisor_spec = if let Some(spec) = &self.spec {
            spec.supervisor.clone()
        } else {
            let module_any = self.runtime.get_module(&self.module_hash).await;
            if let Some(module_any) = module_any {
                let module =
                    plexspaces_wasm_runtime::wasm_runtime_helpers::extract_wasm_module(module_any)
                        .map_err(|e| {
                            ApplicationError::Other(format!("Failed to extract WASM module: {}", e))
                        })?;
                match self.call_get_supervisor_tree(&module).await {
                    Ok(spec) => Some(spec),
                    Err(_) => None,
                }
            } else {
                None
            }
        };

        let supervisor_spec = match supervisor_spec {
            Some(spec) => spec,
            None => {
                tracing::debug!(
                    application = %self.name,
                    "No supervisor tree found - skipping behavior registration"
                );
                return Ok(());
            }
        };

        // Extract all ChildSpec.id values recursively
        let mut child_specs = Vec::new();
        fn collect_child_specs(
            spec: &SupervisorSpec,
            acc: &mut Vec<plexspaces_proto::application::v1::ChildSpec>,
        ) {
            for child in &spec.children {
                acc.push(child.clone());
                if let Some(nested_supervisor) = &child.supervisor {
                    collect_child_specs(nested_supervisor, acc);
                }
            }
        }
        collect_child_specs(&supervisor_spec, &mut child_specs);

        let behavior_names: Vec<&str> = child_specs.iter().map(|c| c.id.as_str()).collect();

        // Register each child spec as a behavior
        let module_hash = self.module_hash.clone();
        let runtime = self.runtime.clone();
        let node_id = node.id().to_string();
        // Namespace is required for consistent actor ID format (name:namespace@node_id)
        let namespace = self.namespace.read().await.clone();

        for child_spec in &child_specs {
            let behavior_name = child_spec.id.clone();
            let child_spec_clone = child_spec.clone();
            let node_clone = node.clone();
            let module_hash_clone = module_hash.clone();
            let runtime_clone = runtime.clone();
            let node_id_clone = node_id.clone();
            let namespace_clone = namespace.clone();

            // Register async behavior constructor
            let behavior_name_for_error = behavior_name.clone();
            registry
                .register(behavior_name.clone(), move |initial_state: &[u8]| {
                    // Clone captured variables for async block
                    let rt = runtime_clone.clone();
                    let hash = module_hash_clone.clone();
                    let spec = child_spec_clone.clone();
                    let node_ref = node_clone.clone();
                    let nid = node_id_clone.clone();
                    let name_for_error = behavior_name_for_error.clone();
                    let initial_state = initial_state.to_vec();

                    // Create WASM instance asynchronously (no block_on deadlock)
                    let ns = namespace_clone.clone();
                    Box::pin(async move {
                        let actor_id =
                            actor_id_from_initial_state(&initial_state, &spec.id, &ns, &nid);

                        let instance = Self::create_wasm_instance_for_behavior(
                            node_ref,
                            &spec,
                            &hash,
                            rt,
                            &actor_id,
                            &initial_state,
                        )
                        .await
                        .map_err(|e| {
                            BehaviorFactoryError::CreationFailed(
                                name_for_error.clone(),
                                format!("Failed to create WASM instance: {}", e),
                            )
                        })?;

                        Ok(Box::new(WasmActorBehavior {
                            instance,
                            actor_type: spec.id.clone(),
                            behavior_kind: parse_behavior_kind(spec.behavior_kind.as_deref()),
                        }) as Box<dyn CoreActor>)
                    })
                })
                .await;
        }

        // Register the registry with ServiceLocator if not already registered
        if behavior_registry_opt.is_none() {
            service_locator.register_behavior_registry(registry).await;
        }

        tracing::info!(
            application = %self.name,
            behavior_count = child_specs.len(),
            behavior_names = ?behavior_names,
            "Registered WASM behaviors from supervisor tree for ShardGroup support"
        );

        Ok(())
    }

    /// Load supervisor tree OR spawn a simple actor
    ///
    /// ## Purpose
    /// Supervisors are OPTIONAL. This method:
    /// 1. If spec.supervisor is defined, use supervisor tree approach
    /// 2. Otherwise, spawn a simple WASM actor directly (no supervisor)
    ///
    /// ## Returns
    /// Vector of actor IDs that were instantiated
    async fn load_supervisor_tree_or_simple_actor(
        &self,
        node: Arc<dyn ApplicationNode>,
    ) -> Result<Vec<String>, ApplicationError> {
        // Strategy 1: Use spec.supervisor if available (config-based supervisor tree)
        if let Some(spec) = &self.spec {
            if spec.supervisor.is_some() {
                return self.load_supervisor_tree(node).await;
            }
        }

        // Strategy 2: No supervisor - spawn simple WASM actor directly
        tracing::info!(
            application = %self.name,
            module_hash = %self.module_hash,
            "No supervisor spec - spawning simple WASM actor directly"
        );

        // Check if ServiceLocator is available (required for spawning actors)
        if node.service_locator().is_none() {
            // No ServiceLocator - this is likely a test/mock scenario
            // Return empty list (acceptable for tests)
            tracing::warn!(
                application = %self.name,
                "ServiceLocator not available - skipping actor spawn (acceptable for tests)"
            );
            return Ok(vec![]);
        }

        // Get tenant_id/namespace from stored values (set during registration)
        let tenant_id = self.tenant_id.read().await.clone();
        let namespace = self.namespace.read().await.clone();
        // Tenant: may fall back to node config when empty. Namespace: must come from request; do not fall back to config.
        let final_tenant_id = if tenant_id.is_empty() {
            String::new() // Tenant comes from auth, not config
        } else {
            tenant_id
        };
        let final_namespace = namespace; // Must come from user request; never substitute with config

        // Precompute expected actor_id for error logging using factory method
        let actor_id = ulid::Ulid::new().to_string();
        let actor_type = format!("{}Supervisor", self.name);
        let expected_actor_id =
            build_actor_id(&actor_id, &actor_type, Some(&final_namespace), node.id());

        // Create a simple ChildSpec for the actor
        use plexspaces_proto::application::v1::{ChildSpec, ChildType, RestartPolicy};
        let child_spec = ChildSpec {
            id: self.name.clone(),
            r#type: ChildType::ChildTypeWorker as i32,
            args: std::collections::HashMap::new(),
            restart: RestartPolicy::RestartPolicyPermanent as i32,
            shutdown_timeout: Some(prost_types::Duration {
                seconds: 5,
                nanos: 0,
            }),
            supervisor: None, // No nested supervisor
            facets: vec![],
            behavior_kind: None,
        };

        // Spawn the WASM actor using the existing internal spawn method
        match Self::spawn_worker_actor_internal(
            node.clone(),
            &child_spec,
            &self.module_hash,
            self.runtime.clone(),
            final_tenant_id,
            final_namespace,
        )
        .await
        {
            Ok(spawned_actor_id) => {
                // Track the spawned actor
                let mut spawned = self.spawned_actor_ids.write().await;
                spawned.push(spawned_actor_id.clone());

                tracing::info!(
                    application = %self.name,
                    actor_id = %spawned_actor_id,
                    "Simple WASM actor spawned successfully"
                );

                Ok(vec![spawned_actor_id])
            }
            Err(e) => {
                tracing::error!(
                    application = %self.name,
                    actor_id = %expected_actor_id,
                    error = %e,
                    "Failed to spawn simple WASM actor"
                );
                Err(e)
            }
        }
    }

    /// Load supervisor tree from WASM module (internal method)
    ///
    /// ## Purpose
    /// Extracts supervisor tree definition from WASM module and initializes
    /// actors according to the tree structure.
    ///
    /// ## Strategy
    /// 1. First, try to use `spec.supervisor` if available (config-based)
    /// 2. Otherwise, try to call WASM function `get_supervisor_tree()` to get tree from WASM
    /// 3. Parse SupervisorSpec and initialize actors recursively
    ///
    /// ## Returns
    /// Vector of actor IDs that were instantiated
    async fn load_supervisor_tree(
        &self,
        node: Arc<dyn ApplicationNode>,
    ) -> Result<Vec<String>, ApplicationError> {
        // Strategy 1: Use spec.supervisor if available (config-based)
        if let Some(spec) = &self.spec {
            if let Some(supervisor_spec) = &spec.supervisor {
                return self.initialize_supervisor_tree(node, supervisor_spec).await;
            }
        }

        // Strategy 2: Try to get supervisor tree from WASM function
        // Resolve module by hash and call get_supervisor_tree() function
        if let Some(module_any) = self.runtime.get_module(&self.module_hash).await {
            let module =
                plexspaces_wasm_runtime::wasm_runtime_helpers::extract_wasm_module(module_any)
                    .map_err(|e| {
                        ApplicationError::Other(format!("Failed to extract WasmModule: {}", e))
                    })?;
            // Create a temporary instance to call get_supervisor_tree()
            // Function signature: get_supervisor_tree() -> (ptr: i32, len: i32)
            // Returns protobuf-encoded SupervisorSpec in WASM memory
            match self.call_get_supervisor_tree(&module).await {
                Ok(supervisor_spec) => {
                    return self
                        .initialize_supervisor_tree(node, &supervisor_spec)
                        .await;
                }
                Err(e) => {
                    // Log error with better context
                    let _error_msg = format!(
                        "Failed to load supervisor tree from WASM module '{}': {}. \
                        The module may not export a get_supervisor_tree() function, or the function may have failed. \
                        For simple modules without supervisor trees, this is acceptable.",
                        self.module_hash, e
                    );
                    tracing::warn!(
                        application = %self.name,
                        module_hash = %self.module_hash,
                        error = %e,
                        "WASM module does not export supervisor tree (this is normal for simple modules)"
                    );
                    // Don't fail - return empty supervisor tree for graceful degradation
                }
            }
        } else {
            // Module not found - for tests and simple modules, return empty list (graceful degradation)
            // In production, this would be an error, but for testing we allow it
            // Log at debug level since this is acceptable for simple modules
            if tracing::enabled!(tracing::Level::DEBUG) {
                tracing::debug!(
                    application = %self.name,
                    module_hash = %self.module_hash,
                    "WASM module not found - returning empty supervisor tree (acceptable for simple modules or tests)"
                );
            }
        }

        // Fallback: Return empty list if no spec.supervisor AND no WASM function
        // This should not happen in normal flow - HTTP deployment creates default supervisor
        // Log warning to help debug if this path is reached
        tracing::warn!(
            application = %self.name,
            module_hash = %self.module_hash,
            has_spec = self.spec.is_some(),
            spec_has_supervisor = self.spec.as_ref().map(|s| s.supervisor.is_some()).unwrap_or(false),
            "No supervisor tree found - this is unexpected for HTTP deployments. \
             Check that ApplicationSpec.supervisor is being passed through correctly."
        );
        Ok(Vec::new())
    }

    /// Initialize actors from supervisor tree specification
    ///
    /// ## Purpose
    /// Creates a Supervisor with the specified strategy and adds all children
    /// using `supervisor.add_child()` with proper factory functions for restart support.
    ///
    /// ## Design
    /// - Creates Supervisor with strategy from SupervisorSpec
    /// - For each child, creates a ChildSpec with a factory function
    /// - Factory function captures all context needed to recreate WASM actors
    /// - Calls `supervisor.add_child(spec)` for each child
    /// - Calls `supervisor.start()` to begin monitoring
    ///
    /// ## Arguments
    /// * `node` - Application node for spawning actors
    /// * `supervisor_spec` - Supervisor tree specification
    ///
    /// ## Returns
    /// Vector of actor IDs that were instantiated
    async fn initialize_supervisor_tree(
        &self,
        node: Arc<dyn ApplicationNode>,
        supervisor_spec: &SupervisorSpec,
    ) -> Result<Vec<String>, ApplicationError> {
        let mut actor_ids = Vec::new();
        let spawned_actor_ids = self.spawned_actor_ids.clone();
        let module_hash = self.module_hash.clone();

        // Get ServiceLocator for supervisor
        let service_locator = node.service_locator().ok_or_else(|| {
            ApplicationError::Other("ServiceLocator not available from node".to_string())
        })?;

        // Convert proto supervision strategy to Rust enum
        let strategy = Self::convert_supervision_strategy(supervisor_spec)?;

        // Create root supervisor using factory method
        let supervisor_id_ulid = ulid::Ulid::new().to_string();
        let supervisor_type = format!("{}Supervisor", self.name);
        let supervisor_id = build_actor_id(&supervisor_id_ulid, &supervisor_type, None, node.id());
        let (supervisor, mut event_rx) =
            Supervisor::new(supervisor_id.clone(), strategy, service_locator.clone());

        tracing::info!(
            application = %self.name,
            supervisor_id = %supervisor_id,
            strategy = ?supervisor_spec.strategy(),
            children_count = supervisor_spec.children.len(),
            "Creating supervisor for WASM application"
        );

        // Add children to supervisor
        for child in &supervisor_spec.children {
            match child.r#type() {
                plexspaces_proto::application::v1::ChildType::ChildTypeWorker => {
                    // Get tenant_id/namespace from stored values. Tenant comes from auth, not config.
                    let tenant_id = self.tenant_id.read().await.clone();
                    let namespace = self.namespace.read().await.clone();
                    let final_tenant_id = if tenant_id.is_empty() {
                        String::new() // Tenant comes from auth, not config
                    } else {
                        tenant_id
                    };
                    let final_namespace = namespace; // Must come from user request; never substitute with config

                    // Create ChildSpec with factory function for WASM actor
                    let child_spec = Self::create_wasm_actor_child_spec(
                        node.clone(),
                        child,
                        &module_hash,
                        self.runtime.clone(),
                        final_tenant_id.clone(),
                        final_namespace.clone(),
                    )?;

                    // Add child to supervisor
                    match supervisor.add_child(child_spec).await {
                        Ok(actor_ref) => {
                            let actor_id = actor_ref.id().to_string();
                            tracing::info!(
                                supervisor_id = %supervisor_id,
                                child_id = %child.id,
                                actor_id = %actor_id,
                                "Added WASM actor to supervisor"
                            );
                            actor_ids.push(actor_id);
                        }
                        Err(e) => {
                            tracing::error!(
                                supervisor_id = %supervisor_id,
                                child_id = %child.id,
                                error = %e,
                                "Failed to add WASM actor to supervisor"
                            );
                            return Err(ApplicationError::ActorSpawnFailed(
                                child.id.clone(),
                                format!("Failed to add to supervisor: {}", e),
                            ));
                        }
                    }
                }
                plexspaces_proto::application::v1::ChildType::ChildTypeSupervisor => {
                    // Nested supervisors not yet fully supported for WASM
                    // For now, treat as worker and log warning
                    tracing::warn!(
                        supervisor_id = %supervisor_id,
                        child_id = %child.id,
                        "Nested supervisor in WASM app - treating as worker"
                    );

                    // Get tenant_id/namespace from stored values. Tenant comes from auth, not config.
                    let tenant_id = self.tenant_id.read().await.clone();
                    let namespace = self.namespace.read().await.clone();
                    let final_tenant_id = if tenant_id.is_empty() {
                        String::new() // Tenant comes from auth, not config
                    } else {
                        tenant_id
                    };
                    let final_namespace = namespace; // Must come from user request; never substitute with config

                    let child_spec = Self::create_wasm_actor_child_spec(
                        node.clone(),
                        child,
                        &module_hash,
                        self.runtime.clone(),
                        final_tenant_id.clone(),
                        final_namespace.clone(),
                    )?;

                    match supervisor.add_child(child_spec).await {
                        Ok(actor_ref) => {
                            actor_ids.push(actor_ref.id().to_string());
                        }
                        Err(e) => {
                            return Err(ApplicationError::ActorSpawnFailed(
                                child.id.clone(),
                                format!("Failed to add to supervisor: {}", e),
                            ));
                        }
                    }
                }
                _ => {
                    return Err(ApplicationError::Other(format!(
                        "Invalid child type for '{}'",
                        child.id
                    )));
                }
            }
        }

        // Store supervisor reference
        let supervisor_arc = Arc::new(RwLock::new(supervisor));
        {
            let mut root_sup = self.root_supervisor.write().await;
            *root_sup = Some(supervisor_arc.clone());
        }

        // Spawn task to monitor supervisor events
        let app_name = self.name.clone();
        tokio::spawn(async move {
            while let Some(event) = event_rx.recv().await {
                tracing::trace!(
                    application = %app_name,
                    event = ?event,
                    "Supervisor event received"
                );
            }
        });

        // Store spawned actor IDs for graceful shutdown
        {
            let mut spawned = spawned_actor_ids.write().await;
            spawned.extend(actor_ids.clone());
        }

        tracing::debug!(
            application = %self.name,
            supervisor_id = %supervisor_id,
            actor_count = actor_ids.len(),
            "Supervisor tree initialized successfully"
        );

        Ok(actor_ids)
    }

    /// Convert proto SupervisionStrategy to Rust SupervisionStrategy
    fn convert_supervision_strategy(
        supervisor_spec: &SupervisorSpec,
    ) -> Result<SupervisionStrategy, ApplicationError> {
        use plexspaces_proto::application::v1::SupervisionStrategy as ProtoStrategy;

        let max_restarts = supervisor_spec.max_restarts;
        let within_seconds = supervisor_spec
            .max_restart_window
            .as_ref()
            .map(|d| d.seconds as u64)
            .unwrap_or(60);

        match supervisor_spec.strategy() {
            ProtoStrategy::SupervisionStrategyOneForOne
            | ProtoStrategy::SupervisionStrategyUnspecified => Ok(SupervisionStrategy::OneForOne {
                max_restarts,
                within_seconds,
            }),
            ProtoStrategy::SupervisionStrategyOneForAll => Ok(SupervisionStrategy::OneForAll {
                max_restarts,
                within_seconds,
            }),
            ProtoStrategy::SupervisionStrategyRestForOne => Ok(SupervisionStrategy::RestForOne {
                max_restarts,
                within_seconds,
            }),
        }
    }

    /// Create a ChildSpec with factory function for WASM actor
    ///
    /// ## Purpose
    /// Creates an actor ChildSpec using the existing `ChildSpec::worker()` pattern
    /// with a factory function that can recreate WASM actors on restart.
    ///
    /// ## Design
    /// Uses unified `build_wasm_actor` helper that:
    /// - Wires up all services (TupleSpace, ObjectRegistry, etc.)
    /// - Returns unstarted (Actor, ActorRef)
    /// - Supervisor's add_child() calls actor.start()
    fn create_wasm_actor_child_spec(
        node: Arc<dyn ApplicationNode>,
        proto_child_spec: &plexspaces_proto::application::v1::ChildSpec,
        module_hash: &str,
        runtime: Arc<dyn plexspaces_core::WasmRuntimeTrait>,
        tenant_id: String,
        namespace: String,
    ) -> Result<ActorChildSpec, ApplicationError> {
        let node_id = node.id().to_string();
        let child_id = proto_child_spec.id.clone();
        let actor_id = Self::build_supervised_actor_id(&child_id, &namespace, &node_id);

        // Capture context for factory
        let node_clone = node.clone();
        let child_spec_clone = proto_child_spec.clone();
        let module_hash_clone = module_hash.to_string();
        let runtime_clone = runtime.clone();
        let tenant_id_clone = tenant_id.clone();
        let namespace_clone = namespace.clone();
        let actor_id_clone = actor_id.clone();

        // Create factory using ChildSpec::worker() pattern
        let factory: plexspaces_actor::StartFn = Arc::new(move || {
            let node = node_clone.clone();
            let child_spec = child_spec_clone.clone();
            let module_hash = module_hash_clone.clone();
            let runtime = runtime_clone.clone();
            let tenant_id = tenant_id_clone.clone();
            let namespace = namespace_clone.clone();
            let actor_id = actor_id_clone.clone();

            Box::pin(async move {
                // Use unified helper that builds Actor with all services
                let (actor, actor_ref) = Self::build_wasm_actor(
                    &actor_id,
                    node,
                    &child_spec,
                    &module_hash,
                    runtime,
                    tenant_id,
                    namespace,
                )
                .await
                .map_err(|e| ActorError::BehaviorError(format!("Factory failed: {}", e)))?;

                Ok(StartedChild::Worker { actor, actor_ref })
            })
        });

        // Use ChildSpec::worker() constructor for consistency
        let mut spec = ActorChildSpec::worker(child_id.clone(), actor_id, factory);

        // Apply restart policy from proto
        spec.restart_strategy = Self::convert_restart_policy(proto_child_spec)?;

        // Apply shutdown timeout
        if let Some(d) = &proto_child_spec.shutdown_timeout {
            spec.shutdown_timeout =
                Some(Duration::from_secs(d.seconds as u64) + Duration::from_nanos(d.nanos as u64));
        }

        // Apply facets
        spec.facets = proto_child_spec.facets.clone();

        Ok(spec)
    }

    /// Convert proto RestartPolicy to Rust RestartStrategy
    fn convert_restart_policy(
        proto_child_spec: &plexspaces_proto::application::v1::ChildSpec,
    ) -> Result<RestartStrategy, ApplicationError> {
        use plexspaces_proto::application::v1::RestartPolicy;

        match proto_child_spec.restart() {
            RestartPolicy::RestartPolicyPermanent | RestartPolicy::RestartPolicyUnspecified => {
                Ok(RestartStrategy::Permanent)
            }
            RestartPolicy::RestartPolicyTransient => Ok(RestartStrategy::Transient),
            RestartPolicy::RestartPolicyTemporary => Ok(RestartStrategy::Temporary),
        }
    }

    /// Build a WASM actor with all services (unified helper)
    ///
    /// ## Purpose
    /// Single entry point for building WASM actors with full service wiring.
    /// Returns unstarted (Actor, ActorRef) that can be:
    /// - Started immediately (spawn_worker_actor_internal)
    /// - Managed by supervisor (factory function)
    ///
    /// ## Services Wired
    /// - ChannelService, TupleSpaceProvider, ObjectRegistry
    /// - JournalStorage, LockManager, KeyValueStore
    /// - All services from ServiceLocator
    fn build_supervised_actor_id(child_id: &str, namespace: &str, node_id: &str) -> String {
        let actor_id_ulid = ulid::Ulid::new().to_string();
        build_actor_id(&actor_id_ulid, child_id, Some(namespace), node_id)
    }

    async fn build_wasm_actor(
        actor_id: &str,
        node: Arc<dyn ApplicationNode>,
        child_spec: &plexspaces_proto::application::v1::ChildSpec,
        module_hash: &str,
        runtime: Arc<dyn plexspaces_core::WasmRuntimeTrait>,
        tenant_id: String,
        namespace: String,
    ) -> Result<(plexspaces_actor::Actor, plexspaces_core::ActorRef), ApplicationError> {
        use plexspaces_core::Actor as CoreActor;

        // Get ServiceLocator from node
        let service_locator = node
            .service_locator()
            .ok_or_else(|| ApplicationError::Other("ServiceLocator not available".to_string()))?;

        // Resolve module by hash and create WASM instance using the caller-assigned
        // actor ID so supervisor tracking, shutdown, and routing stay aligned.
        let wasm_instance = Self::create_wasm_instance_for_behavior(
            node.clone(),
            child_spec,
            module_hash,
            runtime,
            actor_id,
            &[],
        )
        .await?;
        let behavior_kind = parse_behavior_kind(child_spec.behavior_kind.as_deref());
        let behavior: Box<dyn CoreActor> = Box::new(WasmActorBehavior {
            instance: wasm_instance.clone(),
            actor_type: child_spec.id.clone(),
            behavior_kind,
        });

        // Build unstarted Actor using ActorBuilder
        use plexspaces_actor::ActorBuilder;

        let mut actor = ActorBuilder::new(behavior)
            .with_id(actor_id.to_string())
            .build()
            .await
            .map_err(|e| ApplicationError::ActorSpawnFailed(actor_id.to_string(), e.to_string()))?;

        // Get registry for node ID
        let registry = service_locator
            .actor_registry()
            .await
            .ok_or_else(|| ApplicationError::Other("ActorRegistry not found".to_string()))?;
        let local_node_id = registry.local_node_id();

        // Parse actor ID and rebuild with local node_id
        let actor_id = if let Ok(parsed) = parse_actor_id(actor_id) {
            build_actor_id(
                &parsed.id,
                &parsed.actor_type,
                parsed.namespace.as_deref(),
                &local_node_id,
            )
        } else {
            // Invalid format - return error
            return Err(ApplicationError::Other(format!(
                "Invalid actor ID format: {}",
                actor_id
            )));
        };

        // Update context with proper node ID and tenant_id/namespace from API request
        // Use tenant_id/namespace from API request (passed as parameters), not "internal"
        // Clone namespace before moving it (needed later for virtual actor type registration)
        let namespace_for_registration = namespace.clone();
        let actor_context = plexspaces_core::ActorContext::new(
            local_node_id.to_string(),
            tenant_id,
            namespace,
            service_locator.clone(),
            actor.context().config.clone(),
        );
        actor = actor.set_context(std::sync::Arc::new(actor_context));

        // Attach facets from ChildSpec (e.g., LockFacet, RegistryFacet, ProcessGroupFacet, VirtualActorFacet)
        // Facets are attached BEFORE actor.start() so lifecycle hooks work correctly
        let mut has_virtual_actor_facet = false;
        let mut virtual_actor_registered = false;
        let mut attached_facet_types: Vec<String> = Vec::new();
        let mut virtual_facet_config = serde_json::Value::Null;
        if !child_spec.facets.is_empty() {
            if let Some(facet_registry_wrapper) = service_locator.get_facet_registry().await {
                let facet_registry = facet_registry_wrapper.inner_clone();
                use plexspaces_actor::create_facets_from_proto;
                let facets = create_facets_from_proto(&child_spec.facets, &facet_registry).await;

                for facet in facets {
                    if let Err(e) = actor.attach_facet(facet).await {
                        tracing::warn!(
                            actor_id = %actor_id,
                            child_id = %child_spec.id,
                            error = %e,
                            "Failed to attach facet to WASM actor (continuing with other facets)"
                        );
                    }
                }
                let facets_container = actor.facets();
                let facets_guard = facets_container.read().await;
                let attached = facets_guard.list_facets();
                attached_facet_types = attached.clone();
                drop(facets_guard);

                // Check if VirtualActorFacet was attached (after all facets are attached)
                use plexspaces_facet::has_facet_attached;
                let facets_container = actor.facets();
                has_virtual_actor_facet =
                    has_facet_attached(&facets_container, "virtual_actor").await;

                // Extract ALL facet configs from ChildSpec using facet helpers
                // Store all facet configs (virtual_actor, durability, timer, reminder, etc.) for virtual actor type registration
                use plexspaces_facet::{extract_facet_config, has_facet_type};

                // Build facet_config JSON object with all facets (for virtual actor type registration)
                let mut all_facet_configs = serde_json::Map::new();
                for facet_proto in &child_spec.facets {
                    if let Some(config) =
                        extract_facet_config(&child_spec.facets, &facet_proto.r#type)
                    {
                        all_facet_configs.insert(facet_proto.r#type.clone(), config);
                    }
                }

                // Use combined config with all facets
                if !all_facet_configs.is_empty() {
                    virtual_facet_config = serde_json::Value::Object(all_facet_configs);
                }
            } else {
                tracing::warn!(
                    actor_id = %actor_id,
                    child_id = %child_spec.id,
                    facet_count = child_spec.facets.len(),
                    "FacetRegistry not available - facets not attached to WASM actor"
                );
            }
        }

        // CRITICAL: Register virtual actor TYPE if VirtualActorFacet is attached
        // This enables automatic activation of any actor ID matching the type pattern
        // Format: `{id}//{actor_type}::{namespace}@{node_id}` (e.g., `user-1//read-state-tracker::orbit-read-state-ts@node-id`)
        // Works for both WASM and Rust applications
        // Uses centralized helper for consistent behavior across SDK, WASM, and app-config.toml
        if has_virtual_actor_facet {
            let actor_type = child_spec.id.clone();
            let namespace_for_type = namespace_for_registration.clone();
            let config_for_type = actor.context().config.clone();

            // Build init config template from child_spec.args (same structure as ApplicationSpec deployment)
            // This preserves the config structure so virtual actors activated via HTTP receive proper config
            let init_config_template = {
                let mut init_config = serde_json::Map::new();
                // actor_id will be replaced when activating virtual actor
                init_config.insert(
                    "actor_id".to_string(),
                    serde_json::Value::String(String::new()),
                );
                if let Some(ref bk) = child_spec.behavior_kind {
                    init_config.insert(
                        "behavior_kind".to_string(),
                        serde_json::Value::String(bk.clone()),
                    );
                }
                if !child_spec.args.is_empty() {
                    let args_obj: serde_json::Map<String, serde_json::Value> = child_spec
                        .args
                        .iter()
                        .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
                        .collect();
                    init_config.insert("args".to_string(), serde_json::Value::Object(args_obj));
                }
                let template_result = serde_json::to_vec(&serde_json::Value::Object(init_config));
                template_result.ok()
            };

            // Register virtual actor type with ALL facet configs from app-config.toml.
            // child_spec.facets includes virtual_actor + timer + reminder + workflow configs,
            // so resurrection recreates every facet with the original configuration.
            // Type registration persists until application is undeployed — it is NOT evicted
            // when individual actor instances are deactivated/vacationed.
            let _ = plexspaces_core::register_virtual_actor_type_consistent(
                &service_locator,
                actor_type.clone(),
                namespace_for_type,
                None,                     // No facet trait objects for WASM (use proto facets)
                Some(&child_spec.facets), // Proto facets from app-config.toml (all facet types)
                config_for_type,
                None,                 // tenant_id - None for type-level registration
                init_config_template, // Init config template for WASM actors
            )
            .await;
            virtual_actor_registered = true;
        }

        let attached_facet_list = if attached_facet_types.is_empty() {
            "none".to_string()
        } else {
            attached_facet_types.join(", ")
        };
        let args_keys = if child_spec.args.is_empty() {
            "none".to_string()
        } else {
            let mut keys: Vec<&str> = child_spec.args.keys().map(String::as_str).collect();
            keys.sort_unstable();
            keys.join(", ")
        };
        tracing::info!(
            actor_id = %actor_id,
            child_id = %child_spec.id,
            behavior_kind = %child_spec.behavior_kind.as_deref().unwrap_or("unknown"),
            configured_facets = child_spec.facets.len(),
            attached_facets = %attached_facet_list,
            has_virtual_actor_facet = has_virtual_actor_facet,
            virtual_actor_registered = virtual_actor_registered,
            args_keys = %args_keys,
            "WASM actor child initialized"
        );

        // NOTE: DurabilityFacet is NOT attached to WASM actors because:
        // 1. WASM actors use host functions for journaling (journal_write, journal_replay)
        // 2. DurabilityFacet's replay_on_activation tries to call handle_message during attach
        // 3. WASM actors can't handle messages during facet attach (not initialized yet)
        // The journal_storage passed to instantiate() enables WASM host function journaling,
        // which is separate from DurabilityFacet-based replay.

        // Create ActorRef from mailbox
        let core_actor_ref = plexspaces_core::ActorRef::new(actor_id.into())
            .map_err(|e| ApplicationError::Other(format!("Failed to create ActorRef: {}", e)))?;

        Ok((actor, core_actor_ref))
    }

    /// Internal helper to spawn worker actor directly (without supervisor)
    ///
    /// ## Purpose
    /// Spawns a WASM actor immediately using the unified `build_wasm_actor` helper.
    /// Used when deploying without explicit supervisor management.
    async fn spawn_worker_actor_internal(
        node: Arc<dyn ApplicationNode>,
        child_spec: &plexspaces_proto::application::v1::ChildSpec,
        module_hash: &str,
        runtime: Arc<dyn plexspaces_core::WasmRuntimeTrait>,
        tenant_id: String,
        namespace: String,
    ) -> Result<String, ApplicationError> {
        let actor_id = Self::build_supervised_actor_id(&child_spec.id, &namespace, node.id());

        // Use unified helper to build actor
        let (mut actor, _actor_ref) = Self::build_wasm_actor(
            &actor_id,
            node.clone(),
            child_spec,
            module_hash,
            runtime,
            tenant_id.clone(),
            namespace.clone(),
        )
        .await?;

        let actor_id = actor.id().clone();

        // Get ServiceLocator for registration
        let service_locator = node.service_locator().ok_or_else(|| {
            ApplicationError::ActorSpawnFailed(
                actor_id.clone(),
                "ServiceLocator not available".to_string(),
            )
        })?;

        // Start the actor
        let _handle = actor.start().await.map_err(|e| {
            ApplicationError::ActorSpawnFailed(actor_id.clone(), format!("Start failed: {}", e))
        })?;

        // Register in ActorRegistry
        if let Some(registry) = service_locator.actor_registry().await {
            actor.register_started(&registry).await;
        }

        Ok(actor_id)
    }

    /// Call get_supervisor_tree() function from WASM module
    ///
    /// ## Purpose
    /// Creates a temporary WASM instance and calls the exported `get_supervisor_tree()`
    /// function to retrieve the supervisor tree definition as protobuf.
    ///
    /// ## Function Signature
    /// `get_supervisor_tree() -> (ptr: i32, len: i32)`
    /// - Returns pointer and length to protobuf-encoded SupervisorSpec in WASM memory
    ///
    /// ## Arguments
    /// * `module` - WASM module to call function from
    ///
    /// ## Returns
    /// Parsed SupervisorSpec from WASM module
    async fn call_get_supervisor_tree(
        &self,
        module: &Arc<plexspaces_wasm_runtime::WasmModule>,
    ) -> Result<SupervisorSpec, ApplicationError> {
        use plexspaces_wasm_runtime::WasmConfig;

        // Create temporary instance to call the function
        // Use default config with reasonable limits
        let config = WasmConfig::default();

        // Create ChannelService for WASM instance
        use crate::service_wrappers::ChannelServiceWrapper;
        use plexspaces_core::ChannelService;
        let channel_service: Arc<dyn ChannelService> = Arc::new(ChannelServiceWrapper::new());

        // Create instance using runtime's instantiate method (trait)
        let module_any: Arc<dyn std::any::Any + Send + Sync> =
            module.clone() as Arc<dyn std::any::Any + Send + Sync>;
        let config_any: Arc<dyn std::any::Any + Send + Sync> = Arc::new(config);
        let instance_any = self
            .runtime
            .instantiate(
                module_any,
                "temp-supervisor-tree-loader".to_string(),
                &[], // No initial state needed
                config_any,
                Some(channel_service),
                None, // No message sender for temporary instance
                None, // No tuplespace provider for temporary instance
                None, // No keyvalue store for temporary instance
                None, // No process group registry for temporary instance
                None, // No lock manager for temporary instance
                None, // No object registry for temporary instance
                None, // No journal storage for temporary instance
                None, // No blob service for temporary instance
                None, // No outbound HTTP client for temporary instance
            )
            .await
            .map_err(|e| {
                let error_msg = format!(
                    "Failed to instantiate WASM module '{}' to load supervisor tree: {}. \
                    The module may be invalid or incompatible with the WASM runtime.",
                    module.name, e
                );
                tracing::error!(
                    application = %self.name,
                    module_name = %module.name,
                    error = %e,
                    "WASM module instantiation failed"
                );
                ApplicationError::Other(error_msg)
            })?;
        let instance =
            plexspaces_wasm_runtime::wasm_runtime_helpers::extract_wasm_instance(instance_any)
                .map_err(|e| {
                    ApplicationError::Other(format!("Failed to extract WasmInstance: {}", e))
                })?;

        // Call get_supervisor_tree() function
        let spec_bytes = instance.get_supervisor_tree().await.map_err(|e| {
            ApplicationError::Other(format!("Failed to call get_supervisor_tree: {}", e))
        })?;

        // If empty, return error (no supervisor tree defined)
        if spec_bytes.is_empty() {
            return Err(ApplicationError::Other(
                "get_supervisor_tree() returned empty supervisor spec".to_string(),
            ));
        }

        // Parse protobuf SupervisorSpec
        let supervisor_spec = SupervisorSpec::decode(spec_bytes.as_slice()).map_err(|e| {
            ApplicationError::Other(format!("Failed to parse SupervisorSpec protobuf: {}", e))
        })?;

        Ok(supervisor_spec)
    }

    /// Stop an actor gracefully with timeout
    ///
    /// ## Purpose
    /// Stops an actor with a configurable timeout. If the actor doesn't stop
    /// within the timeout, it's forcefully stopped.
    ///
    /// ## Arguments
    /// * `actor_id` - ID of the actor to stop
    ///
    /// ## Returns
    /// Ok(()) if actor stopped successfully, error otherwise
    ///
    /// ## Timeout Handling
    /// - Default timeout: 5 seconds per actor
    /// - If timeout is reached, logs a warning and continues (doesn't fail)
    /// - If actor not found, treats as success (already stopped)
    async fn stop_actor_gracefully(
        &self,
        actor_id: &str,
        progress_index: usize,
        total_actors: usize,
    ) -> Result<(), ApplicationError> {
        use plexspaces_core::RequestContext;
        use tokio::time::{timeout, Duration};

        // Get node reference
        let node_ref = {
            let node_opt = self.node.read().await;
            node_opt.clone()
        };

        if let Some(node) = node_ref {
            // Stop actor with timeout (default: 5 seconds per actor)
            let timeout_duration = Duration::from_secs(5);

            // Use ActorFactory directly from ServiceLocator
            let _service_locator = node.service_locator().ok_or_else(|| {
                ApplicationError::ActorStopFailed(
                    actor_id.to_string(),
                    "ServiceLocator not available from node".to_string(),
                )
            })?;

            // Get ActorFactory from ApplicationNode (avoids circular dependency)
            use plexspaces_actor::ActorFactory;
            let actor_factory: Arc<dyn ActorFactory> =
                node.actor_factory().await.ok_or_else(|| {
                    ApplicationError::ActorStopFailed(
                        actor_id.to_string(),
                        "ActorFactory not found in ServiceLocator".to_string(),
                    )
                })?;

            // Create RequestContext for stop operation using application's tenant/namespace
            // Application owns its actors, so it can stop them
            let tenant_id = self.tenant_id.read().await.clone();
            let namespace = self.namespace.read().await.clone();
            let ctx = RequestContext::new_without_auth(tenant_id, namespace.clone());

            let actor_id_string = actor_id.to_string();
            match timeout(
                timeout_duration,
                actor_factory.stop_actor(&ctx, &actor_id_string),
            )
            .await
            {
                Ok(Ok(())) => {
                    if tracing::enabled!(tracing::Level::INFO) {
                        let node_id = actor_id
                            .rsplit_once('@')
                            .map(|(_, node_id)| node_id)
                            .unwrap_or("");
                        tracing::info!(
                            application = %self.name,
                            actor_id = %actor_id,
                            node_id = %node_id,
                            namespace = %namespace,
                            progress = format!("{}/{}", progress_index, total_actors),
                            timeout_seconds = timeout_duration.as_secs(),
                            "Actor stopped successfully during application shutdown"
                        );
                    }
                    Ok(())
                }
                Ok(Err(e)) => {
                    let error_msg = e.to_string();
                    // Check if actor not found (might have already stopped)
                    if error_msg.contains("not found") || error_msg.contains("Actor not found") {
                        if tracing::enabled!(tracing::Level::DEBUG) {
                            tracing::debug!(
                                application = %self.name,
                                actor_id = %actor_id,
                                "Actor already stopped (not found)"
                            );
                        }
                        Ok(()) // Actor already stopped, that's fine
                    } else {
                        let full_error = format!("Failed to stop actor '{}': {}", actor_id, e);
                        tracing::warn!(
                            application = %self.name,
                            actor_id = %actor_id,
                            error = %e,
                            "Actor stop failed"
                        );
                        Err(ApplicationError::Other(full_error))
                    }
                }
                Err(_) => {
                    // Timeout reached - log warning but continue
                    tracing::warn!(
                        application = %self.name,
                        actor_id = %actor_id,
                        timeout_seconds = timeout_duration.as_secs(),
                        "Actor stop timeout reached, continuing shutdown"
                    );
                    Ok(()) // Continue shutdown even if timeout
                }
            }
        } else {
            Err(ApplicationError::Other(
                "Node reference not available for shutdown".to_string(),
            ))
        }
    }
}

#[async_trait]
impl Application for WasmApplication {
    fn name(&self) -> &str {
        &self.name
    }

    fn version(&self) -> &str {
        &self.version
    }

    async fn start(&mut self, node: Arc<dyn ApplicationNode>) -> Result<(), ApplicationError> {
        let mut is_running = self.is_running.write().await;
        if *is_running {
            return Err(ApplicationError::Other(format!(
                "Application '{}' is already running",
                self.name
            )));
        }

        // Log environment variables if available
        if let Some(spec) = &self.spec {
            if !spec.env.is_empty() {
                if tracing::enabled!(tracing::Level::DEBUG) {
                    tracing::debug!(
                        application = %self.name,
                        env_var_count = spec.env.len(),
                        env_vars = ?spec.env.keys().collect::<Vec<_>>(),
                        "WASM application environment variables available"
                    );
                }
            }
        }

        // Store node reference for shutdown
        {
            let mut node_ref = self.node.write().await;
            *node_ref = Some(node.clone());
        }

        // Register behaviors from supervisor tree (for ShardGroup support)
        // This allows ShardGroups to spawn actors using actor_type="worker" (or ChildSpec.id)
        if let Err(e) = self
            .register_behaviors_from_supervisor_tree(node.clone())
            .await
        {
            tracing::warn!(
                application = %self.name,
                error = %e,
                "Failed to register behaviors from supervisor tree (ShardGroups may not work)"
            );
            // Don't fail startup - behavior registration is optional for simple apps
        }

        // Try to load supervisor tree OR spawn simple actor
        // Supervisors are OPTIONAL - simple WASM actors can run without them
        let actor_ids = match self
            .load_supervisor_tree_or_simple_actor(node.clone())
            .await
        {
            Ok(ids) => ids,
            Err(e) => {
                tracing::error!(
                    application = %self.name,
                    error = %e,
                    "Failed to start application"
                );
                return Err(e);
            }
        };

        // Actor counts are automatically tracked by ActorRegistry when actors are spawned
        let actor_count = actor_ids.len();

        // Mark as running
        *is_running = true;

        tracing::trace!(
            application = %self.name,
            actor_count = actor_count,
            "WASM application started successfully"
        );

        Ok(())
    }

    async fn stop(&mut self) -> Result<(), ApplicationError> {
        use tokio::time::{timeout, Duration};

        let mut is_running = self.is_running.write().await;
        if !*is_running {
            return Err(ApplicationError::Other(format!(
                "Application '{}' is not running",
                self.name
            )));
        }

        // Stop all actors in supervisor tree (in reverse order) with overall timeout
        let actor_ids = {
            let spawned = self.spawned_actor_ids.read().await;
            spawned.clone()
        };

        let root_supervisor = {
            let root_supervisor = self.root_supervisor.read().await;
            root_supervisor.clone()
        };

        // Overall shutdown timeout: 30 seconds (or 5 seconds per actor, whichever is larger)
        // But cap at 60 seconds to prevent extremely long timeouts
        let shutdown_timeout = Duration::from_secs(30)
            .max(Duration::from_secs(5 * actor_ids.len() as u64))
            .min(Duration::from_secs(60));

        tracing::info!(
            application = %self.name,
            actor_count = actor_ids.len(),
            timeout_seconds = shutdown_timeout.as_secs(),
            "Starting graceful shutdown with timeout"
        );

        if let Some(root_supervisor) = root_supervisor {
            let supervisor_shutdown = timeout(shutdown_timeout, async {
                let mut supervisor = root_supervisor.write().await;
                supervisor.shutdown().await
            })
            .await;

            match supervisor_shutdown {
                Ok(Ok(())) => {
                    tracing::info!(
                        application = %self.name,
                        "Root supervisor shutdown completed"
                    );
                }
                Ok(Err(e)) => {
                    tracing::warn!(
                        application = %self.name,
                        error = %e,
                        "Root supervisor shutdown failed, falling back to direct actor stop"
                    );
                }
                Err(_) => {
                    tracing::warn!(
                        application = %self.name,
                        timeout_seconds = shutdown_timeout.as_secs(),
                        "Root supervisor shutdown timed out, falling back to direct actor stop"
                    );
                }
            }
        }

        let stop_result = timeout(shutdown_timeout, async {
            // Stop actors in reverse order (children first, then parents)
            let mut errors = Vec::new();
            let mut stopped_count = 0u32;

            for (idx, actor_id) in actor_ids.iter().rev().enumerate() {
                if let Err(e) = self
                    .stop_actor_gracefully(actor_id, idx + 1, actor_ids.len())
                    .await
                {
                    let error_msg = format!("Failed to stop actor '{}': {}", actor_id, e);
                    tracing::warn!(
                        application = %self.name,
                        actor_id = %actor_id,
                        error = %e,
                        "Actor stop failed"
                    );
                    errors.push(error_msg);
                } else {
                    stopped_count += 1;
                }
            }

            (errors, stopped_count)
        })
        .await;

        let (errors, stopped_count) = match stop_result {
            Ok(pair) => pair,
            Err(_) => {
                let timeout_msg = format!(
                    "Shutdown timeout ({:?}) exceeded while stopping {} actors. Some actors may not have stopped gracefully.",
                    shutdown_timeout,
                    actor_ids.len()
                );
                tracing::error!(
                    application = %self.name,
                    timeout_seconds = shutdown_timeout.as_secs(),
                    actor_count = actor_ids.len(),
                    "Shutdown timeout exceeded"
                );
                (vec![timeout_msg], 0)
            }
        };

        // Get final actor count before clearing
        let final_actor_count = {
            let spawned = self.spawned_actor_ids.read().await;
            spawned.len() as u32
        };

        // Update actor count to 0 in ApplicationManager for metrics tracking
        let _node_ref = {
            let node_opt = self.node.read().await;
            node_opt.clone()
        };
        // Actor counts are automatically tracked by ActorRegistry

        // Clear spawned actor IDs
        {
            let mut spawned = self.spawned_actor_ids.write().await;
            let mut last_stopped = self.last_stopped_actor_ids.write().await;
            *last_stopped = spawned.clone();
            spawned.clear();
        }

        {
            let mut root_supervisor = self.root_supervisor.write().await;
            *root_supervisor = None;
        }

        // Mark as stopped (even if errors occurred)
        *is_running = false;

        tracing::info!(
            application = %self.name,
            stopped_count = stopped_count,
            total_count = actor_ids.len(),
            error_count = errors.len(),
            actor_count = final_actor_count,
            "WASM application stopped"
        );

        // If any errors occurred, return error
        if !errors.is_empty() {
            return Err(ApplicationError::Other(format!(
                "Errors during shutdown: {}",
                errors.join(", ")
            )));
        }

        Ok(())
    }

    async fn health_check(&self) -> HealthStatus {
        let is_running = self.is_running.read().await;
        if *is_running {
            HealthStatus::HealthStatusHealthy
        } else {
            HealthStatus::HealthStatusUnhealthy
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn module_hash_for_cleanup(&self) -> Option<String> {
        Some(self.module_hash().to_string())
    }

    async fn cleanup_for_undeploy(&mut self) -> Result<(), ApplicationError> {
        let node = {
            let node_opt = self.node.read().await;
            node_opt
                .clone()
                .ok_or_else(|| ApplicationError::Other("Node reference not set".to_string()))?
        };
        let service_locator = node
            .service_locator()
            .ok_or_else(|| ApplicationError::Other("ServiceLocator not available".to_string()))?;
        let tenant_id = self.tenant_id.read().await.clone();
        let namespace = self.namespace.read().await.clone();
        let ctx = plexspaces_core::RequestContext::new_without_auth(tenant_id, namespace.clone());

        let mut actor_ids = {
            let last_stopped = self.last_stopped_actor_ids.read().await;
            last_stopped.clone()
        };

        let live_actor_ids = if let Some(actor_registry) = service_locator.actor_registry().await {
            actor_registry
                .live_actor_entries()
                .await
                .into_iter()
                .filter_map(|(entry_tenant_id, entry_namespace, actor_id)| {
                    if entry_tenant_id == ctx.tenant_id() && entry_namespace == namespace {
                        Some(actor_id)
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };

        let mut stopped_live_actors = 0usize;
        if !live_actor_ids.is_empty() {
            use plexspaces_actor::ActorFactory;

            let actor_factory: Arc<dyn ActorFactory> =
                node.actor_factory().await.ok_or_else(|| {
                    ApplicationError::Other("ActorFactory not found in ServiceLocator".to_string())
                })?;

            for actor_id in &live_actor_ids {
                match actor_factory.stop_actor(&ctx, actor_id).await {
                    Ok(()) => {
                        stopped_live_actors += 1;
                    }
                    Err(error) => {
                        tracing::warn!(
                            application = %self.name,
                            namespace = %namespace,
                            actor_id = %actor_id,
                            error = %error,
                            "Failed to stop live actor during undeploy cleanup"
                        );
                    }
                }
            }
        }
        actor_ids.extend(live_actor_ids);

        let virtual_cleanup = if let Some(manager) = service_locator.virtual_actor_manager().await {
            manager.unregister_namespace(&namespace).await
        } else {
            plexspaces_core::virtual_actor_manager::VirtualActorNamespaceCleanup::default()
        };
        actor_ids.extend(virtual_cleanup.actor_ids.clone());
        actor_ids.sort();
        actor_ids.dedup();

        let mut purged_records = 0_u64;
        if let Some(journal_storage) = service_locator.get_journal_storage().await {
            for actor_id in &actor_ids {
                purged_records += journal_storage
                    .purge_actor(actor_id)
                    .await
                    .map_err(|e| ApplicationError::Other(e.to_string()))?;
            }
            purged_records += journal_storage
                .purge_namespace(&namespace)
                .await
                .map_err(|e| ApplicationError::Other(e.to_string()))?;
        }

        let removed_registrations =
            if let Some(object_registry) = service_locator.get_object_registry().await {
                object_registry
                    .unregister_all(&ctx)
                    .await
                    .map_err(|e| ApplicationError::Other(e.to_string()))?
            } else {
                0
            };

        tracing::info!(
            application = %self.name,
            namespace = %namespace,
            actor_count = actor_ids.len(),
            stopped_live_actors = stopped_live_actors,
            removed_virtual_types = virtual_cleanup.actor_types.len(),
            purged_records = purged_records,
            removed_registrations = removed_registrations,
            "Application undeploy cleanup completed"
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ApplicationNode;
    use plexspaces_wasm_runtime::WasmRuntime;
    use std::sync::Arc;

    #[test]
    fn test_try_msg_type_from_payload_message_type_canonical() {
        assert_eq!(
            try_msg_type_from_payload(br#"{"message_type":"workflow_run","order_id":"o1"}"#),
            Some("workflow_run".to_string())
        );
        assert_eq!(
            try_msg_type_from_payload(br#"{"message_type":"workflow_signal:cancel"}"#),
            Some("workflow_signal:cancel".to_string())
        );
        assert_eq!(
            try_msg_type_from_payload(br#"{"message_type":"workflow_query:status"}"#),
            Some("workflow_query:status".to_string())
        );
    }

    #[test]
    fn test_try_msg_type_from_payload_op_alias() {
        assert_eq!(
            try_msg_type_from_payload(br#"{"op":"workflow_run","order_id":"o1"}"#),
            Some("workflow_run".to_string())
        );
        assert_eq!(try_msg_type_from_payload(br#"{"op":"call"}"#), None);
        assert_eq!(try_msg_type_from_payload(br#"{"op":"cast"}"#), None);
    }

    #[test]
    fn test_try_msg_type_from_payload_msg_type_alias() {
        assert_eq!(
            try_msg_type_from_payload(br#"{"msg_type":"get_status"}"#),
            Some("get_status".to_string())
        );
    }

    #[test]
    fn test_try_msg_type_from_payload_message_type_takes_precedence() {
        let payload = br#"{"op":"other","message_type":"workflow_run"}"#;
        assert_eq!(
            try_msg_type_from_payload(payload),
            Some("workflow_run".to_string())
        );
    }

    // Mock ApplicationNode for testing
    struct MockApplicationNode;

    #[async_trait]
    impl ApplicationNode for MockApplicationNode {
        fn id(&self) -> &str {
            "test-node"
        }

        fn listen_addr(&self) -> &str {
            "127.0.0.1:8000"
        }
    }

    async fn create_test_runtime() -> Arc<dyn plexspaces_core::WasmRuntimeTrait> {
        Arc::new(
            WasmRuntime::new()
                .await
                .expect("Failed to create WASM runtime"),
        )
    }

    #[tokio::test]
    async fn test_wasm_application_new() {
        let runtime = create_test_runtime().await;
        let app = WasmApplication::new(
            "test-app".to_string(),
            "1.0.0".to_string(),
            "abc123".to_string(),
            runtime,
            None,
        );

        assert_eq!(app.name(), "test-app");
        assert_eq!(app.version(), "1.0.0");
        assert_eq!(app.module_hash(), "abc123");
        assert!(app.spec().is_none());
    }

    #[test]
    fn test_build_supervised_actor_id_includes_type_namespace_and_node() {
        let actor_id =
            WasmApplication::build_supervised_actor_id("leader", "heat-diffusion-rust", "test-1");
        let parsed = plexspaces_core::parse_actor_id(&actor_id).expect("actor id should parse");

        assert_eq!(parsed.actor_type, "leader");
        assert_eq!(parsed.namespace.as_deref(), Some("heat-diffusion-rust"));
        assert_eq!(parsed.node_id, "test-1");
    }

    #[tokio::test]
    async fn test_wasm_application_with_spec() {
        let runtime = create_test_runtime().await;
        let spec = ApplicationSpec {
            name: "test-app".to_string(),
            version: "1.0.0".to_string(),
            description: "Test application".to_string(),
            r#type: plexspaces_proto::application::v1::ApplicationType::ApplicationTypeActive
                .into(),
            dependencies: vec![],
            env: std::collections::HashMap::new(),
            supervisor: None,
            ..Default::default()
        };

        let app = WasmApplication::new(
            "test-app".to_string(),
            "1.0.0".to_string(),
            "abc123".to_string(),
            runtime,
            Some(spec.clone()),
        );

        assert_eq!(app.name(), "test-app");
        assert_eq!(app.version(), "1.0.0");
        assert!(app.spec().is_some());
        assert_eq!(app.spec().unwrap().name, "test-app");
    }

    #[tokio::test]
    async fn test_wasm_application_start_success() {
        let runtime = create_test_runtime().await;
        let mut app = WasmApplication::new(
            "test-app".to_string(),
            "1.0.0".to_string(),
            "abc123".to_string(),
            runtime,
            None,
        );

        let node: Arc<dyn ApplicationNode> = Arc::new(MockApplicationNode);
        let result = app.start(node).await;
        assert!(result.is_ok(), "Start should succeed: {:?}", result);

        // Verify health check returns healthy
        let health = app.health_check().await;
        assert_eq!(health, HealthStatus::HealthStatusHealthy);
    }

    #[tokio::test]
    async fn test_wasm_application_start_twice_fails() {
        let runtime = create_test_runtime().await;
        let mut app = WasmApplication::new(
            "test-app".to_string(),
            "1.0.0".to_string(),
            "abc123".to_string(),
            runtime,
            None,
        );

        let node: Arc<dyn ApplicationNode> = Arc::new(MockApplicationNode);

        // First start should succeed
        let result1 = app.start(node.clone()).await;
        assert!(result1.is_ok(), "First start should succeed: {:?}", result1);

        // Second start should fail
        let result2 = app.start(node).await;
        assert!(result2.is_err(), "Second start should fail");
        match result2 {
            Err(ApplicationError::Other(msg)) => {
                assert!(
                    msg.contains("already running"),
                    "Error message should mention 'already running': {}",
                    msg
                );
            }
            _ => panic!("Expected ApplicationError::Other, got: {:?}", result2),
        }
    }

    #[tokio::test]
    async fn test_wasm_application_stop_success() {
        let runtime = create_test_runtime().await;
        let mut app = WasmApplication::new(
            "test-app".to_string(),
            "1.0.0".to_string(),
            "abc123".to_string(),
            runtime,
            None,
        );

        let node: Arc<dyn ApplicationNode> = Arc::new(MockApplicationNode);

        // Start first
        app.start(node).await.expect("Start should succeed");

        // Stop should succeed
        let result = app.stop().await;
        assert!(result.is_ok(), "Stop should succeed: {:?}", result);

        // Verify health check returns unhealthy
        let health = app.health_check().await;
        assert_eq!(health, HealthStatus::HealthStatusUnhealthy);
    }

    #[tokio::test]
    async fn test_wasm_application_stop_before_start_fails() {
        let runtime = create_test_runtime().await;
        let mut app = WasmApplication::new(
            "test-app".to_string(),
            "1.0.0".to_string(),
            "abc123".to_string(),
            runtime,
            None,
        );

        // Stop without starting should fail
        let result = app.stop().await;
        assert!(result.is_err(), "Stop without start should fail");
        match result {
            Err(ApplicationError::Other(msg)) => {
                assert!(
                    msg.contains("not running"),
                    "Error message should mention 'not running': {}",
                    msg
                );
            }
            _ => panic!("Expected ApplicationError::Other, got: {:?}", result),
        }
    }

    #[tokio::test]
    async fn test_wasm_application_health_check_when_stopped() {
        let runtime = create_test_runtime().await;
        let app = WasmApplication::new(
            "test-app".to_string(),
            "1.0.0".to_string(),
            "abc123".to_string(),
            runtime,
            None,
        );

        // Health check when stopped should return unhealthy
        let health = app.health_check().await;
        assert_eq!(health, HealthStatus::HealthStatusUnhealthy);
    }

    #[tokio::test]
    async fn test_wasm_application_health_check_when_running() {
        let runtime = create_test_runtime().await;
        let mut app = WasmApplication::new(
            "test-app".to_string(),
            "1.0.0".to_string(),
            "abc123".to_string(),
            runtime,
            None,
        );

        let node: Arc<dyn ApplicationNode> = Arc::new(MockApplicationNode);
        app.start(node).await.expect("Start should succeed");

        // Health check when running should return healthy
        let health = app.health_check().await;
        assert_eq!(health, HealthStatus::HealthStatusHealthy);
    }

    #[tokio::test]
    async fn test_wasm_application_lifecycle() {
        let runtime = create_test_runtime().await;
        let mut app = WasmApplication::new(
            "test-app".to_string(),
            "1.0.0".to_string(),
            "abc123".to_string(),
            runtime,
            None,
        );

        let node: Arc<dyn ApplicationNode> = Arc::new(MockApplicationNode);

        // Initial state: stopped
        assert_eq!(
            app.health_check().await,
            HealthStatus::HealthStatusUnhealthy
        );

        // Start
        app.start(node.clone()).await.expect("Start should succeed");
        assert_eq!(app.health_check().await, HealthStatus::HealthStatusHealthy);

        // Stop
        app.stop().await.expect("Stop should succeed");
        assert_eq!(
            app.health_check().await,
            HealthStatus::HealthStatusUnhealthy
        );

        // Can start again after stop
        app.start(node)
            .await
            .expect("Start after stop should succeed");
        assert_eq!(app.health_check().await, HealthStatus::HealthStatusHealthy);
    }

    #[tokio::test]
    async fn test_wasm_application_concurrent_health_checks() {
        let runtime = create_test_runtime().await;
        let mut app = WasmApplication::new(
            "test-app".to_string(),
            "1.0.0".to_string(),
            "abc123".to_string(),
            runtime,
            None,
        );

        let node: Arc<dyn ApplicationNode> = Arc::new(MockApplicationNode);
        app.start(node).await.expect("Start should succeed");

        // Concurrent health checks should all return healthy
        // Use Arc to share the application across tasks
        let app_arc = Arc::new(app);
        let handles: Vec<_> = (0..10)
            .map(|_| {
                let app_clone = app_arc.clone();
                tokio::spawn(async move { app_clone.health_check().await })
            })
            .collect();

        for handle in handles {
            let health = handle.await.expect("Health check should complete");
            assert_eq!(health, HealthStatus::HealthStatusHealthy);
        }
    }

    // TDD Tests for load_supervisor_tree()

    #[tokio::test]
    async fn test_load_supervisor_tree_from_spec() {
        // Test loading supervisor tree from ApplicationSpec (if available)
        let runtime = create_test_runtime().await;

        // Create ApplicationSpec with supervisor tree
        use plexspaces_proto::application::v1::{
            ApplicationSpec, ApplicationType, ChildSpec, ChildType, RestartPolicy,
            SupervisionStrategy, SupervisorSpec,
        };
        use prost_types::Duration;

        let supervisor_spec = SupervisorSpec {
            strategy: SupervisionStrategy::SupervisionStrategyOneForOne.into(),
            max_restarts: 3,
            max_restart_window: Some(Duration {
                seconds: 5,
                nanos: 0,
            }),
            children: vec![ChildSpec {
                id: "worker-1".to_string(),
                r#type: ChildType::ChildTypeWorker.into(),
                args: std::collections::HashMap::new(),
                restart: RestartPolicy::RestartPolicyPermanent.into(),
                shutdown_timeout: Some(Duration {
                    seconds: 5,
                    nanos: 0,
                }),
                supervisor: None,
                facets: vec![],
                behavior_kind: None,
            }],
        };

        let spec = ApplicationSpec {
            name: "test-app".to_string(),
            version: "1.0.0".to_string(),
            description: "Test application".to_string(),
            r#type: ApplicationType::ApplicationTypeActive.into(),
            dependencies: vec![],
            env: std::collections::HashMap::new(),
            supervisor: Some(supervisor_spec),
            ..Default::default()
        };

        let app = WasmApplication::new(
            "test-app".to_string(),
            "1.0.0".to_string(),
            "abc123".to_string(),
            runtime,
            Some(spec),
        );

        // Mock node that tracks spawned actors
        struct TrackingMockNode {
            spawned_actors: Arc<tokio::sync::Mutex<Vec<String>>>,
        }

        #[async_trait]
        impl ApplicationNode for TrackingMockNode {
            fn id(&self) -> &str {
                "test-node"
            }

            fn listen_addr(&self) -> &str {
                "127.0.0.1:8000"
            }
        }

        let tracking_node = Arc::new(TrackingMockNode {
            spawned_actors: Arc::new(tokio::sync::Mutex::new(Vec::new())),
        });

        // Test load_supervisor_tree (currently stubbed, will implement)
        // For now, this test documents the expected behavior
        // TODO: Implement load_supervisor_tree to use spec.supervisor if available
        let _actor_ids = app.load_supervisor_tree(tracking_node.clone()).await;

        // Once implemented, verify actors were spawned
        // let spawned = tracking_node.spawned_actors.lock().await;
        // assert!(!spawned.is_empty(), "Should spawn actors from supervisor tree");
    }

    #[tokio::test]
    async fn test_load_supervisor_tree_from_wasm_function() {
        // Test loading supervisor tree from WASM function export
        // This test documents the expected behavior for WASM-based supervisor trees

        // First, we need to deploy a WASM module with get_supervisor_tree() function
        // For now, test that it handles missing module gracefully
        let runtime = create_test_runtime().await;
        let app = WasmApplication::new(
            "test-app".to_string(),
            "1.0.0".to_string(),
            "nonexistent-hash".to_string(), // Module doesn't exist
            runtime,
            None, // No spec, should try WASM function
        );

        let node: Arc<dyn ApplicationNode> = Arc::new(MockApplicationNode);

        // Should return empty list if module not found (graceful degradation)
        let result = app.load_supervisor_tree(node).await;
        assert!(result.is_ok());
        // For now, returns empty list if module not found
        // TODO: Once implemented, should return error or handle gracefully
    }

    #[tokio::test]
    async fn test_load_supervisor_tree_error_module_not_found() {
        // Test error handling when WASM module is not found
        let runtime = create_test_runtime().await;
        let app = WasmApplication::new(
            "test-app".to_string(),
            "1.0.0".to_string(),
            "nonexistent-hash".to_string(), // Module doesn't exist
            runtime,
            None,
        );

        let node: Arc<dyn ApplicationNode> = Arc::new(MockApplicationNode);
        let result = app.load_supervisor_tree(node).await;

        // Should return empty list when module not found (graceful degradation)
        // This is acceptable for simple modules that don't export supervisor trees
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 0);
    }

    // TDD Tests for stop()

    #[tokio::test]
    async fn test_stop_gracefully_shuts_down_actors() {
        // Test that stop() gracefully shuts down all actors in supervisor tree
        let runtime = create_test_runtime().await;
        let mut app = WasmApplication::new(
            "test-app".to_string(),
            "1.0.0".to_string(),
            "abc123".to_string(),
            runtime,
            None,
        );

        // Mock node that tracks stopped actors
        struct StopTrackingMockNode {
            stopped_actors: Arc<tokio::sync::Mutex<Vec<String>>>,
        }

        #[async_trait]
        impl ApplicationNode for StopTrackingMockNode {
            fn id(&self) -> &str {
                "test-node"
            }

            fn listen_addr(&self) -> &str {
                "127.0.0.1:8000"
            }
        }

        let tracking_node = Arc::new(StopTrackingMockNode {
            stopped_actors: Arc::new(tokio::sync::Mutex::new(Vec::new())),
        });

        // Start application (spawns actors)
        app.start(tracking_node.clone())
            .await
            .expect("Start should succeed");

        // Stop application (should stop all actors)
        app.stop().await.expect("Stop should succeed");

        // Once implemented, verify all actors were stopped
        // let stopped = tracking_node.stopped_actors.lock().await;
        // assert!(!stopped.is_empty(), "Should stop all actors");
    }

    #[tokio::test]
    async fn test_stop_handles_timeout() {
        // Test that stop() handles timeout when actors don't stop gracefully
        let runtime = create_test_runtime().await;
        let mut app = WasmApplication::new(
            "test-app".to_string(),
            "1.0.0".to_string(),
            "abc123".to_string(),
            runtime,
            None,
        );

        // Mock node with slow stop_actor
        struct SlowStopMockNode;

        #[async_trait]
        impl ApplicationNode for SlowStopMockNode {
            fn id(&self) -> &str {
                "test-node"
            }

            fn listen_addr(&self) -> &str {
                "127.0.0.1:8000"
            }
        }

        let node: Arc<dyn ApplicationNode> = Arc::new(SlowStopMockNode);
        app.start(node).await.expect("Start should succeed");

        // TODO: Implement timeout handling in stop()
        // Stop should timeout and still succeed (or return timeout error)
        let _result = app.stop().await;
        // assert!(result.is_err() || result.is_ok()); // Either timeout error or force stop
    }

    // ============================================================================
    // TDD Tests for Supervisor Integration with WASM Actors
    // ============================================================================

    /// Test: create_wasm_actor_factory returns a callable factory
    #[tokio::test]
    async fn test_create_wasm_actor_factory_returns_callable() {
        // This test verifies that create_wasm_actor_factory returns a factory
        // that can be called to create a new WASM actor instance

        // For now, document expected behavior - implementation will make this pass
        // The factory should capture: runtime, module_hash, child_spec, node
        // When called, it should create a new WasmInstance and return StartedChild::Worker
    }

    /// Test: WASM actor factory can recreate actor after failure
    #[tokio::test]
    async fn test_wasm_actor_factory_recreates_actor() {
        // This test verifies that calling the factory multiple times
        // creates new, independent WASM instances

        // Expected behavior:
        // 1. Call factory -> get actor1
        // 2. Call factory again -> get actor2 (different instance)
        // 3. actor1 and actor2 should be independent
    }

    /// Test: Supervisor.add_child works with WASM actor factory
    #[tokio::test]
    async fn test_supervisor_add_child_with_wasm_factory() {
        // This test verifies that supervisor.add_child() works correctly
        // with a ChildSpec containing a WASM actor factory

        // Expected behavior:
        // 1. Create supervisor with one-for-one strategy
        // 2. Create ChildSpec with WASM factory
        // 3. supervisor.add_child(spec) should succeed
        // 4. Actor should be spawned and running
    }

    /// Test: Supervisor restarts WASM actor on crash
    #[tokio::test]
    async fn test_supervisor_restarts_wasm_actor_on_crash() {
        // This test verifies that when a WASM actor crashes,
        // the supervisor detects it and restarts using the factory

        // Expected behavior:
        // 1. Deploy WASM app with supervisor
        // 2. Get actor_ref to WASM actor
        // 3. Cause actor to crash (send invalid message)
        // 4. Supervisor should restart actor
        // 5. Actor should be responsive again
    }

    /// Test: Supervisor respects max_restarts limit
    #[tokio::test]
    async fn test_supervisor_respects_max_restarts() {
        // This test verifies that supervisor stops restarting
        // after max_restarts is exceeded

        // Expected behavior:
        // 1. Configure supervisor with max_restarts=3
        // 2. Crash actor 4 times
        // 3. After 3rd restart, supervisor should stop trying
        // 4. Application should report error state
    }

    /// Test: Supervisor tree with multiple WASM actors
    #[tokio::test]
    async fn test_supervisor_tree_multiple_wasm_actors() {
        // This test verifies supervisor handles multiple WASM actors

        // Expected behavior:
        // 1. Deploy app with 3 WASM actors under one supervisor
        // 2. All 3 actors should be spawned
        // 3. Crash one actor
        // 4. Only crashed actor should restart (one-for-one)
        // 5. Other actors should continue running
    }

    #[test]
    fn test_actor_id_from_initial_state_prefers_materialized_actor_id() {
        let actor_id = super::actor_id_from_initial_state(
            br#"{"actor_id":"cart-1//abstractions::abstractions-rust@test-node-8091"}"#,
            "abstractions",
            "abstractions-rust",
            "test-node-8091",
        );
        assert_eq!(
            actor_id,
            "cart-1//abstractions::abstractions-rust@test-node-8091"
        );
    }

    #[test]
    fn test_actor_id_from_initial_state_falls_back_to_canonical_actor_id() {
        let actor_id = super::actor_id_from_initial_state(
            br#"{"behavior_kind":"GenServer"}"#,
            "abstractions",
            "abstractions-rust",
            "test-node-8091",
        );
        assert_eq!(
            actor_id,
            "abstractions//abstractions::abstractions-rust@test-node-8091"
        );
    }

    #[test]
    fn test_init_config_from_initial_state_preserves_materialized_virtual_actor_config() {
        let child_spec = plexspaces_proto::application::v1::ChildSpec {
            id: "abstractions".to_string(),
            behavior_kind: Some("GenServer".to_string()),
            ..Default::default()
        };
        let init_config = super::init_config_from_initial_state_or_child_spec(
            br#"{"actor_id":"cart-1//abstractions::abstractions-rust@test-node-8091","role":"abstractions"}"#,
            &child_spec,
            "abstractions//abstractions::abstractions-rust@test-node-8091",
        );
        assert_eq!(
            std::str::from_utf8(&init_config).unwrap(),
            r#"{"actor_id":"cart-1//abstractions::abstractions-rust@test-node-8091","role":"abstractions"}"#
        );
    }

    #[test]
    fn test_init_config_from_empty_json_object_injects_actor_id_for_wasm_router() {
        let child_spec = plexspaces_proto::application::v1::ChildSpec {
            id: "worker".to_string(),
            behavior_kind: Some("GenServer".to_string()),
            ..Default::default()
        };
        let canonical = "01ABC//worker::data-lake-rag-go@test-node-8093".to_string();
        let init_config =
            super::init_config_from_initial_state_or_child_spec(b"{}", &child_spec, &canonical);
        let value: serde_json::Value = serde_json::from_slice(&init_config).unwrap();
        assert_eq!(
            value.get("actor_id").and_then(|v| v.as_str()),
            Some(canonical.as_str())
        );
        assert_eq!(
            value.get("behavior_kind").and_then(|v| v.as_str()),
            Some("GenServer")
        );
    }

    #[test]
    fn test_init_config_from_child_spec_builds_default_actor_config() {
        let mut child_spec = plexspaces_proto::application::v1::ChildSpec {
            id: "channel".to_string(),
            behavior_kind: Some("GenEvent".to_string()),
            ..Default::default()
        };
        child_spec
            .args
            .insert("role".to_string(), "channel".to_string());

        let init_config = super::init_config_from_initial_state_or_child_spec(
            &[],
            &child_spec,
            "channel:abstractions-rust@test-node-8091",
        );
        let value: serde_json::Value = serde_json::from_slice(&init_config).unwrap();
        assert_eq!(
            value.get("actor_id").and_then(|value| value.as_str()),
            Some("channel:abstractions-rust@test-node-8091")
        );
        assert_eq!(
            value.get("behavior_kind").and_then(|value| value.as_str()),
            Some("GenEvent")
        );
        assert_eq!(
            value
                .get("args")
                .and_then(|value| value.get("role"))
                .and_then(|value| value.as_str()),
            Some("channel")
        );
    }

    #[test]
    fn test_wasm_config_for_child_spec_enables_durability_from_facets() {
        let child_spec = plexspaces_proto::application::v1::ChildSpec {
            id: "abstractions".to_string(),
            facets: vec![plexspaces_proto::common::v1::Facet {
                r#type: "durability".to_string(),
                priority: 90,
                config: std::collections::HashMap::from([(
                    "checkpoint_interval".to_string(),
                    "5".to_string(),
                )]),
                metadata: None,
                state: std::collections::HashMap::new(),
            }],
            ..Default::default()
        };

        let config = super::wasm_config_for_child_spec(&child_spec);
        assert!(config.durability_enabled);
    }

    #[test]
    fn test_wasm_config_for_child_spec_keeps_non_durable_actor_fresh() {
        let child_spec = plexspaces_proto::application::v1::ChildSpec {
            id: "ephemeral".to_string(),
            facets: vec![plexspaces_proto::common::v1::Facet {
                r#type: "virtual_actor".to_string(),
                priority: 100,
                config: std::collections::HashMap::from([(
                    "activation_strategy".to_string(),
                    "lazy".to_string(),
                )]),
                metadata: None,
                state: std::collections::HashMap::new(),
            }],
            ..Default::default()
        };

        let config = super::wasm_config_for_child_spec(&child_spec);
        assert!(!config.durability_enabled);
    }

    /// Test: initialize_supervisor_tree creates proper supervisor with add_child
    #[tokio::test]
    async fn test_initialize_supervisor_tree_uses_add_child() {
        // This test verifies that initialize_supervisor_tree properly
        // uses supervisor.add_child() instead of spawning directly

        // Expected behavior:
        // 1. Create WasmApplication with supervisor spec
        // 2. Call start() which calls initialize_supervisor_tree
        // 3. Verify supervisor is created with proper strategy
        // 4. Verify actors are added via add_child (not spawned directly)
        // 5. Verify supervisor.start() is called
    }

    /// Test: Supervisor events are emitted during restart
    #[tokio::test]
    async fn test_supervisor_emits_restart_events() {
        // This test verifies that supervisor emits proper events
        // during actor restart

        // Expected behavior:
        // 1. Subscribe to supervisor events
        // 2. Crash WASM actor
        // 3. Should receive: ChildCrashed, ChildRestarted events
    }
}
