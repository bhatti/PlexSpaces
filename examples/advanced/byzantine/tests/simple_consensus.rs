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

//! Simple Byzantine Generals consensus test
//!
//! This test demonstrates the basic Byzantine Generals algorithm
//! with 4 processes and 1 round, matching the Erlang quick_test.

use byzantine_generals::{ByzantineAlgorithm, GeneralResult, Value};
use plexspaces_actor::ActorFactory;
use plexspaces_actor::actor_factory_impl::ActorFactoryImpl;
use plexspaces_core::RequestContext;
use plexspaces_node::NodeBuilder;
use std::sync::Arc;
use std::collections::HashMap;

#[tokio::test]
async fn test_simple_consensus() {
    // Create a node
        let node = Arc::new(NodeBuilder::new("test-node").build().await);
    let service_locator = node.service_locator();
    
    // Wait for services to be registered
    use plexspaces_core::service_locator::service_names;
    for _ in 0..10 {
        if service_locator.get_service_by_name::<plexspaces_core::ActorRegistry>(service_names::ACTOR_REGISTRY).await.is_some() {
            break;
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    }
    
    // Register ActorFactory
    use plexspaces_actor::actor_factory_impl::ActorFactoryImpl;
    let actor_factory_impl = Arc::new(ActorFactoryImpl::new(service_locator.clone()));
    service_locator.register_service(actor_factory_impl.clone()).await;
    
    // Register BehaviorFactory
    use plexspaces_core::BehaviorRegistry;
    use byzantine_generals::register_byzantine_behaviors;
    use plexspaces::journal::MemoryJournal;
    use plexspaces::tuplespace::TupleSpace;
    let mut behavior_registry = BehaviorRegistry::new();
    let journal = Arc::new(MemoryJournal::new());
    let tuplespace = Arc::new(TupleSpace::with_tenant_namespace("internal", "system"));
    register_byzantine_behaviors(&mut behavior_registry, journal, tuplespace).await;
    service_locator.register_service(Arc::new(behavior_registry)).await;
    
    // Register ReplyWaiterRegistry (required for ask pattern)
    use plexspaces_core::ReplyWaiterRegistry;
    let reply_waiter_registry = Arc::new(ReplyWaiterRegistry::new());
    service_locator.register_service(reply_waiter_registry).await;
    
    // Register ActorService
    use plexspaces_core::ActorService;
    use plexspaces_actor_service::ActorServiceImpl;
    let actor_service_impl = Arc::new(ActorServiceImpl::new(
        service_locator.clone(),
        node.id().as_str().to_string(),
    ));
    service_locator.register_actor_service(actor_service_impl.clone() as Arc<dyn ActorService + Send + Sync>).await;
    
    // Create actor context
    let ctx = plexspaces_core::ActorContext::new(
        node.id().as_str().to_string(),
        "internal".to_string(),
        "system".to_string(),
        service_locator.clone(),
        None,
    );
    
    // Spawn 4 generals
    let num_processes = 4;
    let source_id = 0;
    let num_rounds = 1;
    
    let ctx_arc = Arc::new(ctx);
    let ctx_clone = ctx_arc.clone();
    
    for general_id in 0..num_processes {
        let actor_id = format!("general{}@{}", general_id, &ctx_clone.node_id);
        let initial_state = serde_json::json!({
            "id": general_id,
            "source_id": source_id,
            "num_rounds": num_rounds
        });
        
        let request_ctx = RequestContext::internal();
        actor_factory_impl.spawn_actor(
            &request_ctx,
            &actor_id,
            "ByzantineGeneral",
            serde_json::to_vec(&initial_state).unwrap(),
            None,
            HashMap::new(),
            vec![], // facets
        ).await.expect(&format!("Failed to spawn general {}", general_id));
    }
    
    // Run the algorithm
    let algorithm = ByzantineAlgorithm::new(num_processes, source_id, num_rounds)
        .expect("Failed to create algorithm");
    
    let (duration, total_messages, results) = algorithm.run(&ctx_arc).await
        .expect("Failed to run algorithm");
    
    // Print results
    println!("\nTest Results:");
    println!("Time: {}ms", duration);
    println!("Messages: {}", total_messages);
    
    for result in &results {
        let prefix = if result.is_source { "Source " } else { "" };
        if result.is_faulty {
            println!("{}Process {} is faulty", prefix, result.id);
        } else {
                let decision_str = match result.decision {
                    Value::Zero => "zero",
                    Value::One => "one",
                    Value::Retreat => "retreat",
                };
            println!("{}Process {} decides on value {}", prefix, result.id, decision_str);
        }
    }
    
    println!("✓ Test completed successfully");
    
    // Verify we got results for all processes
    assert_eq!(results.len(), num_processes);
    assert!(total_messages > 0);
}
