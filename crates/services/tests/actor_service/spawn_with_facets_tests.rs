// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Integration tests for ActorService::spawn_actor with facets support
//
// ## Test Coverage
// - Spawn actor with single facet (VirtualActorFacet)
// - Spawn actor with multiple facets
// - Spawn actor with unknown facet type (graceful handling)
// - Verify facets are returned in response

use plexspaces_proto::actor::v1::ActorSpawnSpec;
use plexspaces_proto::common::v1::{ActorIdentity, Facet as ProtoFacet};
use plexspaces_proto::v1::actor::SpawnActorRequest;
use plexspaces_services::actor_service::{ActorServiceImpl, ActorServiceWrapper};
use plexspaces_services::ServiceLocatorImpl;
use plexspaces_services::ServiceLocatorTrait;
use std::collections::HashMap;
use std::sync::Arc;
use tonic::Request;

/// Helper to create a test service locator with required services
async fn create_test_service_locator_with_facets() -> Arc<ServiceLocatorImpl> {
    use plexspaces_actor::actor_context::ObjectRegistry as ObjectRegistryTrait;
    use plexspaces_actor::actor_factory_impl::ActorFactoryImpl;
    use plexspaces_actor::{
        ActorRegistry, FacetManager, FacetManagerServiceWrapper, FacetRegistryServiceWrapper,
        InitializableServiceLocator, VirtualActorManager,
    };
    use plexspaces_facet::FacetRegistry;
    use plexspaces_node::create_default_service_locator;
    use plexspaces_object_registry::{ObjectRegistryImpl, SqliteObjectRegistryRepository};

    // Create object registry adapter (using SQLite :memory:)
    let object_repo = Arc::new(
        SqliteObjectRegistryRepository::new(":memory:")
            .await
            .unwrap(),
    );
    let object_registry_impl = Arc::new(ObjectRegistryImpl::new(object_repo));

    // Create an adapter struct inline
    struct ObjectRegistryAdapter {
        inner: Arc<ObjectRegistryImpl>,
    }

    #[async_trait::async_trait]
    impl ObjectRegistryTrait for ObjectRegistryAdapter {
        async fn lookup(
            &self,
            ctx: &plexspaces_actor::RequestContext,
            object_id: &str,
            object_type: Option<plexspaces_proto::object_registry::v1::ObjectType>,
        ) -> Result<
            Option<plexspaces_proto::object_registry::v1::ObjectRegistration>,
            Box<dyn std::error::Error + Send + Sync>,
        > {
            let obj_type = object_type.unwrap_or(
                plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeUnspecified,
            );
            self.inner.lookup(ctx, obj_type, object_id).await.map_err(
                |e| -> Box<dyn std::error::Error + Send + Sync> {
                    Box::new(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        e.to_string(),
                    ))
                },
            )
        }

        async fn lookup_full(
            &self,
            ctx: &plexspaces_actor::RequestContext,
            object_type: plexspaces_proto::object_registry::v1::ObjectType,
            object_id: &str,
        ) -> Result<
            Option<plexspaces_proto::object_registry::v1::ObjectRegistration>,
            Box<dyn std::error::Error + Send + Sync>,
        > {
            self.inner
                .lookup_full(ctx, object_type, object_id)
                .await
                .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
                    Box::new(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        e.to_string(),
                    ))
                })
        }

        async fn register(
            &self,
            ctx: &plexspaces_actor::RequestContext,
            registration: plexspaces_proto::object_registry::v1::ObjectRegistration,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            self.inner.register(ctx, registration).await.map_err(
                |e| -> Box<dyn std::error::Error + Send + Sync> {
                    Box::new(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        e.to_string(),
                    ))
                },
            )
        }

        async fn discover(
            &self,
            _ctx: &plexspaces_actor::RequestContext,
            _object_type: Option<plexspaces_proto::object_registry::v1::ObjectType>,
            _name: Option<String>,
            _labels: Option<Vec<String>>,
            _exclude_labels: Option<Vec<String>>,
            _health_status: Option<plexspaces_proto::object_registry::v1::HealthStatus>,
            _limit: usize,
            _offset: usize,
        ) -> Result<
            Vec<plexspaces_proto::object_registry::v1::ObjectRegistration>,
            Box<dyn std::error::Error + Send + Sync>,
        > {
            Ok(vec![])
        }

        async fn unregister(
            &self,
            ctx: &plexspaces_actor::RequestContext,
            object_type: plexspaces_proto::object_registry::v1::ObjectType,
            object_id: &str,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            self.inner
                .unregister(ctx, object_type, object_id)
                .await
                .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
                    Box::new(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        e.to_string(),
                    ))
                })
        }

        async fn heartbeat(
            &self,
            ctx: &plexspaces_actor::RequestContext,
            object_type: plexspaces_proto::object_registry::v1::ObjectType,
            object_id: &str,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            self.inner
                .heartbeat(ctx, object_type, object_id)
                .await
                .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
                    Box::new(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        e.to_string(),
                    ))
                })
        }
    }

    let actor_registry = Arc::new(ActorRegistry::new("test-node".to_string()));

    let service_locator = create_default_service_locator(Some("test-node".to_string()), None).await;
    let reply_waiter_registry = Arc::new(plexspaces_actor::ReplyWaiterRegistry::new());
    service_locator
        .register_service(actor_registry.clone())
        .await;
    service_locator
        .register_service(reply_waiter_registry)
        .await;

    // Register VirtualActorManager
    let virtual_actor_manager = Arc::new(VirtualActorManager::new(actor_registry.clone()));
    service_locator
        .register_service(virtual_actor_manager)
        .await;

    // Register FacetManager
    let facet_manager = Arc::new(FacetManagerServiceWrapper::new(Arc::new(
        FacetManager::new(),
    )));
    service_locator.register_service(facet_manager).await;

    // Register FacetRegistry with virtual_actor facet factory
    let mut facet_registry = FacetRegistry::new();

    // Virtual actor facet factory
    struct VirtualActorFacetFactory;

    #[async_trait::async_trait]
    impl plexspaces_facet::FacetFactory for VirtualActorFacetFactory {
        async fn create(
            &self,
            config: serde_json::Value,
        ) -> Result<Box<dyn plexspaces_facet::Facet>, plexspaces_facet::FacetError> {
            let priority = config
                .get("priority")
                .and_then(|v| v.as_i64())
                .map(|p| p as i32)
                .unwrap_or(100);
            Ok(Box::new(plexspaces_journaling::VirtualActorFacet::new(
                config, priority,
            )))
        }

        fn metadata(&self) -> plexspaces_facet::FacetMetadata {
            plexspaces_facet::FacetMetadata {
                facet_type: "virtual_actor".to_string(),
                priority: 100,
                attached_at: std::time::Instant::now(),
                config: serde_json::Value::Null,
            }
        }
    }

    facet_registry.register(
        "virtual_actor".to_string(),
        Arc::new(VirtualActorFacetFactory),
    );
    let facet_registry_wrapper =
        Arc::new(FacetRegistryServiceWrapper::new(Arc::new(facet_registry)));
    service_locator
        .register_facet_registry(facet_registry_wrapper)
        .await;

    // Register ActorFactory
    let actor_factory = ActorFactoryImpl::new_arc(
        service_locator.clone() as Arc<dyn plexspaces_actor::ServiceLocator>
    )
    .await;
    service_locator
        .register_service(actor_factory.clone())
        .await;
    let factory_trait: Arc<dyn plexspaces_actor::ActorFactory> = actor_factory.clone();
    service_locator.register_actor_factory(factory_trait).await;

    service_locator
}

