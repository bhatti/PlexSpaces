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
//! - `ActorSpawnSpec` is the canonical data shape for all metadata
//! - Uses actor_id factory methods for consistent ID parsing/construction
//! - All metadata follows proto definitions

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::virtual_actor_lifecycle_facet::VirtualActorLifecycleFacet;
use crate::Service;
use crate::{ActorId, ActorRegistry};
use plexspaces_common::{
    from_config_str, ActivationStrategy, RequestContext, RequestContextExt, ServiceNameExt,
};
use plexspaces_proto::actor::v1::ActorSpawnSpec;
use plexspaces_proto::common::v1::ActorIdentity;
use plexspaces_proto::common::v1::Message;

/// Compute the WASM init payload bytes for a virtual actor activation.
///
/// ## Purpose
/// Replaces `materialize_init_config_template` for the new `ActorSpawnSpec`-first world.
/// The payload is always fresh (actor_id injected at call time) so the spec can be stored
/// without a placeholder actor_id.
pub fn wasm_init_payload(spec: &ActorSpawnSpec, actor_id: &ActorId) -> Vec<u8> {
    let actor_type = spec
        .identity
        .as_ref()
        .map(|id| id.actor_type.as_str())
        .unwrap_or("");
    // For named virtual actors, spec.identity.name is the instance name (e.g. "session-1"),
    // but BehaviorRegistry dispatch requires the role (e.g. "ephemeral").
    // spec.role carries the role set at registration time.
    // Fall back to identity.name for non-virtual / type-level actors.
    let name_fallback = spec
        .identity
        .as_ref()
        .map(|id| id.name.as_str())
        .unwrap_or("");
    let name = if !spec.role.is_empty() {
        spec.role.as_str()
    } else {
        name_fallback
    };

    // Build full args map for the WASM init payload.
    let user_args: std::collections::HashMap<&str, &str> = spec
        .args
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();

    // Build payload with meta-fields plus a nested "args" map (WASM SDK canonical form).
    // Also flat-merge args at the top level so non-WASM behavior factories that read
    // top-level fields (e.g. `initial_count`) continue to work without modification.
    let mut payload = serde_json::json!({
        "actor_id": actor_id.to_string(),
        "actor_type": actor_type,
        "role": name,
        "behavior_kind": spec.behavior_kind,
        "args": user_args,
    });

    if let (serde_json::Value::Object(ref mut obj), false) = (&mut payload, spec.args.is_empty()) {
        for (k, v) in &spec.args {
            // Only insert if not already a meta key or internal framework key.
            if !matches!(
                k.as_str(),
                "actor_id" | "actor_type" | "role" | "behavior_kind" | "args"
            ) && !k.starts_with("__")
            {
                // Attempt numeric promotion so factories using as_i64()/as_f64() work.
                let val = if let Ok(n) = v.parse::<i64>() {
                    serde_json::Value::Number(n.into())
                } else if let Ok(f) = v.parse::<f64>() {
                    serde_json::json!(f)
                } else if v == "true" {
                    serde_json::Value::Bool(true)
                } else if v == "false" {
                    serde_json::Value::Bool(false)
                } else {
                    serde_json::Value::String(v.clone())
                };
                obj.insert(k.clone(), val);
            }
        }
    }

    serde_json::to_vec(&payload).unwrap_or_default()
}

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
/// - **Proto-First**: ActorSpawnSpec is the canonical data shape
#[derive(Clone)]
pub struct VirtualActorMetadata {
    /// Canonical spawn specification – single source of truth for all actor attributes.
    pub spec: ActorSpawnSpec,
    /// Virtual actor facet (for lifecycle management)
    /// For type-level registration, this may be None (facet created from facet_config)
    pub facet: Option<Arc<RwLock<Box<dyn VirtualActorLifecycleFacet>>>>,
    /// Last deactivation time (None if currently active)
    pub last_deactivated: Option<std::time::SystemTime>,
}

impl VirtualActorMetadata {
    /// Actor type (e.g., "GenServer", "read-state-tracker") – from spec identity.
    pub fn actor_type(&self) -> &str {
        self.spec
            .identity
            .as_ref()
            .map(|id| id.actor_type.as_str())
            .unwrap_or("")
    }

    /// Behavior kind, if set (e.g. "GenServer", "GenEvent").
    pub fn behavior_kind(&self) -> Option<&str> {
        if self.spec.behavior_kind.is_empty() {
            None
        } else {
            Some(&self.spec.behavior_kind)
        }
    }

    /// Canonical keyed facet config derived from proto facets.
    ///
    /// Returns `None` when the spec has no facets or all facets produce an empty config.
    pub fn facet_config(&self) -> Option<serde_json::Value> {
        use plexspaces_facet::facet_helpers::extract_facet_config_for_registration;
        if self.spec.facets.is_empty() {
            return None;
        }
        let cfg = extract_facet_config_for_registration(None, Some(&self.spec.facets));
        if cfg.as_object().map(|o| o.is_empty()).unwrap_or(true) {
            None
        } else {
            Some(cfg)
        }
    }

    /// Activation strategy derived from the virtual_actor facet config.
    pub fn activation_strategy(&self) -> ActivationStrategy {
        let facet_config = self.facet_config().unwrap_or(serde_json::Value::Null);
        VirtualActorManager::activation_strategy_from_facet_config(&facet_config)
    }

    // --- backward-compat accessors (inline delegation to spec) ---

    /// Namespace for tenant isolation.
    pub fn namespace(&self) -> &str {
        &self.spec.namespace
    }

    /// Tenant ID.
    pub fn tenant_id(&self) -> &str {
        &self.spec.tenant_id
    }

    /// Actor config (resource requirements, etc.).
    pub fn config(&self) -> Option<&plexspaces_proto::v1::actor::ActorConfig> {
        self.spec.config.as_ref()
    }

    /// Proto-first facets.
    pub fn proto_facets(&self) -> &[plexspaces_proto::common::v1::Facet] {
        &self.spec.facets
    }

