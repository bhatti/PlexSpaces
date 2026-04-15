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

//! Virtual Actor Manager for Orleans-inspired virtual actor lifecycle
//!
//! ## Purpose
//! Manages virtual actors (actors that are always addressable but activated on-demand).
//! Virtual actors are registered by type and can be messaged without explicit creation.
//!
//! ## Architecture Context
//! VirtualActorManager is the source of truth for virtual actor metadata. It tracks:
//! - Virtual actor types (for type-based registration - WASM and Rust applications)
//! - Individual virtual actor instances (for activation tracking)
//! - Pending activations (messages queued during activation)
//!
//! ActorRegistry tracks active instances and MessageSenders, but VirtualActorManager
//! tracks the virtual actor lifecycle metadata.
//!
//! ## Proto-First Design
//! - `actor_type` is required (from proto Actor.actor_type field)
//! - Uses actor_id factory methods for consistent ID parsing/construction
//! - All metadata follows proto definitions

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::virtual_actor_lifecycle_facet::VirtualActorLifecycleFacet;
use crate::virtual_actor_registration::VirtualActorDefinitionRegistration;
use crate::Service;
use crate::{ActorId, ActorRegistry};
use plexspaces_common::{from_config_str, ActivationStrategy};
use plexspaces_proto::common::v1::Facet as ProtoFacet;
use plexspaces_proto::common::v1::Message;
use plexspaces_proto::v1::actor::ActorConfig;

/// Virtual Actor Metadata
///
/// ## Purpose
/// Stores all metadata needed to recreate a virtual actor after suspension.
/// Per Orleans design: Virtual actors always exist (virtually) - metadata persists
/// even when the actor instance is deactivated.
///
/// ## Design Principles
/// - **Source of Truth**: VirtualActorManager is the ONLY place storing virtual actor metadata
/// - **ActorRegistry Separation**: ActorRegistry only tracks active instances and MessageSenders
/// - **No Memory Leaks**: Regular actors don't have metadata here (only in ActorRegistry)
/// - **Always Rebuildable**: All info needed to rebuild suspended actors is stored here
/// - **Proto-First**: actor_type is required (from proto Actor message) for consistency
///
/// ## Usage
/// - Instance-level: Stores metadata for specific actor instances (keyed by actor_id)
/// - Type-level: Stores metadata for virtual actor types (keyed by actor_type)
///   Used for WASM and Rust applications to enable automatic activation
#[derive(Clone)]
pub struct VirtualActorMetadata {
    /// Virtual actor facet (for lifecycle management)
    /// For type-level registration, this may be None (facet created from facet_config)
    pub facet: Option<Arc<RwLock<Box<dyn VirtualActorLifecycleFacet>>>>,
    /// Last deactivation time (None if currently active)
    pub last_deactivated: Option<std::time::SystemTime>,
    /// Actor type (e.g., "GenServer", "read-state-tracker") - REQUIRED
    /// From proto Actor.actor_type field - follows proto-first design
    pub actor_type: String,
    /// Actor configuration (resource requirements, etc.) - needed to rebuild suspended actors
    pub config: Option<ActorConfig>,
    /// Behavior kind captured at registration time.
    pub behavior_kind: Option<String>,
    /// Tenant ID (for proper isolation) - needed to rebuild suspended actors
    pub tenant_id: String,
    /// Namespace (for proper isolation) - needed to rebuild suspended actors
    pub namespace: String,
    /// Virtual actor facet config (for creating new instances from type-level registration)
    /// Only used for type-level registration (when facet is None)
    ///
    /// **Format**: Must be a JSON object keyed by facet type (e.g., `{"virtual_actor": {...}}`).
    /// This format is required for `create_facets_from_config()` to properly parse and create facets.
    /// Supports multiple facets: `{"virtual_actor": {...}, "durability": {...}}`.
    pub facet_config: Option<serde_json::Value>,
    /// Init config template for WASM actors (JSON bytes)
    ///
    /// **Purpose**: Preserves the config structure from ApplicationSpec's ChildSpec.args
    /// so that virtual WASM actors activated via HTTP receive the same config structure
    /// as actors deployed via ApplicationSpec.
    ///
    /// **Format**: JSON bytes matching the structure built in wasm_application.rs:
    /// ```json
    /// {
    ///   "actor_id": "<will be replaced>",
    ///   "behavior_kind": "...",
    ///   "args": { ... }
    /// }
    /// ```
    ///
    /// **Usage**: When activating a virtual actor, parse this template, replace `actor_id`
    /// with the actual actor_id, and pass as `initial_state` to `spawn_actor()`.
    pub init_config_template: Option<Vec<u8>>,
    /// Proto-first facet metadata captured at registration time.
    pub proto_facets: Vec<ProtoFacet>,

    /// Initial state bytes captured at registration.
    ///
    /// **Purpose**: Enables type-primary rebuild — activate_virtual_actor always calls
    /// spawn_actor with these bytes instead of storing and unwrapping an actor Arc.
    pub initial_state: Vec<u8>,

    /// Actor labels captured at registration.
    ///
    /// **Purpose**: Preserved alongside initial_state so spawn_actor can fully
    /// reconstruct the actor with the same parameters on reactivation.
    pub labels: HashMap<String, String>,

    /// Activation strategy for this actor instance.
    ///
    /// **Purpose**: Used by evict_lru_if_needed to skip eager/prewarm actors.
    /// Only lazy actors (Unspecified or Lazy) are subject to LRU eviction.
    pub activation_strategy: ActivationStrategy,
}

/// Active instance tracking for LRU eviction
/// Tracks active instances per actor_type with last_access time
#[derive(Clone)]
pub struct ActiveInstance {
    pub actor_id: ActorId,
    pub last_access: std::time::SystemTime,
}

/// Result of removing all virtual actor metadata owned by an application namespace.
#[derive(Debug, Default, Clone)]
pub struct VirtualActorNamespaceCleanup {
    /// Removed virtual actor instance IDs.
    pub actor_ids: Vec<ActorId>,
    /// Removed type registrations.
    pub actor_types: Vec<String>,
}

/// Virtual Actor Registry - stores virtual actor metadata
pub struct VirtualActorRegistry {
    /// Virtual actor metadata: actor_id -> VirtualActorMetadata
    virtual_actors: Arc<RwLock<HashMap<ActorId, VirtualActorMetadata>>>,
    /// Pending messages for actors being activated: actor_id -> Vec<Message>
    pending_activations: Arc<RwLock<HashMap<ActorId, Vec<Message>>>>,
    /// Virtual actor types: actor_type -> VirtualActorMetadata
    /// Used to check if an actor type is virtual (for WASM and Rust applications)
    /// Key is actor_type (e.g., "read-state-tracker"), value is metadata for creating instances
    /// Uses VirtualActorMetadata with actor_type field set (required for type-level registration)
    virtual_actor_types: Arc<RwLock<HashMap<String, VirtualActorMetadata>>>,
    /// Active instances per actor_type for LRU eviction: actor_type -> Vec<ActiveInstance>
    /// Used to track active instances and evict LRU when max_pool_per_actor_type is exceeded
    active_instances_by_type: Arc<RwLock<HashMap<String, Vec<ActiveInstance>>>>,
}

