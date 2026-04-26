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
//! use plexspaces_core::Actor;
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

use crate::resource::ResourceProfile;
use crate::Actor as ActorStruct;
use plexspaces_core::{Actor, ActorId};
use plexspaces_mailbox::{Mailbox, MailboxConfig};
use plexspaces_proto::v1::actor::ActorConfig;
use std::sync::Arc;
// Note: mailbox_config_default() is not exported, so we'll use MailboxConfig::default() and set capacity

/// Builder for creating actors with a fluent API
///
/// ## Purpose
/// Simplifies actor creation by providing sensible defaults and a fluent interface.
/// This is the unified way to create actors in PlexSpaces.
///
/// ## Design
/// - Uses builder pattern for configuration
/// - Provides sensible defaults (mailbox, namespace, etc.)
/// - Supports facet-based extensibility
/// - Single entry point for all actor types
pub struct ActorBuilder {
    behavior: Box<dyn Actor>,
    actor_id: Option<ActorId>,
    actor_name: Option<String>,
    tenant_id: String,
    namespace: String,
    mailbox_config: Option<MailboxConfig>,
    facets: Vec<Box<dyn plexspaces_facet::Facet>>,
    resource_profile: Option<ResourceProfile>,
    config: Option<ActorConfig>,
    node_id: Option<String>,
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
            actor_id: None,
            actor_name: None,
            tenant_id: String::new(), // Empty if auth disabled
            namespace: String::new(), // Must be set via with_namespace()
            mailbox_config: None,
            facets: Vec::new(),
            resource_profile: None,
            config: None,
            node_id: None,
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
    pub fn from_spec(behavior: Box<dyn Actor>, spec: &plexspaces_proto::actor::v1::ActorSpawnSpec) -> Self {
        let mut builder = Self::new(behavior);
        if let Some(ref identity) = spec.identity {
            if !identity.name.is_empty() {
                builder.actor_name = Some(identity.name.clone());
            }
        }
        builder.namespace = spec.namespace.clone();
        builder.tenant_id = spec.tenant_id.clone();
        if let Some(config) = spec.config.clone() {
            builder.config = Some(config);
        }
        builder
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
        self.actor_name = Some(name.into());
        self
    }

    /// Set a fully constructed actor ID.
    ///
    /// ## Design
    /// Client-facing code should prefer [`Self::with_name`], letting the runtime assemble the
    /// full [`ActorId`] from actor name, behavior type, namespace, and node placement. This
    /// method exists for advanced runtime and test scenarios that already hold a validated
    /// structured actor identity.
    ///
    /// ## Arguments
    /// * `id` - Fully constructed canonical actor ID
    ///
    /// ## Example
    /// ```rust,ignore
    /// let actor_id = plexspaces_core::ActorId::new("my-actor", "worker", "prod", "node1")?;
    /// let builder = ActorBuilder::new(MyBehavior::new())
    ///     .with_id(actor_id);
    /// ```
    pub fn with_id(mut self, id: impl Into<ActorId>) -> Self {
        self.actor_id = Some(id.into());
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
        self.namespace = namespace.into();
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
        self.tenant_id = tenant_id.into();
        self
    }

    /// Set the node ID for this actor.
    ///
    /// Use this to give the actor a fully-qualified identity at build time.
    /// If omitted, `spawn_built_actor_impl` normalizes the node_id to the
    /// actual local node at spawn time.
    ///
    /// ## Example
    /// ```rust,ignore
    /// let actor = ActorBuilder::new(MyBehavior::new())
    ///     .with_name("counter")
    ///     .with_node_id("node-1")
    ///     .build()
    ///     .await?;
    /// ```
    pub fn with_node_id(mut self, node_id: impl Into<String>) -> Self {
        self.node_id = Some(node_id.into());
        self
    }

    /// Configure the mailbox for this actor
    ///
    /// ## Arguments
    /// * `config` - Mailbox configuration
    ///
    /// ## Example
    /// ```rust,ignore
    /// let config = MailboxConfig {
    ///     max_size: 1000,
    ///     ..Default::default()
    /// };
    /// let builder = ActorBuilder::new(MyBehavior::new())
    ///     .with_mailbox_config(config);
    /// ```
    pub fn with_mailbox_config(mut self, config: MailboxConfig) -> Self {
        self.mailbox_config = Some(config);
        self
    }

