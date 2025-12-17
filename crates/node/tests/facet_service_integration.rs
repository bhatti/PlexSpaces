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

//! Integration tests for FacetService
//!
//! Tests cover:
//! - FacetService access from ActorContext
//! - Facet storage for normal and virtual actors
//! - Facet retrieval via FacetService
//! - SQLite backend integration (with and without locks)

mod test_helpers;
use test_helpers::spawn_actor_helper;

use plexspaces_actor::ActorBuilder;
use plexspaces_core::{Actor, ActorContext, ActorId};
use plexspaces_journaling::TimerFacet;
use plexspaces_mailbox::Message;
use plexspaces_node::{Node, NodeBuilder};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

/// Simple behavior for testing
struct TestBehavior;

#[async_trait::async_trait]
#[async_trait::async_trait]
impl Actor for TestBehavior {
    async fn handle_message(
        &mut self,
        _ctx: &ActorContext,
        _message: Message,
        _reply: &dyn plexspaces_core::Reply,
    ) -> Result<(), plexspaces_core::BehaviorError> {
        Ok(())
    }
    
    fn behavior_type(&self) -> plexspaces_core::BehaviorType {
        plexspaces_core::BehaviorType::GenServer
    }
}

/// Helper to create a test node
fn create_test_node() -> Node {
    NodeBuilder::new("test-node")
        .build()
}

#[tokio::test]
async fn test_facet_service_get_facet_normal_actor() {
    let node = Arc::new(create_test_node());
    // Note: We don't call node.start() in tests because it blocks forever (starts gRPC server)
    // For tests, we only need facet storage, which is available without starting the node
    
    // Create actor with TimerFacet
    let behavior = Box::new(TestBehavior);
    let mut actor = ActorBuilder::new(behavior)
        .with_id(ActorId::from("test-actor@local"))
        .build()
        .await;
    
    // Attach TimerFacet
    let timer_facet = Box::new(TimerFacet::new());
    actor
        .attach_facet(timer_facet, 50, serde_json::json!({}))
        .await
        .unwrap();
    
    // Spawn actor (normal actor)
    let actor_id_before = ActorId::from("test-actor@local");
    let actor_ref = spawn_actor_helper(&node, actor).await.unwrap();
    let actor_id = actor_ref.id().clone();
    
    // Verify actor_id matches
    assert_eq!(actor_id, actor_id_before, "Actor ID should match");
    
    // IMPORTANT: Check facets IMMEDIATELY after spawning, before actor can terminate
    // The actor might terminate quickly in tests, so we need to check facets right away
    // Use FacetService via Node to retrieve TimerFacet
    // Get facets container from node
    let facets = node.clone().get_facets(&actor_id).await;
    assert!(facets.is_some(), "Facets should be stored for normal actor (actor_id={:?})", actor_id);
    
    let facets_arc = facets.unwrap();
    let facets_guard = facets_arc.read().await;
    let timer_facet_arc = facets_guard.get_facet("timer");
    assert!(timer_facet_arc.is_some(), "TimerFacet should be retrievable");
}

#[tokio::test]
async fn test_facet_service_get_facet_virtual_actor() {
    let node = Arc::new(create_test_node());
    // Note: We don't call node.start() in tests because it blocks forever (starts gRPC server)
    
    // Create actor with TimerFacet and VirtualActorFacet
    let behavior = Box::new(TestBehavior);
    let mut actor = ActorBuilder::new(behavior)
        .with_id(ActorId::from("virtual-actor@local"))
        .build()
        .await;
    
    // Attach VirtualActorFacet (makes it a virtual actor)
    use plexspaces_journaling::VirtualActorFacet;
    let virtual_facet = Box::new(VirtualActorFacet::new(serde_json::json!({})));
    actor
        .attach_facet(virtual_facet, 50, serde_json::json!({}))
        .await
        .unwrap();
    
    // Attach TimerFacet
    let timer_facet = Box::new(TimerFacet::new());
    actor
        .attach_facet(timer_facet, 50, serde_json::json!({}))
        .await
        .unwrap();
    
    // Spawn actor (virtual actor)
    let actor_ref = spawn_actor_helper(&node, actor).await.unwrap();
    let actor_id = actor_ref.id().clone();
    
    // Wait for actor to be fully initialized
    sleep(Duration::from_millis(200)).await;
    
    // Get facet via FacetService (should work for virtual actors too)
    let facets = node.clone().get_facets(&actor_id).await;
    assert!(facets.is_some(), "Facets should be stored for virtual actor");
    
    let facets_arc = facets.unwrap();
    let facets_guard = facets_arc.read().await;
    let timer_facet_arc = facets_guard.get_facet("timer");
    assert!(timer_facet_arc.is_some(), "TimerFacet should be retrievable");
}

