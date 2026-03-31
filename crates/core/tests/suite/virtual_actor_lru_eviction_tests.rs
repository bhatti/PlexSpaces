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

//! Tests for VirtualActorManager LRU eviction

use plexspaces_actor::actor_ref::ActorRef;
use plexspaces_actor::TestServiceLocatorStub;
use plexspaces_common::virtual_actor_config::DEFAULT_MAX_POOL_PER_ACTOR_TYPE;
use plexspaces_common::ActivationStrategy;
use plexspaces_core::virtual_actor_lifecycle_facet::{
    VirtualActorLifecycleFacet, VirtualActorLifecycleState,
};
use plexspaces_core::{ActorId, ActorHandle, ActorRegistry, MessageSender, RequestContext, ServiceLocator, VirtualActorManager};
use plexspaces_mailbox::Mailbox;
use plexspaces_object_registry::{ObjectRegistryImpl, SqliteObjectRegistryRepository};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::RwLock;

use async_trait::async_trait;

/// Minimal [`ActorHandle`] so [`ActorRegistry::is_actor_state_active`] is true for LRU tests.
struct TestActiveActorHandle;

#[async_trait]
impl ActorHandle for TestActiveActorHandle {
    async fn actor_state(&self) -> i32 {
        plexspaces_proto::v1::actor::ActorState::ActorStateActive as i32
    }

    async fn stop_actor(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }
}

fn test_service_locator() -> Arc<dyn ServiceLocator> {
    Arc::new(TestServiceLocatorStub::new())
}

/// Register a sender plus a handle that reports Active, required for `evict_lru_if_needed` filtering.
async fn register_actor_as_active_in_registry(
    actor_registry: &Arc<ActorRegistry>,
    actor_id: &ActorId,
    actor_type: &str,
    tenant: &str,
    namespace: &str,
    service_locator: Arc<dyn ServiceLocator>,
) {
    let ctx = RequestContext::new_without_auth(tenant.to_string(), namespace.to_string());
    let mailbox = Arc::new(
        Mailbox::new(plexspaces_mailbox::mailbox_config_default(), actor_id.clone())
            .await
            .unwrap(),
    );
    let actor_ref = ActorRef::local(
        actor_id.clone(),
        tenant.to_string(),
        namespace.to_string(),
        mailbox,
        service_locator,
    );
    actor_registry
        .register_actor(
            &ctx,
            actor_id.clone(),
            Arc::new(actor_ref) as Arc<dyn MessageSender>,
            actor_type.to_string(),
            None,
            Some(Arc::new(TestActiveActorHandle)),
            None,
        )
        .await;
}

/// Create a mock VirtualActorLifecycleFacet for testing
#[derive(Debug)]
struct MockVirtualActorLifecycleFacet {
    lifecycle_state: VirtualActorLifecycleState,
}

#[async_trait::async_trait]
impl VirtualActorLifecycleFacet for MockVirtualActorLifecycleFacet {
    async fn get_activation_strategy(&self) -> ActivationStrategy {
        ActivationStrategy::ActivationStrategyLazy
    }

    async fn get_lifecycle_state(&self) -> VirtualActorLifecycleState {
        self.lifecycle_state.clone()
    }

    async fn should_activate(&self) -> bool {
        false
    }

    async fn start_activation(&self) -> bool {
        false
    }

    async fn mark_activated(&self) {}

    async fn mark_deactivated(&self) {}

    async fn should_deactivate(&self) -> bool {
        false
    }

    async fn update_access_time(&self) {}
}

async fn create_test_object_registry() -> Arc<dyn plexspaces_core::ObjectRegistry> {
    let repository = Arc::new(
        SqliteObjectRegistryRepository::new(":memory:")
            .await
            .unwrap(),
    );
    Arc::new(ObjectRegistryImpl::new(repository)) as Arc<dyn plexspaces_core::ObjectRegistry>
}

fn create_mock_facet(_actor_id: ActorId) -> Arc<RwLock<Box<dyn VirtualActorLifecycleFacet>>> {
    Arc::new(RwLock::new(Box::new(MockVirtualActorLifecycleFacet {
        lifecycle_state: VirtualActorLifecycleState {
            last_activated: Some(SystemTime::now()),
            last_accessed: Some(SystemTime::now()),
            activation_count: 0,
            is_activating: false,
            idle_timeout: Duration::from_secs(60),
        },
    })))
}

async fn create_test_actor_registry() -> Arc<ActorRegistry> {
    Arc::new(ActorRegistry::new(
        create_test_object_registry().await,
        "test-node".to_string(),
    ))
}

#[allow(dead_code)]
async fn create_test_actor_registry_with_node(node_id: &str) -> Arc<ActorRegistry> {
    Arc::new(ActorRegistry::new(
        create_test_object_registry().await,
        node_id.to_string(),
    ))
}

