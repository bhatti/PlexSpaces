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

//! ServiceLocator - Centralized service registration and gRPC client caching
//!
//! ## Purpose
//! ServiceLocator provides centralized service registration and retrieval, as well as
//! gRPC client caching for remote node communication. This eliminates the need to pass
//! individual services to every component and enables efficient connection reuse.
//!
//! ## Design Philosophy
//! - **Centralized Management**: Single place to register/get services
//! - **gRPC Client Pooling**: Reuse connections across ActorRefs (one client per node)
//! - **Type Safety**: Type-based service lookup using `TypeId`
//! - **Thread Safety**: Uses `Arc<RwLock<...>>` for read-heavy workloads
//!
//! ## Usage
//!
//! ### Registering Services
//! ```rust,ignore
//! let service_locator = Arc::new(ServiceLocator::new());
//!
//! let actor_registry = Arc::new(ActorRegistry::new());
//! service_locator.register_service(actor_registry.clone());
//!
//! let reply_tracker = Arc::new(ReplyTracker::new());
//! service_locator.register_service(reply_tracker);
//! ```
//!
//! ### Retrieving Services
//! ```rust,ignore
//! let actor_registry: Arc<ActorRegistry> = service_locator.get_service().await
//!     .ok_or("ActorRegistry not registered")?;
//! ```
//!
//! ### Getting gRPC Clients
//! ```rust,ignore
//! let client = service_locator.get_node_client("node-2").await?;
//! // Client is cached and reused for subsequent calls
//! ```

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use plexspaces_proto::actor::v1::actor_service_client::ActorServiceClient;
use tonic::transport::Channel;

// Import ActorService and TupleSpaceProvider traits for trait object storage
use crate::actor_context::{ActorService, TupleSpaceProvider};
use crate::monitoring::NodeMetricsUpdater;

/// Trait for services that can be registered in ServiceLocator
pub trait Service: Send + Sync
where
    Self: 'static,
{
    /// Get the service type identifier
    fn service_type_id(&self) -> TypeId {
        TypeId::of::<Self>()
    }
}

/// ServiceLocator for centralized service registration and gRPC client caching
pub struct ServiceLocator {
    /// Registered services (TypeId -> Arc<dyn Any>)
    services: Arc<RwLock<HashMap<TypeId, Arc<dyn Any + Send + Sync>>>>,
    
    /// Registered ActorService (stored separately for type-safe access)
    /// This allows ActorContext::get_actor_service() to work without unsafe code
    /// Stored as Arc<dyn ActorService + Send + Sync> for explicit bounds, but ActorService
    /// trait already requires Send + Sync, so this is equivalent to Arc<dyn ActorService>
    actor_service: Arc<RwLock<Option<Arc<dyn ActorService + Send + Sync>>>>,
    
    /// Registered TupleSpaceProvider (stored separately for type-safe access)
    /// This allows ActorContext::get_tuplespace() to work without unsafe code
    tuplespace_provider: Arc<RwLock<Option<Arc<dyn TupleSpaceProvider + Send + Sync>>>>,
    
    /// Registered NodeMetricsUpdater (stored separately for type-safe access)
    /// This allows monitoring helpers to update NodeMetrics without depending on Node type
    node_metrics_updater: Arc<RwLock<Option<Arc<dyn NodeMetricsUpdater + Send + Sync>>>>,
    
    /// Node configuration (for accessing default_tenant_id, default_namespace, auth settings)
    /// Read-only after initialization, uses Mutex for one-time initialization
    node_config: Arc<tokio::sync::Mutex<Option<plexspaces_proto::node::v1::NodeConfig>>>,
    
    /// Cached gRPC clients (node_id -> ActorServiceClient)
    grpc_clients: Arc<RwLock<HashMap<String, ActorServiceClient<Channel>>>>,
}