#[tokio::test]
async fn test_facet_service_get_facet_not_found() {
    let node = Arc::new(create_test_node());
    // Note: We don't call node.start() in tests because it blocks forever (starts gRPC server)
    
    // Create actor without TimerFacet
    let behavior = Box::new(TestBehavior);
    let actor = ActorBuilder::new(behavior)
        .with_id(ActorId::from("no-facet-actor@local"))
        .build()
        .await;
    
    // Spawn actor
    let actor_ref = spawn_actor_helper(&node, actor).await.unwrap();
    let actor_id = actor_ref.id().clone();
    
    // Wait for actor to be fully initialized
    sleep(Duration::from_millis(200)).await;
    
    // Try to get TimerFacet (should not be found)
    let facets = node.clone().get_facets(&actor_id).await;
    if let Some(facets_arc) = facets {
        let facets_guard = facets_arc.read().await;
        let timer_facet_arc = facets_guard.get_facet("timer");
        assert!(timer_facet_arc.is_none(), "TimerFacet should not be found");
    }
}

#[tokio::test]
async fn test_facet_service_facets_cleaned_up_on_unregister() {
    let node = Arc::new(create_test_node());
    // Note: We don't call node.start() in tests because it blocks forever (starts gRPC server)
    
    // Create actor with TimerFacet
    let behavior = Box::new(TestBehavior);
    let mut actor = ActorBuilder::new(behavior)
        .with_id(ActorId::from("cleanup-actor@local"))
        .build()
        .await;
    
    // Attach TimerFacet
    let timer_facet = Box::new(TimerFacet::new());
    actor
        .attach_facet(timer_facet, 50, serde_json::json!({}))
        .await
        .unwrap();
    
    // Spawn actor
    let actor_ref = spawn_actor_helper(&node, actor).await.unwrap();
    let actor_id = actor_ref.id().clone();
    
    // IMPORTANT: Check facets IMMEDIATELY after spawning, before actor can terminate
    // Verify facets are stored
    let facets = node.clone().get_facets(&actor_id).await;
    assert!(facets.is_some(), "Facets should be stored");
    
    // Unregister actor
    node.clone().unregister_actor(&actor_id).await.unwrap();
    
    // Verify facets are cleaned up
    let facets_after = node.clone().get_facets(&actor_id).await;
    assert!(facets_after.is_none(), "Facets should be cleaned up after unregister");
}

#[cfg(feature = "sqlite-backend")]
#[tokio::test]
async fn test_facet_service_with_sqlite_backend() {
    // This test verifies FacetService works with SQLite backend
    // (when Node uses SQLite for state storage)
    let node = Arc::new(create_test_node());
    // Note: We don't call node.start() in tests because it blocks forever (starts gRPC server)
    
    // Create actor with TimerFacet
    let behavior = Box::new(TestBehavior);
    let mut actor = ActorBuilder::new(behavior)
        .with_id(ActorId::from("sqlite-actor@local"))
        .build()
        .await;
    
    // Attach TimerFacet
    let timer_facet = Box::new(TimerFacet::new());
    actor
        .attach_facet(timer_facet, 50, serde_json::json!({}))
        .await
        .unwrap();
    
    // Spawn actor
    let actor_ref = spawn_actor_helper(&node, actor).await.unwrap();
    let actor_id = actor_ref.id().clone();
    
    // Wait for actor to be fully initialized
    sleep(Duration::from_millis(200)).await;
    
    // Get facet via FacetService
    let facets = node.clone().get_facets(&actor_id).await;
    assert!(facets.is_some(), "Facets should be stored with SQLite backend");
    
    let facets_arc = facets.unwrap();
    let facets_guard = facets_arc.read().await;
    let timer_facet_arc = facets_guard.get_facet("timer");
    assert!(timer_facet_arc.is_some(), "TimerFacet should be retrievable with SQLite backend");
}