#[tokio::test]
async fn test_lru_eviction_basic() {
    // Create ActorRegistry and VirtualActorManager
    let actor_registry = create_test_actor_registry().await;
    let manager = Arc::new(VirtualActorManager::new(actor_registry.clone()));

    // Set max_pool_per_actor_type to 3 for testing
    manager.set_max_pool_per_actor_type(3).await;

    let actor_type = "TestActor".to_string();

    // Register 4 actors (exceeds limit of 3)
    let actor_ids: Vec<ActorId> = (0..4).map(|i| format!("actor-{}@test-node", i)).collect();

    let service_locator = test_service_locator();

    for actor_id in &actor_ids {
        let facet = create_mock_facet(actor_id.clone());
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

        register_actor_as_active_in_registry(
            &actor_registry,
            actor_id,
            &actor_type,
            "tenant",
            "namespace",
            service_locator.clone(),
        )
        .await;

        manager.mark_activated(actor_id).await.unwrap();
    }

    {
        let active_instances = manager.registry().active_instances_by_type().read().await;
        let instances = active_instances.get(&actor_type);
        assert!(instances.is_some(), "Should have active instances tracked");
        assert_eq!(
            instances.unwrap().len(),
            4,
            "Should have 4 active instances"
        );
    }

    // Try to activate a 5th actor - should evict 2 (4 + 1 - 3 = 2 to evict)
    let actor_id_5 = "actor-5@test-node".to_string();
    let facet_5 = create_mock_facet(actor_id_5.clone());
    manager
        .register(
            actor_id_5.clone(),
            facet_5,
            actor_type.clone(),
            None,
            "tenant".to_string(),
            "namespace".to_string(),
            vec![],
            std::collections::HashMap::new(),
            ActivationStrategy::ActivationStrategyLazy,
        )
        .await
        .unwrap();

    // Evict LRU (should evict 2 oldest)
    let evicted = manager.evict_lru_if_needed(&actor_type, None).await;
    assert_eq!(
        evicted.len(),
        2,
        "Should evict 2 actors to stay under limit"
    );
    assert!(evicted.contains(&actor_ids[0]), "Should evict oldest actor");
    assert!(
        evicted.contains(&actor_ids[1]),
        "Should evict second oldest actor"
    );

    // Remaining tracked instances: 4 - 2 evicted = 2 (actor-5 was never mark_activated).
    {
        let active_instances_after = manager.registry().active_instances_by_type().read().await;
        let instances_after = active_instances_after.get(&actor_type).unwrap();
        assert_eq!(
            instances_after.len(),
            2,
            "Should have 2 active instances after eviction"
        );
        assert!(
            !instances_after.iter().any(|i| i.actor_id == actor_ids[0]),
            "Oldest should be evicted"
        );
        assert!(
            !instances_after.iter().any(|i| i.actor_id == actor_ids[1]),
            "Second oldest should be evicted"
        );
    }
}

#[tokio::test]
async fn test_lru_eviction_ordering() {
    // Create ActorRegistry and VirtualActorManager
    let actor_registry = create_test_actor_registry().await;
    let manager = Arc::new(VirtualActorManager::new(actor_registry.clone()));

    // Set max_pool_per_actor_type to 2
    manager.set_max_pool_per_actor_type(2).await;

    let actor_type = "TestActor".to_string();
    let service_locator = test_service_locator();

    // Register 2 actors
    let actor_id_1 = "actor-1@test-node".to_string();
    let actor_id_2 = "actor-2@test-node".to_string();

    let facet_1 = create_mock_facet(actor_id_1.clone());
    manager
        .register(
            actor_id_1.clone(),
            facet_1,
            actor_type.clone(),
            None,
            "tenant".to_string(),
            "namespace".to_string(),
            vec![],
            std::collections::HashMap::new(),
            ActivationStrategy::ActivationStrategyLazy,
        )
        .await
        .unwrap();
    register_actor_as_active_in_registry(
        &actor_registry,
        &actor_id_1,
        &actor_type,
        "tenant",
        "namespace",
        service_locator.clone(),
    )
    .await;
    manager.mark_activated(&actor_id_1).await.unwrap();

    // Small delay to ensure different timestamps
    tokio::time::sleep(Duration::from_millis(10)).await;

    let facet_2 = create_mock_facet(actor_id_2.clone());
    manager
        .register(
            actor_id_2.clone(),
            facet_2,
            actor_type.clone(),
            None,
            "tenant".to_string(),
            "namespace".to_string(),
            vec![],
            std::collections::HashMap::new(),
            ActivationStrategy::ActivationStrategyLazy,
        )
        .await
        .unwrap();
    register_actor_as_active_in_registry(
        &actor_registry,
        &actor_id_2,
        &actor_type,
        "tenant",
        "namespace",
        service_locator.clone(),
    )
    .await;
    manager.mark_activated(&actor_id_2).await.unwrap();

    // Update last_access for actor_2 (making it more recent)
    tokio::time::sleep(Duration::from_millis(10)).await;
    manager.update_last_access(&actor_id_2).await;

    // Register 3rd actor - should evict actor_1 (oldest)
    let actor_id_3 = "actor-3@test-node".to_string();
    let facet_3 = create_mock_facet(actor_id_3.clone());
    manager
        .register(
            actor_id_3.clone(),
            facet_3,
            actor_type.clone(),
            None,
            "tenant".to_string(),
            "namespace".to_string(),
            vec![],
            std::collections::HashMap::new(),
            ActivationStrategy::ActivationStrategyLazy,
        )
        .await
        .unwrap();
    register_actor_as_active_in_registry(
        &actor_registry,
        &actor_id_3,
        &actor_type,
        "tenant",
        "namespace",
        service_locator.clone(),
    )
    .await;

    let evicted = manager.evict_lru_if_needed(&actor_type, None).await;
    assert_eq!(evicted.len(), 1, "Should evict 1 actor");
    assert_eq!(
        evicted[0], actor_id_1,
        "Should evict oldest actor (actor_1)"
    );
    assert!(
        !evicted.contains(&actor_id_2),
        "Should not evict actor_2 (more recent)"
    );
}