impl ServiceLocator {
    /// Create a new ServiceLocator (empty, no services registered)
    ///
    /// ## Note
    /// For tests and examples, use `create_default()` instead which registers all essential services.
    /// This method should only be used internally by Node or when you need a completely empty ServiceLocator.
    pub fn new() -> Self {
        Self {
            services: Arc::new(RwLock::new(HashMap::new())),
            actor_service: Arc::new(RwLock::new(None)),
            tuplespace_provider: Arc::new(RwLock::new(None)),
            node_metrics_updater: Arc::new(RwLock::new(None)),
            node_config: Arc::new(tokio::sync::Mutex::new(None)),
            grpc_clients: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    
    /// Register NodeConfig for accessing default tenant/namespace and auth settings
    /// Note: This should be called once during node initialization
    pub async fn register_node_config(&self, config: plexspaces_proto::node::v1::NodeConfig) {
        let mut node_config = self.node_config.lock().await;
        *node_config = Some(config);
    }
    
    /// Get NodeConfig (for accessing default_tenant_id, default_namespace, auth settings)
    pub async fn get_node_config(&self) -> Option<plexspaces_proto::node::v1::NodeConfig> {
        let node_config = self.node_config.lock().await;
        node_config.clone()
    }

    /// Register a service
    ///
    /// ## Arguments
    /// * `service` - Service to register (must implement `Service` trait)
    ///
    /// ## Example
    /// ```rust,ignore
    /// let actor_registry = Arc::new(ActorRegistry::new());
    /// service_locator.register_service(actor_registry);
    /// ```
    pub async fn register_service<T: Service + 'static>(&self, service: Arc<T>) {
        let mut services = self.services.write().await;
        services.insert(TypeId::of::<T>(), service);
    }

    /// Get a registered service
    ///
    /// ## Arguments
    /// * Type parameter `T` - Service type to retrieve
    ///
    /// ## Returns
    /// `Some(Arc<T>)` if service is registered, `None` otherwise
    ///
    /// ## Example
    /// ```rust,ignore
    /// let actor_registry: Arc<ActorRegistry> = service_locator.get_service().await
    ///     .ok_or("ActorRegistry not registered")?;
    /// ```
    pub async fn get_service<T: Service + 'static>(&self) -> Option<Arc<T>> {
        let services = self.services.read().await;
        services
            .get(&TypeId::of::<T>())
            .and_then(|s| s.clone().downcast::<T>().ok())
    }

    /// Register ActorService as a trait object
    ///
    /// ## Purpose
    /// Allows ActorService to be retrieved by trait type when the concrete type is unknown.
    /// This is used by Node to register ActorServiceWrapper both as concrete type and as trait object.
    ///
    /// ## Arguments
    /// * `service` - ActorService as a trait object
    ///
    /// ## Example
    /// ```rust,ignore
    /// // Register as concrete type
    /// service_locator.register_service(actor_service_wrapper.clone()).await;
    /// // Also register as trait object
    /// let actor_service: Arc<dyn ActorService> = actor_service_wrapper.clone() as Arc<dyn ActorService>;
    /// service_locator.register_actor_service(actor_service).await;
    /// ```
    pub async fn register_actor_service(&self, service: Arc<dyn ActorService + Send + Sync>) {
        let mut actor_service = self.actor_service.write().await;
        *actor_service = Some(service);
    }

    /// Get ActorService
    ///
    /// ## Purpose
    /// Retrieves ActorService that was registered as a trait object.
    /// This allows ActorContext::get_actor_service() to work without unsafe code.
    ///
    /// ## Returns
    /// `Some(Arc<dyn ActorService>)` if registered, `None` otherwise
    pub async fn get_actor_service(&self) -> Option<Arc<dyn ActorService>> {
        let actor_service = self.actor_service.read().await;
        // ActorService already has Send + Sync bounds, so this is safe
        actor_service.clone().map(|s| s as Arc<dyn ActorService>)
    }

