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
        tenant_id: &str,
        object_id: &str,
        namespace: &str,
        object_type: Option<plexspaces_proto::object_registry::v1::ObjectType>,
    ) -> Result<Option<ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
        let obj_type = object_type.unwrap_or(plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeUnspecified);
        self.inner
            .lookup(tenant_id, namespace, obj_type, object_id)
            .await
            .map_err(|e| Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())) as Box<dyn std::error::Error + Send + Sync>)
    }

    async fn lookup_full(
        &self,
        tenant_id: &str,
        namespace: &str,
        object_type: plexspaces_proto::object_registry::v1::ObjectType,
        object_id: &str,
    ) -> Result<Option<ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
        self.inner
            .lookup(tenant_id, namespace, object_type, object_id)
            .await
            .map_err(|e| Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())) as Box<dyn std::error::Error + Send + Sync>)
    }

    async fn register(
        &self,
        registration: ObjectRegistration,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.inner
            .register(registration)
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
        tenant_id: "default".to_string(),
        namespace: "default".to_string(),
        ..Default::default()
    };

    registry.register(registration).await?;

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

    // Create ActorService (using backward-compatible constructor)
    let service_locator = Arc::new(plexspaces_core::ServiceLocator::new());
    let reply_tracker = Arc::new(plexspaces_core::ReplyTracker::new());
    let reply_waiter_registry = Arc::new(plexspaces_core::ReplyWaiterRegistry::new());
    service_locator.register_service(actor_registry.clone()).await;
    service_locator.register_service(reply_tracker).await;
    service_locator.register_service(reply_waiter_registry).await;
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