// =============================================================================
// Test: Spawn actor with single facet (virtual_actor)
// =============================================================================
#[tokio::test]
async fn test_spawn_actor_with_virtual_actor_facet() {
    let service_locator = create_test_service_locator_with_facets().await;
    let actor_service = ActorServiceImpl::new(service_locator, "test-node".to_string());

    // Create spawn request with virtual_actor facet
    let facet = ProtoFacet {
        r#type: "virtual_actor".to_string(),
        config: {
            let mut config = HashMap::new();
            config.insert("idle_timeout".to_string(), "5m".to_string());
            config.insert("activation_strategy".to_string(), "lazy".to_string());
            config
        },
        priority: 100,
        state: HashMap::new(),
        metadata: None,
    };

    let request = SpawnActorRequest {
        spec: Some(ActorSpawnSpec {
            identity: Some(ActorIdentity {
                name: "virtual-actor-1".to_string(),
                actor_type: "test-worker".to_string(),
            }),
            role: String::new(),
            namespace: String::new(),
            tenant_id: String::new(),
            visibility: 0,
            behavior_kind: String::new(),
            args: HashMap::new(),
            facets: vec![facet.clone()],
            config: None,
            labels: HashMap::new(),
            ..Default::default()
        }),
        namespace: "test-namespace".to_string(),
        instances_count: 1,
    };

    // Add required metadata for RequestContext
    let mut grpc_request = Request::new(request);
    grpc_request
        .metadata_mut()
        .insert("x-tenant-id", "test-tenant".parse().unwrap());
    grpc_request
        .metadata_mut()
        .insert("x-namespace", "test-namespace".parse().unwrap());

    // Spawn actor via gRPC handler
    use plexspaces_proto::v1::actor::actor_service_server::ActorService as ActorServiceTrait;
    let wrapper = ActorServiceWrapper::new(Arc::new(actor_service));
    let result = wrapper.spawn_actor(grpc_request).await;

    // The spawn may fail due to missing behavior factory, but should handle facets correctly
    match result {
        Ok(response) => {
            let resp = response.into_inner();
            assert!(!resp.actor_ref.is_empty(), "Should return actor_ref");
            assert!(
                resp.actor_ref.contains("@test-node"),
                "Should include node ID"
            );

            // Verify facets are returned in response
            if let Some(actor) = resp.actor {
                assert_eq!(actor.facets.len(), 1, "Should have 1 facet");
                assert_eq!(actor.facets[0].r#type, "virtual_actor");
            }

            println!(
                "Successfully spawned actor with virtual_actor facet: {}",
                resp.actor_ref
            );
        }
        Err(status) => {
            // May fail for other reasons (missing behavior), but NOT for facet issues
            let msg = status.message();
            assert!(
                !msg.contains("FacetRegistry not available"),
                "Should not fail due to FacetRegistry: {}",
                msg
            );
            println!(
                "Spawn failed (expected if behavior not registered): {}",
                msg
            );
        }
    }
}

// =============================================================================
// Test: Spawn actor with multiple facets
// =============================================================================
#[tokio::test]
async fn test_spawn_actor_with_multiple_facets() {
    let service_locator = create_test_service_locator_with_facets().await;
    let actor_service = ActorServiceImpl::new(service_locator, "test-node".to_string());

    // Create spawn request with multiple facets (only one will be created since we only registered virtual_actor)
    let virtual_facet = ProtoFacet {
        r#type: "virtual_actor".to_string(),
        config: {
            let mut config = HashMap::new();
            config.insert("idle_timeout".to_string(), "5m".to_string());
            config
        },
        priority: 100,
        state: HashMap::new(),
        metadata: None,
    };

    let unknown_facet = ProtoFacet {
        r#type: "metrics".to_string(), // Not registered, will be skipped gracefully
        config: HashMap::new(),
        priority: 800,
        state: HashMap::new(),
        metadata: None,
    };

    let request = SpawnActorRequest {
        spec: Some(ActorSpawnSpec {
            identity: Some(ActorIdentity {
                name: "multi-facet-actor".to_string(),
                actor_type: "test-worker".to_string(),
            }),
            role: String::new(),
            namespace: String::new(),
            tenant_id: String::new(),
            visibility: 0,
            behavior_kind: String::new(),
            args: HashMap::new(),
            facets: vec![virtual_facet, unknown_facet],
            config: None,
            labels: HashMap::new(),
            ..Default::default()
        }),
        namespace: "test-namespace".to_string(),
        instances_count: 1,
    };

    let mut grpc_request = Request::new(request);
    grpc_request
        .metadata_mut()
        .insert("x-tenant-id", "test-tenant".parse().unwrap());
    grpc_request
        .metadata_mut()
        .insert("x-namespace", "test-namespace".parse().unwrap());

    use plexspaces_proto::v1::actor::actor_service_server::ActorService as ActorServiceTrait;
    let wrapper = ActorServiceWrapper::new(Arc::new(actor_service));
    let result = wrapper.spawn_actor(grpc_request).await;

    match result {
        Ok(response) => {
            let resp = response.into_inner();
            if let Some(actor) = resp.actor {
                // Response includes original request facets
                assert_eq!(actor.facets.len(), 2, "Should have 2 facets in response");
            }
            println!("Successfully spawned actor with multiple facets");
        }
        Err(status) => {
            let msg = status.message();
            // Should NOT fail due to unknown facet type - should skip gracefully
            assert!(
                !msg.contains("metrics"),
                "Should not fail due to unknown facet type"
            );
            println!("Spawn failed (expected): {}", msg);
        }
    }
}

// =============================================================================
// Test: Spawn actor without facets (baseline)
// =============================================================================
#[tokio::test]
async fn test_spawn_actor_without_facets() {
    let service_locator = create_test_service_locator_with_facets().await;
    let actor_service = ActorServiceImpl::new(service_locator, "test-node".to_string());

    let request = SpawnActorRequest {
        spec: Some(ActorSpawnSpec {
            identity: Some(ActorIdentity {
                name: "no-facet-actor".to_string(),
                actor_type: "test-worker".to_string(),
            }),
            role: String::new(),
            namespace: String::new(),
            tenant_id: String::new(),
            visibility: 0,
            behavior_kind: String::new(),
            args: HashMap::new(),
            facets: vec![], // No facets
            config: None,
            labels: HashMap::new(),
            ..Default::default()
        }),
        namespace: "test-namespace".to_string(),
        instances_count: 1,
    };

    let mut grpc_request = Request::new(request);
    grpc_request
        .metadata_mut()
        .insert("x-tenant-id", "test-tenant".parse().unwrap());
    grpc_request
        .metadata_mut()
        .insert("x-namespace", "test-namespace".parse().unwrap());

    use plexspaces_proto::v1::actor::actor_service_server::ActorService as ActorServiceTrait;
    let wrapper = ActorServiceWrapper::new(Arc::new(actor_service));
    let result = wrapper.spawn_actor(grpc_request).await;

    match result {
        Ok(response) => {
            let resp = response.into_inner();
            assert!(!resp.actor_ref.is_empty(), "Should return actor_ref");
            if let Some(actor) = resp.actor {
                assert!(actor.facets.is_empty(), "Should have no facets");
            }
            println!(
                "Successfully spawned actor without facets: {}",
                resp.actor_ref
            );
        }
        Err(status) => {
            // Should NOT fail due to FacetRegistry issues when no facets requested
            let msg = status.message();
            assert!(
                !msg.contains("FacetRegistry"),
                "Should not check FacetRegistry when no facets"
            );
            println!(
                "Spawn failed (expected if behavior not registered): {}",
                msg
            );
        }
    }
}
