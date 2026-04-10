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

//! Comprehensive tests for ActorRegistry parent-child relationship tracking (Phase 3)
//!
//! Tests cover:
//! - Parent-child registration
//! - Parent-child unregistration
//! - Get children
//! - Get parent
//! - Subtree traversal
//! - Cleanup on actor termination
//! - Edge cases (multiple children, nested supervisors, etc.)

use plexspaces_core::{ActorId, ActorRegistry, RequestContext};
use plexspaces_object_registry::{ObjectRegistryImpl, SqliteObjectRegistryRepository};
use std::sync::Arc;

// Helper to wrap ObjectRegistry for ActorRegistry
struct ObjectRegistryAdapter {
    inner: Arc<ObjectRegistryImpl>,
}

#[async_trait::async_trait]
impl plexspaces_core::actor_context::ObjectRegistry for ObjectRegistryAdapter {
    async fn lookup(
        &self,
        ctx: &plexspaces_core::RequestContext,
        object_id: &str,
        object_type: Option<plexspaces_proto::object_registry::v1::ObjectType>,
    ) -> Result<
        Option<plexspaces_proto::object_registry::v1::ObjectRegistration>,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        let obj_type = object_type
            .unwrap_or(plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeUnspecified);
        self.inner
            .lookup(ctx, obj_type, object_id)
            .await
            .map_err(|e| {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                )) as Box<dyn std::error::Error + Send + Sync>
            })
    }

    async fn lookup_full(
        &self,
        ctx: &plexspaces_core::RequestContext,
        object_type: plexspaces_proto::object_registry::v1::ObjectType,
        object_id: &str,
    ) -> Result<
        Option<plexspaces_proto::object_registry::v1::ObjectRegistration>,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        self.inner
            .lookup_full(ctx, object_type, object_id)
            .await
            .map_err(|e| {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                )) as Box<dyn std::error::Error + Send + Sync>
            })
    }

    async fn register(
        &self,
        ctx: &plexspaces_core::RequestContext,
        registration: plexspaces_proto::object_registry::v1::ObjectRegistration,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.inner.register(ctx, registration).await.map_err(|e| {
            Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            )) as Box<dyn std::error::Error + Send + Sync>
        })
    }

    async fn discover(
        &self,
        ctx: &plexspaces_core::RequestContext,
        object_type: Option<plexspaces_proto::object_registry::v1::ObjectType>,
        object_id_prefix: Option<String>,
        object_ids: Option<Vec<String>>,
        tags: Option<Vec<String>>,
        health_status: Option<plexspaces_proto::object_registry::v1::HealthStatus>,
        limit: usize,
        offset: usize,
    ) -> Result<
        Vec<plexspaces_proto::object_registry::v1::ObjectRegistration>,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        self.inner
            .discover(
                ctx,
                object_type,
                object_id_prefix,
                object_ids,
                tags,
                health_status,
                limit,
                offset,
            )
            .await
            .map_err(|e| {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                )) as Box<dyn std::error::Error + Send + Sync>
            })
    }
    async fn unregister(
        &self,
        ctx: &plexspaces_core::RequestContext,
        object_type: plexspaces_proto::object_registry::v1::ObjectType,
        object_id: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.inner
            .unregister(ctx, object_type, object_id)
            .await
            .map_err(|e| {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                )) as Box<dyn std::error::Error + Send + Sync>
            })
    }
    async fn heartbeat(
        &self,
        ctx: &plexspaces_core::RequestContext,
        object_type: plexspaces_proto::object_registry::v1::ObjectType,
        object_id: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.inner
            .heartbeat(ctx, object_type, object_id)
            .await
            .map_err(|e| {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                )) as Box<dyn std::error::Error + Send + Sync>
            })
    }
}

async fn create_test_registry() -> Arc<ActorRegistry> {
    let object_repo = Arc::new(
        SqliteObjectRegistryRepository::new(":memory:")
            .await
            .unwrap(),
    );
    let object_registry_impl = Arc::new(ObjectRegistryImpl::new(object_repo));
    let object_registry: Arc<dyn plexspaces_core::actor_context::ObjectRegistry> =
        Arc::new(ObjectRegistryAdapter {
            inner: object_registry_impl,
        });

    Arc::new(ActorRegistry::new(object_registry, "test-node".to_string()))
}

fn test_actor_id(name: &str) -> ActorId {
    ActorId::new(name, "gen_server", "default", "test-node").unwrap()
}

