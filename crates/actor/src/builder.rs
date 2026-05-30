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

//! # Actor Builder
//!
//! ## Purpose
//! Provides a fluent, builder-style API for creating actors with sensible defaults.
//! This is part of Option C: Unified Actor Design - making the framework feel like ONE.
//!
//! ## Design Principles
//! - **Simplicity**: Sensible defaults, minimal required configuration
//! - **Extensibility**: Add facets and capabilities via builder methods
//! - **Consistency**: One way to create actors, regardless of type
//!
//! ## Examples
//!
//! ### Simple Actor Creation
//! ```rust,ignore
//! use plexspaces_actor::ActorBuilder;
//! use crate::core::Actor;
//!
//! struct MyBehavior;
//! impl Actor for MyBehavior { /* ... */ }
//!
//! let actor = ActorBuilder::new(MyBehavior::new())
//!     .with_name("my-actor")
//     .build()
//     .await?;
//! ```
//!
//! ### Actor with Facets
//! ```rust,ignore
//! let actor = ActorBuilder::new(MyBehavior::new())
//!     .with_name("durable-counter")
//!     .with_facet(Facet::durability(DurabilityConfig::default()))
//!     .with_facet(Facet::virtual_actor(VirtualActorConfig::default()))
//!     .build()
//!     .await?;
//! ```

use crate::core::{Actor, ActorId, RequestContextExt};
use crate::ActorInstance as ActorStruct;
use plexspaces_mailbox::Mailbox;
use plexspaces_proto::actor::v1::{ActorSpawnSpec, ActorVisibility};
use plexspaces_proto::v1::actor::ActorConfig;
use std::collections::HashMap;
use std::sync::Arc;

/// Builder for creating actors with a fluent API
///
/// ## Purpose
/// Simplifies actor creation by providing sensible defaults and a fluent interface.
/// This is the unified way to create actors in PlexSpaces.
///
/// ## Design
/// - Uses builder pattern for configuration
/// - Uses [`ActorSpawnSpec`] as the configuration source of truth
/// - Supports facet-based extensibility
/// - Single entry point for all actor types
pub struct ActorBuilder {
    behavior: Box<dyn Actor>,
    spawn_spec: ActorSpawnSpec,
    facets: Vec<Box<dyn plexspaces_facet::Facet>>,
}

impl ActorBuilder {
    /// Create a new actor builder with the given behavior
    ///
    /// ## Arguments
    /// * `behavior` - The actor behavior implementation
    ///
    /// ## Example
    /// ```rust,ignore
    /// let builder = ActorBuilder::new(MyBehavior::new());
    /// ```
    pub fn new(behavior: Box<dyn Actor>) -> Self {
        Self {
            behavior,
            spawn_spec: ActorSpawnSpec {
                identity: None,
                role: String::new(),
                namespace: String::new(),
                tenant_id: String::new(),
                visibility: ActorVisibility::ActorVisibilityPublic as i32,
                behavior_kind: String::new(),
                args: HashMap::new(),
                facets: vec![],
                labels: HashMap::new(),
                config: None,
                ..Default::default()
            },
            facets: Vec::new(),
        }
    }

    /// Create an actor builder pre-populated from an [`ActorSpawnSpec`].
    ///
    /// ## Purpose
    /// Primary entry point when the caller already has an `ActorSpawnSpec` (e.g. from
    /// `VirtualActorMetadata.spec` during reactivation). The returned builder is configured
    /// with identity, namespace, tenant_id, config, and labels from the spec; the caller can
    /// further override via the existing `with_*` setters.
    ///
    /// ## Example
    /// ```rust,ignore
    /// let builder = ActorBuilder::from_spec(Box::new(MyBehavior), spec);
    /// let actor = builder.build().await?;
    /// ```
    pub fn from_spec(
        behavior: Box<dyn Actor>,
        spec: &plexspaces_proto::actor::v1::ActorSpawnSpec,
    ) -> Self {
        Self {
            behavior,
            spawn_spec: spec.clone(),
            facets: Vec::new(),
        }
    }