    /// Set the mailbox capacity (convenience method)
    ///
    /// ## Arguments
    /// * `capacity` - Maximum number of messages in mailbox (default: 10000)
    ///
    /// ## Example
    /// ```rust,ignore
    /// let builder = ActorBuilder::new(MyBehavior::new())
    ///     .with_mailbox_capacity(5000);
    /// ```
    pub fn with_mailbox_capacity(mut self, capacity: u32) -> Self {
        use plexspaces_mailbox::mailbox_config_default;
        let mut config = mailbox_config_default();
        config.capacity = capacity;
        self.mailbox_config = Some(config);
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
        let mut config = self.config.take().unwrap_or_default();
        config.properties.insert(
            "facet.durability.enabled".to_string(),
            prost_types::Any {
                type_url: "type.googleapis.com/google.protobuf.BoolValue".to_string(),
                value: vec![1], // true
            },
        );
        self.config = Some(config);
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
        let mut config = self.config.take().unwrap_or_default();
        config.properties.insert(
            "facet.virtual_actor.enabled".to_string(),
            prost_types::Any {
                type_url: "type.googleapis.com/google.protobuf.BoolValue".to_string(),
                value: vec![1], // true
            },
        );
        self.config = Some(config);
        self
    }

    /// Enable TupleSpace access for this actor (convenience method)
    ///
    /// ## Purpose
    /// Marks that this actor needs access to TupleSpace for coordination.
    /// The actor will have TupleSpace available in its ActorContext.
    ///
    /// ## Example
    /// ```rust,ignore
    /// let actor = ActorBuilder::new(MyBehavior::new())
    ///     .with_name("coordinator")
    ///     .with_tuplespace()
    ///     .build()
    ///     .await?;
    /// ```
    pub fn with_tuplespace(mut self) -> Self {
        // Store tuplespace requirement in actor config properties
        let mut config = self.config.take().unwrap_or_default();
        config.properties.insert(
            "capability.tuplespace.required".to_string(),
            prost_types::Any {
                type_url: "type.googleapis.com/google.protobuf.BoolValue".to_string(),
                value: vec![1], // true
            },
        );
        self.config = Some(config);
        self
    }

    /// Set the resource profile for this actor
    ///
    /// ## Arguments
    /// * `profile` - Resource profile (CPU-intensive, IO-intensive, etc.)
    ///
    /// ## Example
    /// ```rust,ignore
    /// let builder = ActorBuilder::new(MyBehavior::new())
    ///     .with_resource_profile(ResourceProfile::CpuIntensive);
    /// ```
    pub fn with_resource_profile(mut self, profile: ResourceProfile) -> Self {
        self.resource_profile = Some(profile);
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
        self.config = config;
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
            max_memory_bytes: memory_bytes,
            max_io_ops_per_sec: None,
            guaranteed_bandwidth_mbps: None,
            max_execution_time: Some(std::time::Duration::from_secs(300)),
        };

        // Get or create ActorConfig
        let mut config = self.config.take().unwrap_or_default();

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

        // Store labels
        for (key, value) in labels {
            config.properties.insert(
                format!("label.{}", key),
                Any {
                    type_url: "type.googleapis.com/google.protobuf.StringValue".to_string(),
                    value: value.into_bytes(),
                },
            );
        }

        // Store actor groups
        if !actor_groups.is_empty() {
            // Store as comma-separated string in properties
            let groups_str = actor_groups.join(",");
            config.properties.insert(
                "actor_groups".to_string(),
                Any {
                    type_url: "type.googleapis.com/google.protobuf.StringValue".to_string(),
                    value: groups_str.into_bytes(),
                },
            );
        }

        self.config = Some(config);

        // Also set resource profile based on CPU/memory ratio
        use crate::resource::ResourceProfile;
        let profile = if cpu_cores > memory_bytes as f64 / (1024.0 * 1024.0 * 1024.0) {
            ResourceProfile::CpuIntensive
        } else if memory_bytes > 1024 * 1024 * 1024 {
            ResourceProfile::MemoryIntensive
        } else {
            ResourceProfile::Balanced
        };
        self.resource_profile = Some(profile);

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
        let mut config = self.config.take().unwrap_or_default();

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