#[tokio::test]
async fn test_register_parent_child() {
    let registry = create_test_registry().await;

    let parent_id = test_actor_id("supervisor1");
    let child_id = test_actor_id("worker1");

    registry.register_parent_child(&parent_id, &child_id).await;

    // Verify parent -> children mapping
    let children = registry.get_children(&parent_id).await;
    assert_eq!(children.len(), 1);
    assert_eq!(children[0], child_id);

    // Verify child -> parent mapping
    let parent = registry.get_parent(&child_id).await;
    assert_eq!(parent, Some(parent_id));
}

#[tokio::test]
async fn test_register_multiple_children() {
    let registry = create_test_registry().await;

    let parent_id = test_actor_id("supervisor1");
    let child1 = test_actor_id("worker1");
    let child2 = test_actor_id("worker2");
    let child3 = test_actor_id("worker3");

    registry.register_parent_child(&parent_id, &child1).await;
    registry.register_parent_child(&parent_id, &child2).await;
    registry.register_parent_child(&parent_id, &child3).await;

    let children = registry.get_children(&parent_id).await;
    assert_eq!(children.len(), 3);
    assert!(children.contains(&child1));
    assert!(children.contains(&child2));
    assert!(children.contains(&child3));

    assert_eq!(registry.children_count(&parent_id).await, 3);
}

#[tokio::test]
async fn test_unregister_parent_child() {
    let registry = create_test_registry().await;

    let parent_id = test_actor_id("supervisor1");
    let child_id = test_actor_id("worker1");

    registry.register_parent_child(&parent_id, &child_id).await;
    assert_eq!(registry.get_children(&parent_id).await.len(), 1);

    registry
        .unregister_parent_child(&parent_id, &child_id)
        .await;

    // Verify parent -> children mapping is empty
    let children = registry.get_children(&parent_id).await;
    assert_eq!(children.len(), 0);

    // Verify child -> parent mapping is removed
    let parent = registry.get_parent(&child_id).await;
    assert_eq!(parent, None);
}

#[tokio::test]
async fn test_unregister_one_child_of_many() {
    let registry = create_test_registry().await;

    let parent_id = test_actor_id("supervisor1");
    let child1 = test_actor_id("worker1");
    let child2 = test_actor_id("worker2");
    let child3 = test_actor_id("worker3");

    registry.register_parent_child(&parent_id, &child1).await;
    registry.register_parent_child(&parent_id, &child2).await;
    registry.register_parent_child(&parent_id, &child3).await;

    registry.unregister_parent_child(&parent_id, &child2).await;

    let children = registry.get_children(&parent_id).await;
    assert_eq!(children.len(), 2);
    assert!(children.contains(&child1));
    assert!(!children.contains(&child2));
    assert!(children.contains(&child3));

    assert_eq!(registry.get_parent(&child2).await, None);
}

#[tokio::test]
async fn test_get_children_empty() {
    let registry = create_test_registry().await;

    let parent_id = test_actor_id("supervisor1");
    let children = registry.get_children(&parent_id).await;

    assert_eq!(children.len(), 0);
}

#[tokio::test]
async fn test_get_parent_none() {
    let registry = create_test_registry().await;

    let child_id = test_actor_id("worker1");
    let parent = registry.get_parent(&child_id).await;

    assert_eq!(parent, None);
}

#[tokio::test]
async fn test_get_subtree_single_level() {
    let registry = create_test_registry().await;

    let parent_id = test_actor_id("supervisor1");
    let child1 = test_actor_id("worker1");
    let child2 = test_actor_id("worker2");

    registry.register_parent_child(&parent_id, &child1).await;
    registry.register_parent_child(&parent_id, &child2).await;

    let subtree = registry.get_subtree(&parent_id).await;
    assert_eq!(subtree.len(), 2);
    assert!(subtree.iter().any(|actor_id| actor_id.name() == child1.name()));
    assert!(subtree.iter().any(|actor_id| actor_id.name() == child2.name()));
}

#[tokio::test]
async fn test_get_subtree_nested() {
    let registry = create_test_registry().await;

    // Create nested supervision tree:
    // root-supervisor
    //   ├── worker1
    //   ├── worker2
    //   └── nested-supervisor
    //       ├── worker3
    //       └── worker4

    let root = test_actor_id("root-supervisor");
    let worker1 = test_actor_id("worker1");
    let worker2 = test_actor_id("worker2");
    let nested = test_actor_id("nested-supervisor");
    let worker3 = test_actor_id("worker3");
    let worker4 = test_actor_id("worker4");

    registry.register_parent_child(&root, &worker1).await;
    registry.register_parent_child(&root, &worker2).await;
    registry.register_parent_child(&root, &nested).await;
    registry.register_parent_child(&nested, &worker3).await;
    registry.register_parent_child(&nested, &worker4).await;

    let subtree = registry.get_subtree(&root).await;
    assert_eq!(subtree.len(), 5); // worker1, worker2, nested, worker3, worker4
    assert!(subtree.iter().any(|actor_id| actor_id.name() == worker1.name()));
    assert!(subtree.iter().any(|actor_id| actor_id.name() == worker2.name()));
    assert!(subtree.iter().any(|actor_id| actor_id.name() == nested.name()));
    assert!(subtree.iter().any(|actor_id| actor_id.name() == worker3.name()));
    assert!(subtree.iter().any(|actor_id| actor_id.name() == worker4.name()));
}

