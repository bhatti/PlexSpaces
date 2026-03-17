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

use plexspaces_common::virtual_actor_config::DEFAULT_MAX_POOL_PER_ACTOR_TYPE;
use plexspaces_common::ActivationStrategy;
use plexspaces_core::actor_ref::ActorRef;
use plexspaces_core::virtual_actor_lifecycle_facet::{
    VirtualActorLifecycleFacet, VirtualActorLifecycleState,
};
use plexspaces_core::{ActorId, ActorRegistry, MessageSender, RequestContext, VirtualActorManager};
use plexspaces_mailbox::Mailbox;
use plexspaces_node::create_default_service_locator;
use plexspaces_object_registry::{ObjectRegistryImpl, SqliteObjectRegistryRepository};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::RwLock;

/// Create a mock VirtualActorLifecycleFacet for testing
#[derive(Debug)]
struct MockVirtualActorLifecycleFacet {
    lifecycle_state: VirtualActorLifecycleState,
}

#[async_trait::async_trait]
impl VirtualActorLifecycleFacet for MockVirtualActorLifecycleFacet {
    async fn get_activation_strategy(&self) -> ActivationStrategy {
        ActivationStrategy::Lazy
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

    // Register actors and mark them as active
    // For tests, we register them in ActorRegistry to make is_actor_state_active() return true
    use plexspaces_core::actor_ref::ActorRef;
    use plexspaces_core::{MessageSender, RequestContext};
    use plexspaces_mailbox::Mailbox;
    use plexspaces_node::create_default_service_locator;

    let service_locator =
        create_default_service_locator(Some("test-node".to_string()), None, None).await;
    service_locator
        .register_service(actor_registry.clone())
        .await;
    let manager_clone = manager.clone();
    service_locator.register_service(manager_clone).await;

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
            )
            .await
            .unwrap();

        // Register actor in ActorRegistry as active (simulate active state)
        let ctx = RequestContext::new_without_auth("tenant".to_string(), "namespace".to_string());
        let mailbox = Arc::new(Mailbox::new());
        let actor_ref = ActorRef::local(
            actor_id.clone(),
            "tenant".to_string(),
            "namespace".to_string(),
            mailbox,
            service_locator.clone(),
        );

        actor_registry
            .register_actor(
                &ctx,
                actor_id.clone(),
                Arc::new(actor_ref) as Arc<dyn MessageSender>,
                Some(actor_type.clone()),
                None,
                None, // No instance for test
                None,
            )
            .await;

        // Mark as activated to add to active tracking
        manager.mark_activated(actor_id).await.unwrap();
    }

    // Check that we have 4 active instances tracked
    let active_instances = manager.registry().active_instances_by_type().read().await;
    let instances = active_instances.get(&actor_type);
    assert!(instances.is_some(), "Should have active instances tracked");
    assert_eq!(
        instances.unwrap().len(),
        4,
        "Should have 4 active instances"
    );

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

    // Verify remaining active instances
    let active_instances_after = manager.registry().active_instances_by_type().read().await;
    let instances_after = active_instances_after.get(&actor_type).unwrap();
    assert_eq!(
        instances_after.len(),
        3,
        "Should have 3 active instances after eviction"
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

#[tokio::test]
async fn test_lru_eviction_ordering() {
    // Create ActorRegistry and VirtualActorManager
    let actor_registry = create_test_actor_registry().await;
    let manager = Arc::new(VirtualActorManager::new(actor_registry.clone()));

    // Set max_pool_per_actor_type to 2
    manager.set_max_pool_per_actor_type(2).await;

    let actor_type = "TestActor".to_string();

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
        )
        .await
        .unwrap();
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
        )
        .await
        .unwrap();
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
        )
        .await
        .unwrap();

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
            )
            .await
            .unwrap();
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
            )
            .await
            .unwrap();
        manager.mark_activated(&actor_id).await.unwrap();
    }

    // Evict LRU for Type1 - should evict 1 actor
    let evicted_1 = manager.evict_lru_if_needed(&actor_type_1, None).await;
    assert_eq!(evicted_1.len(), 1, "Should evict 1 actor from Type1");

    // Evict LRU for Type2 - should evict 1 actor
    let evicted_2 = manager.evict_lru_if_needed(&actor_type_2, None).await;
    assert_eq!(evicted_2.len(), 1, "Should evict 1 actor from Type2");

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
        )
        .await
        .unwrap();

    manager.mark_activated(&actor_id).await.unwrap();

    // Get initial last_access
    let active_instances = manager.registry().active_instances_by_type().read().await;
    let instances = active_instances.get(&actor_type).unwrap();
    let initial_access = instances
        .iter()
        .find(|i| i.actor_id == actor_id)
        .unwrap()
        .last_access;

    // Wait a bit and update last_access
    tokio::time::sleep(Duration::from_millis(10)).await;
    manager.update_last_access(&actor_id).await;

    // Verify last_access was updated
    let active_instances_after = manager.registry().active_instances_by_type().read().await;
    let instances_after = active_instances_after.get(&actor_type).unwrap();
    let updated_access = instances_after
        .iter()
        .find(|i| i.actor_id == actor_id)
        .unwrap()
        .last_access;

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
        )
        .await
        .unwrap();

    manager.mark_activated(&actor_id).await.unwrap();

    // Verify actor is tracked
    let active_instances = manager.registry().active_instances_by_type().read().await;
    let instances = active_instances.get(&actor_type).unwrap();
    assert_eq!(instances.len(), 1, "Should have 1 active instance");

    // Remove from tracking
    manager.remove_from_active_tracking(&actor_id).await;

    // Verify actor is no longer tracked
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
    let actor_registry = create_test_actor_registry();
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
