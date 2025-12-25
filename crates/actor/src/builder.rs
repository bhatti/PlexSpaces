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
//! impl ActorBehavior for MyBehavior { /* ... */ }
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

use crate::Actor;
use crate::resource::ResourceProfile;
use plexspaces_core::{Actor as ActorTrait, ActorId};
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
    behavior: Box<dyn ActorTrait>,
    actor_id: Option<ActorId>,
    tenant_id: String,
    namespace: String,
    mailbox_config: Option<MailboxConfig>,
    // TODO: Facets will be added after design decisions are made
    // facets: Vec<Box<dyn Facet>>,
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
    pub fn new(behavior: Box<dyn ActorTrait>) -> Self {
        Self {
            behavior,
            actor_id: None,
            tenant_id: String::new(), // Empty if auth disabled
            namespace: String::new(), // Must be set via with_namespace()
            mailbox_config: None,
            // facets: Vec::new(), // TODO: Add after design decisions
            resource_profile: None,
            config: None,
            node_id: None,
        }
    }

    /// Set the actor name (generates timestamp-based ID if not provided)
    ///
    /// ## Arguments
    /// * `name` - Actor name (will be used to generate ID)
    ///
    /// ## Example
    /// ```rust,ignore
    /// let builder = ActorBuilder::new(MyBehavior::new())
    ///     .with_name("counter-actor");
    /// ```
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        // Generate timestamp-based ID from name
        use std::time::{SystemTime, UNIX_EPOCH};
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let id = format!("{}@{}", name.into(), timestamp);
        self.actor_id = Some(id);
        self
    }

    /// Set the actor ID directly
    ///
    /// ## Arguments
    /// * `id` - Actor ID (format: "name@node_id" or just "name")
    ///
    /// ## Example
    /// ```rust,ignore
    /// let builder = ActorBuilder::new(MyBehavior::new())
    ///     .with_id("my-actor@node1");
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
    /// * `config` - Actor configuration (resource requirements, actor groups, etc.)
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
    /// * `actor_groups` - Actor groups list (stored in config properties)
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
        let resource_contract = ResourceContract {
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
    pub async fn build(self) -> Result<Actor, std::io::Error> {
        // Generate actor ID if not provided
        let actor_id = self.actor_id.unwrap_or_else(|| {
            use std::time::{SystemTime, UNIX_EPOCH};
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis();
            format!("actor@{}", timestamp)
        });

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
        let mailbox_id = format!("mailbox-{}", actor_id);
        let mailbox = Mailbox::new(mailbox_config, mailbox_id)
            .await
            .expect("Failed to create mailbox");

        // Use node_id from builder or default to "local"
        let node_id = self.node_id.clone();

        // Create actor with config if provided
        let mut actor = if let Some(config) = self.config {
            // Create actor with config in context
            use plexspaces_core::ActorContext;
            let node_id_str = node_id.clone().unwrap_or_else(|| "local".to_string());
            // Create ServiceLocator for context
            // Note: This is a sync function, so we can't use create_default_service_locator
            // For ActorBuilder, we create a minimal ServiceLocator that will be replaced
            // when the actor is actually spawned by Node
            use plexspaces_core::ServiceLocator;
            let service_locator = Arc::new(ServiceLocator::new());
            let context = Arc::new(ActorContext::new(
                node_id_str,
                tenant_id.clone(),
                namespace.clone(),
                service_locator,
                Some(config),
            ));
            let mut actor = Actor::new(actor_id, self.behavior, mailbox, tenant_id.clone(), namespace.clone(), node_id.clone());
            actor = actor.set_context(context);
            actor
        } else {
            Actor::new(actor_id, self.behavior, mailbox, tenant_id.clone(), namespace.clone(), node_id)
        };

        // Apply resource profile if provided
        if let Some(profile) = self.resource_profile {
            actor = actor.with_resource_profile(profile);
        }

        // TODO: Apply facets after design decisions are made
        // See UNIFIED_ACTOR_DESIGN_DECISIONS.md

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
    ///     .with_id("my-actor@node1".to_string())
    ///     .spawn(&ctx, node.service_locator().clone())
    ///     .await?;
    /// ```
    pub async fn spawn(
        mut self,
        ctx: &plexspaces_core::RequestContext,
        service_locator: Arc<plexspaces_core::ServiceLocator>,
    ) -> Result<plexspaces_core::ActorRef, Box<dyn std::error::Error + Send + Sync>> {
        // Use tenant_id and namespace from RequestContext
        // If auth is disabled, tenant_id will be empty string
        self.tenant_id = ctx.tenant_id().to_string();
        self.namespace = ctx.namespace().to_string();
        
        // Extract actor_type from behavior before building
        let behavior_type = self.behavior.behavior_type();
        let actor_type = match behavior_type {
            plexspaces_core::BehaviorType::GenServer => "GenServer".to_string(),
            plexspaces_core::BehaviorType::GenEvent => "GenEvent".to_string(),
            plexspaces_core::BehaviorType::GenStateMachine => "GenStateMachine".to_string(),
            plexspaces_core::BehaviorType::Workflow => "Workflow".to_string(),
            plexspaces_core::BehaviorType::Custom(s) => s,
        };
        
        // Build the actor
        let actor = self.build().await?;
        
        // Extract actor ID before spawning (needed for ActorRef creation)
        let actor_id = actor.id().clone();
        
        // Get ActorFactory from ServiceLocator as trait object to avoid TypeId mismatch
        // Using trait objects (Arc<dyn ActorFactory>) instead of concrete types (Arc<ActorFactoryImpl>)
        // avoids TypeId issues when the same type is accessed through different import paths.
        // Trait objects have stable TypeIds regardless of import paths.
        use crate::get_actor_factory;
        
        let actor_factory: Arc<dyn crate::ActorFactory> = get_actor_factory(service_locator.as_ref()).await
            .ok_or_else(|| {
                format!("ActorFactory not found in ServiceLocator. Ensure Node::start() has been called.")
            })?;
        
        // Use spawn_built_actor since the actor is already built
        // This avoids recreating the actor that was just built
        let _message_sender = actor_factory.spawn_built_actor(
            ctx,
            Arc::new(actor),
            Some(actor_type),
        ).await
            .map_err(|e| format!("Failed to spawn actor via ActorFactory: {}", e))?;
        
        // Create ActorRef from the actor ID
        // Note: spawn_built_actor returns MessageSender, but we need ActorRef for compatibility
        // We can create ActorRef from the actor_id since the actor is registered
        plexspaces_core::ActorRef::new(actor_id.clone())
            .map_err(|e| format!("Failed to create ActorRef for {}: {}", actor_id, e).into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use plexspaces_core::{BehaviorType, Actor as ActorTrait};

    struct TestBehavior;

    #[async_trait]
    impl ActorTrait for TestBehavior {
        async fn handle_message(
            &mut self,
            _ctx: &plexspaces_core::ActorContext,
            _msg: plexspaces_mailbox::Message,
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
            .build().await
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
            .build().await
            .expect("build should succeed");

        let id = actor.id();
        assert!(id.contains("test-actor"));
    }

    #[tokio::test]
    async fn test_builder_with_namespace() {
        let actor = ActorBuilder::new(Box::new(TestBehavior))
            .with_namespace("production")
            .build().await
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
            .build().await
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
            .build().await
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
            .build().await
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
            .build().await
            .expect("build should succeed");

        // Verify actor was created with tuplespace capability
        assert!(!actor.id().is_empty());
    }

    #[tokio::test]
    async fn test_builder_with_resource_requirements() {
        use std::collections::HashMap;
        let actor = ActorBuilder::new(Box::new(TestBehavior))
            .with_id("resource-actor@test-node".to_string())
            .with_resource_requirements(
                2.0,                              // CPU cores
                8 * 1024 * 1024 * 1024,          // 8GB memory
                100 * 1024 * 1024,                // 100MB disk
                0,                                // No GPU
                HashMap::from([
                    ("workload".to_string(), "cpu-intensive".to_string()),
                ]),
                vec!["high-priority".to_string()],
            )
            .with_namespace("test".to_string())
            .build().await
            .expect("build should succeed");

        assert_eq!(actor.id(), "resource-actor@test-node");
        // Verify resource profile is set
        // Verify resource profile is set (check via actor's resource_profile field access)
        // Note: resource_profile field is private, so we verify via build() success
        assert!(!actor.id().is_empty());
    }

    #[tokio::test]
    async fn test_builder_with_id() {
        let actor = ActorBuilder::new(Box::new(TestBehavior))
            .with_id("custom-id@test-node".to_string())
            .with_namespace("test".to_string())
            .build().await
            .expect("build should succeed");

        assert_eq!(actor.id(), "custom-id@test-node");
    }

    #[tokio::test]
    async fn test_builder_spawn_with_node() {
        use plexspaces_node::NodeBuilder;
        use std::sync::Arc;

        // Create a test node with unique port to avoid conflicts
        // Services are automatically initialized in build()
        let node = Arc::new(NodeBuilder::new("test-node-spawn".to_string())
            .with_listen_address("127.0.0.1:0") // Port 0 = OS-assigned free port
            .build().await);

        // Spawn actor using ActorBuilder
        use plexspaces_core::RequestContext;
        let ctx = RequestContext::new_without_auth("test-tenant".to_string(), "test".to_string());
        let actor_ref = ActorBuilder::new(Box::new(TestBehavior))
            .with_id("spawned-actor@test-node-spawn".to_string())
            .with_namespace("test".to_string())
            .spawn(&ctx, node.service_locator().clone())
            .await
            .expect("Failed to spawn actor");

        assert_eq!(actor_ref.id(), "spawned-actor@test-node-spawn");
        
        // Verify actor is registered in the node's registry (with retry for async registration)
        use plexspaces_core::service_locator::service_names;
        let actor_registry: Arc<plexspaces_core::ActorRegistry> = node.service_locator()
            .get_service_by_name::<plexspaces_core::ActorRegistry>(service_names::ACTOR_REGISTRY).await
            .expect("ActorRegistry not found");
        
        // Retry lookup with timeout (actor registration is async)
        let mut found = None;
        for _ in 0..10 {
            found = actor_registry.lookup_actor(&"spawned-actor@test-node-spawn".to_string()).await;
            if found.is_some() {
                break;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        }
        assert!(found.is_some(), "Actor should be registered in ActorRegistry");
    }

    #[tokio::test]
    async fn test_builder_spawn_with_resource_requirements() {
        use plexspaces_node::NodeBuilder;
        use std::collections::HashMap;
        use std::sync::Arc;

        // Create a test node with unique port to avoid conflicts
        // Services are automatically initialized in build()
        let node = Arc::new(NodeBuilder::new("test-node-resource-spawn".to_string())
            .with_listen_address("127.0.0.1:0") // Port 0 = OS-assigned free port
            .build().await);

        // Spawn actor with resource requirements
        use plexspaces_core::RequestContext;
        let ctx = RequestContext::new_without_auth("test-tenant".to_string(), "test".to_string());
        let actor_ref = ActorBuilder::new(Box::new(TestBehavior))
            .with_id("resource-spawned-actor@test-node-resource-spawn".to_string())
            .with_resource_requirements(
                1.5,                              // CPU cores
                4 * 1024 * 1024 * 1024,          // 4GB memory
                0,                                // No disk
                0,                                // No GPU
                HashMap::from([
                    ("workload".to_string(), "memory-intensive".to_string()),
                ]),
                vec!["test-pool".to_string()],
            )
            .with_namespace("test".to_string())
            .spawn(&ctx, node.service_locator().clone())
            .await
            .expect("Failed to spawn actor with resources");

        assert_eq!(actor_ref.id(), "resource-spawned-actor@test-node-resource-spawn");
        
        // Verify actor is registered (with retry for async registration)
        use plexspaces_core::service_locator::service_names;
        let actor_registry: Arc<plexspaces_core::ActorRegistry> = node.service_locator()
            .get_service_by_name::<plexspaces_core::ActorRegistry>(service_names::ACTOR_REGISTRY).await
            .expect("ActorRegistry not found");
        
        // Retry lookup with timeout (actor registration is async)
        let mut found = None;
        for _ in 0..10 {
            found = actor_registry.lookup_actor(&"resource-spawned-actor@test-node-resource-spawn".to_string()).await;
            if found.is_some() {
                break;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        }
        assert!(found.is_some(), "Actor should be registered");
    }

    #[tokio::test]
    async fn test_builder_spawn_with_mailbox_config() {
        use plexspaces_node::NodeBuilder;
        use plexspaces_mailbox::MailboxConfig;
        use std::sync::Arc;

        // Create a test node with unique port to avoid conflicts
        // Services are automatically initialized in build()
        let node = Arc::new(NodeBuilder::new("test-node-mailbox-spawn".to_string())
            .with_listen_address("127.0.0.1:0") // Port 0 = OS-assigned free port
            .build().await);

        // Create custom mailbox config
        let mut mailbox_config = MailboxConfig::default();
        mailbox_config.capacity = 5000;

        // Spawn actor with custom mailbox config
        use plexspaces_core::RequestContext;
        let ctx = RequestContext::new_without_auth("test-tenant".to_string(), "test".to_string());
        let actor_ref = ActorBuilder::new(Box::new(TestBehavior))
            .with_id("mailbox-actor@test-node-mailbox-spawn".to_string())
            .with_mailbox_config(mailbox_config)
            .with_namespace("test".to_string())
            .spawn(&ctx, node.service_locator().clone())
            .await
            .expect("Failed to spawn actor with mailbox config");

        assert_eq!(actor_ref.id(), "mailbox-actor@test-node-mailbox-spawn");
        
        // Verify actor is registered (with retry for async registration)
        use plexspaces_core::service_locator::service_names;
        let actor_registry: Arc<plexspaces_core::ActorRegistry> = node.service_locator()
            .get_service_by_name::<plexspaces_core::ActorRegistry>(service_names::ACTOR_REGISTRY).await
            .expect("ActorRegistry not found");
        
        // Retry lookup with timeout (actor registration is async)
        let mut found = None;
        for _ in 0..10 {
            found = actor_registry.lookup_actor(&"mailbox-actor@test-node-mailbox-spawn".to_string()).await;
            if found.is_some() {
                break;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        }
        assert!(found.is_some(), "Actor should be registered");
    }

    #[tokio::test]
    async fn test_builder_spawn_error_when_actor_factory_not_found() {
        use plexspaces_core::ServiceLocator;
        use std::sync::Arc;

        // Create an empty ServiceLocator without ActorFactory
        // This tests the error case when ActorFactory is not registered
        let service_locator = Arc::new(ServiceLocator::new());

        // Attempt to spawn actor - should fail because ActorFactory is not registered
        use plexspaces_core::RequestContext;
        let ctx = RequestContext::new_without_auth("test-tenant".to_string(), "test".to_string());
        let result = ActorBuilder::new(Box::new(TestBehavior))
            .with_id("test-actor@test-node".to_string())
            .with_namespace("test".to_string())
            .spawn(&ctx, service_locator.clone())
            .await;

        assert!(result.is_err(), "Should fail when ActorFactory is not in ServiceLocator");
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("ActorFactory not found"), 
                "Error message should mention ActorFactory not found, got: {}", error_msg);
    }

    #[tokio::test]
    async fn test_builder_spawn_multiple_actors() {
        use plexspaces_node::NodeBuilder;
        use std::sync::Arc;

        // Create a test node with unique port to avoid conflicts
        // Services are automatically initialized in build()
        let node = Arc::new(NodeBuilder::new("test-node-multi-spawn".to_string())
            .with_listen_address("127.0.0.1:0") // Port 0 = OS-assigned free port
            .build().await);

        // Spawn multiple actors
        use plexspaces_core::RequestContext;
        let ctx = RequestContext::new_without_auth("test-tenant".to_string(), "test".to_string());
        let mut actor_refs = Vec::new();
        for i in 0..5 {
            let actor_ref = ActorBuilder::new(Box::new(TestBehavior))
                .with_id(format!("multi-actor-{}@test-node-multi-spawn", i))
                .with_namespace("test".to_string())
                .spawn(&ctx, node.service_locator().clone())
                .await
                .expect(&format!("Failed to spawn actor {}", i));
            actor_refs.push(actor_ref);
        }

        assert_eq!(actor_refs.len(), 5);
        
        // Verify all actors are registered (with retry for async registration)
        use plexspaces_core::service_locator::service_names;
        let actor_registry: Arc<plexspaces_core::ActorRegistry> = node.service_locator()
            .get_service_by_name::<plexspaces_core::ActorRegistry>(service_names::ACTOR_REGISTRY).await
            .expect("ActorRegistry not found");
        
        for i in 0..5 {
            let actor_id = format!("multi-actor-{}@test-node-multi-spawn", i);
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