    /// Labels.
    pub fn labels(&self) -> &HashMap<String, String> {
        &self.spec.labels
    }
}

/// Active instance tracking for LRU eviction
/// Tracks active instances per actor_type with last_access time
#[derive(Clone)]
pub struct ActiveInstance {
    pub actor_id: ActorId,
    pub last_access: std::time::SystemTime,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct VirtualActorDefinitionKey {
    namespace: String,
    name: String,
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
    /// Pending messages for actors being activated: actor_id -> (message, caller context).
    ///
    /// Preserves the `RequestContext` from each `tell` so post-activation delivery runs the same
    /// visibility and tracing semantics as an immediate send (see `ActorRef::tell`).
    pending_activations: Arc<RwLock<HashMap<ActorId, Vec<(Message, RequestContext)>>>>,
    /// Virtual actor types: actor_type -> VirtualActorMetadata
    /// Key is actor_type (behavior class, e.g. "inference_worker").
    /// Used for BehaviorRegistry lookup, canonical ID construction, and LRU eviction.
    virtual_actor_types: Arc<RwLock<HashMap<String, VirtualActorMetadata>>>,
    /// Declaration-scoped virtual actor definitions keyed by namespace + declaration name.
    /// This preserves distinct init templates/facets for multiple child specs that share
    /// one behavior class.
    named_virtual_actor_definitions:
        Arc<RwLock<HashMap<VirtualActorDefinitionKey, VirtualActorMetadata>>>,
    /// Reverse index: namespace + instance name -> actor_type (behavior class).
    /// Only populated when ChildSpec.id != ChildSpec.actor_type (e.g. "inference_worker_a" -> "inference_worker").
    /// Used by HTTP routing to resolve a client-supplied name to the behavior class.
    name_to_actor_type: Arc<RwLock<HashMap<VirtualActorDefinitionKey, String>>>,
    /// Active instances per actor_type for LRU eviction: actor_type -> Vec<ActiveInstance>
    active_instances_by_type: Arc<RwLock<HashMap<String, Vec<ActiveInstance>>>>,
}

impl VirtualActorRegistry {
    fn new() -> Self {
        Self {
            virtual_actors: Arc::new(RwLock::new(HashMap::new())),
            pending_activations: Arc::new(RwLock::new(HashMap::new())),
            virtual_actor_types: Arc::new(RwLock::new(HashMap::new())),
            named_virtual_actor_definitions: Arc::new(RwLock::new(HashMap::new())),
            name_to_actor_type: Arc::new(RwLock::new(HashMap::new())),
            active_instances_by_type: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn virtual_actors(&self) -> &Arc<RwLock<HashMap<ActorId, VirtualActorMetadata>>> {
        &self.virtual_actors
    }

    pub fn pending_activations(
        &self,
    ) -> &Arc<RwLock<HashMap<ActorId, Vec<(Message, RequestContext)>>>> {
        &self.pending_activations
    }

    pub fn virtual_actor_types(&self) -> &Arc<RwLock<HashMap<String, VirtualActorMetadata>>> {
        &self.virtual_actor_types
    }

    fn named_virtual_actor_definitions(
        &self,
    ) -> &Arc<RwLock<HashMap<VirtualActorDefinitionKey, VirtualActorMetadata>>> {
        &self.named_virtual_actor_definitions
    }

    pub fn name_to_actor_type(&self) -> &Arc<RwLock<HashMap<VirtualActorDefinitionKey, String>>> {
        &self.name_to_actor_type
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

impl VirtualActorError {
    /// Returns the proto error code for this error variant.
    pub fn code(&self) -> plexspaces_proto::actor::v1::VirtualActorErrorCode {
        use plexspaces_proto::actor::v1::VirtualActorErrorCode;
        match self {
            VirtualActorError::ActorNotFound(_) => VirtualActorErrorCode::VirtualActorErrorNotFound,
            VirtualActorError::ActivationFailed(_) => {
                VirtualActorErrorCode::VirtualActorErrorActivationFailed
            }
        }
    }
}

/// Extract `HashMap<String,String>` args from a JSON init-config template.
///
/// Supports two formats:
///   `{"args": {"k": "v"}}` — args nested under an "args" key (canonical form)
///   `{"k": "v"}` — flat top-level scalars (legacy WASM callers)
/// Meta fields (actor_id, actor_type, role, behavior_kind, args) are excluded.
fn extract_args_from_template(template: Option<&[u8]>) -> HashMap<String, String> {
    const META: &[&str] = &["actor_id", "actor_type", "role", "behavior_kind", "args"];
    template
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(b).ok())
        .and_then(|v| {
            let obj = v.as_object()?;
            if let Some(serde_json::Value::Object(map)) = obj.get("args") {
                return Some(
                    map.iter()
                        .map(|(k, v)| {
                            let s = match v {
                                serde_json::Value::String(s) => s.clone(),
                                _ => v.to_string(),
                            };
                            (k.clone(), s)
                        })
                        .collect(),
                );
            }
            let flat: HashMap<String, String> = obj
                .iter()
                .filter(|(k, _)| !META.contains(&k.as_str()))
                .filter_map(|(k, v)| {
                    let s = match v {
                        serde_json::Value::String(s) => s.clone(),
                        serde_json::Value::Number(n) => n.to_string(),
                        serde_json::Value::Bool(b) => b.to_string(),
                        _ => return None,
                    };
                    Some((k.clone(), s))
                })
                .collect();
            if flat.is_empty() {
                None
            } else {
                Some(flat)
            }
        })
        .unwrap_or_default()
}

/// Convert a keyed facet-config JSON value into proto `Facet` messages.
///
/// Expected format: `{"virtual_actor": {"activation_strategy": "eager", ...}, ...}`
fn facet_config_to_proto_facets(
    facet_config: &serde_json::Value,
) -> Vec<plexspaces_proto::common::v1::Facet> {
    facet_config
        .as_object()
        .map(|obj| {
            obj.iter()
                .map(|(facet_type, config_val)| {
                    let config_map = config_val
                        .as_object()
                        .map(|m| {
                            m.iter()
                                .filter_map(|(k, v)| {
                                    let s = match v {
                                        serde_json::Value::String(s) => s.clone(),
                                        other => other.to_string(),
                                    };
                                    Some((k.clone(), s))
                                })
                                .collect::<HashMap<String, String>>()
                        })
                        .unwrap_or_default();
                    plexspaces_proto::common::v1::Facet {
                        r#type: facet_type.clone(),
                        config: config_map,
                        priority: 0,
                        state: HashMap::new(),
                        metadata: None,
                    }
                })
                .collect()
        })
        .unwrap_or_default()
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
    fn definition_key(namespace: &str, name: &str) -> VirtualActorDefinitionKey {
        VirtualActorDefinitionKey {
            namespace: namespace.to_string(),
            name: name.to_string(),
        }
    }

    /// Derive the activation strategy from a canonical keyed facet config JSON value.
    pub fn activation_strategy_from_facet_config(
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
    /// ## Arguments
    /// * `actor_id` - Actor ID
    /// * `facet` - VirtualActorLifecycleFacet for lifecycle management
    /// * `spec` - ActorSpawnSpec containing all actor metadata
    pub async fn register(
        &self,
        actor_id: ActorId,
        facet: Arc<RwLock<Box<dyn VirtualActorLifecycleFacet>>>,
        spec: ActorSpawnSpec,
    ) -> Result<(), VirtualActorError> {
        let actor_type = spec
            .identity
            .as_ref()
            .map(|id| id.actor_type.as_str())
            .unwrap_or("");
        if actor_type.is_empty() {
            return Err(VirtualActorError::ActivationFailed(
                "actor_type is required (from proto Actor.actor_type)".to_string(),
            ));
        }

        let existing_instance_spec = {
            let virtual_actors = self.registry.virtual_actors().read().await;
            virtual_actors.get(&actor_id).map(|m| m.spec.clone())
        };
        let type_spec = {
            let virtual_types = self.registry.virtual_actor_types().read().await;
            virtual_types.get(actor_type).map(|m| m.spec.clone())
        };

        // Merge: prefer provided spec, fall back to existing instance then type-level for
        // behavior_kind and facets so they are never lost on reactivation.
        let merged_spec = {
            let behavior_kind = if spec.behavior_kind.is_empty() {
                existing_instance_spec
                    .as_ref()
                    .filter(|s| !s.behavior_kind.is_empty())
                    .map(|s| s.behavior_kind.clone())
                    .or_else(|| {
                        type_spec
                            .as_ref()
                            .filter(|s| !s.behavior_kind.is_empty())
                            .map(|s| s.behavior_kind.clone())
                    })
                    .unwrap_or_default()
            } else {
                spec.behavior_kind.clone()
            };

            let facets = if spec.facets.is_empty() {
                existing_instance_spec
                    .as_ref()
                    .filter(|s| !s.facets.is_empty())
                    .map(|s| s.facets.clone())
                    .or_else(|| {
                        type_spec
                            .as_ref()
                            .filter(|s| !s.facets.is_empty())
                            .map(|s| s.facets.clone())
                    })
                    .unwrap_or_default()
            } else {
                spec.facets.clone()
            };

            let args = if spec.args.is_empty() {
                existing_instance_spec
                    .as_ref()
                    .filter(|s| !s.args.is_empty())
                    .map(|s| s.args.clone())
                    .or_else(|| {
                        type_spec
                            .as_ref()
                            .filter(|s| !s.args.is_empty())
                            .map(|s| s.args.clone())
                    })
                    .unwrap_or_default()
            } else {
                spec.args.clone()
            };

            ActorSpawnSpec {
                behavior_kind,
                facets,
                args,
                ..spec
            }
        };

        let mut virtual_actors = self.registry.virtual_actors().write().await;
        virtual_actors.insert(
            actor_id,
            VirtualActorMetadata {
                spec: merged_spec,
                facet: Some(facet),
                last_deactivated: None,
            },
        );
        Ok(())
    }

    /// Seed per-instance metadata from a declaration-scoped virtual actor definition.
    ///
    /// This keeps first activation aligned with the declaration that the client addressed,
    /// even when multiple child specs share a behavior class.
    pub async fn prime_instance_from_definition(
        &self,
        actor_id: &ActorId,
        definition: &VirtualActorMetadata,
    ) {
        let mut virtual_actors = self.registry.virtual_actors().write().await;
        // Always refresh the spec from the definition so the next activation re-derives
        // wasm_init_payload from definition args (e.g. initial_count=5), not stale instance args.
        // The definition is the ground truth; running actors use in-WASM state, not spec.args.
        let entry =
            virtual_actors
                .entry(actor_id.clone())
                .or_insert_with(|| VirtualActorMetadata {
                    spec: definition.spec.clone(),
                    facet: None,
                    last_deactivated: None,
                });
        entry.spec = definition.spec.clone();
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
        drop(virtual_types);

        // Also check named definitions scoped to the actor's namespace: actor_type may be
        // registered via a named child spec (name != actor_type) which lives in
        // named_virtual_actor_definitions, not virtual_actor_types.
        // Filter by namespace to preserve tenant/namespace isolation.
        let actor_ns = actor_id.namespace();
        let name_idx = self.registry.name_to_actor_type().read().await;
        if name_idx
            .iter()
            .any(|(key, t)| key.namespace == actor_ns && t == actor_id.actor_type())
        {
            return true;
        }

        false
    }

    /// Register a virtual actor type using a `facet_config` JSON map.
    ///
    /// ## Purpose
    /// Compatibility shim for callers (tests, SDK) that build registration data from
    /// `serde_json::Value` facet configs rather than fully-formed `ActorSpawnSpec` protos.
    /// All parameters are forwarded into an `ActorSpawnSpec` and stored via
    /// `register_virtual_actor_definition`.
    ///
    /// TODO: Unify with `register_virtual_actor_definition` — both paths should accept only
    /// `ActorSpawnSpec`. Named actor (name != actor_type) and type-level (name == actor_type, or
    /// empty name auto-assigned ULID by factory) should use the same code path and data structure.
    /// `register_virtual_actor_type` is a legacy shim and should be removed once all callers
    /// migrate to passing `ActorSpawnSpec` directly.
    pub async fn register_virtual_actor_type(
        &self,
        actor_type: String,
        config: Option<plexspaces_proto::v1::actor::ActorConfig>,
        namespace: String,
        facet_config: serde_json::Value,
        tenant_id: Option<String>,
        init_config_template: Option<Vec<u8>>,
    ) -> Result<(), VirtualActorError> {
        // Extract args from init_config_template so callers that previously passed JSON bytes
        // still get their args surfaced in ActorSpawnSpec.args for wasm_init_payload().
        let args = extract_args_from_template(init_config_template.as_deref());

        // Convert facet_config JSON → proto Facets.
        let facets = facet_config_to_proto_facets(&facet_config);

        let spec = ActorSpawnSpec {
            identity: Some(ActorIdentity {
                name: String::new(), // type-level registration: name == type
                actor_type: actor_type.clone(),
            }),
            role: String::new(),
            namespace: namespace.clone(),
            tenant_id: tenant_id.unwrap_or_default(),
            behavior_kind: String::new(),
            args,
            facets,
            labels: HashMap::new(),
            config,
            visibility: 0,
            ..Default::default()
        };
        self.register_virtual_actor_definition(spec).await
    }

    /// Register a virtual actor definition.
    ///
    /// ## Purpose
    /// Accepts an `ActorSpawnSpec` as the single source of truth for all actor metadata.
    /// Callers include WASM applications (from `ChildSpec`), the Rust SDK, and tests.
    pub async fn register_virtual_actor_definition(
        &self,
        spec: ActorSpawnSpec,
    ) -> Result<(), VirtualActorError> {
        let actor_type = spec
            .identity
            .as_ref()
            .map(|id| id.actor_type.clone())
            .unwrap_or_default();
        let instance_name = spec
            .identity
            .as_ref()
            .map(|id| id.name.clone())
            .unwrap_or_default();
        let namespace = spec.namespace.clone();

        if actor_type.is_empty() {
            return Err(VirtualActorError::ActivationFailed(
                "actor_type is required (from proto Actor.actor_type)".to_string(),
            ));
        }

        let facet_types: Vec<String> = spec.facets.iter().map(|f| f.r#type.clone()).collect();

        let metadata = VirtualActorMetadata {
            spec,
            facet: None,
            last_deactivated: None,
        };
        if !instance_name.is_empty() && instance_name != actor_type {
            let key = Self::definition_key(&namespace, &instance_name);
            {
                let mut name_idx = self.registry.name_to_actor_type().write().await;
                name_idx.insert(key.clone(), actor_type.clone());
            }
            let mut named_defs = self
                .registry
                .named_virtual_actor_definitions()
                .write()
                .await;
            named_defs.insert(key, metadata);
        } else {
            let mut virtual_types = self.registry.virtual_actor_types().write().await;
            virtual_types.insert(actor_type.clone(), metadata);
        }
        if tracing::enabled!(tracing::Level::TRACE) {
            tracing::trace!(
                actor_type = %actor_type,
                facet_types = ?facet_types,
                "Registered virtual actor type"
            );
        }
        Ok(())
    }

    /// Get virtual actor type metadata by actor_type (behavior class).
    ///
    /// ## Purpose
    /// Retrieves metadata for a virtual actor type keyed by behavior class
    /// (e.g. "inference_worker"). Used for BehaviorRegistry lookup, facet recreation,
    /// and canonical ID construction.
    ///
    /// ## Returns
    /// `Some(VirtualActorMetadata)` if actor type is registered, `None` otherwise
    pub async fn get_virtual_actor_type(&self, actor_type: &str) -> Option<VirtualActorMetadata> {
        let virtual_types = self.registry.virtual_actor_types().read().await;
        virtual_types.get(actor_type).cloned()
    }

    /// Get a declaration-scoped virtual actor definition by namespace and child name.
    pub async fn get_virtual_actor_definition(
        &self,
        namespace: &str,
        name: &str,
    ) -> Option<VirtualActorMetadata> {
        let named_defs = self.registry.named_virtual_actor_definitions().read().await;
        named_defs
            .get(&Self::definition_key(namespace, name))
            .cloned()
    }

    /// Resolve instance name to actor_type (behavior class) using the name→type index.
    ///
    /// ## Purpose
    /// HTTP routing receives the instance name (e.g. "inference_worker_a") but needs
    /// the behavior class (e.g. "inference_worker") to construct the canonical ActorId
    /// and look up type metadata. Returns the actor_type if a reverse mapping exists,
    /// otherwise returns the name unchanged (name == type case).
    pub async fn resolve_actor_type_for_name(&self, namespace: &str, name: &str) -> String {
        let idx = self.registry.name_to_actor_type().read().await;
        idx.get(&Self::definition_key(namespace, name))
            .cloned()
            .unwrap_or_else(|| name.to_string())
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
                .filter(|(_, metadata)| metadata.spec.namespace == namespace)
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
                .filter(|(_, metadata)| metadata.spec.namespace == namespace)
                .map(|(actor_type, _)| actor_type.clone())
                .collect();
            for actor_type in &actor_types {
                virtual_types.remove(actor_type);
            }
            cleanup.actor_types = actor_types;
        }

        {
            let mut named_defs = self
                .registry
                .named_virtual_actor_definitions()
                .write()
                .await;
            let keys: Vec<VirtualActorDefinitionKey> = named_defs
                .keys()
                .filter(|key| key.namespace == namespace)
                .cloned()
                .collect();
            for key in &keys {
                named_defs.remove(key);
            }
        }

        {
            let mut name_idx = self.registry.name_to_actor_type().write().await;
            let keys: Vec<VirtualActorDefinitionKey> = name_idx
                .keys()
                .filter(|key| key.namespace == namespace)
                .cloned()
                .collect();
            for key in &keys {
                name_idx.remove(key);
            }
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

    /// Queue a message for processing after activation, retaining the caller `RequestContext`.
    pub async fn queue_message(&self, actor_id: &ActorId, message: Message, ctx: &RequestContext) {
        let mut pending = self.registry.pending_activations().write().await;
        pending
            .entry(actor_id.clone())
            .or_insert_with(Vec::new)
            .push((message, ctx.clone()));
    }

    /// Get and clear pending (message, caller context) pairs for an actor.
    pub async fn take_pending_messages(
        &self,
        actor_id: &ActorId,
    ) -> Vec<(Message, RequestContext)> {
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
                metadata.actor_type().to_string()
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
                            m.activation_strategy(),
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
                metadata.actor_type().to_string()
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
                metadata.actor_type().to_string()
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
        crate::ServiceName::ServiceNameVirtualActorManager
            .as_str()
            .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actor_context::ObjectRegistry;
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

    /// Build a minimal ActorSpawnSpec for testing.
    fn test_spec(actor_type: &str, namespace: &str, tenant_id: &str) -> ActorSpawnSpec {
        ActorSpawnSpec {
            identity: Some(ActorIdentity {
                name: actor_type.to_string(),
                actor_type: actor_type.to_string(),
            }),
            namespace: namespace.to_string(),
            tenant_id: tenant_id.to_string(),
            ..Default::default()
        }
    }

    // ── wasm_init_payload unit tests ─────────────────────────────────────────

    #[test]
    fn wasm_init_payload_emits_role_key() {
        let spec = ActorSpawnSpec {
            identity: Some(ActorIdentity {
                name: "ephemeral".to_string(),
                actor_type: "abstractions_wasm".to_string(),
            }),
            role: "ephemeral".to_string(),
            namespace: "ns".to_string(),
            tenant_id: "t1".to_string(),
            behavior_kind: "GenServer".to_string(),
            args: HashMap::from([("workers".to_string(), "4".to_string())]),
            ..Default::default()
        };
        let actor_id = ActorId::new("session-1", "abstractions_wasm", "ns", "node-1").unwrap();
        let payload = wasm_init_payload(&spec, &actor_id);
        let json: serde_json::Value = serde_json::from_slice(&payload).unwrap();

        assert_eq!(json["role"], "ephemeral", "payload must carry 'role' key");
        assert!(
            json.get("declaration_name").is_none(),
            "legacy key must not appear"
        );
        assert_eq!(json["actor_type"], "abstractions_wasm");
        assert_eq!(json["behavior_kind"], "GenServer");
        assert_eq!(json["args"]["workers"], "4");
        // Promoted scalar must also appear at top level for non-WASM factories.
        assert_eq!(json["workers"], 4_i64);
    }

    #[test]
    fn wasm_init_payload_falls_back_to_identity_name_when_role_empty() {
        let spec = ActorSpawnSpec {
            identity: Some(ActorIdentity {
                name: "cart-99".to_string(),
                actor_type: "cart_wasm".to_string(),
            }),
            role: String::new(), // no role set — identity.name is the fallback
            namespace: "shop".to_string(),
            ..Default::default()
        };
        let actor_id = ActorId::new("cart-99", "cart_wasm", "shop", "node-1").unwrap();
        let payload = wasm_init_payload(&spec, &actor_id);
        let json: serde_json::Value = serde_json::from_slice(&payload).unwrap();

        assert_eq!(json["role"], "cart-99");
        assert!(json.get("declaration_name").is_none());
    }

    #[test]
    fn wasm_init_payload_meta_keys_excluded_from_top_level_promotion() {
        // args containing meta-named keys must not overwrite the framework meta fields.
        let spec = ActorSpawnSpec {
            identity: Some(ActorIdentity {
                name: "worker".to_string(),
                actor_type: "worker_wasm".to_string(),
            }),
            role: "worker".to_string(),
            namespace: "ns".to_string(),
            args: HashMap::from([
                ("role".to_string(), "should-not-overwrite".to_string()),
                ("actor_id".to_string(), "should-not-overwrite".to_string()),
                ("custom_key".to_string(), "custom_val".to_string()),
            ]),
            ..Default::default()
        };
        let actor_id = ActorId::new("worker", "worker_wasm", "ns", "node-1").unwrap();
        let payload = wasm_init_payload(&spec, &actor_id);
        let json: serde_json::Value = serde_json::from_slice(&payload).unwrap();

        // Framework field must not be shadowed by user arg.
        assert_eq!(json["role"], "worker");
        assert_eq!(json["actor_id"], actor_id.to_string());
        // Non-meta arg is promoted.
        assert_eq!(json["custom_key"], "custom_val");
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
        assert_eq!(metadata.actor_type(), actor_type);
        assert_eq!(metadata.spec.namespace, namespace);
        assert_eq!(metadata.spec.tenant_id, ""); // Default for type-level registration
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
        let config = Some(plexspaces_proto::v1::actor::ActorConfig {
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
        assert_eq!(metadata.spec.config, config);
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
        assert_eq!(metadata.spec.tenant_id, tenant_id.unwrap());
    }

    #[tokio::test]
    async fn test_register_virtual_actor_definition_preserves_behavior_and_proto_facets() {
        let manager = create_test_manager().await;

        let actor_type = "workflow-type".to_string();
        let namespace = "namespace".to_string();

        let result = manager
            .register_virtual_actor_definition(ActorSpawnSpec {
                identity: Some(ActorIdentity {
                    name: String::new(),
                    actor_type: actor_type.clone(),
                }),
                role: String::new(),
                namespace: namespace.clone(),
                tenant_id: "tenant-123".to_string(),
                behavior_kind: "Workflow".to_string(),
                args: HashMap::new(),
                facets: vec![
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
                ..Default::default()
            })
            .await;

        assert!(result.is_ok());

        let metadata = manager
            .get_virtual_actor_type(&actor_type)
            .await
            .expect("metadata should exist");
        assert_eq!(metadata.actor_type(), actor_type);
        assert_eq!(metadata.spec.namespace, namespace);
        assert_eq!(metadata.behavior_kind(), Some("Workflow"));
        assert_eq!(metadata.spec.tenant_id, "tenant-123");
        assert_eq!(metadata.spec.facets.len(), 2);
        assert_eq!(metadata.spec.facets[0].r#type, "virtual_actor");
        assert_eq!(metadata.spec.facets[1].r#type, "durability");
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
                ActorSpawnSpec {
                    identity: Some(ActorIdentity {
                        name: "cart-1".to_string(),
                        actor_type: "abstractions".to_string(),
                    }),
                    role: String::new(),
                    namespace: "demo".to_string(),
                    tenant_id: "tenant".to_string(),
                    behavior_kind: String::new(),
                    args: HashMap::new(),
                    facets: vec![],
                    ..Default::default()
                },
            )
            .await
            .expect("virtual actor instance should register");

        manager
            .register_virtual_actor_definition(ActorSpawnSpec {
                identity: Some(ActorIdentity {
                    name: String::new(),
                    actor_type: "abstractions".to_string(),
                }),
                role: String::new(),
                namespace: "demo".to_string(),
                tenant_id: "tenant".to_string(),
                behavior_kind: "GenServer".to_string(),
                args: HashMap::new(),
                facets: vec![ProtoFacet {
                    r#type: "virtual_actor".to_string(),
                    config: HashMap::from([
                        ("idle_timeout".to_string(), "5m".to_string()),
                        ("activation_strategy".to_string(), "lazy".to_string()),
                    ]),
                    priority: 0,
                    state: HashMap::new(),
                    metadata: None,
                }],
                ..Default::default()
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
        ActorId::new(name, "gen_server", "namespace", "node-1").unwrap()
    }

    #[tokio::test]
    async fn test_register_virtual_actor_with_required_actor_type() {
        let manager = create_test_manager().await;

        let actor_id = test_actor_id("test-actor");
        let facet = create_test_virtual_actor_facet();

        // Register with required actor_type
        let result = manager
            .register(
                actor_id.clone(),
                facet,
                test_spec("gen_server", "namespace", "tenant"),
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
        let facet = create_test_virtual_actor_facet();
        let config = Some(plexspaces_proto::v1::actor::ActorConfig {
            max_mailbox_size: 500,
            ..Default::default()
        });

        let spec = ActorSpawnSpec {
            identity: Some(ActorIdentity {
                name: "configured-actor".to_string(),
                actor_type: "gen_server".to_string(),
            }),
            role: String::new(),
            namespace: "namespace".to_string(),
            tenant_id: "tenant".to_string(),
            behavior_kind: String::new(),
            args: HashMap::new(),
            facets: vec![],
            labels: HashMap::new(),
            config: config.clone(),
            ..Default::default()
        };

        let result = manager.register(actor_id.clone(), facet, spec).await;

        assert!(result.is_ok());

        // Verify metadata
        let virtual_actors = manager.registry().virtual_actors().read().await;
        let metadata = virtual_actors.get(&actor_id);
        assert!(metadata.is_some());
        let metadata = metadata.unwrap();
        assert_eq!(metadata.spec.config, config);
        assert_eq!(metadata.spec.tenant_id, "tenant");
        assert_eq!(metadata.spec.namespace, "namespace");
        assert_eq!(metadata.actor_type(), "gen_server");
    }

    #[tokio::test]
    async fn test_register_virtual_actor_preserves_reactivation_metadata() {
        let manager = create_test_manager().await;

        let actor_id = test_actor_id("configured-actor");
        let facet = create_test_virtual_actor_facet();
        let mut labels = HashMap::new();
        labels.insert("role".to_string(), "primary".to_string());

        let spec = ActorSpawnSpec {
            identity: Some(ActorIdentity {
                name: "configured-actor".to_string(),
                actor_type: "gen_server".to_string(),
            }),
            role: String::new(),
            namespace: "namespace".to_string(),
            tenant_id: "tenant".to_string(),
            behavior_kind: String::new(),
            args: HashMap::new(),
            facets: vec![],
            labels: labels.clone(),
            ..Default::default()
        };

        manager
            .register(actor_id.clone(), facet, spec)
            .await
            .unwrap();

        let metadata = manager.get_metadata(&actor_id).await.unwrap();
        assert_eq!(metadata.actor_type(), "gen_server");
        assert_eq!(metadata.spec.labels, labels);
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
                ActorSpawnSpec {
                    identity: Some(ActorIdentity {
                        name: "test-actor".to_string(),
                        actor_type: String::new(), // Empty actor_type
                    }),
                    role: String::new(),
                    namespace: "namespace".to_string(),
                    tenant_id: "tenant".to_string(),
                    behavior_kind: String::new(),
                    args: HashMap::new(),
                    facets: vec![],
                    ..Default::default()
                },
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
        let facet = create_test_virtual_actor_facet();

        manager
            .register(
                actor_id.clone(),
                facet,
                test_spec("gen_server", "namespace", "tenant"),
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
    async fn test_instance_registration_inherits_type_level_rebuild_metadata() {
        let manager = create_test_manager().await;

        let actor_type = "durable-counter".to_string();

        manager
            .register_virtual_actor_definition(ActorSpawnSpec {
                identity: Some(ActorIdentity {
                    name: String::new(),
                    actor_type: actor_type.clone(),
                }),
                role: String::new(),
                namespace: "test-ns".to_string(),
                tenant_id: "tenant-a".to_string(),
                behavior_kind: "GenServer".to_string(),
                args: HashMap::from([("role".to_string(), "abstractions".to_string())]),
                facets: vec![
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
                ..Default::default()
            })
            .await
            .unwrap();

        // Register instance with empty facets — should inherit from type-level
        manager
            .register(
                ActorId::new("cart-1", "durable-counter", "test-ns", "test-node").unwrap(),
                create_test_virtual_actor_facet(),
                ActorSpawnSpec {
                    identity: Some(ActorIdentity {
                        name: "cart-1".to_string(),
                        actor_type: "durable-counter".to_string(),
                    }),
                    role: String::new(),
                    namespace: "test-ns".to_string(),
                    tenant_id: "tenant-a".to_string(),
                    behavior_kind: String::new(), // empty — should inherit "GenServer"
                    args: HashMap::new(),
                    facets: vec![], // empty — should inherit from type
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        let metadata = manager
            .get_metadata(
                &ActorId::new("cart-1", "durable-counter", "test-ns", "test-node").unwrap(),
            )
            .await
            .expect("instance metadata should exist");
        assert_eq!(metadata.behavior_kind(), Some("GenServer"));
        assert_eq!(metadata.spec.facets.len(), 2);
    }

    #[tokio::test]
    async fn test_take_pending_messages() {
        let manager = create_test_manager().await;

        let actor_id = test_actor_id("test-actor");

        // Initially no pending messages
        let messages = manager.take_pending_messages(&actor_id).await;
        assert!(messages.is_empty());

        let ctx = crate::RequestContext::new_without_auth("t1".into(), "ns1".into());
        // Add pending messages
        let mut pending = manager.registry().pending_activations().write().await;
        pending.insert(
            actor_id.clone(),
            vec![
                (
                    Message {
                        id: "msg1".to_string(),
                        ..Default::default()
                    },
                    ctx.clone(),
                ),
                (
                    Message {
                        id: "msg2".to_string(),
                        ..Default::default()
                    },
                    ctx.clone(),
                ),
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
        let ctx = crate::RequestContext::new_without_auth("t1".into(), "ns1".into());

        // Queue message
        manager
            .queue_message(&actor_id, message.clone(), &ctx)
            .await;

        // Verify message is queued
        let messages = manager.take_pending_messages(&actor_id).await;
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].0.id, message.id);
    }

    #[tokio::test]
    async fn test_get_facet() {
        let manager = create_test_manager().await;

        let actor_id = test_actor_id("test-actor");
        let facet = create_test_virtual_actor_facet();

        manager
            .register(
                actor_id.clone(),
                facet.clone(),
                test_spec("gen_server", "namespace", "tenant"),
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
        let facet = create_test_virtual_actor_facet();

        manager
            .register(
                actor_id.clone(),
                facet,
                test_spec("gen_server", "namespace", "tenant"),
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
            metadata.activation_strategy(),
            ActivationStrategy::ActivationStrategyEager
        );
    }

    #[tokio::test]
    async fn test_mark_activated_not_virtual() {
        let manager = create_test_manager().await;

        let actor_id = ActorId::new("not-virtual", "gen_server", "namespace", "node-1").unwrap();

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

        let actor_id = ActorId::new("test-actor", "gen_server", "namespace", "node-1").unwrap();

        // Not registered, should return false
        assert!(!manager.is_active(&actor_id).await);

        // Register as virtual
        let facet = create_test_virtual_actor_facet();

        manager
            .register(
                actor_id.clone(),
                facet,
                test_spec("gen_server", "namespace", "tenant"),
            )
            .await
            .unwrap();

        // Registered but not active (ActorRegistry doesn't have it)
        // is_active() checks ActorRegistry.is_actor_state_active() which will return false
        assert!(!manager.is_active(&actor_id).await);
    }

    /// Verifies that `register()` merge logic correctly inherits `args` from an instance spec
    /// that was primed via `prime_instance_from_definition` (a named definition stored in
    /// `named_virtual_actor_definitions`, not in `virtual_actor_types`).
    ///
    /// This covers the Step-8 reactivation bug: when an actor's behavior class is registered
    /// only under a named definition (name ≠ actor_type), `get_virtual_actor_type(actor_type)`
    /// returns None and `args` must be inherited from the existing instance spec instead.
    #[tokio::test]
    async fn test_register_merge_inherits_args_from_named_definition_instance() {
        let manager = create_test_manager().await;

        let actor_type = "abstractions_wasm";
        let instance_name = "ephemeral"; // name != actor_type → goes to named_virtual_actor_definitions
        let namespace = "test-ns";

        // Register via register_virtual_actor_definition (name != actor_type path)
        let def_spec = ActorSpawnSpec {
            identity: Some(ActorIdentity {
                name: instance_name.to_string(),
                actor_type: actor_type.to_string(),
            }),
            role: String::new(),
            namespace: namespace.to_string(),
            tenant_id: "tenant-1".to_string(),
            behavior_kind: "GenServer".to_string(),
            args: HashMap::from([("initial_count".to_string(), "5".to_string())]),
            facets: vec![],
            ..Default::default()
        };
        manager
            .register_virtual_actor_definition(def_spec.clone())
            .await
            .unwrap();

        // Verify it went to named_virtual_actor_definitions (not virtual_actor_types)
        assert!(
            manager.get_virtual_actor_type(actor_type).await.is_none(),
            "should NOT be in virtual_actor_types when name != actor_type"
        );
        let def = manager
            .get_virtual_actor_definition(namespace, instance_name)
            .await;
        assert!(
            def.is_some(),
            "should be in named_virtual_actor_definitions"
        );
        assert_eq!(def.unwrap().spec.args.get("initial_count").unwrap(), "5");

        // Simulate prime_instance_from_definition (called by actor_service before first activation)
        let actor_id = ActorId::new(instance_name, actor_type, namespace, "node-1").unwrap();
        let definition_meta = manager
            .get_virtual_actor_definition(namespace, instance_name)
            .await
            .unwrap();
        manager
            .prime_instance_from_definition(&actor_id, &definition_meta)
            .await;

        // Instance is now in virtual_actors with the correct args from the definition
        let instance = manager.get_metadata(&actor_id).await.unwrap();
        assert_eq!(instance.spec.args.get("initial_count").unwrap(), "5");
        assert!(instance.facet.is_none(), "primed instance has no facet yet");

        // Simulate spawn_built_actor_impl registering the instance (first activation).
        // The merge logic must pick up args from instance_metadata when get_virtual_actor_type returns None.
        let facet = create_test_virtual_actor_facet();
        let update_spec = ActorSpawnSpec {
            identity: Some(ActorIdentity {
                name: instance_name.to_string(),
                actor_type: actor_type.to_string(),
            }),
            role: String::new(),
            namespace: namespace.to_string(),
            tenant_id: "tenant-1".to_string(),
            behavior_kind: String::new(), // intentionally empty — merge must fill from existing
            args: HashMap::new(),         // intentionally empty — merge must fill from existing
            facets: vec![],
            ..Default::default()
        };
        manager
            .register(actor_id.clone(), facet, update_spec)
            .await
            .unwrap();

        // After register() merge, args should be preserved
        let registered = manager.get_metadata(&actor_id).await.unwrap();
        assert_eq!(
            registered.spec.args.get("initial_count").unwrap(),
            "5",
            "args must survive register() merge — required for correct wasm_init_payload on reactivation"
        );
        assert_eq!(registered.spec.behavior_kind, "GenServer");
    }

    /// Virtual actors are never removed from registry on explicit stop.
    /// prime_instance_from_definition always refreshes spec from definition args.
    #[tokio::test]
    async fn test_virtual_actor_retained_after_stop() {
        let manager = create_test_manager().await;
        let actor_id = ActorId::new("session-1", "abstractions_wasm", "ns", "node-1").unwrap();
        let facet = create_test_virtual_actor_facet();

        let spec = ActorSpawnSpec {
            identity: Some(ActorIdentity {
                name: "session-1".to_string(),
                actor_type: "abstractions_wasm".to_string(),
            }),
            role: String::new(),
            namespace: "ns".to_string(),
            tenant_id: "t1".to_string(),
            behavior_kind: "GenServer".to_string(),
            args: HashMap::from([("initial_count".to_string(), "7".to_string())]),
            facets: vec![],
            ..Default::default()
        };
        manager
            .register(actor_id.clone(), facet, spec)
            .await
            .unwrap();
        // Simulate stop: no changes to VirtualActorManager — instance stays registered
        assert!(
            manager.get_metadata(&actor_id).await.is_some(),
            "instance must be retained after stop"
        );
        assert!(
            manager.is_virtual(&actor_id).await,
            "is_virtual must remain true after stop"
        );
    }

    /// After purging a non-durable instance, is_virtual must still return true when the
    /// actor_type is registered via a named definition (name != actor_type path).
    /// This ensures routing still works after stop so the next poll can trigger reactivation.
    #[tokio::test]
    async fn test_is_virtual_after_actor_stopped() {
        let manager = create_test_manager().await;

        // Register named definition: name="ephemeral", actor_type="abstractions_wasm"
        let spec = ActorSpawnSpec {
            identity: Some(ActorIdentity {
                name: "ephemeral".to_string(),
                actor_type: "abstractions_wasm".to_string(),
            }),
            role: String::new(),
            namespace: "abstractions-typescript".to_string(),
            tenant_id: "t1".to_string(),
            behavior_kind: "GenServer".to_string(),
            args: HashMap::from([("initial_count".to_string(), "5".to_string())]),
            facets: vec![],
            ..Default::default()
        };
        manager
            .register_virtual_actor_definition(spec)
            .await
            .unwrap();

        let actor_id = ActorId::new(
            "session-1",
            "abstractions_wasm",
            "abstractions-typescript",
            "node-1",
        )
        .unwrap();
        let instance_spec = ActorSpawnSpec {
            identity: Some(ActorIdentity {
                name: "session-1".to_string(),
                actor_type: "abstractions_wasm".to_string(),
            }),
            role: String::new(),
            namespace: "abstractions-typescript".to_string(),
            tenant_id: "t1".to_string(),
            behavior_kind: "GenServer".to_string(),
            args: HashMap::from([("initial_count".to_string(), "7".to_string())]),
            facets: vec![],
            ..Default::default()
        };
        let facet = create_test_virtual_actor_facet();
        manager
            .register(actor_id.clone(), facet, instance_spec)
            .await
            .unwrap();
        assert!(
            manager.is_virtual(&actor_id).await,
            "must be virtual before stop"
        );

        // Stop: no changes to VirtualActorManager — instance stays registered
        assert!(
            manager.get_metadata(&actor_id).await.is_some(),
            "instance retained after stop"
        );
        assert!(
            manager.is_virtual(&actor_id).await,
            "is_virtual must return true after stop"
        );
    }

    /// Namespace isolation: is_virtual must NOT return true for an actor whose actor_type is
    /// registered only under a different namespace.
    #[tokio::test]
    async fn test_is_virtual_named_definition_namespace_isolation() {
        let manager = create_test_manager().await;

        // Register named definition in namespace "ns-a"
        let spec = ActorSpawnSpec {
            identity: Some(ActorIdentity {
                name: "worker".to_string(),
                actor_type: "worker_wasm".to_string(),
            }),
            role: String::new(),
            namespace: "ns-a".to_string(),
            tenant_id: "t1".to_string(),
            behavior_kind: "GenServer".to_string(),
            args: HashMap::new(),
            facets: vec![],
            ..Default::default()
        };
        manager
            .register_virtual_actor_definition(spec)
            .await
            .unwrap();

        // Actor in ns-a should be virtual
        let actor_in_ns_a = ActorId::new("w1", "worker_wasm", "ns-a", "node-1").unwrap();
        assert!(
            manager.is_virtual(&actor_in_ns_a).await,
            "actor in ns-a must be virtual"
        );

        // Same actor_type in a different namespace must NOT be recognized as virtual
        let actor_in_ns_b = ActorId::new("w1", "worker_wasm", "ns-b", "node-1").unwrap();
        assert!(
            !manager.is_virtual(&actor_in_ns_b).await,
            "actor with same actor_type but different namespace must NOT be virtual (namespace isolation)"
        );
    }
}
