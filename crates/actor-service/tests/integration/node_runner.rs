// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! Node Runner - Standalone ActorService Node for Integration Testing
//!
//! This binary spawns a single ActorService node with:
//! - In-memory actor registry
//! - gRPC server on specified port
//! - Self-registration in registry for discovery
//!
//! Usage:
//!   node_runner <node_id> <port>
//!
//! Example:
//!   node_runner node1 9001

use plexspaces_actor_service::ActorServiceImpl;
use plexspaces_core::{ActorRegistry, ObjectRegistry as CoreObjectRegistry, ObjectRegistration};
use plexspaces_object_registry::ObjectRegistry;
use plexspaces_proto::object_registry::v1::{ObjectRegistration as ProtoObjectRegistration, ObjectType};
use plexspaces_keyvalue::InMemoryKVStore;
use plexspaces_proto::ActorServiceServer;
use std::sync::Arc;
use tonic::transport::Server;
use async_trait::async_trait;

/// Simple wrapper to adapt ObjectRegistry to CoreObjectRegistry trait
struct ObjectRegistryAdapter {
    inner: Arc<ObjectRegistry>,
}

#[async_trait]
impl CoreObjectRegistry for ObjectRegistryAdapter {
    async fn lookup(
        &self,
        ctx: &plexspaces_core::RequestContext,
        object_id: &str,
        object_type: Option<plexspaces_proto::object_registry::v1::ObjectType>,
    ) -> Result<Option<ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
        let obj_type = object_type.unwrap_or(plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeUnspecified);
        self.inner
            .lookup(ctx, obj_type, object_id)
            .await
            .map_err(|e| Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())) as Box<dyn std::error::Error + Send + Sync>)
    }

    async fn lookup_full(
        &self,
        ctx: &plexspaces_core::RequestContext,
        object_type: plexspaces_proto::object_registry::v1::ObjectType,
        object_id: &str,
    ) -> Result<Option<ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
        self.inner
            .lookup_full(ctx, object_type, object_id)
            .await
            .map_err(|e| Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())) as Box<dyn std::error::Error + Send + Sync>)
    }

    async fn register(
        &self,
        ctx: &plexspaces_core::RequestContext,
        registration: ObjectRegistration,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.inner
            .register(ctx, registration)
            .await
            .map_err(|e| Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())) as Box<dyn std::error::Error + Send + Sync>)
    }

    async fn discover(
        &self,
        ctx: &plexspaces_core::RequestContext,
        object_type: Option<plexspaces_proto::object_registry::v1::ObjectType>,
        object_category: Option<String>,
        capabilities: Option<Vec<String>>,
        labels: Option<Vec<String>>,
        health_status: Option<plexspaces_proto::object_registry::v1::HealthStatus>,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
        self.inner
            .discover(ctx, object_type, object_category, capabilities, labels, health_status, offset, limit)
            .await
            .map_err(|e| Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())) as Box<dyn std::error::Error + Send + Sync>)
    }
}

/// Register this node in the registry for discovery by other nodes
async fn register_node(
    registry: &plexspaces_object_registry::ObjectRegistry,
    node_id: &str,
    port: u16,
) -> Result<(), Box<dyn std::error::Error>> {
    let node_address = format!("127.0.0.1:{}", port);
    let node_object_id = format!("_node@{}", node_id);

    let registration = ProtoObjectRegistration {
        object_id: node_object_id.clone(),
        object_type: ObjectType::ObjectTypeService as i32,
        object_category: "node".to_string(),
        grpc_address: node_address.clone(),
        // tenant_id and namespace come from RequestContext, not registration
        ..Default::default()
    };

    let ctx = plexspaces_core::RequestContext::new_without_auth("default".to_string(), "default".to_string());
    registry.register(&ctx, registration).await?;

    println!("Registered node {} at {}", node_id, node_address);
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Parse command-line arguments
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 3 {
        eprintln!("Usage: {} <node_id> <port>", args[0]);
        eprintln!("Example: {} node1 9001", args[0]);
        std::process::exit(1);
    }

    let node_id = &args[1];
    let port: u16 = args[2]
        .parse()
        .map_err(|_| format!("Invalid port: {}", args[2]))?;

    println!("Starting ActorService node: {}", node_id);
    println!("Port: {}", port);

    // Create in-memory object registry
    let kv = Arc::new(InMemoryKVStore::new());
    let object_registry_impl = Arc::new(ObjectRegistry::new(kv));
    let object_registry: Arc<dyn CoreObjectRegistry> = Arc::new(ObjectRegistryAdapter {
        inner: object_registry_impl.clone(),
    });

    // Register this node for discovery
    register_node(&object_registry_impl, node_id, port).await?;

    // Create ActorRegistry (composes ObjectRegistry)
    let actor_registry = Arc::new(ActorRegistry::new(object_registry, node_id.to_string()));

    // Create ServiceLocator and register services manually
    let service_locator = Arc::new(plexspaces_core::ServiceLocator::new());
    let reply_tracker = Arc::new(plexspaces_core::ReplyTracker::new());
    let reply_waiter_registry = Arc::new(plexspaces_core::ReplyWaiterRegistry::new());
    service_locator.register_service(actor_registry.clone()).await;
    service_locator.register_service(reply_tracker).await;
    service_locator.register_service(reply_waiter_registry).await;
    
    // Register VirtualActorManager
    use plexspaces_core::VirtualActorManager;
    let virtual_actor_manager = Arc::new(VirtualActorManager::new(actor_registry.clone()));
    service_locator.register_service(virtual_actor_manager).await;
    
    // Register FacetManager (from ActorRegistry) - use wrapper since FacetManager doesn't implement Service
    use plexspaces_core::facet_service_wrapper::FacetManagerServiceWrapper;
    let facet_manager = actor_registry.facet_manager().clone();
    let facet_manager_wrapper = Arc::new(FacetManagerServiceWrapper::new(facet_manager));
    service_locator.register_service_by_name(plexspaces_core::service_locator::service_names::FACET_MANAGER, facet_manager_wrapper).await;
    
    // Register ActorFactoryImpl
    use plexspaces_actor::{ActorFactory, actor_factory_impl::ActorFactoryImpl};
    let actor_factory = Arc::new(ActorFactoryImpl::new(service_locator.clone()));
    service_locator.register_service(actor_factory).await;
    let service = ActorServiceImpl::new(service_locator, node_id.to_string());

    // Start gRPC server
    let addr = format!("127.0.0.1:{}", port).parse()?;
    println!("Node {} listening on {}", node_id, addr);
    println!("Press Ctrl+C to stop");

    Server::builder()
        .add_service(ActorServiceServer::new(service))
        .serve(addr)
        .await?;

    Ok(())
}