#[tokio::test]
async fn test_lru_eviction_multiple_types() {
    // Create ActorRegistry and VirtualActorManager
    let actor_registry = create_test_actor_registry().await;
    let manager = Arc::new(VirtualActorManager::new(actor_registry.clone()));

    // Set max_pool_per_actor_type to 2
    manager.set_max_pool_per_actor_type(2).await;

    let actor_type_1 = "Type1".to_string();
    let actor_type_2 = "Type2".to_string();
    let service_locator = test_service_locator();

    // Register 3 actors of Type1
    for i in 0..3 {
        let actor_id = format!("type1-actor-{}@test-node", i);
        let facet = create_mock_facet(actor_id.clone());
        manager
            .register(
                actor_id.clone(),
                facet,
                actor_type_1.clone(),
                None,
                "tenant".to_string(),
                "namespace".to_string(),
                vec![],
                std::collections::HashMap::new(),
                ActivationStrategy::ActivationStrategyLazy,
            )
            .await
            .unwrap();
        register_actor_as_active_in_registry(
            &actor_registry,
            &actor_id,
            &actor_type_1,
            "tenant",
            "namespace",
            service_locator.clone(),
        )
        .await;
        manager.mark_activated(&actor_id).await.unwrap();
    }

    // Register 3 actors of Type2
    for i in 0..3 {
        let actor_id = format!("type2-actor-{}@test-node", i);
        let facet = create_mock_facet(actor_id.clone());
        manager
            .register(
                actor_id.clone(),
                facet,
                actor_type_2.clone(),
                None,
                "tenant".to_string(),
                "namespace".to_string(),
                vec![],
                std::collections::HashMap::new(),
                ActivationStrategy::ActivationStrategyLazy,
            )
            .await
            .unwrap();
        register_actor_as_active_in_registry(
            &actor_registry,
            &actor_id,
            &actor_type_2,
            "tenant",
            "namespace",
            service_locator.clone(),
        )
        .await;
        manager.mark_activated(&actor_id).await.unwrap();
    }

    // max_pool=2, 3 actives → evict 3 - (2-1) = 2 per type
    let evicted_1 = manager.evict_lru_if_needed(&actor_type_1, None).await;
    assert_eq!(evicted_1.len(), 2, "Should evict 2 actors from Type1");

    let evicted_2 = manager.evict_lru_if_needed(&actor_type_2, None).await;
    assert_eq!(evicted_2.len(), 2, "Should evict 2 actors from Type2");

    // Verify types don't interfere
    assert!(
        !evicted_1.iter().any(|id| id.contains("type2")),
        "Type1 eviction shouldn't affect Type2"
    );
    assert!(
        !evicted_2.iter().any(|id| id.contains("type1")),
        "Type2 eviction shouldn't affect Type1"
    );
}

