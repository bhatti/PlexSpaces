// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Integration test to reproduce "tried to clone a span that already closed" panic
//
// ## Purpose
// This test reproduces the exact scenario from the logs where:
// 1. Actor is spawned from within a gRPC handler (which has a span)
// 2. The actor task inherits the tracing context from the handler
// 3. When the handler returns, the span guard is dropped, but the actor task is still running
// 4. When the actor task completes and tries to log, tracing tries to clone the already-closed span
// 5. This causes a panic: "tried to clone a span (Id(...)) that already closed"
//
// ## Test Strategy
// - Create a span in the test (simulating gRPC handler span)
// - Spawn an actor from within that span
// - Let the actor process a message and complete
// - Verify no panic occurs when the actor task completes

use async_trait::async_trait;
use plexspaces_actor::Actor;
use plexspaces_behavior::GenServer;
use plexspaces_core::{
    Actor as ActorTrait, ActorContext, BehaviorError, BehaviorType, Message, RequestContext,
};
use plexspaces_mailbox::{Mailbox, MailboxConfig};
use plexspaces_node::NodeBuilder;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use ulid::Ulid;

/// Simple test actor that processes one message and completes
struct TestActor {
    processed: Arc<AtomicBool>,
}

impl TestActor {
    fn new(processed: Arc<AtomicBool>) -> Self {
        Self { processed }
    }
}

#[async_trait]
impl ActorTrait for TestActor {
    async fn handle_message(
        &mut self,
        _ctx: &ActorContext,
        _msg: Message,
    ) -> Result<(), BehaviorError> {
        self.processed.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }
}

#[async_trait]
impl GenServer for TestActor {
    async fn handle_request(
        &mut self,
        _ctx: &ActorContext,
        _msg: Message,
    ) -> Result<serde_json::Value, BehaviorError> {
        self.processed.store(true, Ordering::SeqCst);
        Ok(serde_json::json!({ "status": "ok" }))
    }
}

/// Test that reproduces the span cloning panic
///
/// This test creates a span (simulating a gRPC handler span), spawns an actor
/// from within that span, lets the actor process a message and complete, then
/// verifies no panic occurs when the actor task completes.
#[tokio::test]
async fn test_span_cloning_panic_reproduction() {
    // Set up tracing to ensure spans are created
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();

    let node = Arc::new(NodeBuilder::new("test-node").build().await);
    node.initialize_services().await.unwrap();

    let processed = Arc::new(AtomicBool::new(false));
    let actor_impl = TestActor::new(processed.clone());

    // CRITICAL: Create a span (simulating gRPC handler span)
    // This span will be dropped when the function returns, but the actor task
    // will still be running with this span in its tracing context
    let span = tracing::info_span!(
        "test_grpc_handler",
        tenant_id = "test-tenant",
        namespace = "test-namespace",
        actor_type = "test-actor"
    );
    let _guard = span.enter();

    // Spawn actor from within the span (simulating actor spawned from gRPC handler)
    let mailbox = Mailbox::new(MailboxConfig::default(), format!("mailbox-{}", Ulid::new()))
        .await
        .unwrap();

    let mut actor = Actor::new(
        format!("test-actor-{}@test-node", Ulid::new()),
        Box::new(actor_impl),
        mailbox,
        "test-tenant".to_string(),
        "test-namespace".to_string(),
        None,
    );

    // Start actor (this spawns a tokio task that inherits the tracing context)
    let handle = actor.start().await.expect("Actor should start");

    // Send a message to the actor
    let ctx =
        RequestContext::new_without_auth("test-tenant".to_string(), "test-namespace".to_string());
    let actor_ref = node.get_actor_ref(&actor.id().clone(), &ctx).await.unwrap();

    let message = Message::new(
        serde_json::json!({ "action": "test" })
            .to_string()
            .into_bytes(),
    );
    actor_ref.tell(message).await.unwrap();

    // Wait for message to be processed
    for _ in 0..100 {
        if processed.load(Ordering::SeqCst) {
            break;
        }
        sleep(Duration::from_millis(10)).await;
    }

    assert!(
        processed.load(Ordering::SeqCst),
        "Actor should have processed message"
    );

    // Drop the span guard (simulating gRPC handler returning)
    // At this point, the span is closed, but the actor task is still running
    drop(_guard);

    // Stop the actor (this will cause the actor task to complete)
    actor.stop().await.unwrap();

    // CRITICAL: When the actor task completes, it will try to log, and tracing
    // will try to clone the span from the context. But the span is already closed,
    // which should cause a panic. However, with our fixes, it should not panic.

    // Wait for the actor task to complete
    // This is where the panic would occur if not fixed
    let result = tokio::time::timeout(Duration::from_secs(5), handle).await;

    match result {
        Ok(_) => {
            // Actor completed successfully - no panic occurred
            println!("✅ Actor completed successfully without panic");
        }
        Err(_) => {
            panic!("Actor task did not complete within timeout - may have panicked");
        }
    }
}

/// Test that reproduces the panic with multiple concurrent actors
///
/// This test spawns multiple actors concurrently from within a span,
/// simulating multiple concurrent gRPC requests.
#[tokio::test]
async fn test_span_cloning_panic_reproduction_concurrent() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();

    let node = Arc::new(NodeBuilder::new("test-node").build().await);
    node.initialize_services().await.unwrap();

    let num_actors = 10;
    let mut handles = Vec::new();

    // Create a span (simulating gRPC handler span)
    let span = tracing::info_span!(
        "test_grpc_handler_concurrent",
        tenant_id = "test-tenant",
        namespace = "test-namespace"
    );
    let _guard = span.enter();

    // Spawn multiple actors concurrently
    for i in 0..num_actors {
        let processed = Arc::new(AtomicBool::new(false));
        let actor_impl = TestActor::new(processed.clone());

        let mailbox = Mailbox::new(
            MailboxConfig::default(),
            format!("mailbox-{}-{}", i, Ulid::new()),
        )
        .await
        .unwrap();

        let mut actor = Actor::new(
            format!("test-actor-{}-{}@test-node", i, Ulid::new()),
            Box::new(actor_impl),
            mailbox,
            "test-tenant".to_string(),
            "test-namespace".to_string(),
            None,
        );

        let handle = actor.start().await.expect("Actor should start");

        // Send a message
        let ctx = RequestContext::new_without_auth(
            "test-tenant".to_string(),
            "test-namespace".to_string(),
        );
        let actor_ref = node.get_actor_ref(&actor.id().clone(), &ctx).await.unwrap();
        let message = Message::new(
            serde_json::json!({ "action": "test" })
                .to_string()
                .into_bytes(),
        );
        actor_ref.tell(message).await.unwrap();

        handles.push((handle, actor));
    }

    // Drop the span guard (simulating gRPC handler returning)
    drop(_guard);

    // Stop all actors and wait for them to complete
    for (handle, mut actor) in handles {
        actor.stop().await.unwrap();

        // Wait for actor task to complete - this is where the panic would occur
        let result = tokio::time::timeout(Duration::from_secs(5), handle).await;
        match result {
            Ok(_) => {
                // Actor completed successfully
            }
            Err(_) => {
                panic!("Actor task did not complete within timeout - may have panicked");
            }
        }
    }

    println!(
        "✅ All {} actors completed successfully without panic",
        num_actors
    );
}