    /// Register TupleSpaceProvider as a trait object
    ///
    /// ## Purpose
    /// Allows TupleSpaceProvider to be retrieved by trait type when the concrete type is unknown.
    ///
    /// ## Arguments
    /// * `provider` - TupleSpaceProvider as a trait object
    pub async fn register_tuplespace_provider(&self, provider: Arc<dyn TupleSpaceProvider + Send + Sync>) {
        let mut tuplespace = self.tuplespace_provider.write().await;
        *tuplespace = Some(provider);
    }

    /// Get TupleSpaceProvider
    ///
    /// ## Purpose
    /// Retrieves TupleSpaceProvider that was registered as a trait object.
    /// This allows ActorContext::get_tuplespace() to work without unsafe code.
    ///
    /// ## Returns
    /// `Some(Arc<dyn TupleSpaceProvider>)` if registered, `None` otherwise
    pub async fn get_tuplespace_provider(&self) -> Option<Arc<dyn TupleSpaceProvider>> {
        let tuplespace = self.tuplespace_provider.read().await;
        tuplespace.clone().map(|s| s as Arc<dyn TupleSpaceProvider>)
    }

    /// Register NodeMetricsUpdater as a trait object
    ///
    /// ## Purpose
    /// Allows NodeMetricsUpdater to be retrieved by trait type when the concrete type is unknown.
    /// This is used by Node to register NodeMetricsUpdaterWrapper as a trait object.
    ///
    /// ## Arguments
    /// * `updater` - NodeMetricsUpdater as a trait object
    pub async fn register_node_metrics_updater(&self, updater: Arc<dyn NodeMetricsUpdater + Send + Sync>) {
        let mut metrics_updater = self.node_metrics_updater.write().await;
        *metrics_updater = Some(updater);
    }

    /// Get NodeMetricsUpdater
    ///
    /// ## Purpose
    /// Retrieves NodeMetricsUpdater that was registered as a trait object.
    /// This allows monitoring helpers to update NodeMetrics without depending on Node type.
    ///
    /// ## Returns
    /// `Some(Arc<dyn NodeMetricsUpdater>)` if registered, `None` otherwise
    pub async fn get_node_metrics_updater(&self) -> Option<Arc<dyn NodeMetricsUpdater + Send + Sync>> {
        let metrics_updater = self.node_metrics_updater.read().await;
        // Clone the Arc to return it
        metrics_updater.as_ref().map(|s| s.clone())
    }

    /// Get or create a gRPC client for a remote node
    ///
    /// ## Arguments
    /// * `node_id` - Node ID to get client for
    ///
    /// ## Returns
    /// Cached or newly created `ActorServiceClient` for the node
    ///
    /// ## Design Notes
    /// - Clients are cached per node_id (one client per node)
    /// - Clients are reused across all ActorRefs for the same node
    /// - Clients are closed when ServiceLocator is dropped (Node shutdown)
    ///
    /// ## Example
    /// ```rust,ignore
    /// let client = service_locator.get_node_client("node-2").await?;
    /// ```
    pub async fn get_node_client(
        &self,
        node_id: &str,
    ) -> Result<ActorServiceClient<Channel>, Box<dyn std::error::Error + Send + Sync>> {
        // Check cache first (read lock)
        {
            let clients = self.grpc_clients.read().await;
            if let Some(client) = clients.get(node_id) {
                return Ok(client.clone());
            }
        }

        // Get node address from ActorRegistry
        use crate::ActorRegistry;
        let actor_registry: Arc<ActorRegistry> = self
            .get_service()
            .await
            .ok_or_else(|| "ActorRegistry not registered")?;

        use crate::RequestContext;
        let ctx = RequestContext::internal();
        let node_address = actor_registry
            .lookup_node_address(&ctx, node_id)
            .await
            .map_err(|e| format!("Failed to lookup node address: {}", e))?
            .ok_or_else(|| format!("Node not found: {}", node_id))?;

        // Create new client
        let endpoint = Channel::from_shared(format!("http://{}", node_address))
            .map_err(|e| format!("Invalid endpoint: {}", e))?;
        let channel = endpoint
            .connect()
            .await
            .map_err(|e| format!("Connection failed: {}", e))?;
        let client = ActorServiceClient::new(channel);

        // Cache client (write lock)
        let mut clients = self.grpc_clients.write().await;
        clients.insert(node_id.to_string(), client.clone());

        Ok(client)
    }