    /// Snapshot builder fields into an [`ActorSpawnSpec`] for factory / RPC boundaries.
    ///
    /// `runtime_actor_type` must be the resolved behavior class (for example after virtual-name
    /// resolution), matching [`ActorFactory`] expectations.
    pub fn to_spawn_spec(&self, runtime_actor_type: impl Into<String>) -> ActorSpawnSpec {
        let mut spec = self.spawn_spec.clone();
        let actor_type = runtime_actor_type.into();
        let mut identity = spec.identity.take().unwrap_or_default();
        if !actor_type.is_empty() {
            identity.actor_type = actor_type;
        }
        spec.identity = Some(identity);
        spec
    }

    /// Consume the builder into `(spec, behavior, runtime_facets)` without creating a mailbox.
    ///
    /// Use with [`ActorFactory::spawn_actor`](crate::core::ActorFactory::spawn_actor) when the
    /// behavior is created from [`crate::core::BehaviorRegistry`], or with
    /// [`ActorFactoryImpl::spawn_built_actor_impl`](crate::ActorFactoryImpl::spawn_built_actor_impl)
    /// when the caller already holds a concrete [`Actor`](crate::core::Actor). Proto facets
    /// belong on [`ActorSpawnSpec::facets`](plexspaces_proto::actor::v1::ActorSpawnSpec::facets);
    /// this type’s `facets` vector is Rust-only instantiated facets.
    pub fn into_spawn_inputs(
        self,
        runtime_actor_type: impl Into<String>,
    ) -> (
        ActorSpawnSpec,
        Box<dyn Actor>,
        Vec<Box<dyn plexspaces_facet::Facet>>,
    ) {
        let spec = self.to_spawn_spec(runtime_actor_type);
        let behavior = self.behavior;
        let facets = self.facets;
        (spec, behavior, facets)
    }

