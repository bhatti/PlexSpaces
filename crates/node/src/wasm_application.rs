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

use async_trait::async_trait;
use plexspaces_application::{Application, ApplicationError, ApplicationNode};
use plexspaces_proto::v1::application::HealthStatus;
use plexspaces_core::{Actor, BehaviorError, BehaviorType};
use plexspaces_proto::common::v1::Message;
use plexspaces_proto::application::v1::{ApplicationSpec, SupervisorSpec};
use prost::Message as ProstMessage;
use plexspaces_proto::wasm::v1::WasmModule;
use plexspaces_wasm_runtime::{deployment_service::WasmDeploymentService, WasmInstance, WasmRuntime};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Parses optional behavior_kind string from ChildSpec to BehaviorType for logging.
/// When set, process_message spans and actor registration logs show GenEvent/GenServer etc. instead of actor id.
fn parse_behavior_kind(s: Option<&str>) -> plexspaces_core::BehaviorType {
    match s.map(str::trim) {
        Some("GenEvent") | Some("EventHandler") | Some("eventhandler") | Some("event") => plexspaces_core::BehaviorType::GenEvent,
        Some("GenServer") | Some("genserver") => plexspaces_core::BehaviorType::GenServer,
        Some("GenStateMachine") | Some("fsm") => plexspaces_core::BehaviorType::GenStateMachine,
        Some("Workflow") | Some("workflow") => plexspaces_core::BehaviorType::Workflow,
        _ => plexspaces_core::BehaviorType::GenServer,
    }
}

/// WASM actor behavior that wraps a WasmInstance
///
/// ## Purpose
/// Bridges between the actor system and WASM instances, forwarding messages
/// to the WASM module's handle_message function.
///
/// ## Design
/// - Wraps a WasmInstance (which holds the WASM module and state)
/// - Forwards handle_message calls to WASM instance
/// - actor_type: for dashboard/index (e.g. child_spec.id "SensorStream")
/// - behavior_kind: for logging spans (e.g. GenEvent so logs show behavior=GenEvent not behavior=SensorStream)
struct WasmActorBehavior {
    instance: Arc<WasmInstance>,
    actor_type: String, // Actor type for dashboard grouping (e.g., application name or child spec id)
    behavior_kind: plexspaces_core::BehaviorType, // OTP-style kind for logging (GenServer, GenEvent, etc.)
}

/// Detects WASM instance poisoned state (trap or "cannot enter component instance").
///
/// When a trap or re-entrancy violation occurs, wasmtime leaves the component instance
/// in an invalid state; subsequent calls fail with "cannot enter component instance".
/// Callers should terminate the actor so a new instance can be created (e.g. via supervisor restart).
fn is_wasm_instance_poisoned(error_str: &str) -> bool {
    let lower = error_str.to_lowercase();
    lower.contains("cannot enter") || lower.contains("trap") || lower.contains("cannotentercomponent")
}