#[tokio::test]
async fn test_get_subtree_empty() {
    let registry = create_test_registry().await;

    let root = test_actor_id("root-supervisor");
    let subtree = registry.get_subtree(&root).await;

    assert_eq!(subtree.len(), 0);
}

#[tokio::test]
async fn test_cleanup_on_unregister() {
    let registry = create_test_registry().await;

    let parent_id = test_actor_id("supervisor1");
    let child_id = test_actor_id("worker1");

    registry.register_parent_child(&parent_id, &child_id).await;
    assert_eq!(registry.get_children(&parent_id).await.len(), 1);

    // Unregister actor (should clean up parent-child relationships)
    let _ = registry.unregister_with_cleanup(&child_id).await;

    // Verify parent-child relationship is cleaned up
    let children = registry.get_children(&parent_id).await;
    assert_eq!(children.len(), 0);

    let parent = registry.get_parent(&child_id).await;
    assert_eq!(parent, None);
}

#[tokio::test]
async fn test_multiple_parents_not_allowed() {
    // Test that a child can only have one parent
    let registry = create_test_registry().await;

    let parent1 = test_actor_id("supervisor1");
    let parent2 = test_actor_id("supervisor2");
    let child = test_actor_id("worker1");

    registry.register_parent_child(&parent1, &child).await;
    assert_eq!(registry.get_parent(&child).await, Some(parent1.clone()));

    // Registering with second parent should replace the first
    registry.register_parent_child(&parent2, &child).await;
    assert_eq!(registry.get_parent(&child).await, Some(parent2.clone()));

    // First parent should no longer have this child
    let children1 = registry.get_children(&parent1).await;
    assert!(!children1.contains(&child));

    // Second parent should have this child
    let children2 = registry.get_children(&parent2).await;
    assert!(children2.contains(&child));
}

#[tokio::test]
async fn test_children_count() {
    let registry = create_test_registry().await;

    let parent_id = test_actor_id("supervisor1");

    assert_eq!(registry.children_count(&parent_id).await, 0);

    registry
        .register_parent_child(&parent_id, &test_actor_id("worker1"))
        .await;
    assert_eq!(registry.children_count(&parent_id).await, 1);

    registry
        .register_parent_child(&parent_id, &test_actor_id("worker2"))
        .await;
    assert_eq!(registry.children_count(&parent_id).await, 2);

    registry
        .unregister_parent_child(&parent_id, &test_actor_id("worker1"))
        .await;
    assert_eq!(registry.children_count(&parent_id).await, 1);
}

#[tokio::test]
async fn test_unregister_nonexistent_child() {
    // Test that unregistering a non-existent child is safe
    let registry = create_test_registry().await;

    let parent_id = test_actor_id("supervisor1");
    let child_id = test_actor_id("worker1");

    // Unregister without registering first (should be safe)
    registry
        .unregister_parent_child(&parent_id, &child_id)
        .await;

    // Verify state is still consistent
    let children = registry.get_children(&parent_id).await;
    assert_eq!(children.len(), 0);

    let parent = registry.get_parent(&child_id).await;
    assert_eq!(parent, None);
}

#[tokio::test]
async fn test_deeply_nested_subtree() {
    // Test subtree with 3 levels of nesting
    let registry = create_test_registry().await;

    let level1 = test_actor_id("level1");
    let level2 = test_actor_id("level2");
    let level3 = test_actor_id("level3");
    let worker = test_actor_id("worker1");

    registry.register_parent_child(&level1, &level2).await;
    registry.register_parent_child(&level2, &level3).await;
    registry.register_parent_child(&level3, &worker).await;

    let subtree = registry.get_subtree(&level1).await;
    assert_eq!(subtree.len(), 3); // level2, level3, worker
    assert!(subtree.iter().any(|actor_id| actor_id.name() == level2.name()));
    assert!(subtree.iter().any(|actor_id| actor_id.name() == level3.name()));
    assert!(subtree.iter().any(|actor_id| actor_id.name() == worker.name()));
}