    /// Shutdown all gRPC clients
    ///
    /// ## Purpose
    /// Closes all cached gRPC connections. Called when Node shuts down.
    ///
    /// ## Note
    /// gRPC clients are automatically closed when dropped, but this method
    /// provides explicit cleanup for graceful shutdown.
    pub async fn shutdown(&self) {
        let mut clients = self.grpc_clients.write().await;
        clients.clear();
    }

    /// Create a mailbox with default configuration (memory backend)
    ///
    /// ## Purpose
    /// Creates a mailbox using the default memory backend.
    /// This will be extended to use mailbox_provider from RuntimeConfig when available.
    ///
    /// ## Arguments
    /// * `mailbox_id` - Unique identifier for the mailbox
    ///
    /// ## Returns
    /// Created Mailbox instance with memory backend
    ///
    /// ## Example
    /// ```rust,ignore
    /// let mailbox = service_locator.create_default_mailbox("actor-1:mailbox".to_string()).await?;
    /// ```
    pub async fn create_default_mailbox(
        &self,
        mailbox_id: String,
    ) -> Result<plexspaces_mailbox::Mailbox, Box<dyn std::error::Error + Send + Sync>> {
        use plexspaces_mailbox::{Mailbox, MailboxConfig};
        use plexspaces_proto::channel::v1::ChannelBackend;
        
        // Create default mailbox config (defaults to memory)
        let mut mailbox_config = plexspaces_mailbox::mailbox_config_default();
        
        // Default to memory backend (will be extended to use mailbox_provider from RuntimeConfig)
        mailbox_config.channel_backend = ChannelBackend::ChannelBackendInMemory as i32;
        
        // Create mailbox with the configured backend
        Mailbox::new(mailbox_config, mailbox_id)
            .await
            .map_err(|e| format!("Failed to create mailbox: {}", e).into())
    }

    /// Create a channel with default configuration (memory backend)
    ///
    /// ## Purpose
    /// Creates a channel using the default memory backend.
    /// This will be extended to use channel_provider from RuntimeConfig when available.
    ///
    /// ## Arguments
    /// * `channel_name` - Unique identifier for the channel
    ///
    /// ## Returns
    /// Created Channel instance with memory backend
    ///
    /// ## Example
    /// ```rust,ignore
    /// let channel = service_locator.create_default_channel("my-channel".to_string()).await?;
    /// ```
    pub async fn create_default_channel(
        &self,
        channel_name: String,
    ) -> Result<Arc<dyn plexspaces_channel::Channel>, Box<dyn std::error::Error + Send + Sync>> {
        use plexspaces_proto::channel::v1::{ChannelBackend, ChannelConfig, DeliveryGuarantee, OrderingGuarantee};
        
        // Create default channel config (memory backend)
        let channel_config = ChannelConfig {
            name: channel_name,
            backend: ChannelBackend::ChannelBackendInMemory as i32,
            capacity: 1000, // Default capacity
            delivery: DeliveryGuarantee::DeliveryGuaranteeAtLeastOnce as i32,
            ordering: OrderingGuarantee::OrderingGuaranteeFifo as i32,
            ..Default::default()
        };
        
        // Create channel using the channel crate's create_channel function
        let channel = plexspaces_channel::create_channel(channel_config)
            .await
            .map_err(|e| format!("Failed to create channel: {}", e))?;
        
        Ok(Arc::from(channel))
    }
}