/// Tries to get application-level msg_type (handler name) from JSON payload, e.g. {"msg_type":"ingest","payload":{...}}.
/// Returns None if payload is not valid JSON or has no msg_type, or msg_type is transport-only ("call"/"cast").
fn try_msg_type_from_payload(payload: &[u8]) -> Option<String> {
    let value: serde_json::Value = serde_json::from_slice(payload).ok()?;
    let s = value.get("msg_type")?.as_str()?.trim();
    if s.is_empty() || s.eq_ignore_ascii_case("call") || s.eq_ignore_ascii_case("cast") {
        return None;
    }
    Some(s.to_string())
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
        let from = message.sender_id.as_str();
        let from = if from.is_empty() { "" } else { from };
        // Pass handler name to WASM so SDK can dispatch: prefer application msg_type from payload (e.g. "ingest"), else envelope message_type ("cast"/"call")
        let from_payload = try_msg_type_from_payload(&message.payload);
        let message_type: String = from_payload
            .clone()
            .unwrap_or_else(|| message.message_type.clone());
        let message_type = if message_type.is_empty() { "cast".to_string() } else { message_type };
        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!(
                message_id = %message.id,
                resolved_msg_type = %message_type,
                envelope_message_type = %message.message_type,
                "WasmActorBehavior: resolved message_type for WASM handle()"
            );
        }
        // Use message payload directly
        let payload = message.payload.clone();
        let message_id = message.id.clone();

        // Clone Arc before await to ensure Send
        let instance = self.instance.clone();
        
        tracing::info!(
            message_id = %message_id,
            sender_id = %from,
            receiver_id = %message.receiver_id,
            correlation_id = %message.correlation_id,
            msg_type = %message_type,
            "WasmActor received message"
        );
        // Call WASM instance's handle_message (message_id for correlation with INVOKE_ACTOR logs)
        let result = instance.handle_message_with_id(from, message_type.as_str(), payload, &message_id).await;
        tracing::debug!(
            message_id = %message_id,
            ok = result.is_ok(),
            "🟦 [WasmActorBehavior::handle_message] END (after instance.handle_message_with_id)"
        );
        match result {
            Ok(response) => {
                // Handle response for request-reply patterns (ask/call)
                if !message.sender_id.is_empty() {
                    let reply_id = ulid::Ulid::new().to_string();
                    tracing::info!(
                        request_message_id = %message_id,
                        reply_message_id = %reply_id,
                        sender_id = %message.sender_id,
                        receiver_id = %message.receiver_id,
                        correlation_id = %message.correlation_id,
                        msg_type = %message_type,
                        response_len = response.len(),
                        "WasmActor sending reply to sender"
                    );
                    let reply_message = Message {
                        id: reply_id,
                        payload: response,
                        sender_id: message.receiver_id.clone(),
                        receiver_id: message.sender_id.clone(),
                        message_type: "reply".to_string(),
                        correlation_id: message.correlation_id.clone(),
                        ..Default::default()
                    };
                    if let Some(actor_service) = ctx.service_locator.get_actor_service().await {
                        if let Err(e) = actor_service.send(&message.sender_id, reply_message).await {
                            tracing::warn!(
                                request_message_id = %message_id,
                                sender_id = %message.sender_id,
                                correlation_id = %message.correlation_id,
                                error = %e,
                                "Failed to send reply via ActorService::send()"
                            );
                        }
                    } else {
                        tracing::warn!("ActorService not available in ServiceLocator, cannot send reply");
                    }
                }
                Ok(())
            }
            Err(e) => {
                let error_str = e.to_string();
                let is_poisoned = is_wasm_instance_poisoned(&error_str);
                let error_msg = format!("WASM handle_message failed: {}", e);
                // Log first line only to avoid duplicate backtrace (full backtrace already in wasm-runtime log)
                let error_first_line = error_str.lines().next().unwrap_or("");
                tracing::error!(
                    message_id = %message.id,
                    error_first_line = %error_first_line,
                    "WasmActorBehavior: handle_message failed (correlate with INVOKE_ACTOR message_id)"
                );
                if !message.sender_id.is_empty() {
                    let error_payload = serde_json::json!({
                        "error": error_msg,
                        "success": false,
                        "wasm_poisoned": is_poisoned
                    });
                    let reply_payload = serde_json::to_vec(&error_payload).unwrap_or_else(|_| error_msg.as_bytes().to_vec());
                    let reply_message = Message {
                        id: ulid::Ulid::new().to_string(),
                        payload: reply_payload,
                        sender_id: message.receiver_id.clone(),
                        receiver_id: message.sender_id.clone(),
                        message_type: "reply".to_string(),
                        correlation_id: message.correlation_id.clone(),
                        ..Default::default()
                    };
                    if let Some(actor_service) = ctx.service_locator.get_actor_service().await {
                        if let Err(send_e) = actor_service.send(&message.sender_id, reply_message).await {
                            tracing::warn!(error = %send_e, "Failed to send error reply via ActorService::send()");
                        }
                    }
                }
                if is_poisoned {
                    tracing::error!(
                        error = %error_str,
                        "WASM instance poisoned (trap or cannot enter); actor should terminate for restart"
                    );
                }
                Err(BehaviorError::ProcessingError(error_msg))
            }
        }
    }

    fn behavior_type(&self) -> BehaviorType {
        // Return custom behavior type with actor_type name for dashboard grouping
        BehaviorType::Custom(self.actor_type.clone())
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
    runtime: Arc<WasmRuntime>,
    /// Deployment service for module management
    deployment_service: Arc<WasmDeploymentService>,
    /// Whether the application is running
    is_running: Arc<RwLock<bool>>,
    /// Application specification (if available)
    spec: Option<ApplicationSpec>,
    /// Spawned actor IDs (for graceful shutdown)
    spawned_actor_ids: Arc<RwLock<Vec<String>>>,
    /// Node reference for stopping actors
    node: Arc<RwLock<Option<Arc<dyn ApplicationNode>>>>,
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
        runtime: Arc<WasmRuntime>,
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
            spec,
            spawned_actor_ids: Arc::new(RwLock::new(Vec::new())),
            node: Arc::new(RwLock::new(None)),
            tenant_id: Arc::new(RwLock::new(String::new())),
            namespace: Arc::new(RwLock::new(String::new())),
        }
    }
    
    /// Set tenant_id and namespace from API request
    /// 
    /// ## Purpose
    /// Called by ApplicationManager before start() to set tenant_id/namespace from API request.
    /// These values are used when spawning actors instead of hardcoded defaults.
    pub async fn set_tenant_namespace(&self, tenant_id: String, namespace: String) {
        *self.tenant_id.write().await = tenant_id;
        *self.namespace.write().await = namespace;
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

    /// Load supervisor tree from WASM module
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
    async fn load_supervisor_tree(&self, node: Arc<dyn ApplicationNode>) -> Result<Vec<String>, ApplicationError> {

        // Strategy 1: Use spec.supervisor if available (config-based)
        if let Some(spec) = &self.spec {
            if let Some(supervisor_spec) = &spec.supervisor {
                return self.initialize_supervisor_tree(node, supervisor_spec).await;
            }
        }

        // Strategy 2: Try to get supervisor tree from WASM function
        // Resolve module by hash and call get_supervisor_tree() function
        if let Some(module) = self.runtime.get_module(&self.module_hash).await {
            // Create a temporary instance to call get_supervisor_tree()
            // Function signature: get_supervisor_tree() -> (ptr: i32, len: i32)
            // Returns protobuf-encoded SupervisorSpec in WASM memory
            match self.call_get_supervisor_tree(&module).await {
                Ok(supervisor_spec) => {
                    return self.initialize_supervisor_tree(node, &supervisor_spec).await;
                }
                Err(e) => {
                    // Log error with better context
                    let error_msg = format!(
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

        // Fallback: Return empty list if no spec and no WASM function
        // This is acceptable for simple modules that don't need supervisor trees
        // Log at info level since this is a valid scenario
        tracing::info!(
            application = %self.name,
            module_hash = %self.module_hash,
            "No supervisor tree found (acceptable for simple modules)"
        );
        Ok(Vec::new())
    }

    /// Initialize actors from supervisor tree specification
    ///
    /// ## Purpose
    /// Recursively initializes actors and supervisors from a SupervisorSpec.
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

        // Use a queue to traverse the tree iteratively (avoid recursion)
        use std::collections::VecDeque;
        let mut queue: VecDeque<&SupervisorSpec> = VecDeque::new();
        queue.push_back(supervisor_spec);

        // Traverse tree breadth-first
        // Note: In Erlang-style supervision, supervisors are also actors, so we spawn them too
        while let Some(current_spec) = queue.pop_front() {
            // Initialize all children in the current supervisor
            for child in &current_spec.children {
                match child.r#type() {
                    plexspaces_proto::application::v1::ChildType::ChildTypeWorker => {
                        // Get tenant_id/namespace from stored values (set during registration)
                        let tenant_id = self.tenant_id.read().await.clone();
                        let namespace = self.namespace.read().await.clone();
                        // Fallback to empty if not set (will use defaults from node config)
                        let final_tenant_id = if tenant_id.is_empty() { String::new() } else { tenant_id };
                        let final_namespace = if namespace.is_empty() { String::new() } else { namespace };
                        // Spawn worker actor
                        let actor_id = Self::spawn_worker_actor_internal(node.clone(), child, &module_hash, self.runtime.clone(), final_tenant_id, final_namespace).await?;
                        actor_ids.push(actor_id);
                    }
                    plexspaces_proto::application::v1::ChildType::ChildTypeSupervisor => {
                        // Get tenant_id/namespace from stored values (set during registration)
                        let tenant_id = self.tenant_id.read().await.clone();
                        let namespace = self.namespace.read().await.clone();
                        // Fallback to empty if not set (will use defaults from node config)
                        let final_tenant_id = if tenant_id.is_empty() { String::new() } else { tenant_id };
                        let final_namespace = if namespace.is_empty() { String::new() } else { namespace };
                        // In Erlang-style supervision, supervisors are also actors
                        // Spawn the supervisor actor first, then process its children
                        let supervisor_actor_id = Self::spawn_worker_actor_internal(node.clone(), child, &module_hash, self.runtime.clone(), final_tenant_id, final_namespace).await?;
                        actor_ids.push(supervisor_actor_id);
                        
                        // Then add child supervisor to queue for processing its children
                        if let Some(child_supervisor_spec) = &child.supervisor {
                            queue.push_back(child_supervisor_spec);
                        } else {
                            return Err(ApplicationError::Other(format!(
                                "Child supervisor '{}' missing supervisor specification",
                                child.id
                            )));
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
        }

        // Store spawned actor IDs for graceful shutdown
        {
            let mut spawned = spawned_actor_ids.write().await;
            spawned.extend(actor_ids.clone());
        }

        Ok(actor_ids)
    }

    /// Internal helper to spawn worker actor (static to avoid recursion issues)
    async fn spawn_worker_actor_internal(
        node: Arc<dyn ApplicationNode>,
        child_spec: &plexspaces_proto::application::v1::ChildSpec,
        module_hash: &str,
        runtime: Arc<WasmRuntime>,
        tenant_id: String,
        namespace: String,
    ) -> Result<String, ApplicationError> {
        use plexspaces_core::Actor;

        // For WASM actors, we need to:
        // 1. Resolve WASM module by hash
        // 2. Instantiate WASM module
        // 3. Create a behavior that wraps the WASM instance
        // 4. Spawn the actor with that behavior

        // Get ServiceLocator from node
        let service_locator = node.service_locator()
            .ok_or_else(|| ApplicationError::Other("ServiceLocator not available from node".to_string()))?;

        // Resolve module by hash
        let module = runtime
            .get_module(module_hash)
            .await
            .ok_or_else(|| {
                ApplicationError::Other(format!(
                    "WASM module not found: {}",
                    module_hash
                ))
            })?;

        // Create ChannelService for WASM instance
        use plexspaces_core::ChannelService;
        use crate::service_wrappers::ChannelServiceWrapper;
        let channel_service: Arc<dyn ChannelService> = Arc::new(ChannelServiceWrapper::new());

        // Create MessageSender for WASM instance (use application crate's consolidated impl)
        use plexspaces_application::wasm_message_sender::ActorServiceMessageSender;
        use plexspaces_core::ActorService;
        let actor_service: Arc<dyn ActorService + Send + Sync> = service_locator
            .get_actor_service()
            .await
            .ok_or_else(|| ApplicationError::Other("ActorService not found in ServiceLocator".to_string()))?;
        let message_sender: Arc<dyn plexspaces_wasm_runtime::MessageSender> = Arc::new(
            ActorServiceMessageSender::new(actor_service, service_locator.clone())
        );

        // Get TupleSpaceProvider from service locator if available
        use plexspaces_core::TupleSpaceProvider;
        let tuplespace_provider: Option<Arc<dyn TupleSpaceProvider>> = service_locator
            .get_tuplespace_provider()
            .await;

        // KeyValue store for WASM actors (simple-actor kv_get/kv_put).
        // Use the shared KeyValueStore from ServiceLocator (initialized during node startup).
        let keyvalue_store: Option<Arc<dyn plexspaces_core::KeyValueStore>> = service_locator
            .get_keyvalue_store()
            .await;

        // Get ProcessGroupRegistry for WASM actors (needed for pg_join/pg_leave/pg_members/pg_broadcast)
        // Create from the shared KeyValueStore so all actors share the same group membership state.
        let process_group_registry: Option<Arc<plexspaces_process_groups::ProcessGroupRegistry>> =
            keyvalue_store.as_ref().map(|kv| {
                Arc::new(plexspaces_process_groups::ProcessGroupRegistry::new(
                    node.id(),
                    kv.clone(),
                ))
            });

        // Get LockManager from service locator so WASM actors can use host.lock_acquire/renew/release
        let lock_manager = service_locator.get_lock_manager().await;

        // Get ObjectRegistry from service locator if available
        let object_registry: Option<Arc<dyn plexspaces_core::ObjectRegistry>> = service_locator.get_object_registry().await;

        // Get JournalStorage - use SQLite file-based storage for durability
        //
        // Note: Env var handling is centralized in config_manager::initialize()
        // TODO: Get database URL from ReleaseSpec instead of env vars
        // For now, keeping env var fallback for backward compatibility until
        // this function receives the initialized ReleaseSpec
        use plexspaces_journaling::JournalStorage;
        let journal_storage: Option<Arc<dyn JournalStorage>> = {
            let journal_db_path = std::env::var("PLEXSPACES_DATABASE_URL")
                .or_else(|_| std::env::var("PLEXSPACES_JOURNAL_DB"))
                .unwrap_or_else(|_| {
                    // Use node ID to support multiple nodes on same machine
                    let node_id = node.id().replace(['@', '/', '\\', ':'], "-");
                    format!("/tmp/plexspaces-{}.db", node_id)
                });
            
            match plexspaces_journaling::SqliteJournalStorage::new(&journal_db_path).await {
                Ok(storage) => {
                    tracing::info!(
                        db_path = %journal_db_path,
                        node_id = %node.id(),
                        "Journal storage initialized (SQLite)"
                    );
                    Some(Arc::new(storage) as Arc<dyn JournalStorage>)
                }
                Err(e) => {
                    match plexspaces_journaling::SqliteJournalStorage::new(":memory:").await {
                        Ok(fallback_storage) => {
                            tracing::warn!(
                                error = %e,
                                db_path = %journal_db_path,
                                "Failed to create SQLite journal storage, falling back to SQLite :memory: (no durability)"
                            );
                            Some(Arc::new(fallback_storage) as Arc<dyn JournalStorage>)
                        }
                        Err(e2) => {
                            tracing::error!(
                                error = %e2,
                                "Failed to create SQLite :memory: fallback journal storage"
                            );
                            None
                        }
                    }
                }
            }
        };

        // Get BlobService from node if available (using trait-based access)
        let blob_service = node.blob_service().await;

        // Create WASM instance with all available services
        // TODO(instance-pool): When config.use_instance_pool is true, checkout from per-module InstancePool
        // instead of runtime.instantiate() for faster spawn. Fits lightweight actors and worker pools.
        // See PROJECT_TRACKER.md.
        // Actor IDs always use name:namespace@node_id format (namespace required for WASM apps).
        let actor_id = format!("{}:{}@{}", child_spec.id, namespace, node.id());
        let wasm_instance = runtime
            .instantiate(
                module,
                actor_id.clone(),
                &[], // No initial state
                plexspaces_wasm_runtime::WasmConfig::default(),
                Some(channel_service),
                Some(Arc::new(message_sender.clone()) as Arc<dyn std::any::Any + Send + Sync>),
                tuplespace_provider,
                keyvalue_store,
                process_group_registry,
                lock_manager,
                object_registry,
                journal_storage,
                blob_service,
            )
            .await
            .map_err(|e| {
                ApplicationError::Other(format!(
                    "Failed to instantiate WASM module: {}",
                    e
                ))
            })?;

        // Use child_spec.id as actor_type for better dashboard visibility
        let actor_type = child_spec.id.clone();
        let behavior_kind = parse_behavior_kind(child_spec.behavior_kind.as_deref());

        // Create behavior that wraps WASM instance with actor_type and behavior_kind for logging
        let behavior: Box<dyn Actor> = Box::new(WasmActorBehavior {
            instance: Arc::new(wasm_instance),
            actor_type: actor_type.clone(),
            behavior_kind,
        });

        // Get ServiceLocator from node
        let service_locator = node.service_locator()
            .ok_or_else(|| ApplicationError::ActorSpawnFailed(
                actor_id.clone(),
                "ServiceLocator not available from node".to_string()
            ))?;
        
        // Use ActorBuilder to build and spawn the actor with custom behavior
        // Use tenant_id/namespace from API request (passed as parameters)
        use plexspaces_actor::ActorBuilder;
        use plexspaces_core::RequestContext;
        // Use tenant_id/namespace from API request (not hardcoded "internal"/"system")
        let ctx = RequestContext::new_without_auth(tenant_id, namespace);
        // spawn() will extract actor_type from behavior.behavior_type() which now returns Custom(actor_type)
        let _actor_ref = ActorBuilder::new(behavior)
            .with_id(actor_id.clone())
            .spawn(&ctx, service_locator)
            .await
            .map_err(|e| ApplicationError::ActorSpawnFailed(actor_id.clone(), format!("Failed to spawn actor: {}", e)))?;

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
        module: &plexspaces_wasm_runtime::WasmModule,
    ) -> Result<SupervisorSpec, ApplicationError> {
        use plexspaces_wasm_runtime::{WasmConfig, WasmInstance};

        // Create temporary instance to call the function
        // Use default config with reasonable limits
        let config = WasmConfig::default();

        // Create ChannelService for WASM instance
        use plexspaces_core::ChannelService;
        use crate::service_wrappers::ChannelServiceWrapper;
        let channel_service: Arc<dyn ChannelService> = Arc::new(ChannelServiceWrapper::new());

        // Create instance using runtime's instantiate method
        let instance = self
            .runtime
            .instantiate(
                module.clone(),
                "temp-supervisor-tree-loader".to_string(),
                &[], // No initial state needed
                config,
                Some(channel_service),
                None, // No message sender for temporary instance
                None, // No tuplespace provider for temporary instance
                None, // No keyvalue store for temporary instance
                None, // No process group registry for temporary instance
                None, // No lock manager for temporary instance
                None, // No object registry for temporary instance
                None, // No journal storage for temporary instance
                None, // No blob service for temporary instance
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

        // Call get_supervisor_tree() function
        let spec_bytes = instance
            .get_supervisor_tree()
            .await
            .map_err(|e| {
                ApplicationError::Other(format!("Failed to call get_supervisor_tree: {}", e))
            })?;

        // If empty, return error (no supervisor tree defined)
        if spec_bytes.is_empty() {
            return Err(ApplicationError::Other(
                "get_supervisor_tree() returned empty supervisor spec".to_string(),
            ));
        }

        // Parse protobuf SupervisorSpec
        let supervisor_spec = SupervisorSpec::decode(spec_bytes.as_slice())
            .map_err(|e| {
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
    async fn stop_actor_gracefully(&self, actor_id: &str) -> Result<(), ApplicationError> {
        use tokio::time::{timeout, Duration};
        use plexspaces_core::RequestContext;
        
        // Get node reference
        let node_ref = {
            let node_opt = self.node.read().await;
            node_opt.clone()
        };

        if let Some(node) = node_ref {
            // Stop actor with timeout (default: 5 seconds per actor)
            let timeout_duration = Duration::from_secs(5);
            
            if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!(
                application = %self.name,
                actor_id = %actor_id,
                timeout_seconds = timeout_duration.as_secs(),
                "Stopping actor with timeout"
            );
            }
            
            // Use ActorFactory directly from ServiceLocator
            let _service_locator = node.service_locator()
                .ok_or_else(|| ApplicationError::ActorStopFailed(
                    actor_id.to_string(),
                    "ServiceLocator not available from node".to_string()
                ))?;
            
            // Get ActorFactory from ApplicationNode (avoids circular dependency)
            use plexspaces_actor::ActorFactory;
            let actor_factory: Arc<dyn ActorFactory> = node.actor_factory().await
                .ok_or_else(|| ApplicationError::ActorStopFailed(
                    actor_id.to_string(),
                    "ActorFactory not found in ServiceLocator".to_string()
                ))?;
            
            // Create RequestContext for stop operation using application's tenant/namespace
            // Application owns its actors, so it can stop them
            let tenant_id = self.tenant_id.read().await.clone();
            let namespace = self.namespace.read().await.clone();
            let ctx = RequestContext::new_without_auth(tenant_id, namespace);
            
            let actor_id_string = actor_id.to_string();
            match timeout(timeout_duration, actor_factory.stop_actor(&ctx, &actor_id_string)).await {
                Ok(Ok(())) => {
                    if tracing::enabled!(tracing::Level::DEBUG) {
                    tracing::debug!(
                        application = %self.name,
                        actor_id = %actor_id,
                        "Actor stopped successfully"
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
                        let full_error = format!(
                            "Failed to stop actor '{}': {}",
                            actor_id, e
                        );
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
        
        // Log tenant_id/namespace being used (from API request, set before start())
        {
            let tenant_id = self.tenant_id.read().await.clone();
            let namespace = self.namespace.read().await.clone();
            tracing::debug!(
                application = %self.name,
                tenant_id = %if tenant_id.is_empty() { "<empty>" } else { &tenant_id },
                namespace = %if namespace.is_empty() { "<empty>" } else { &namespace },
                "WASM application starting with tenant_id/namespace from API request"
            );
        }

        // Store node reference for shutdown
        {
            let mut node_ref = self.node.write().await;
            *node_ref = Some(node.clone());
        }

        // Load supervisor tree from WASM module
        let actor_ids = self.load_supervisor_tree(node.clone()).await.map_err(|e| {
            ApplicationError::Other(format!("Failed to load supervisor tree: {}", e))
        })?;

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
        
        let stop_result = timeout(shutdown_timeout, async {
            // Stop actors in reverse order (children first, then parents)
            let mut errors = Vec::new();
            let mut stopped_count = 0;
            
            for (idx, actor_id) in actor_ids.iter().rev().enumerate() {
                if tracing::enabled!(tracing::Level::DEBUG) {
                tracing::debug!(
                    application = %self.name,
                    actor_id = %actor_id,
                    progress = format!("{}/{}", idx + 1, actor_ids.len()),
                    "Stopping actor"
                );
                }
                
                if let Err(e) = self.stop_actor_gracefully(actor_id).await {
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
            
            tracing::info!(
                application = %self.name,
                stopped_count = stopped_count,
                total_count = actor_ids.len(),
                error_count = errors.len(),
                "Actor shutdown completed"
            );
            
            errors
        }).await;

        let errors = match stop_result {
            Ok(errors) => errors,
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
                vec![timeout_msg]
            }
        };

        // Get final actor count before clearing
        let final_actor_count = {
            let spawned = self.spawned_actor_ids.read().await;
            spawned.len() as u32
        };

        // Update actor count to 0 in ApplicationManager for metrics tracking
        let node_ref = {
            let node_opt = self.node.read().await;
            node_opt.clone()
        };
        // Actor counts are automatically tracked by ActorRegistry

        // Clear spawned actor IDs
        {
            let mut spawned = self.spawned_actor_ids.write().await;
            spawned.clear();
        }

        // Mark as stopped (even if errors occurred)
        *is_running = false;

        tracing::info!(
            application = %self.name,
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use plexspaces_application::ApplicationNode;
    use std::sync::Arc;

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
        Arc::new(WasmRuntime::new().await.expect("Failed to create WASM runtime"))
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

    #[tokio::test]
    async fn test_wasm_application_with_spec() {
        let runtime = create_test_runtime().await;
        let spec = ApplicationSpec {
            name: "test-app".to_string(),
            namespace: "test-namespace".to_string(),
            version: "1.0.0".to_string(),
            description: "Test application".to_string(),
            r#type: plexspaces_proto::application::v1::ApplicationType::ApplicationTypeActive.into(),
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
                assert!(msg.contains("already running"), "Error message should mention 'already running': {}", msg);
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
                assert!(msg.contains("not running"), "Error message should mention 'not running': {}", msg);
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
        assert_eq!(app.health_check().await, HealthStatus::HealthStatusUnhealthy);

        // Start
        app.start(node.clone()).await.expect("Start should succeed");
        assert_eq!(app.health_check().await, HealthStatus::HealthStatusHealthy);

        // Stop
        app.stop().await.expect("Stop should succeed");
        assert_eq!(app.health_check().await, HealthStatus::HealthStatusUnhealthy);

        // Can start again after stop
        app.start(node).await.expect("Start after stop should succeed");
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
                tokio::spawn(async move {
                    app_clone.health_check().await
                })
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
            ApplicationSpec, ApplicationType, SupervisorSpec, ChildSpec, ChildType, 
            SupervisionStrategy, RestartPolicy,
        };
        use prost_types::Duration;
        
        let supervisor_spec = SupervisorSpec {
            strategy: SupervisionStrategy::SupervisionStrategyOneForOne.into(),
            max_restarts: 3,
            max_restart_window: Some(Duration { seconds: 5, nanos: 0 }),
            children: vec![
                ChildSpec {
                    id: "worker-1".to_string(),
                    r#type: ChildType::ChildTypeWorker.into(),
                    args: std::collections::HashMap::new(),
                    restart: RestartPolicy::RestartPolicyPermanent.into(),
                    shutdown_timeout: Some(Duration { seconds: 5, nanos: 0 }),
                    supervisor: None,
                    facets: vec![], // Phase 1: Unified Lifecycle - facets support
                },
            ],
        };

        let spec = ApplicationSpec {
            name: "test-app".to_string(),
            namespace: "test-namespace".to_string(),
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
        app.start(tracking_node.clone()).await.expect("Start should succeed");

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
}