#[tokio::test]
async fn test_lru_eviction_skips_eager_virtual_actors() {
    let actor_registry = create_test_actor_registry().await;
    let manager = Arc::new(VirtualActorManager::new(actor_registry.clone()));
    manager.set_max_pool_per_actor_type(2).await;

    let actor_type = "TestActor".to_string();
    let service_locator = test_service_locator();

    for (actor_id, strategy) in [
        (
            "eager-actor@test-node".to_string(),
            ActivationStrategy::ActivationStrategyEager,
        ),
        (
            "lazy-actor-1@test-node".to_string(),
            ActivationStrategy::ActivationStrategyLazy,
        ),
        (
            "lazy-actor-2@test-node".to_string(),
            ActivationStrategy::ActivationStrategyLazy,
        ),
    ] {
        manager
            .register(
                actor_id.clone(),
                create_mock_facet(actor_id.clone()),
                actor_type.clone(),
                None,
                "tenant".to_string(),
                "namespace".to_string(),
                vec![],
                std::collections::HashMap::new(),
                strategy,
            )
            .await
            .unwrap();

        register_actor_as_active_in_registry(
            &actor_registry,
            &actor_id,
            &actor_type,
            "tenant",
            "namespace",
            service_locator.clone(),
        )
        .await;
        manager.mark_activated(&actor_id).await.unwrap();
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    let actor_id_4 = "lazy-actor-3@test-node".to_string();
    manager
        .register(
            actor_id_4.clone(),
            create_mock_facet(actor_id_4.clone()),
            actor_type.clone(),
            None,
            "tenant".to_string(),
            "namespace".to_string(),
            vec![],
            std::collections::HashMap::new(),
            ActivationStrategy::ActivationStrategyLazy,
        )
        .await
        .unwrap();

    let evicted = manager.evict_lru_if_needed(&actor_type, None).await;
    assert_eq!(evicted.len(), 1, "should evict exactly one lazy actor");
    assert!(
        !evicted.contains(&"eager-actor@test-node".to_string()),
        "eager actors must not be evicted"
    );
    assert_eq!(evicted[0], "lazy-actor-1@test-node".to_string());
}

#[tokio::test]
async fn test_update_last_access() {
    // Create ActorRegistry and VirtualActorManager
    let actor_registry = create_test_actor_registry().await;
    let manager = Arc::new(VirtualActorManager::new(actor_registry.clone()));

    let actor_type = "TestActor".to_string();
    let actor_id = "actor-1@test-node".to_string();

    let facet = create_mock_facet(actor_id.clone());
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
            ActivationStrategy::ActivationStrategyLazy,
        )
        .await
        .unwrap();

    manager.mark_activated(&actor_id).await.unwrap();

    let initial_access = {
        let active_instances = manager.registry().active_instances_by_type().read().await;
        let instances = active_instances.get(&actor_type).unwrap();
        instances
            .iter()
            .find(|i| i.actor_id == actor_id)
            .unwrap()
            .last_access
    };

    // Wait a bit and update last_access
    tokio::time::sleep(Duration::from_millis(10)).await;
    manager.update_last_access(&actor_id).await;

    let updated_access = {
        let active_instances_after = manager.registry().active_instances_by_type().read().await;
        let instances_after = active_instances_after.get(&actor_type).unwrap();
        instances_after
            .iter()
            .find(|i| i.actor_id == actor_id)
            .unwrap()
            .last_access
    };

    assert!(
        updated_access > initial_access,
        "last_access should be updated"
    );
}

#[tokio::test]
async fn test_remove_from_active_tracking() {
    // Create ActorRegistry and VirtualActorManager
    let actor_registry = create_test_actor_registry().await;
    let manager = Arc::new(VirtualActorManager::new(actor_registry.clone()));

    let actor_type = "TestActor".to_string();
    let actor_id = "actor-1@test-node".to_string();

    let facet = create_mock_facet(actor_id.clone());
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
            ActivationStrategy::ActivationStrategyLazy,
        )
        .await
        .unwrap();

    manager.mark_activated(&actor_id).await.unwrap();

    {
        let active_instances = manager.registry().active_instances_by_type().read().await;
        let instances = active_instances.get(&actor_type).unwrap();
        assert_eq!(instances.len(), 1, "Should have 1 active instance");
    }

    manager.remove_from_active_tracking(&actor_id).await;

    let active_instances_after = manager.registry().active_instances_by_type().read().await;
    let instances_after = active_instances_after.get(&actor_type);
    assert!(
        instances_after.is_none() || instances_after.unwrap().is_empty(),
        "Should have no active instances after removal"
    );
}

#[tokio::test]
async fn test_max_pool_per_actor_type_config() {
    // Create ActorRegistry and VirtualActorManager
    let actor_registry = create_test_actor_registry().await;
    let manager = Arc::new(VirtualActorManager::new(actor_registry.clone()));

    // Verify default
    let default_max = manager.get_max_pool_per_actor_type().await;
    assert_eq!(
        default_max, DEFAULT_MAX_POOL_PER_ACTOR_TYPE,
        "Should use default max_pool"
    );

    // Set custom value
    manager.set_max_pool_per_actor_type(50).await;
    let custom_max = manager.get_max_pool_per_actor_type().await;
    assert_eq!(custom_max, 50, "Should use custom max_pool");
}