impl VirtualActorRegistry {
    fn new() -> Self {
        Self {
            virtual_actors: Arc::new(RwLock::new(HashMap::new())),
            pending_activations: Arc::new(RwLock::new(HashMap::new())),
            virtual_actor_types: Arc::new(RwLock::new(HashMap::new())),
            active_instances_by_type: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn virtual_actors(&self) -> &Arc<RwLock<HashMap<ActorId, VirtualActorMetadata>>> {
        &self.virtual_actors
    }

    pub fn pending_activations(&self) -> &Arc<RwLock<HashMap<ActorId, Vec<Message>>>> {
        &self.pending_activations
    }

    pub fn virtual_actor_types(&self) -> &Arc<RwLock<HashMap<String, VirtualActorMetadata>>> {
        &self.virtual_actor_types
    }

    pub fn active_instances_by_type(&self) -> &Arc<RwLock<HashMap<String, Vec<ActiveInstance>>>> {
        &self.active_instances_by_type
    }
}

/// Virtual Actor Error
#[derive(Debug, Clone, thiserror::Error)]
pub enum VirtualActorError {
    /// Actor not found
    #[error("Actor not found: {0}")]
    ActorNotFound(String),

    /// Activation failed
    #[error("Activation failed: {0}")]
    ActivationFailed(String),
}

/// Virtual Actor Manager - manages virtual actor lifecycle
pub struct VirtualActorManager {
    registry: VirtualActorRegistry,
    actor_registry: Arc<ActorRegistry>,
    /// Max pool size per actor type (for LRU eviction)
    /// Defaults to DEFAULT_MAX_POOL_PER_ACTOR_TYPE (100) if not set
    max_pool_per_actor_type: Arc<RwLock<u32>>,
}

impl VirtualActorManager {
    fn activation_strategy_from_facet_config(
        facet_config: &serde_json::Value,
    ) -> ActivationStrategy {
        facet_config
            .get("virtual_actor")
            .and_then(|config| config.get("activation_strategy"))
            .and_then(serde_json::Value::as_str)
            .map(from_config_str)
            .unwrap_or(ActivationStrategy::ActivationStrategyLazy)
    }

    pub fn new(actor_registry: Arc<ActorRegistry>) -> Self {
        use plexspaces_common::virtual_actor_config::DEFAULT_MAX_POOL_PER_ACTOR_TYPE;
        Self {
            registry: VirtualActorRegistry::new(),
            actor_registry,
            max_pool_per_actor_type: Arc::new(RwLock::new(DEFAULT_MAX_POOL_PER_ACTOR_TYPE)),
        }
    }

    /// Update max_pool_per_actor_type from RuntimeConfig
    ///
    /// ## Purpose
    /// Sets the maximum number of active instances per actor type for LRU eviction.
    /// Called during node initialization after RuntimeConfig is available.
    ///
    /// ## Arguments
    /// * `max_pool` - Maximum pool size per actor type (from RuntimeConfig.default_virtual_actor_config)
    pub async fn set_max_pool_per_actor_type(&self, max_pool: u32) {
        let mut pool_size = self.max_pool_per_actor_type.write().await;
        *pool_size = max_pool;
    }

    /// Get max_pool_per_actor_type
    pub async fn get_max_pool_per_actor_type(&self) -> u32 {
        let pool_size = self.max_pool_per_actor_type.read().await;
        *pool_size
    }

    /// Get virtual actor registry (for node-level access)
    ///
    /// ## Design
    /// VirtualActorManager is the source of truth for virtual actor metadata.
    /// This getter allows Node to access the registry for observability/debugging.
    pub fn registry(&self) -> &VirtualActorRegistry {
        &self.registry
    }

    /// Register a virtual actor (always addressable, may not be active)
    ///
    /// ## Purpose
    /// Registers a virtual actor in the system. Per Orleans design, virtual actors
    /// are always registered (even when not active) so they can receive messages.
    ///
    /// ## Orleans Design Principles
    /// - Virtual actors always exist (virtually) - metadata persists across activation/deactivation
    /// - Metadata is stored in VirtualActorManager (source of truth)
    /// - ActorRegistry only tracks active instances and MessageSenders
    /// - Suspended actors can be rebuilt from metadata stored here
    ///
    /// ## Arguments
    /// * `actor_id` - Actor ID
    /// * `facet` - VirtualActorLifecycleFacet for lifecycle management
    /// * `actor_type` - Actor type (e.g., "GenServer") - REQUIRED, from proto Actor.actor_type
    /// * `config` - Actor configuration - needed to rebuild suspended actors
    /// * `tenant_id` - Tenant ID for isolation
    /// * `namespace` - Namespace for isolation
    ///
    /// ## State After Registration
    /// - Actor is registered (always addressable)
    /// - Metadata stored in VirtualActorManager (persists across suspension)
    /// - For lazy actors: Not active until first local message
    /// - For eager actors: Active immediately (ActorRef in registry, message loop running)
    pub async fn register(
        &self,
        actor_id: ActorId,
        facet: Arc<RwLock<Box<dyn VirtualActorLifecycleFacet>>>,
        actor_type: String,
        config: Option<ActorConfig>,
        tenant_id: String,
        namespace: String,
        initial_state: Vec<u8>,
        labels: HashMap<String, String>,
        activation_strategy: ActivationStrategy,
    ) -> Result<(), VirtualActorError> {
        if actor_type.is_empty() {
            return Err(VirtualActorError::ActivationFailed(
                "actor_type is required (from proto Actor.actor_type)".to_string(),
            ));
        }

        let type_metadata = {
            let virtual_types = self.registry.virtual_actor_types().read().await;
            virtual_types.get(&actor_type).cloned()
        };

        let mut virtual_actors = self.registry.virtual_actors().write().await;
        virtual_actors.insert(
            actor_id,
            VirtualActorMetadata {
                init_config_template: type_metadata
                    .as_ref()
                    .and_then(|metadata| metadata.init_config_template.clone()),
                facet: Some(facet),
                last_deactivated: None,
                actor_type,
                config,
                behavior_kind: type_metadata
                    .as_ref()
                    .and_then(|metadata| metadata.behavior_kind.clone()),
                tenant_id,
                namespace,
                facet_config: type_metadata
                    .as_ref()
                    .and_then(|metadata| metadata.facet_config.clone()),
                proto_facets: type_metadata
                    .as_ref()
                    .map(|metadata| metadata.proto_facets.clone())
                    .unwrap_or_default(),
                initial_state,
                labels,
                activation_strategy,
            },
        );
        Ok(())
    }

    /// Update virtual actor metadata (e.g., when actor is activated with config)
    ///
    /// ## Purpose
    /// Updates metadata for an already-registered virtual actor.
    /// Used when actor is activated and we have additional info (config, etc.)
    ///
    /// ## Design
    /// Idempotent - can be called multiple times safely.
    pub async fn update_metadata(
        &self,
        actor_id: &ActorId,
        actor_type: String,
        config: Option<ActorConfig>,
    ) -> Result<(), VirtualActorError> {
        let mut virtual_actors = self.registry.virtual_actors().write().await;
        if let Some(metadata) = virtual_actors.get_mut(actor_id) {
            metadata.actor_type = actor_type;
            if let Some(config) = config {
                metadata.config = Some(config);
            }
            Ok(())
        } else {
            Err(VirtualActorError::ActorNotFound(actor_id.to_string()))
        }
    }

    /// Check if actor is virtual (registered as virtual actor)
    ///
    /// ## Returns
    /// true if actor is registered as virtual actor, false otherwise
    ///
    /// ## Note
    /// This checks if the actor is registered as a virtual actor, not if it's active.
    /// Virtual actors are always registered (always addressable), even when not active.
    pub async fn is_virtual(&self, actor_id: &ActorId) -> bool {
        let virtual_actors = self.registry.virtual_actors().read().await;
        if virtual_actors.contains_key(actor_id) {
            return true;
        }
        drop(virtual_actors);

        let virtual_types = self.registry.virtual_actor_types().read().await;
        if virtual_types.contains_key(actor_id.actor_type()) {
            return true;
        }

        false
    }

    /// Register a virtual actor type (for WASM and Rust applications)
    ///
    /// ## Purpose
    /// Registers an actor type as virtual, enabling automatic activation of any actor ID
    /// matching that type pattern. Uses VirtualActorMetadata with actor_type field set.
    ///
    /// ## Arguments
    /// * `actor_type` - Actor type (e.g., "read-state-tracker") - REQUIRED, from proto Actor.actor_type
    /// * `config` - Actor configuration template
    /// * `namespace` - Namespace for isolation
    /// * `facet_config` - Virtual actor facet configuration (for creating new instances)
    ///   **MUST be a JSON object keyed by facet type** (e.g., `{"virtual_actor": {"idle_timeout": "5m", "activation_strategy": "lazy"}}`).
    ///   This format is required for `create_facets_from_config()` to properly parse and create facets.
    ///   Supports multiple facets: `{"virtual_actor": {...}, "durability": {...}}`.
    /// * `tenant_id` - Tenant ID for isolation (defaults to empty for type-level registration)
    /// * `init_config_template` - Init config template for WASM actors (JSON bytes, optional)
    ///   Preserves config structure from ApplicationSpec's ChildSpec.args for virtual actor activation
    pub async fn register_virtual_actor_type(
        &self,
        actor_type: String,
        config: Option<ActorConfig>,
        namespace: String,
        facet_config: serde_json::Value,
        tenant_id: Option<String>,
        init_config_template: Option<Vec<u8>>,
    ) -> Result<(), VirtualActorError> {
        self.register_virtual_actor_definition(VirtualActorDefinitionRegistration {
            actor_type,
            behavior_kind: None,
            namespace,
            actor_config: config,
            proto_facets: Vec::new(),
            facet_config,
            tenant_id,
            init_config_template,
        })
        .await
    }

    /// Register a virtual actor definition using the shared framework metadata shape.
    pub async fn register_virtual_actor_definition(
        &self,
        definition: VirtualActorDefinitionRegistration,
    ) -> Result<(), VirtualActorError> {
        let VirtualActorDefinitionRegistration {
            actor_type,
            behavior_kind,
            namespace,
            actor_config,
            proto_facets,
            facet_config,
            tenant_id,
            init_config_template,
        } = definition;

        if actor_type.is_empty() {
            return Err(VirtualActorError::ActivationFailed(
                "actor_type is required (from proto Actor.actor_type)".to_string(),
            ));
        }

        let activation_strategy = Self::activation_strategy_from_facet_config(&facet_config);

        let mut virtual_types = self.registry.virtual_actor_types().write().await;

        // Log facet_config for debugging (show which facets are being registered)
        let facet_types: Vec<String> = facet_config
            .as_object()
            .map(|obj| obj.keys().cloned().collect())
            .unwrap_or_default();

        virtual_types.insert(
            actor_type.clone(),
            VirtualActorMetadata {
                facet: None, // Facet created from facet_config when needed
                last_deactivated: None,
                actor_type: actor_type.clone(),
                config: actor_config,
                behavior_kind,
                tenant_id: tenant_id.unwrap_or_default(),
                namespace,
                facet_config: Some(facet_config),
                init_config_template,
                proto_facets,
                initial_state: Vec::new(),
                labels: HashMap::new(),
                activation_strategy,
            },
        );
        if tracing::enabled!(tracing::Level::TRACE) {
            tracing::trace!(
                actor_type = %actor_type,
                facet_types = ?facet_types,
                "Registered virtual actor type"
            );
        }
        Ok(())
    }

    /// Get virtual actor type metadata
    ///
    /// ## Purpose
    /// Retrieves metadata for a virtual actor type, used when auto-activating actors.
    /// Includes facet_config with all configured facets (virtual_actor, timer, reminder, workflow)
    /// so that resurrection recreates every facet with its original config.
    ///
    /// ## Invariant
    /// Type registrations are NEVER evicted on actor vacation (deactivation/LRU eviction).
    /// They are only removed when an application is explicitly undeployed.
    /// This means actors can always be resurrected by type even after long idle periods.
    ///
    /// ## Returns
    /// `Some(VirtualActorMetadata)` if actor type is virtual, `None` otherwise
    pub async fn get_virtual_actor_type(&self, actor_type: &str) -> Option<VirtualActorMetadata> {
        let virtual_types = self.registry.virtual_actor_types().read().await;
        virtual_types.get(actor_type).cloned()
    }

    /// Check if actor type is virtual
    ///
    /// ## Purpose
    /// Checks if an actor type is registered as virtual (for WASM and Rust applications).
    ///
    /// ## Returns
    /// true if actor type is virtual, false otherwise
    pub async fn is_virtual_actor_type(&self, actor_type: &str) -> bool {
        let virtual_types = self.registry.virtual_actor_types().read().await;
        virtual_types.contains_key(actor_type)
    }

    /// Get virtual actor metadata
    ///
    /// ## Purpose
    /// Retrieves metadata for a virtual actor instance. Used when rebuilding suspended actors.
    ///
    /// ## Returns
    /// `Some(VirtualActorMetadata)` if actor is registered as virtual, `None` otherwise
    pub async fn get_metadata(&self, actor_id: &ActorId) -> Option<VirtualActorMetadata> {
        let virtual_actors = self.registry.virtual_actors().read().await;
        virtual_actors.get(actor_id).cloned()
    }

    /// Remove all virtual actor registrations owned by an application namespace.
    ///
    /// ## Purpose
    /// Explicit undeploy must remove both type registrations and instance metadata so a later
    /// redeploy starts from a clean slate instead of resurrecting stale virtual actors.
    pub async fn unregister_namespace(&self, namespace: &str) -> VirtualActorNamespaceCleanup {
        let mut cleanup = VirtualActorNamespaceCleanup::default();

        {
            let mut virtual_actors = self.registry.virtual_actors().write().await;
            let actor_ids: Vec<ActorId> = virtual_actors
                .iter()
                .filter(|(_, metadata)| metadata.namespace == namespace)
                .map(|(actor_id, _)| actor_id.clone())
                .collect();
            for actor_id in &actor_ids {
                virtual_actors.remove(actor_id);
            }
            cleanup.actor_ids = actor_ids;
        }

        {
            let mut pending = self.registry.pending_activations().write().await;
            for actor_id in &cleanup.actor_ids {
                pending.remove(actor_id);
            }
        }

        {
            let mut virtual_types = self.registry.virtual_actor_types().write().await;
            let actor_types: Vec<String> = virtual_types
                .iter()
                .filter(|(_, metadata)| metadata.namespace == namespace)
                .map(|(actor_type, _)| actor_type.clone())
                .collect();
            for actor_type in &actor_types {
                virtual_types.remove(actor_type);
            }
            cleanup.actor_types = actor_types;
        }

        {
            let mut active_instances = self.registry.active_instances_by_type().write().await;
            for actor_type in &cleanup.actor_types {
                active_instances.remove(actor_type);
            }
        }

        cleanup
    }

    /// Get virtual actor facet
    ///
    /// ## Returns
    /// Facet implementing `VirtualActorLifecycleFacet` trait.
    /// Uses trait-based design instead of `Any` types for type safety.
    pub async fn get_facet(
        &self,
        actor_id: &ActorId,
    ) -> Result<Arc<RwLock<Box<dyn VirtualActorLifecycleFacet>>>, VirtualActorError> {
        let virtual_actors = self.registry.virtual_actors().read().await;
        let virtual_meta = virtual_actors
            .get(actor_id)
            .ok_or_else(|| VirtualActorError::ActorNotFound(actor_id.to_string()))?;

        virtual_meta.facet.clone().ok_or_else(|| {
            VirtualActorError::ActivationFailed(format!(
                "Virtual actor {} has no facet (type-level registration)",
                actor_id
            ))
        })
    }

    /// Queue a message for processing after activation
    pub async fn queue_message(&self, actor_id: &ActorId, message: Message) {
        let mut pending = self.registry.pending_activations().write().await;
        pending
            .entry(actor_id.clone())
            .or_insert_with(Vec::new)
            .push(message);
    }

    /// Get and clear pending messages for an actor
    pub async fn take_pending_messages(&self, actor_id: &ActorId) -> Vec<Message> {
        let mut pending = self.registry.pending_activations().write().await;
        pending.remove(actor_id).unwrap_or_default()
    }

    /// Check if actor is active (has running message loop)
    ///
    /// ## Purpose
    /// Checks if the actor is currently active (has a running message loop processing messages).
    ///
    /// ## Returns
    /// - `true`: Actor is active (ActorRef in registry, message loop running)
    /// - `false`: Actor is passivated or has not been activated yet
    ///
    /// ## Design Note (Orleans-Inspired)
    /// Virtual actors can be registered but not active. This method checks the actual
    /// activation state via ActorRegistry.
    pub async fn is_active(&self, actor_id: &ActorId) -> bool {
        // Check if actor is registered as virtual
        if !self.is_virtual(actor_id).await {
            return false;
        }

        // Check actor state via ActorRegistry
        self.actor_registry.is_actor_state_active(actor_id).await
    }

    /// Mark a virtual actor as activated
    ///
    /// ## Purpose
    /// Marks a virtual actor as activated (in memory). Used after actor activation completes.
    /// Updates last_access time for LRU tracking.
    ///
    /// ## Arguments
    /// * `actor_id` - Actor ID to mark as activated
    ///
    /// ## Returns
    /// Ok(()) if successful, VirtualActorError if failed
    pub async fn mark_activated(&self, actor_id: &ActorId) -> Result<(), VirtualActorError> {
        // Verify actor is virtual
        if !self.is_virtual(actor_id).await {
            return Err(VirtualActorError::ActorNotFound(format!(
                "Actor {} is not virtual",
                actor_id
            )));
        }

        // Get actor_type for LRU tracking
        let actor_type = {
            let virtual_actors = self.registry.virtual_actors().read().await;
            if let Some(metadata) = virtual_actors.get(actor_id) {
                metadata.actor_type.clone()
            } else {
                // Try type-level registration
                actor_id.actor_type().to_string()
            }
        };

        // Track active instance for LRU eviction
        let now = std::time::SystemTime::now();
        let mut active_instances = self.registry.active_instances_by_type().write().await;
        let instances = active_instances.entry(actor_type).or_insert_with(Vec::new);

        // Update or add entry
        if let Some(existing) = instances.iter_mut().find(|i| i.actor_id == *actor_id) {
            existing.last_access = now;
        } else {
            instances.push(ActiveInstance {
                actor_id: actor_id.clone(),
                last_access: now,
            });
        }

        // Actor state is managed by ActorRegistry, so we just verify it exists
        // The actual activation state is tracked in ActorRegistry
        Ok(())
    }

    /// Evict LRU actors if max_pool_per_actor_type is exceeded
    ///
    /// ## Purpose
    /// Checks if activating a new actor would exceed max_pool_per_actor_type.
    /// If so, evicts the least recently used (LRU) actors of that type.
    ///
    /// ## Arguments
    /// * `actor_type` - Actor type to check
    /// * `service_locator` - ServiceLocator for deactivating actors (optional, uses default if None)
    ///
    /// ## Returns
    /// Vector of actor IDs that were evicted (for logging/observability)
    pub async fn evict_lru_if_needed(
        &self,
        actor_type: &str,
        service_locator: Option<Arc<dyn crate::ServiceLocator>>,
    ) -> Vec<ActorId> {
        let max_pool = self.get_max_pool_per_actor_type().await;
        let mut active_instances = self.registry.active_instances_by_type().write().await;

        let instances = match active_instances.get_mut(actor_type) {
            Some(instances) => instances,
            None => return Vec::new(), // No active instances, no eviction needed
        };

        // Filter to only currently active lazy instances (check ActorRegistry).
        // Eager and prewarm actors are never subject to LRU eviction.
        let mut active_only: Vec<ActiveInstance> = Vec::new();
        for instance in instances.iter() {
            // Skip eager/prewarm actors — they run until explicitly stopped.
            // Only Unspecified and Lazy strategies are subject to LRU eviction.
            let is_lazy = {
                let virtual_actors = self.registry.virtual_actors().read().await;
                virtual_actors
                    .get(&instance.actor_id)
                    .map(|m| {
                        matches!(
                            m.activation_strategy,
                            ActivationStrategy::ActivationStrategyLazy
                                | ActivationStrategy::ActivationStrategyUnspecified
                        )
                    })
                    .unwrap_or(true) // default to evictable if no metadata
            };
            if !is_lazy {
                continue;
            }
            if self
                .actor_registry
                .is_actor_state_active(&instance.actor_id)
                .await
            {
                active_only.push(instance.clone());
            }
        }

        // If we're under the limit, no eviction needed
        if active_only.len() < max_pool as usize {
            // Update instances list (remove inactive ones)
            *instances = active_only;
            return Vec::new();
        }

        // Sort by last_access (oldest first) for LRU eviction
        active_only.sort_by_key(|i| i.last_access);

        // Evict oldest instances until we're under the limit
        let to_evict = active_only.len() - (max_pool as usize - 1); // Keep one slot for new actor
        let evicted: Vec<ActorId> = active_only[..to_evict]
            .iter()
            .map(|i| i.actor_id.clone())
            .collect();

        // Remove evicted actors from tracking
        instances.retain(|i| !evicted.contains(&i.actor_id));

        // Deactivate evicted actors while preserving metadata for later reactivation.
        if let Some(service_locator) = service_locator {
            for actor_id in &evicted {
                if let Some(actor_factory) = service_locator.get_actor_factory().await {
                    let ctx = service_locator
                        .request_context_for_system_operations()
                        .await;
                    if actor_factory.stop_actor(&ctx, actor_id).await.is_ok() {
                        tracing::debug!(
                            actor_id = %actor_id,
                            actor_type = %actor_type,
                            "LRU-evicted virtual actor suspended (metadata preserved)"
                        );
                    }
                }
            }
        }

        evicted
    }

    /// Update last access time for an active actor (for LRU tracking)
    ///
    /// ## Purpose
    /// Updates the last_access time when an actor receives a message or performs an operation.
    /// Used to maintain accurate LRU ordering.
    ///
    /// ## Arguments
    /// * `actor_id` - Actor ID to update
    pub async fn update_last_access(&self, actor_id: &ActorId) {
        // Get actor_type
        let actor_type = {
            let virtual_actors = self.registry.virtual_actors().read().await;
            if let Some(metadata) = virtual_actors.get(actor_id) {
                metadata.actor_type.clone()
            } else {
                actor_id.actor_type().to_string()
            }
        };

        // Update last_access time
        let now = std::time::SystemTime::now();
        let mut active_instances = self.registry.active_instances_by_type().write().await;
        if let Some(instances) = active_instances.get_mut(&actor_type) {
            if let Some(instance) = instances.iter_mut().find(|i| i.actor_id == *actor_id) {
                instance.last_access = now;
            }
        }
    }

    /// Remove actor from active instances tracking (called on deactivation)
    ///
    /// ## Purpose
    /// Removes an actor from active instances tracking when it's deactivated.
    /// This keeps the LRU tracking accurate.
    ///
    /// ## Arguments
    /// * `actor_id` - Actor ID to remove
    pub async fn remove_from_active_tracking(&self, actor_id: &ActorId) {
        // Get actor_type
        let actor_type = {
            let virtual_actors = self.registry.virtual_actors().read().await;
            if let Some(metadata) = virtual_actors.get(actor_id) {
                metadata.actor_type.clone()
            } else {
                actor_id.actor_type().to_string()
            }
        };

        // Remove from active instances
        let mut active_instances = self.registry.active_instances_by_type().write().await;
        if let Some(instances) = active_instances.get_mut(&actor_type) {
            instances.retain(|i| i.actor_id != *actor_id);
        }
    }
}

// Implement Service trait for VirtualActorManager (required for ServiceLocator)
impl crate::Service for VirtualActorManager {
    fn service_name(&self) -> String {
        crate::service_names::VIRTUAL_ACTOR_MANAGER.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actor_context::ObjectRegistry;
    use crate::virtual_actor_registration::VirtualActorDefinitionRegistration;
    use crate::ActorRegistry;
    use async_trait::async_trait;
    use plexspaces_object_registry::{ObjectRegistryImpl, SqliteObjectRegistryRepository};
    use plexspaces_proto::common::v1::Facet as ProtoFacet;
    use std::collections::HashMap;
    use std::sync::Arc;

    // Helper to wrap ObjectRegistry for ActorRegistry
    struct ObjectRegistryAdapter {
        inner: Arc<ObjectRegistryImpl>,
    }

    #[async_trait]
    impl ObjectRegistry for ObjectRegistryAdapter {
        async fn lookup(
            &self,
            ctx: &crate::RequestContext,
            object_id: &str,
            object_type: Option<plexspaces_proto::object_registry::v1::ObjectType>,
        ) -> Result<
            Option<plexspaces_proto::object_registry::v1::ObjectRegistration>,
            Box<dyn std::error::Error + Send + Sync>,
        > {
            self.inner
                .lookup(ctx, object_type.unwrap_or_default(), object_id)
                .await
                .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
        }

        async fn lookup_full(
            &self,
            ctx: &crate::RequestContext,
            object_type: plexspaces_proto::object_registry::v1::ObjectType,
            object_id: &str,
        ) -> Result<
            Option<plexspaces_proto::object_registry::v1::ObjectRegistration>,
            Box<dyn std::error::Error + Send + Sync>,
        > {
            self.inner.lookup_full(ctx, object_type, object_id).await
        }

        async fn register(
            &self,
            ctx: &crate::RequestContext,
            registration: plexspaces_proto::object_registry::v1::ObjectRegistration,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            self.inner
                .register(ctx, registration)
                .await
                .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
        }

        async fn unregister(
            &self,
            ctx: &crate::RequestContext,
            object_type: plexspaces_proto::object_registry::v1::ObjectType,
            object_id: &str,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            self.inner
                .unregister(ctx, object_type, object_id)
                .await
                .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
        }

        async fn heartbeat(
            &self,
            ctx: &crate::RequestContext,
            object_type: plexspaces_proto::object_registry::v1::ObjectType,
            object_id: &str,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            self.inner
                .heartbeat(ctx, object_type, object_id)
                .await
                .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
        }

        async fn discover(
            &self,
            ctx: &crate::RequestContext,
            object_type: Option<plexspaces_proto::object_registry::v1::ObjectType>,
            object_category: Option<String>,
            capabilities: Option<Vec<String>>,
            labels: Option<Vec<String>>,
            health_status: Option<plexspaces_proto::object_registry::v1::HealthStatus>,
            offset: usize,
            limit: usize,
        ) -> Result<
            Vec<plexspaces_proto::object_registry::v1::ObjectRegistration>,
            Box<dyn std::error::Error + Send + Sync>,
        > {
            self.inner
                .discover(
                    ctx,
                    object_type,
                    object_category,
                    capabilities,
                    labels,
                    health_status,
                    offset,
                    limit,
                )
                .await
                .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
        }
    }

    async fn create_test_manager() -> VirtualActorManager {
        let object_repo = Arc::new(
            SqliteObjectRegistryRepository::new(":memory:")
                .await
                .unwrap(),
        );
        let object_registry_impl = Arc::new(ObjectRegistryImpl::new(object_repo));
        let object_registry: Arc<dyn ObjectRegistry> = Arc::new(ObjectRegistryAdapter {
            inner: object_registry_impl,
        });
        let actor_registry = Arc::new(ActorRegistry::new(object_registry, "test-node".to_string()));
        VirtualActorManager::new(actor_registry)
    }

    #[tokio::test]
    async fn test_register_virtual_actor_type() {
        let manager = create_test_manager().await;

        let actor_type = "read-state-tracker".to_string();
        let namespace = "orbit-read-state-ts".to_string();
        let facet_config = serde_json::json!({
            "virtual_actor": {
                "idle_timeout": "5m",
                "activation_strategy": "lazy"
            }
        });

        // Register virtual actor type
        let result = manager
            .register_virtual_actor_type(
                actor_type.clone(),
                None,
                namespace.clone(),
                facet_config.clone(),
                None,
                None, // init_config_template
            )
            .await;

        assert!(result.is_ok());

        // Verify it's registered
        assert!(manager.is_virtual_actor_type(&actor_type).await);

        // Get metadata
        let metadata = manager.get_virtual_actor_type(&actor_type).await;
        assert!(metadata.is_some());
        let metadata = metadata.unwrap();
        assert_eq!(metadata.actor_type, actor_type);
        assert_eq!(metadata.namespace, namespace);
        assert_eq!(metadata.facet_config, Some(facet_config));
        assert_eq!(metadata.tenant_id, ""); // Default for type-level registration
        assert_eq!(
            metadata.activation_strategy,
            ActivationStrategy::ActivationStrategyLazy
        );
    }

    #[tokio::test]
    async fn test_register_virtual_actor_type_with_config() {
        let manager = create_test_manager().await;

        let actor_type = "configured-type".to_string();
        let namespace = "namespace".to_string();
        let facet_config = serde_json::json!({
            "virtual_actor": {
                "idle_timeout": "5m",
                "activation_strategy": "lazy"
            }
        });
        let config = Some(ActorConfig {
            max_mailbox_size: 1000,
            ..Default::default()
        });

        let result = manager
            .register_virtual_actor_type(
                actor_type.clone(),
                config.clone(),
                namespace.clone(),
                facet_config.clone(),
                None,
                None,
            )
            .await;

        assert!(result.is_ok());

        let metadata = manager.get_virtual_actor_type(&actor_type).await;
        assert!(metadata.is_some());
        let metadata = metadata.unwrap();
        assert_eq!(metadata.config, config);
    }

    #[tokio::test]
    async fn test_register_virtual_actor_type_with_tenant_id() {
        let manager = create_test_manager().await;

        let actor_type = "tenant-type".to_string();
        let namespace = "namespace".to_string();
        let facet_config = serde_json::json!({
            "virtual_actor": {
                "idle_timeout": "5m",
                "activation_strategy": "lazy"
            }
        });
        let tenant_id = Some("tenant-123".to_string());

        let result = manager
            .register_virtual_actor_type(
                actor_type.clone(),
                None,
                namespace.clone(),
                facet_config.clone(),
                tenant_id.clone(),
                None,
            )
            .await;

        assert!(result.is_ok());

        let metadata = manager.get_virtual_actor_type(&actor_type).await;
        assert!(metadata.is_some());
        let metadata = metadata.unwrap();
        assert_eq!(metadata.tenant_id, tenant_id.unwrap());
    }

    #[tokio::test]
    async fn test_register_virtual_actor_definition_preserves_behavior_and_proto_facets() {
        let manager = create_test_manager().await;

        let actor_type = "workflow-type".to_string();
        let namespace = "namespace".to_string();
        let facet_config = serde_json::json!({
            "virtual_actor": {
                "idle_timeout": "5m",
                "activation_strategy": "lazy"
            },
            "durability": {
                "checkpoint_interval": 100
            }
        });

        let result = manager
            .register_virtual_actor_definition(VirtualActorDefinitionRegistration {
                actor_type: actor_type.clone(),
                behavior_kind: Some("Workflow".to_string()),
                namespace: namespace.clone(),
                actor_config: None,
                proto_facets: vec![
                    ProtoFacet {
                        r#type: "virtual_actor".to_string(),
                        config: HashMap::from([
                            ("idle_timeout".to_string(), "5m".to_string()),
                            ("activation_strategy".to_string(), "lazy".to_string()),
                        ]),
                        priority: 100,
                        state: HashMap::new(),
                        metadata: None,
                    },
                    ProtoFacet {
                        r#type: "durability".to_string(),
                        config: HashMap::from([(
                            "checkpoint_interval".to_string(),
                            "100".to_string(),
                        )]),
                        priority: 90,
                        state: HashMap::new(),
                        metadata: None,
                    },
                ],
                facet_config: facet_config.clone(),
                tenant_id: Some("tenant-123".to_string()),
                init_config_template: Some(br#"{"actor_id":"template"}"#.to_vec()),
            })
            .await;

        assert!(result.is_ok());

        let metadata = manager
            .get_virtual_actor_type(&actor_type)
            .await
            .expect("metadata should exist");
        assert_eq!(metadata.actor_type, actor_type);
        assert_eq!(metadata.namespace, namespace);
        assert_eq!(metadata.behavior_kind.as_deref(), Some("Workflow"));
        assert_eq!(metadata.tenant_id, "tenant-123");
        assert_eq!(metadata.facet_config, Some(facet_config));
        assert_eq!(metadata.proto_facets.len(), 2);
        assert_eq!(metadata.proto_facets[0].r#type, "virtual_actor");
        assert_eq!(metadata.proto_facets[1].r#type, "durability");
        assert_eq!(
            metadata.init_config_template,
            Some(br#"{"actor_id":"template"}"#.to_vec())
        );
    }

    #[tokio::test]
    async fn test_unregister_namespace_removes_virtual_types_and_instances() {
        let manager = create_test_manager().await;
        let actor_id = ActorId::new("cart-1", "abstractions", "demo", "test-node").unwrap();

        manager
            .register(
                actor_id.clone(),
                Arc::new(RwLock::new(
                    Box::new(MockLifecycleFacet) as Box<dyn VirtualActorLifecycleFacet>
                )),
                "abstractions".to_string(),
                None,
                "tenant".to_string(),
                "demo".to_string(),
                Vec::new(),
                HashMap::new(),
                ActivationStrategy::ActivationStrategyLazy,
            )
            .await
            .expect("virtual actor instance should register");

        manager
            .register_virtual_actor_definition(VirtualActorDefinitionRegistration {
                actor_type: "abstractions".to_string(),
                behavior_kind: Some("GenServer".to_string()),
                namespace: "demo".to_string(),
                actor_config: None,
                proto_facets: vec![],
                facet_config: serde_json::json!({
                    "virtual_actor": {
                        "idle_timeout": "5m",
                        "activation_strategy": "lazy"
                    }
                }),
                tenant_id: Some("tenant".to_string()),
                init_config_template: None,
            })
            .await
            .expect("virtual actor type should register");

        let cleanup = manager.unregister_namespace("demo").await;
        assert_eq!(cleanup.actor_ids, vec![actor_id.clone()]);
        assert_eq!(cleanup.actor_types, vec!["abstractions".to_string()]);
        assert!(manager.get_metadata(&actor_id).await.is_none());
        assert!(!manager.is_virtual_actor_type("abstractions").await);
    }

    #[tokio::test]
    async fn test_register_virtual_actor_type_empty_actor_type() {
        let manager = create_test_manager().await;

        let result = manager
            .register_virtual_actor_type(
                String::new(),
                None,
                "namespace".to_string(),
                serde_json::Value::Null,
                None,
                None,
            )
            .await;

        assert!(result.is_err());
        match result.unwrap_err() {
            VirtualActorError::ActivationFailed(_) => {}
            _ => panic!("Expected ActivationFailed error"),
        }
    }

    /// Minimal mock lifecycle facet for unit tests (no dependency on plexspaces_journaling)
    #[derive(Debug)]
    struct MockLifecycleFacet;

    #[async_trait::async_trait]
    impl VirtualActorLifecycleFacet for MockLifecycleFacet {
        async fn get_activation_strategy(&self) -> plexspaces_common::ActivationStrategy {
            plexspaces_common::ActivationStrategy::ActivationStrategyLazy
        }
        async fn get_lifecycle_state(&self) -> crate::VirtualActorLifecycleState {
            crate::VirtualActorLifecycleState {
                last_activated: None,
                last_accessed: Some(std::time::SystemTime::now()),
                activation_count: 0,
                is_activating: false,
                idle_timeout: std::time::Duration::from_secs(300),
            }
        }
        async fn should_activate(&self) -> bool {
            false
        }
        async fn start_activation(&self) -> bool {
            true
        }
        async fn mark_activated(&self) {}
        async fn mark_deactivated(&self) {}
        async fn should_deactivate(&self) -> bool {
            false
        }
        async fn update_access_time(&self) {}
    }

    /// Helper to create a test VirtualActorFacet (uses inline mock to avoid crate version conflicts)
    fn create_test_virtual_actor_facet(
    ) -> Arc<tokio::sync::RwLock<Box<dyn VirtualActorLifecycleFacet>>> {
        Arc::new(tokio::sync::RwLock::new(
            Box::new(MockLifecycleFacet) as Box<dyn VirtualActorLifecycleFacet>
        ))
    }

    fn test_actor_id(name: &str) -> ActorId {
        ActorId::new(name, "GenServer", "namespace", "node-1").unwrap()
    }

    #[tokio::test]
    async fn test_register_virtual_actor_with_required_actor_type() {
        let manager = create_test_manager().await;

        let actor_id = test_actor_id("test-actor");
        let actor_type = "GenServer".to_string();
        let facet = create_test_virtual_actor_facet();

        // Register with required actor_type
        let result = manager
            .register(
                actor_id.clone(),
                facet,
                actor_type.clone(),
                None,
                "tenant".to_string(),
                "namespace".to_string(),
                vec![],
                HashMap::new(),
                ActivationStrategy::ActivationStrategyLazy,
            )
            .await;

        assert!(result.is_ok());

        // Verify actor is virtual
        assert!(manager.is_virtual(&actor_id).await);
    }

    #[tokio::test]
    async fn test_register_virtual_actor_with_config() {
        let manager = create_test_manager().await;

        let actor_id = test_actor_id("configured-actor");
        let actor_type = "GenServer".to_string();
        let facet = create_test_virtual_actor_facet();
        let config = Some(ActorConfig {
            max_mailbox_size: 500,
            ..Default::default()
        });

        let result = manager
            .register(
                actor_id.clone(),
                facet,
                actor_type.clone(),
                config.clone(),
                "tenant".to_string(),
                "namespace".to_string(),
                vec![],
                HashMap::new(),
                ActivationStrategy::ActivationStrategyLazy,
            )
            .await;

        assert!(result.is_ok());

        // Verify metadata
        let virtual_actors = manager.registry().virtual_actors().read().await;
        let metadata = virtual_actors.get(&actor_id);
        assert!(metadata.is_some());
        let metadata = metadata.unwrap();
        assert_eq!(metadata.config, config);
        assert_eq!(metadata.tenant_id, "tenant");
        assert_eq!(metadata.namespace, "namespace");
        assert_eq!(metadata.actor_type, actor_type);
    }

    #[tokio::test]
    async fn test_register_virtual_actor_preserves_reactivation_metadata() {
        let manager = create_test_manager().await;

        let actor_id = test_actor_id("configured-actor");
        let actor_type = "GenServer".to_string();
        let facet = create_test_virtual_actor_facet();
        let mut labels = HashMap::new();
        labels.insert("role".to_string(), "primary".to_string());
        let initial_state = vec![1, 2, 3, 4];

        manager
            .register(
                actor_id.clone(),
                facet,
                actor_type.clone(),
                None,
                "tenant".to_string(),
                "namespace".to_string(),
                initial_state.clone(),
                labels.clone(),
                ActivationStrategy::ActivationStrategyEager,
            )
            .await
            .unwrap();

        let metadata = manager.get_metadata(&actor_id).await.unwrap();
        assert_eq!(metadata.actor_type, actor_type);
        assert_eq!(metadata.initial_state, initial_state);
        assert_eq!(metadata.labels, labels);
        assert_eq!(
            metadata.activation_strategy,
            ActivationStrategy::ActivationStrategyEager
        );
    }

    #[tokio::test]
    async fn test_register_virtual_actor_empty_actor_type() {
        let manager = create_test_manager().await;

        let actor_id = test_actor_id("test-actor");
        let facet = create_test_virtual_actor_facet();

        // Try to register with empty actor_type - should fail
        let result = manager
            .register(
                actor_id.clone(),
                facet,
                String::new(), // Empty actor_type
                None,
                "tenant".to_string(),
                "namespace".to_string(),
                vec![],
                HashMap::new(),
                ActivationStrategy::ActivationStrategyLazy,
            )
            .await;

        assert!(result.is_err());
        match result.unwrap_err() {
            VirtualActorError::ActivationFailed(_) => {}
            _ => panic!("Expected ActivationFailed error"),
        }
    }

    #[tokio::test]
    async fn test_is_virtual_with_actor_type() {
        let manager = create_test_manager().await;

        // Register virtual actor type
        manager
            .register_virtual_actor_type(
                "read-state-tracker".to_string(),
                None,
                "namespace".to_string(),
                serde_json::json!({
                    "virtual_actor": {
                        "idle_timeout": "5m",
                        "activation_strategy": "lazy"
                    }
                }),
                None,
                None,
            )
            .await
            .unwrap();

        // Check if actor ID matching the type is virtual
        // Format: {id}//{actor_type}::{namespace}@{node_id}
        let actor_id =
            ActorId::new("user-123", "read-state-tracker", "namespace", "node-1").unwrap();

        assert!(manager.is_virtual(&actor_id).await);
    }

    #[tokio::test]
    async fn test_is_virtual_with_individual_registration() {
        let manager = create_test_manager().await;

        let actor_id = test_actor_id("individual-actor");
        let actor_type = "GenServer".to_string();
        let facet = create_test_virtual_actor_facet();

        manager
            .register(
                actor_id.clone(),
                facet,
                actor_type,
                None,
                "tenant".to_string(),
                "namespace".to_string(),
                vec![],
                HashMap::new(),
                ActivationStrategy::ActivationStrategyLazy,
            )
            .await
            .unwrap();

        assert!(manager.is_virtual(&actor_id).await);
    }

    #[tokio::test]
    async fn test_is_virtual_not_registered() {
        let manager = create_test_manager().await;

        let actor_id = test_actor_id("not-virtual");

        assert!(!manager.is_virtual(&actor_id).await);
    }

    #[tokio::test]
    async fn test_get_virtual_actor_type_not_found() {
        let manager = create_test_manager().await;

        let metadata = manager.get_virtual_actor_type("nonexistent").await;
        assert!(metadata.is_none());
    }

    #[tokio::test]
    async fn test_is_virtual_actor_type() {
        let manager = create_test_manager().await;

        assert!(!manager.is_virtual_actor_type("nonexistent").await);

        manager
            .register_virtual_actor_type(
                "test-type".to_string(),
                None,
                "namespace".to_string(),
                serde_json::json!({
                    "virtual_actor": {
                        "idle_timeout": "5m",
                        "activation_strategy": "lazy"
                    }
                }),
                None,
                None,
            )
            .await
            .unwrap();

        assert!(manager.is_virtual_actor_type("test-type").await);
        assert!(!manager.is_virtual_actor_type("other-type").await);
    }

    #[tokio::test]
    async fn test_update_metadata() {
        let manager = create_test_manager().await;

        let actor_id = test_actor_id("test-actor");
        let actor_type = "GenServer".to_string();
        let facet = create_test_virtual_actor_facet();

        manager
            .register(
                actor_id.clone(),
                facet,
                actor_type.clone(),
                None,
                "tenant".to_string(),
                "namespace".to_string(),
                vec![],
                std::collections::HashMap::new(),
                plexspaces_common::ActivationStrategy::ActivationStrategyLazy,
            )
            .await
            .unwrap();

        // Update metadata
        let new_type = "UpdatedType".to_string();
        let new_config = Some(ActorConfig {
            max_mailbox_size: 2000,
            ..Default::default()
        });

        let result = manager
            .update_metadata(&actor_id, new_type.clone(), new_config.clone())
            .await;

        assert!(result.is_ok());

        // Verify update
        let virtual_actors = manager.registry().virtual_actors().read().await;
        let metadata = virtual_actors.get(&actor_id).unwrap();
        assert_eq!(metadata.actor_type, new_type);
        assert_eq!(metadata.config, new_config);
    }

    #[tokio::test]
    async fn test_instance_registration_inherits_type_level_rebuild_metadata() {
        let manager = create_test_manager().await;

        let actor_type = "durable-counter".to_string();
        let facet_config = serde_json::json!({
            "virtual_actor": {
                "idle_timeout": "5m",
                "activation_strategy": "lazy"
            },
            "durability": {
                "checkpoint_interval": 5
            },
            "timer": {
                "interval_ms": 1000
            }
        });

        manager
            .register_virtual_actor_definition(VirtualActorDefinitionRegistration {
                actor_type: actor_type.clone(),
                behavior_kind: Some("GenServer".to_string()),
                namespace: "test-ns".to_string(),
                actor_config: None,
                proto_facets: vec![
                    ProtoFacet {
                        r#type: "virtual_actor".to_string(),
                        config: HashMap::from([
                            ("idle_timeout".to_string(), "5m".to_string()),
                            ("activation_strategy".to_string(), "lazy".to_string()),
                        ]),
                        priority: 100,
                        state: HashMap::new(),
                        metadata: None,
                    },
                    ProtoFacet {
                        r#type: "durability".to_string(),
                        config: HashMap::from([(
                            "checkpoint_interval".to_string(),
                            "5".to_string(),
                        )]),
                        priority: 90,
                        state: HashMap::new(),
                        metadata: None,
                    },
                ],
                facet_config: facet_config.clone(),
                tenant_id: Some("tenant-a".to_string()),
                init_config_template: Some(
                    br#"{"actor_id":"","args":{"role":"abstractions"}}"#.to_vec(),
                ),
            })
            .await
            .unwrap();

        manager
            .register(
                ActorId::new("cart-1", "durable-counter", "test-ns", "test-node").unwrap(),
                create_test_virtual_actor_facet(),
                actor_type.clone(),
                None,
                "tenant-a".to_string(),
                "test-ns".to_string(),
                vec![1, 2, 3],
                HashMap::new(),
                plexspaces_common::ActivationStrategy::ActivationStrategyLazy,
            )
            .await
            .unwrap();

        let metadata = manager
            .get_metadata(
                &ActorId::new("cart-1", "durable-counter", "test-ns", "test-node").unwrap(),
            )
            .await
            .expect("instance metadata should exist");
        assert_eq!(metadata.facet_config, Some(facet_config));
        assert_eq!(metadata.behavior_kind.as_deref(), Some("GenServer"));
        assert_eq!(metadata.proto_facets.len(), 2);
        assert_eq!(
            metadata.init_config_template,
            Some(br#"{"actor_id":"","args":{"role":"abstractions"}}"#.to_vec())
        );
    }

    #[tokio::test]
    async fn test_update_metadata_not_found() {
        let manager = create_test_manager().await;

        let result = manager
            .update_metadata(&test_actor_id("nonexistent"), "NewType".to_string(), None)
            .await;

        assert!(result.is_err());
        match result.unwrap_err() {
            VirtualActorError::ActorNotFound(_) => {}
            _ => panic!("Expected ActorNotFound error"),
        }
    }

    #[tokio::test]
    async fn test_take_pending_messages() {
        let manager = create_test_manager().await;

        let actor_id = test_actor_id("test-actor");

        // Initially no pending messages
        let messages = manager.take_pending_messages(&actor_id).await;
        assert!(messages.is_empty());

        // Add pending messages
        let mut pending = manager.registry().pending_activations().write().await;
        pending.insert(
            actor_id.clone(),
            vec![
                Message {
                    id: "msg1".to_string(),
                    ..Default::default()
                },
                Message {
                    id: "msg2".to_string(),
                    ..Default::default()
                },
            ],
        );
        drop(pending);

        // Take pending messages
        let messages = manager.take_pending_messages(&actor_id).await;
        assert_eq!(messages.len(), 2);

        // Verify messages are removed
        let messages = manager.take_pending_messages(&actor_id).await;
        assert!(messages.is_empty());
    }

    #[tokio::test]
    async fn test_queue_message() {
        let manager = create_test_manager().await;

        let actor_id = test_actor_id("test-actor");
        let message = Message {
            id: "msg1".to_string(),
            ..Default::default()
        };

        // Queue message
        manager.queue_message(&actor_id, message.clone()).await;

        // Verify message is queued
        let messages = manager.take_pending_messages(&actor_id).await;
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].id, message.id);
    }

    #[tokio::test]
    async fn test_get_facet() {
        let manager = create_test_manager().await;

        let actor_id = test_actor_id("test-actor");
        let actor_type = "GenServer".to_string();
        let facet = create_test_virtual_actor_facet();

        manager
            .register(
                actor_id.clone(),
                facet.clone(),
                actor_type,
                None,
                "tenant".to_string(),
                "namespace".to_string(),
                vec![],
                HashMap::new(),
                ActivationStrategy::ActivationStrategyLazy,
            )
            .await
            .unwrap();

        // Get facet
        let retrieved_facet = manager.get_facet(&actor_id).await;
        assert!(retrieved_facet.is_ok());
    }

    #[tokio::test]
    async fn test_get_facet_not_found() {
        let manager = create_test_manager().await;

        let result = manager.get_facet(&test_actor_id("nonexistent")).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            VirtualActorError::ActorNotFound(_) => {}
            _ => panic!("Expected ActorNotFound error"),
        }
    }

    #[tokio::test]
    async fn test_get_facet_type_level_registration() {
        let manager = create_test_manager().await;

        // Register virtual actor type (no facet instance)
        manager
            .register_virtual_actor_type(
                "test-type".to_string(),
                None,
                "namespace".to_string(),
                serde_json::json!({
                    "virtual_actor": {
                        "idle_timeout": "5m",
                        "activation_strategy": "lazy"
                    }
                }),
                None,
                None,
            )
            .await
            .unwrap();

        // Try to get facet for type-level registration - should fail
        // (type-level registrations don't have facet instances, only config)
        let actor_id = ActorId::new("instance-1", "test-type", "namespace", "node-1").unwrap();

        // This actor_id is virtual (type-level) but has no facet instance
        assert!(manager.is_virtual(&actor_id).await);

        // get_facet() should fail for type-level registrations — no facet instance exists yet,
        // only type-level config. The registry returns ActorNotFound (not ActivationFailed).
        let result = manager.get_facet(&actor_id).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            VirtualActorError::ActorNotFound(_) => {}
            _ => panic!("Expected ActorNotFound error for type-level registration (no instance spawned yet)"),
        }
    }

    #[tokio::test]
    async fn test_mark_activated() {
        let manager = create_test_manager().await;

        let actor_id = test_actor_id("test-actor");
        let actor_type = "GenServer".to_string();
        let facet = create_test_virtual_actor_facet();

        manager
            .register(
                actor_id.clone(),
                facet,
                actor_type,
                None,
                "tenant".to_string(),
                "namespace".to_string(),
                vec![],
                HashMap::new(),
                ActivationStrategy::ActivationStrategyLazy,
            )
            .await
            .unwrap();

        // Mark as activated
        let result = manager.mark_activated(&actor_id).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_register_virtual_actor_type_preserves_eager_strategy() {
        let manager = create_test_manager().await;

        manager
            .register_virtual_actor_type(
                "eager-type".to_string(),
                None,
                "namespace".to_string(),
                serde_json::json!({
                    "virtual_actor": {
                        "idle_timeout": "5m",
                        "activation_strategy": "eager"
                    }
                }),
                None,
                None,
            )
            .await
            .unwrap();

        let metadata = manager.get_virtual_actor_type("eager-type").await.unwrap();
        assert_eq!(
            metadata.activation_strategy,
            ActivationStrategy::ActivationStrategyEager
        );
    }

    #[tokio::test]
    async fn test_mark_activated_not_virtual() {
        let manager = create_test_manager().await;

        let actor_id = ActorId::new("not-virtual", "GenServer", "namespace", "node-1").unwrap();

        let result = manager.mark_activated(&actor_id).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            VirtualActorError::ActorNotFound(_) => {}
            _ => panic!("Expected ActorNotFound error"),
        }
    }

    #[tokio::test]
    async fn test_is_active() {
        let manager = create_test_manager().await;

        let actor_id = ActorId::new("test-actor", "GenServer", "namespace", "node-1").unwrap();

        // Not registered, should return false
        assert!(!manager.is_active(&actor_id).await);

        // Register as virtual
        let actor_type = "GenServer".to_string();
        let facet = create_test_virtual_actor_facet();

        manager
            .register(
                actor_id.clone(),
                facet,
                actor_type,
                None,
                "tenant".to_string(),
                "namespace".to_string(),
                vec![],
                HashMap::new(),
                ActivationStrategy::ActivationStrategyLazy,
            )
            .await
            .unwrap();

        // Registered but not active (ActorRegistry doesn't have it)
        // is_active() checks ActorRegistry.is_actor_state_active() which will return false
        assert!(!manager.is_active(&actor_id).await);
    }
}