impl Default for ServiceLocator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockService {
        value: u32,
    }

    impl Service for MockService {}

    #[tokio::test]
    async fn test_register_and_get_service() {
        let locator = ServiceLocator::new();
        let service = Arc::new(MockService { value: 42 });

        locator.register_service(service.clone()).await;

        let retrieved: Arc<MockService> = locator.get_service().await.unwrap();
        assert_eq!(retrieved.value, 42);
    }

    #[tokio::test]
    async fn test_get_service_not_registered() {
        let locator = ServiceLocator::new();
        let retrieved: Option<Arc<MockService>> = locator.get_service().await;
        assert!(retrieved.is_none());
    }

    #[tokio::test]
    async fn test_service_type_safety() {
        let locator = ServiceLocator::new();

        struct ServiceA;
        impl Service for ServiceA {}

        struct ServiceB;
        impl Service for ServiceB {}

        locator.register_service(Arc::new(ServiceA)).await;
        locator.register_service(Arc::new(ServiceB)).await;

        let a: Option<Arc<ServiceA>> = locator.get_service().await;
        let b: Option<Arc<ServiceB>> = locator.get_service().await;

        assert!(a.is_some());
        assert!(b.is_some());
    }

    #[tokio::test]
    async fn test_multiple_services() {
        let locator = ServiceLocator::new();

        let service_a = Arc::new(MockService { value: 10 });
        let service_b = Arc::new(MockService { value: 20 });

        // Register different service types
        struct ServiceA;
        impl Service for ServiceA {}
        
        struct ServiceB;
        impl Service for ServiceB {}

        let service_a_impl = Arc::new(ServiceA);
        let service_b_impl = Arc::new(ServiceB);

        locator.register_service(service_a_impl.clone()).await;
        locator.register_service(service_b_impl.clone()).await;

        let retrieved_a: Arc<ServiceA> = locator.get_service().await.unwrap();
        let retrieved_b: Arc<ServiceB> = locator.get_service().await.unwrap();

        assert_eq!(Arc::as_ptr(&retrieved_a), Arc::as_ptr(&service_a_impl));
        assert_eq!(Arc::as_ptr(&retrieved_b), Arc::as_ptr(&service_b_impl));
    }

    #[tokio::test]
    async fn test_service_overwrite() {
        let locator = ServiceLocator::new();

        let service1 = Arc::new(MockService { value: 1 });
        let service2 = Arc::new(MockService { value: 2 });

        locator.register_service(service1.clone()).await;
        locator.register_service(service2.clone()).await; // Overwrites service1

        let retrieved: Arc<MockService> = locator.get_service().await.unwrap();
        assert_eq!(retrieved.value, 2);
    }

    #[tokio::test]
    async fn test_shutdown_clears_grpc_clients() {
        let locator = ServiceLocator::new();
        
        // Note: We can't easily test gRPC client creation without actual network setup
        // This test verifies shutdown() doesn't panic
        locator.shutdown().await;
        
        // Verify shutdown can be called multiple times
        locator.shutdown().await;
    }

    #[tokio::test]
    async fn test_concurrent_service_access() {
        let locator = Arc::new(ServiceLocator::new());
        let service = Arc::new(MockService { value: 100 });

        locator.register_service(service.clone()).await;

        // Spawn multiple tasks that concurrently access the service
        let mut handles = vec![];
        for _ in 0..10 {
            let locator_clone = locator.clone();
            let handle = tokio::spawn(async move {
                let retrieved: Option<Arc<MockService>> = locator_clone.get_service().await;
                retrieved.map(|s| s.value)
            });
            handles.push(handle);
        }

        // All tasks should successfully retrieve the service
        for handle in handles {
            let value = handle.await.unwrap();
            assert_eq!(value, Some(100));
        }
    }

    #[tokio::test]
    async fn test_get_node_client_without_registry() {
        let locator = ServiceLocator::new();
        
        // Should fail because ActorRegistry is not registered
        let result = locator.get_node_client("node-1").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("ActorRegistry not registered"));
    }

    #[tokio::test]
    async fn test_default_impl() {
        let locator = ServiceLocator::default();
        let service = Arc::new(MockService { value: 99 });

        locator.register_service(service.clone()).await;
        let retrieved: Arc<MockService> = locator.get_service().await.unwrap();
        assert_eq!(retrieved.value, 99);
    }
}