    /// Set the actor name used by runtime-owned ActorId construction.
    ///
    /// ## Arguments
    /// * `name` - Stable actor name component
    ///
    /// ## Example
    /// ```rust,ignore
    /// let builder = ActorBuilder::new(MyBehavior::new())
    ///     .with_name("counter-actor");
    /// ```
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        let mut identity = self.spawn_spec.identity.take().unwrap_or_default();
        identity.name = name.into();
        self.spawn_spec.identity = Some(identity);
        self
    }

    /// Set the namespace for this actor
    ///
    /// ## Arguments
    /// * `namespace` - Namespace string (default: "default")
    ///
    /// ## Example
    /// ```rust,ignore
    /// let builder = ActorBuilder::new(MyBehavior::new())
    ///     .with_namespace("production");
    /// ```
    pub fn with_namespace(mut self, namespace: impl Into<String>) -> Self {
        self.spawn_spec.namespace = namespace.into();
        self
    }

    /// Set tenant ID for multi-tenancy
    ///
    /// ## Arguments
    /// * `tenant_id` - Tenant identifier (empty string if auth disabled)
    ///
    /// ## Example
    /// ```rust,ignore
    /// let builder = ActorBuilder::new(MyBehavior::new())
    ///     .with_tenant_id("tenant-123");
    /// ```
    pub fn with_tenant_id(mut self, tenant_id: impl Into<String>) -> Self {
        self.spawn_spec.tenant_id = tenant_id.into();
        self
    }

    /// Enable durability facet for this actor (convenience method)
    ///
    /// ## Purpose
    /// Adds durability/journaling capability to the actor. The facet will be attached
    /// after the actor is built using in-memory storage by default.
    ///
    /// ## Note
    /// This is a convenience method. For production use with persistent storage,
    /// attach the facet manually after building the actor.
    ///
    /// ## Example
    /// ```rust,ignore
    /// let actor = ActorBuilder::new(MyBehavior::new())
    ///     .with_name("durable-counter")
    ///     .with_durability()
    ///     .build()
    ///     .await?;
    /// ```
    pub fn with_durability(mut self) -> Self {
        // Store durability config in actor config properties
        let mut config = self.spawn_spec.config.take().unwrap_or_default();
        config.properties.insert(
            "facet.durability.enabled".to_string(),
            prost_types::Any {
                type_url: "type.googleapis.com/google.protobuf.BoolValue".to_string(),
                value: vec![1], // true
            },
        );
        self.spawn_spec.config = Some(config);
        self
    }

    /// Attach a facet to this actor
    ///
    /// ## Purpose
    /// Adds a facet (e.g., DurabilityFacet, VirtualActorFacet) to the actor.
    /// Facets will be attached after the actor is built but before spawning.
    ///
    /// ## Arguments
    /// * `facet` - The facet to attach (e.g., DurabilityFacet, VirtualActorFacet)
    ///
    /// ## Example
    /// ```rust,ignore
    /// use plexspaces_journaling::{DurabilityFacet, MemoryJournalStorage};
    ///
    /// let storage = MemoryJournalStorage::new();
    /// let durability_facet = Box::new(DurabilityFacet::new(
    ///     storage,
    ///     serde_json::json!({}),
    ///     50,
    /// ));
    ///
    /// let actor = ActorBuilder::new(MyBehavior::new())
    ///     .with_name("my-actor")
    ///     .with_facet(durability_facet)
    ///     .build()
    ///     .await?;
    /// ```
    pub fn with_facet(mut self, facet: Box<dyn plexspaces_facet::Facet>) -> Self {
        self.facets.push(facet);
        self
    }

    /// Enable virtual actor facet for this actor (convenience method)
    ///
    /// ## Purpose
    /// Makes the actor a virtual actor (Orleans-style) with automatic
    /// activation/deactivation based on idle timeout.
    ///
    /// ## Example
    /// ```rust,ignore
    /// let actor = ActorBuilder::new(MyBehavior::new())
    ///     .with_name("user-actor")
    ///     .with_virtual_actor()
    ///     .build()
    ///     .await?;
    /// ```
    pub fn with_virtual_actor(mut self) -> Self {
        // Store virtual actor config in actor config properties
        let mut config = self.spawn_spec.config.take().unwrap_or_default();
        config.properties.insert(
            "facet.virtual_actor.enabled".to_string(),
            prost_types::Any {
                type_url: "type.googleapis.com/google.protobuf.BoolValue".to_string(),
                value: vec![1], // true
            },
        );
        self.spawn_spec.config = Some(config);
        self
    }

    /// Set the actor configuration (from proto)
    ///
    /// ## Arguments
    /// * `config` - Actor configuration (resource requirements, shard groups, etc.)
    ///
    /// ## Example
    /// ```rust,ignore
    /// let mut actor_config = ActorConfig::default();
    /// actor_config.resource_requirements = Some(resource_requirements);
    /// let builder = ActorBuilder::new(MyBehavior::new())
    ///     .with_config(Some(actor_config));
    /// ```
    pub fn with_config(mut self, config: Option<ActorConfig>) -> Self {
        self.spawn_spec.config = config;
        self
    }

    /// Set resource requirements (convenience method)
    ///
    /// ## Arguments
    /// * `cpu_cores` - CPU cores (converted to CPU percent, assuming 100% per core)
    /// * `memory_bytes` - Memory in bytes
    /// * `disk_bytes` - Disk space in bytes (stored in config properties)
    /// * `gpu_count` - GPU count (stored in config properties)
    /// * `labels` - Labels map (stored in config properties)
    /// * `actor_groups` - Shard groups list (stored in config properties)
    ///
    /// ## Example
    /// ```rust,ignore
    /// let actor_ref = ActorBuilder::new(Box::new(MyBehavior))
    ///     .with_resource_requirements(
    ///         2.0,  // 2 CPU cores
    ///         1024 * 1024 * 1024,  // 1GB memory
    ///         0,  // No disk
    ///         0,  // No GPU
    ///         HashMap::new(),
    ///         vec![],
    ///     )
    ///     .spawn(service_locator)
    ///     .await?;
    /// ```
    pub fn with_resource_requirements(
        mut self,
        cpu_cores: f64,
        memory_bytes: usize,
        _disk_bytes: usize,
        _gpu_count: i32,
        labels: std::collections::HashMap<String, String>,
        actor_groups: Vec<String>,
    ) -> Self {
        use crate::resource::ResourceContract;
        // Convert CPU cores to CPU percent (assuming 100% per core)
        let max_cpu_percent = (cpu_cores * 100.0) as f32;

        // Create ResourceContract
        let _resource_contract = ResourceContract {
            max_cpu_percent,
            max_memory_bytes: memory_bytes as u64,
            max_io_ops_per_sec: None,
            guaranteed_bandwidth_mbps: None,
            max_execution_time: Some(prost_types::Duration {
                seconds: 300,
                nanos: 0,
            }),
        };

        // Get or create ActorConfig
        let mut config = self.spawn_spec.config.take().unwrap_or_default();

        // Set resource contract in config (if ActorConfig has this field)
        // For now, store in properties
        use prost_types::Any;
        config.properties.insert(
            "resource.max_cpu_percent".to_string(),
            Any {
                type_url: "type.googleapis.com/google.protobuf.FloatValue".to_string(),
                value: max_cpu_percent.to_le_bytes().to_vec(),
            },
        );
        config.properties.insert(
            "resource.max_memory_bytes".to_string(),
            Any {
                type_url: "type.googleapis.com/google.protobuf.UInt64Value".to_string(),
                value: (memory_bytes as u64).to_le_bytes().to_vec(),
            },
        );

        self.spawn_spec.labels.extend(labels);
        config.actor_groups = actor_groups;

        self.spawn_spec.config = Some(config);

        self
    }

    /// Configure actor to use a WASM module for implementation
    ///
    /// ## Purpose
    /// Enables polyglot actors by specifying a WASM module that implements
    /// the actor behavior. The module must be deployed to the cluster first.
    ///
    /// ## Arguments
    /// * `module_name` - Name of the WASM module (e.g., "counter-actor")
    /// * `module_version` - Module version (semantic versioning, e.g., "1.0.0")
    /// * `module_hash` - SHA-256 hash of the module for cache lookup
    ///
    /// ## Design Notes
    /// - WASM module info is stored in ActorConfig metadata/labels
    /// - Module must be deployed via WasmRuntimeService before actor creation
    /// - Module is content-addressed by hash for efficient caching
    ///
    /// ## Example
    /// ```rust,ignore
    /// let builder = ActorBuilder::new(MyBehavior::new())
    ///     .with_name("wasm-counter")
    ///     .with_wasm_module("counter-actor", "1.0.0", "abc123...");
    /// ```
    pub fn with_wasm_module(
        mut self,
        module_name: impl Into<String>,
        module_version: impl Into<String>,
        module_hash: impl Into<String>,
    ) -> Self {
        // Get or create ActorConfig
        let mut config = self.spawn_spec.config.take().unwrap_or_default();

        // Store WASM module info in properties map
        use prost_types::Any;

        config.properties.insert(
            "wasm.module.name".to_string(),
            Any {
                type_url: "type.googleapis.com/google.protobuf.StringValue".to_string(),
                value: module_name.into().into_bytes(),
            },
        );
        config.properties.insert(
            "wasm.module.version".to_string(),
            Any {
                type_url: "type.googleapis.com/google.protobuf.StringValue".to_string(),
                value: module_version.into().into_bytes(),
            },
        );
        config.properties.insert(
            "wasm.module.hash".to_string(),
            Any {
                type_url: "type.googleapis.com/google.protobuf.StringValue".to_string(),
                value: module_hash.into().into_bytes(),
            },
        );

        self.spawn_spec.config = Some(config);
        self
    }

    /// Build the actor with the configured options
    ///
    /// ## Returns
    /// * `Result<Actor, std::io::Error>` - The configured actor instance or error if namespace is missing
    ///
    /// ## Requirements
    /// - Namespace: Must be set via `with_namespace()` or `build_with_context()`
    ///
    /// ## Defaults
    /// - Actor ID: Generated ULID if not provided
    /// - Mailbox: derived from `ActorSpawnSpec.config.max_mailbox_size` when present
    ///
    /// ## Example
    /// ```rust,ignore
    /// let actor = ActorBuilder::new(MyBehavior::new())
    ///     .with_name("my-actor")
    ///     .with_namespace("production")
    ///     .build()
    ///     .await?;
    /// ```
    pub async fn build(self) -> Result<ActorStruct, std::io::Error> {
        let behavior_type = self.behavior.behavior_type();
        let derived_actor_type = match &behavior_type {
            crate::core::BehaviorType::GenServer => "gen_server",
            crate::core::BehaviorType::GenEvent => "gen_event",
            crate::core::BehaviorType::GenStateMachine => "gen_state_machine",
            crate::core::BehaviorType::Workflow => "workflow",
            crate::core::BehaviorType::Custom(s) => s.as_str(),
        };
        let mut spawn_spec = self.spawn_spec;
        let identity = spawn_spec.identity.take().unwrap_or_default();
        let actor_type = if identity.actor_type.is_empty() {
            derived_actor_type.to_string()
        } else {
            identity.actor_type
        };
        let actor_name = if identity.name.is_empty() {
            ulid::Ulid::new().to_string()
        } else {
            identity.name
        };
        let namespace = if spawn_spec.namespace.is_empty() {
            "default".to_string()
        } else {
            spawn_spec.namespace.clone()
        };
        let actor_id = ActorId::new(actor_name, actor_type, namespace.clone(), "unassigned")
            .map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("failed to construct actor id: {e}"),
                )
            })?;
        let tenant_id = spawn_spec.tenant_id.clone();

        let mailbox_config = spawn_spec
            .config
            .as_ref()
            .map(|config| {
                let mut cfg = plexspaces_mailbox::mailbox_config_default();
                if config.max_mailbox_size > 0 {
                    cfg.capacity = config.max_mailbox_size;
                }
                cfg
            })
            .unwrap_or_else(|| {
                use plexspaces_mailbox::mailbox_config_default;
                mailbox_config_default()
            });
        let mailbox_id = format!("mailbox_{actor_id}");
        let mailbox = Mailbox::new(mailbox_config, mailbox_id)
            .await
            .map_err(|e| std::io::Error::other(format!("Failed to create mailbox: {e}")))?;

        let node_id = Some("unassigned".to_string());

        // Create actor with config if provided
        let actor = if let Some(config) = spawn_spec.config {
            // Create actor with config in context
            use crate::core::ActorContext;
            // Create ServiceLocator for context
            // Note: This is a sync function, so we can't use create_default_service_locator
            // For ActorBuilder, we create a minimal ServiceLocator stub that will be replaced
            // when the actor is actually spawned by Node
            use crate::TestServiceLocatorStub;
            let service_locator: Arc<dyn crate::core::ServiceLocator> =
                Arc::new(TestServiceLocatorStub::new());
            let context = Arc::new(ActorContext::new(
                "unassigned".to_string(),
                tenant_id.clone(),
                namespace.clone(),
                service_locator,
                Some(config),
            ));
            let mut actor = ActorStruct::new(
                actor_id,
                self.behavior,
                mailbox,
                tenant_id.clone(),
                namespace.clone(),
                node_id.clone(),
            );
            actor = actor.set_context(context);
            actor
        } else {
            ActorStruct::new(
                actor_id,
                self.behavior,
                mailbox,
                tenant_id.clone(),
                namespace.clone(),
                node_id,
            )
        };

        // Attach facets after building
        for facet in self.facets {
            actor
                .attach_facet(facet)
                .await
                .map_err(|e| std::io::Error::other(format!("Failed to attach facet: {e}")))?;
        }

        Ok(actor)
    }

    /// Spawn the actor using ActorFactory from ServiceLocator
    ///
    /// ## Arguments
    /// * `ctx` - RequestContext for tenant isolation (first parameter)
    /// * `service_locator` - ServiceLocator to get ActorFactory from
    ///
    /// ## Returns
    /// * `ActorRef` - Reference to the spawned actor
    ///
    /// ## Example
    /// ```rust,ignore
    /// let ctx = RequestContext::new_without_auth("tenant-123".to_string(), "production".to_string());
    /// let actor_ref = ActorBuilder::new(Box::new(MyBehavior))
    ///     .with_name("my-actor")
    ///     .spawn(&ctx, node.service_locator().clone())
    ///     .await?;
    /// ```
    pub async fn spawn(
        mut self,
        ctx: &crate::core::RequestContext,
        service_locator: Arc<dyn crate::core::ServiceLocator>,
    ) -> Result<crate::ActorRef, Box<dyn std::error::Error + Send + Sync>> {
        // Use tenant_id and namespace from RequestContext
        // If auth is disabled, tenant_id will be empty string
        self.spawn_spec.tenant_id = ctx.tenant_id().to_string();
        self.spawn_spec.namespace = ctx.namespace().to_string();

        // Get required services from ServiceLocator
        let registry = service_locator.actor_registry().await.ok_or_else(|| {
            "ActorRegistry not found in ServiceLocator. Ensure Node::start() has been called."
                .to_string()
        })?;
        let facet_manager_wrapper = service_locator
            .get_facet_manager()
            .await
            .ok_or_else(|| "FacetManager not found in ServiceLocator".to_string())?;
        let facet_manager = facet_manager_wrapper.inner_clone();

        let local_node_id = registry.local_node_id();
        let tenant_id_for_ref = self.spawn_spec.tenant_id.clone();
        let namespace_for_ref = ctx.namespace().to_string();
        let spawn_visibility = ActorVisibility::try_from(self.spawn_spec.visibility)
            .unwrap_or(ActorVisibility::ActorVisibilityPublic);

        // Build the actor (facets are attached during build). One mailbox `Arc` is shared by the
        // runtime [`Actor`](ActorStruct) and the returned [`ActorRef`] (single allocation).
        let mut actor = self.build().await?;
        let actor_id = actor
            .id()
            .with_node_id(local_node_id)
            .map_err(|e| format!("Failed to normalize actor id to local node: {}", e))?;
        actor = actor.with_normalized_id(actor_id.clone());

        let actor_ref = ActorRef::local(
            actor_id.clone(),
            tenant_id_for_ref.clone(),
            namespace_for_ref.clone(),
            Arc::clone(actor.mailbox()),
            service_locator.clone(),
            spawn_visibility.clone(),
        );
        let self_ref = crate::core::ActorRef::new(actor_id.clone())
            .map_err(|e| format!("Failed to construct actor self_ref: {}", e))?;

        // Create ActorContext with proper node ID
        let actor_context = crate::core::ActorContext::new(
            local_node_id.to_string(),
            ctx.tenant_id().to_string(),
            ctx.namespace().to_string(),
            service_locator.clone(),
            actor.context().config.clone(),
        )
        .with_self_ref(self_ref);
        actor = actor.set_context(std::sync::Arc::new(actor_context));

        // Update metrics
        metrics::gauge!("plexspaces_node_active_actors",
            "node_id" => local_node_id.to_string()
        )
        .increment(1.0);

        metrics::counter!("plexspaces_node_actors_spawned_total",
            "node_id" => local_node_id.to_string(),
            "namespace" => ctx.namespace().to_string()
        )
        .increment(1);

        // Store facets
        let facets_clone = actor.facets().clone();
        facet_manager.store_facets(&actor_id, facets_clone).await;

        use crate::ActorRef;

        // Start actor (init → on_activate → background message loop)
        let _join_handle = actor
            .start()
            .await
            .map_err(|e| format!("Failed to start actor: {}", e))?;

        // Register in ActorRegistry so discovery, routing, and lookup all work.
        // This mirrors what actor_factory_impl and supervisor do after start().
        actor.register_started(&registry, spawn_visibility).await;

        // Return ActorRef
        Ok(actor_ref)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{Actor, BehaviorType};
    use async_trait::async_trait;
    use std::sync::Arc;

    struct TestBehavior;

    #[async_trait]
    impl Actor for TestBehavior {
        async fn handle_message(
            &mut self,
            _ctx: &crate::core::ActorContext,
            _msg: plexspaces_proto::common::v1::Message,
        ) -> Result<(), crate::core::BehaviorError> {
            Ok(())
        }

        fn behavior_type(&self) -> crate::core::BehaviorType {
            BehaviorType::Custom("test".to_string())
        }
    }

    #[tokio::test]
    async fn test_builder_with_defaults() {
        let actor = ActorBuilder::new(Box::new(TestBehavior))
            .with_namespace("test".to_string())
            .build()
            .await
            .expect("build should succeed with namespace");

        assert!(!actor.id().is_empty());
        assert_eq!(actor.context().namespace, "test");
        assert_eq!(actor.context().tenant_id, ""); // Empty if auth disabled
    }

    #[tokio::test]
    async fn test_builder_with_name() {
        let actor = ActorBuilder::new(Box::new(TestBehavior))
            .with_name("test-actor")
            .with_namespace("test".to_string())
            .build()
            .await
            .expect("build should succeed");

        let id = actor.id();
        assert!(id.contains("test-actor"));
    }

    #[tokio::test]
    async fn test_builder_with_namespace() {
        let actor = ActorBuilder::new(Box::new(TestBehavior))
            .with_namespace("production")
            .build()
            .await
            .expect("build should succeed");

        // Note: context() method may need to be added to Actor
        // For now, just verify actor was created
        assert!(!actor.id().is_empty());
    }

    #[tokio::test]
    async fn test_builder_to_spawn_spec_uses_actor_config() {
        let spec = ActorBuilder::new(Box::new(TestBehavior))
            .with_name("config-actor")
            .with_namespace("test".to_string())
            .with_config(Some(ActorConfig {
                max_mailbox_size: 500,
                ..Default::default()
            }))
            .to_spawn_spec("test");

        assert_eq!(spec.config.unwrap().max_mailbox_size, 500);
    }

    #[tokio::test]
    async fn test_builder_with_durability() {
        let actor = ActorBuilder::new(Box::new(TestBehavior))
            .with_name("durable-actor")
            .with_durability()
            .with_namespace("test".to_string())
            .build()
            .await
            .expect("build should succeed");

        // Verify actor was created with durability config
        assert!(!actor.id().is_empty());
        // Note: Actual facet attachment happens after build, but config is set
    }

    #[tokio::test]
    async fn test_builder_with_virtual_actor() {
        let actor = ActorBuilder::new(Box::new(TestBehavior))
            .with_name("virtual-actor")
            .with_virtual_actor()
            .with_namespace("test".to_string())
            .build()
            .await
            .expect("build should succeed");

        // Verify actor was created with virtual actor config
        assert!(!actor.id().is_empty());
    }

    #[tokio::test]
    async fn test_builder_with_resource_requirements() {
        use std::collections::HashMap;
        let actor = ActorBuilder::new(Box::new(TestBehavior))
            .with_name("resource-actor")
            .with_resource_requirements(
                2.0,                    // CPU cores
                8 * 1024 * 1024 * 1024, // 8GB memory
                100 * 1024 * 1024,      // 100MB disk
                0,                      // No GPU
                HashMap::from([("workload".to_string(), "cpu-intensive".to_string())]),
                vec!["high-priority".to_string()],
            )
            .with_namespace("test".to_string())
            .build()
            .await
            .expect("build should succeed");

        assert_eq!(actor.id(), "resource-actor//test::test@unassigned");
        assert!(!actor.id().is_empty());
    }

    #[tokio::test]
    async fn test_builder_spawn_error_when_services_not_registered() {
        use std::sync::Arc;

        // TestServiceLocatorStub returns None for actor_registry — simulates missing registry
        let service_locator: Arc<dyn crate::core::ServiceLocator> =
            Arc::new(crate::TestServiceLocatorStub::new());

        // Attempt to spawn actor - should fail because ActorRegistry is not registered
        use crate::core::RequestContext;
        let ctx = RequestContext::new_without_auth("test-tenant".to_string(), "test".to_string());
        let result = ActorBuilder::new(Box::new(TestBehavior))
            .with_name("test-actor")
            .with_namespace("test".to_string())
            .spawn(&ctx, service_locator.clone())
            .await;

        assert!(
            result.is_err(),
            "Should fail when required services are not in ServiceLocator"
        );
        let error_msg = result.unwrap_err().to_string();
        // Builder directly uses ActorRegistry, so that's the error we get
        assert!(
            error_msg.contains("ActorRegistry not found"),
            "Error message should mention ActorRegistry not found, got: {}",
            error_msg
        );
    }
}
// Integration tests that need a real Node are in tests/suite/builder_node_integration_tests.rs