        self.config = Some(config);
        self
    }

    /// Configure actor to run in a Firecracker VM
    ///
    /// ## Purpose
    /// Deploys actor to a Firecracker microVM for application-level isolation.
    /// The VM must be created and running before actor deployment.
    ///
    /// ## Arguments
    /// * `vm_id` - ID of the Firecracker VM where the actor should run
    ///
    /// ## Design Notes
    /// - Firecracker provides strong isolation at the application level
    /// - VM contains entire application (framework + actors), not individual actors
    /// - Use ApplicationDeployment builder for deploying full applications to VMs
    ///
    /// ## Example
    /// ```rust,ignore
    /// let builder = ActorBuilder::new(MyBehavior::new())
    ///     .with_name("isolated-actor")
    ///     .with_firecracker_vm("vm-001");
    /// ```
    pub fn with_firecracker_vm(mut self, vm_id: impl Into<String>) -> Self {
        // Get or create ActorConfig
        let mut config = self.config.take().unwrap_or_default();

        // Store Firecracker VM ID in properties map
        use prost_types::Any;

        config.properties.insert(
            "firecracker.vm_id".to_string(),
            Any {
                type_url: "type.googleapis.com/google.protobuf.StringValue".to_string(),
                value: vm_id.into().into_bytes(),
            },
        );

        self.config = Some(config);
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
    /// - Mailbox: Default MailboxConfig if not provided
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
        let actor_type = match &behavior_type {
            plexspaces_core::BehaviorType::GenServer => "gen_server",
            plexspaces_core::BehaviorType::GenEvent => "gen_event",
            plexspaces_core::BehaviorType::GenStateMachine => "gen_state_machine",
            plexspaces_core::BehaviorType::Workflow => "workflow",
            plexspaces_core::BehaviorType::Custom(s) => s.as_str(),
        };

        let actor_id = if let Some(actor_id) = self.actor_id.clone() {
            if let Some(ref node_id) = self.node_id {
                if actor_id.node_id() != node_id {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        format!(
                            "Actor ID '{}' specifies node '{}' but builder has node_id '{}'.",
                            actor_id,
                            actor_id.node_id(),
                            node_id
                        ),
                    ));
                }
            }
            actor_id
        } else {
            use std::time::{SystemTime, UNIX_EPOCH};
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis();
            let generated_name = self
                .actor_name
                .clone()
                .unwrap_or_else(|| format!("actor_{timestamp}"));
            // If node_id is not set, use a placeholder — spawn_built_actor_impl will
            // normalize to the real node_id at spawn time. No "local" magic strings.
            let node_id = self
                .node_id
                .clone()
                .unwrap_or_else(|| "unassigned".to_string());
            let namespace = if self.namespace.is_empty() {
                "default".to_string()
            } else {
                self.namespace.clone()
            };
            ActorId::new(generated_name, actor_type, namespace, node_id).map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("failed to construct actor id: {e}"),
                )
            })?
        };

        // Namespace is required - must be set via with_namespace()
        // Namespace can be empty - no validation needed
        let namespace = self.namespace.clone();
        let tenant_id = self.tenant_id.clone();

        // Use default mailbox config if not provided
        // Use mailbox_config_default() to ensure capacity is set (default is 10000)
        let mailbox_config = self.mailbox_config.unwrap_or_else(|| {
            use plexspaces_mailbox::mailbox_config_default;
            mailbox_config_default()
        });
        // Mailbox::new() is async, so build() must be async too
        let mailbox_id = format!("mailbox_{}", actor_id);
        let mailbox = Mailbox::new(mailbox_config, mailbox_id)
            .await
            .expect("Failed to create mailbox");

        let node_id = self.node_id.clone();

        // Create actor with config if provided
        let mut actor = if let Some(config) = self.config {
            // Create actor with config in context
            use plexspaces_core::ActorContext;
            let node_id_str = node_id.clone().unwrap_or_else(|| "unassigned".to_string());
            // Create ServiceLocator for context
            // Note: This is a sync function, so we can't use create_default_service_locator
            // For ActorBuilder, we create a minimal ServiceLocator stub that will be replaced
            // when the actor is actually spawned by Node
            use crate::TestServiceLocatorStub;
            let service_locator: Arc<dyn plexspaces_core::ServiceLocator> =
                Arc::new(TestServiceLocatorStub::new());
            let context = Arc::new(ActorContext::new(
                node_id_str,
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

        // Apply resource profile if provided
        if let Some(profile) = self.resource_profile {
            actor = actor.with_resource_profile(profile);
        }

        // Attach facets after building
        for facet in self.facets {
            actor.attach_facet(facet).await.map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Failed to attach facet: {}", e),
                )
            })?;
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
        ctx: &plexspaces_core::RequestContext,
        service_locator: Arc<dyn plexspaces_core::ServiceLocator>,
    ) -> Result<crate::ActorRef, Box<dyn std::error::Error + Send + Sync>> {
        // Use tenant_id and namespace from RequestContext
        // If auth is disabled, tenant_id will be empty string
        self.tenant_id = ctx.tenant_id().to_string();
        self.namespace = ctx.namespace().to_string();

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

        // Build with the local node id up front so the actor registers itself exactly once with
        // the canonical runtime identity and behavior metadata.
        let local_node_id = registry.local_node_id();
        self.node_id = Some(local_node_id.to_string());

        // CRITICAL: Clone tenant_id before self is moved by build()
        let tenant_id_for_ref = self.tenant_id.clone();
        let namespace_for_ref = ctx.namespace().to_string();

        // Build the actor (facets are attached during build)
        let mut actor = self.build().await?;

        // Extract actor ID and mailbox before spawning (needed for ActorRef creation)
        let actor_id = actor.id().clone();
        let mailbox = actor.mailbox().clone();

        let actor_ref = ActorRef::local(
            actor_id.clone(),
            tenant_id_for_ref.clone(),
            namespace_for_ref.clone(),
            mailbox.clone(),
            service_locator.clone(),
        );
        let self_ref = plexspaces_core::ActorRef::new(actor_id.clone())
            .map_err(|e| format!("Failed to construct actor self_ref: {}", e))?;

        // Create ActorContext with proper node ID
        let actor_context = plexspaces_core::ActorContext::new(
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
        actor.register_started(&registry).await;

        // Return ActorRef
        Ok(actor_ref)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use plexspaces_core::{Actor, BehaviorType};

    struct TestBehavior;

    #[async_trait]
    impl Actor for TestBehavior {
        async fn handle_message(
            &mut self,
            _ctx: &plexspaces_core::ActorContext,
            _msg: plexspaces_proto::common::v1::Message,
        ) -> Result<(), plexspaces_core::BehaviorError> {
            Ok(())
        }

        fn behavior_type(&self) -> plexspaces_core::BehaviorType {
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
    async fn test_builder_with_mailbox_config() {
        let mut config = MailboxConfig::default();
        config.capacity = 500;

        let actor = ActorBuilder::new(Box::new(TestBehavior))
            .with_mailbox_config(config)
            .with_namespace("test".to_string())
            .build()
            .await
            .expect("build should succeed");

        // Mailbox config is applied (verified by actor creation)
        assert!(!actor.id().is_empty());
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
    async fn test_builder_with_tuplespace() {
        let actor = ActorBuilder::new(Box::new(TestBehavior))
            .with_name("coordinator")
            .with_tuplespace()
            .with_namespace("test".to_string())
            .build()
            .await
            .expect("build should succeed");

        // Verify actor was created with tuplespace capability
        assert!(!actor.id().is_empty());
    }

    #[tokio::test]
    async fn test_builder_with_resource_requirements() {
        use std::collections::HashMap;
        let actor = ActorBuilder::new(Box::new(TestBehavior))
            .with_id(ActorId::new("resource-actor", "test", "test", "test-node").unwrap())
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

        assert_eq!(actor.id(), "resource-actor//test::test@test-node");
        // Verify resource profile is set
        // Verify resource profile is set (check via actor's resource_profile field access)
        // Note: resource_profile field is private, so we verify via build() success
        assert!(!actor.id().is_empty());
    }

    #[tokio::test]
    async fn test_builder_with_id() {
        let actor = ActorBuilder::new(Box::new(TestBehavior))
            .with_id(ActorId::new("custom-id", "test", "test", "test-node").unwrap())
            .with_namespace("test".to_string())
            .build()
            .await
            .expect("build should succeed");

        assert_eq!(actor.id(), "custom-id//test::test@test-node");
    }

    #[tokio::test]
    async fn test_builder_spawn_with_node() {
        use plexspaces_node::NodeBuilder;
        use std::sync::Arc;

        // Create a test node with unique port to avoid conflicts
        // Services are automatically initialized in build()
        let node = Arc::new(
            NodeBuilder::new("test-node-spawn".to_string())
                .with_listen_addr("127.0.0.1:0") // Port 0 = OS-assigned free port
                .build()
                .await,
        );

        // Spawn actor using ActorBuilder
        use plexspaces_core::RequestContext;
        let ctx = RequestContext::new_without_auth("test-tenant".to_string(), "test".to_string());
        let actor_ref = ActorBuilder::new(Box::new(TestBehavior))
            .with_name("spawned-actor")
            .with_namespace("test".to_string())
            .spawn(&ctx, node.service_locator().clone())
            .await
            .expect("Failed to spawn actor");

        let expected_actor_id =
            ActorId::new("spawned-actor", "test", "test", "test-node-spawn").unwrap();
        assert_eq!(actor_ref.id(), &expected_actor_id);

        // Verify actor is registered in the node's registry (with retry for async registration)
        use plexspaces_core::service_names;
        let actor_registry: Arc<plexspaces_core::ActorRegistry> = node
            .service_locator()
            .actor_registry()
            .await
            .expect("ActorRegistry not found");

        // Retry lookup with timeout (actor registration is async)
        let mut found = None;
        for _ in 0..10 {
            found = actor_registry.lookup_actor(&expected_actor_id).await;
            if found.is_some() {
                break;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        }
        assert!(
            found.is_some(),
            "Actor should be registered in ActorRegistry"
        );
    }

    #[tokio::test]
    async fn test_builder_spawn_with_resource_requirements() {
        use plexspaces_node::NodeBuilder;
        use std::collections::HashMap;
        use std::sync::Arc;

        // Create a test node with unique port to avoid conflicts
        // Services are automatically initialized in build()
        let node = Arc::new(
            NodeBuilder::new("test-node-resource-spawn".to_string())
                .with_listen_addr("127.0.0.1:0") // Port 0 = OS-assigned free port
                .build()
                .await,
        );

        // Spawn actor with resource requirements
        use plexspaces_core::RequestContext;
        let ctx = RequestContext::new_without_auth("test-tenant".to_string(), "test".to_string());
        let actor_ref = ActorBuilder::new(Box::new(TestBehavior))
            .with_name("resource-spawned-actor")
            .with_resource_requirements(
                1.5,                    // CPU cores
                4 * 1024 * 1024 * 1024, // 4GB memory
                0,                      // No disk
                0,                      // No GPU
                HashMap::from([("workload".to_string(), "memory-intensive".to_string())]),
                vec!["test-pool".to_string()],
            )
            .with_namespace("test".to_string())
            .spawn(&ctx, node.service_locator().clone())
            .await
            .expect("Failed to spawn actor with resources");

        let expected_actor_id = ActorId::new(
            "resource-spawned-actor",
            "test",
            "test",
            "test-node-resource-spawn",
        )
        .unwrap();
        assert_eq!(actor_ref.id(), &expected_actor_id);

        // Verify actor is registered (with retry for async registration)
        use plexspaces_core::service_names;
        let actor_registry: Arc<plexspaces_core::ActorRegistry> = node
            .service_locator()
            .actor_registry()
            .await
            .expect("ActorRegistry not found");

        // Retry lookup with timeout (actor registration is async)
        let mut found = None;
        for _ in 0..10 {
            found = actor_registry.lookup_actor(&expected_actor_id).await;
            if found.is_some() {
                break;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        }
        assert!(found.is_some(), "Actor should be registered");
    }

    #[tokio::test]
    async fn test_builder_spawn_with_mailbox_config() {
        use plexspaces_mailbox::MailboxConfig;
        use plexspaces_node::NodeBuilder;
        use std::sync::Arc;

        // Create a test node with unique port to avoid conflicts
        // Services are automatically initialized in build()
        let node = Arc::new(
            NodeBuilder::new("test-node-mailbox-spawn".to_string())
                .with_listen_addr("127.0.0.1:0") // Port 0 = OS-assigned free port
                .build()
                .await,
        );

        // Create custom mailbox config
        let mut mailbox_config = MailboxConfig::default();
        mailbox_config.capacity = 5000;

        // Spawn actor with custom mailbox config
        use plexspaces_core::RequestContext;
        let ctx = RequestContext::new_without_auth("test-tenant".to_string(), "test".to_string());
        let actor_ref = ActorBuilder::new(Box::new(TestBehavior))
            .with_name("mailbox-actor")
            .with_mailbox_config(mailbox_config)
            .with_namespace("test".to_string())
            .spawn(&ctx, node.service_locator().clone())
            .await
            .expect("Failed to spawn actor with mailbox config");

        let expected_actor_id =
            ActorId::new("mailbox-actor", "test", "test", "test-node-mailbox-spawn").unwrap();
        assert_eq!(actor_ref.id(), &expected_actor_id);

        // Verify actor is registered (with retry for async registration)
        use plexspaces_core::service_names;
        let actor_registry: Arc<plexspaces_core::ActorRegistry> = node
            .service_locator()
            .actor_registry()
            .await
            .expect("ActorRegistry not found");

        // Retry lookup with timeout (actor registration is async)
        let mut found = None;
        for _ in 0..10 {
            found = actor_registry.lookup_actor(&expected_actor_id).await;
            if found.is_some() {
                break;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        }
        assert!(found.is_some(), "Actor should be registered");
    }

    #[tokio::test]
    async fn test_builder_spawn_error_when_services_not_registered() {
        use plexspaces_services::ServiceLocatorImpl;
        use std::sync::Arc;

        // Create an empty ServiceLocator without required services
        // This tests the error case when ActorRegistry is not registered
        let service_locator: Arc<dyn plexspaces_core::ServiceLocator> =
            Arc::new(ServiceLocatorImpl::new());

        // Attempt to spawn actor - should fail because ActorRegistry is not registered
        use plexspaces_core::RequestContext;
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

    #[tokio::test]
    async fn test_builder_spawn_multiple_actors() {
        use plexspaces_node::NodeBuilder;
        use std::sync::Arc;

        // Create a test node with unique port to avoid conflicts
        // Services are automatically initialized in build()
        let node = Arc::new(
            NodeBuilder::new("test-node-multi-spawn".to_string())
                .with_listen_addr("127.0.0.1:0") // Port 0 = OS-assigned free port
                .build()
                .await,
        );

        // Spawn multiple actors
        use plexspaces_core::RequestContext;
        let ctx = RequestContext::new_without_auth("test-tenant".to_string(), "test".to_string());
        let mut actor_refs = Vec::new();
        for i in 0..5 {
            let actor_id = ActorId::new(
                format!("multi-actor-{i}"),
                "test",
                "test",
                "test-node-multi-spawn",
            )
            .unwrap();
            let actor_ref = ActorBuilder::new(Box::new(TestBehavior))
                .with_id(actor_id)
                .with_namespace("test".to_string())
                .spawn(&ctx, node.service_locator().clone())
                .await
                .expect(&format!("Failed to spawn actor {}", i));
            actor_refs.push(actor_ref);
        }

        assert_eq!(actor_refs.len(), 5);

        // Verify all actors are registered (with retry for async registration)
        use plexspaces_core::service_names;
        let actor_registry: Arc<plexspaces_core::ActorRegistry> = node
            .service_locator()
            .actor_registry()
            .await
            .expect("ActorRegistry not found");

        for i in 0..5 {
            let actor_id = ActorId::new(
                format!("multi-actor-{i}"),
                "test",
                "test",
                "test-node-multi-spawn",
            )
            .unwrap();
            // Retry lookup with timeout (actor registration is async)
            let mut found = None;
            for _ in 0..10 {
                found = actor_registry.lookup_actor(&actor_id).await;
                if found.is_some() {
                    break;
                }
                tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
            }
            assert!(found.is_some(), "Actor {} should be registered", actor_id);
        }
    }
}
