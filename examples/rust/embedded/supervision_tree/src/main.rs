// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Supervision Tree Example
//
// Demonstrates Erlang/OTP-style fault tolerance with supervision trees:
// - Supervisor strategies: OneForOne, OneForAll, RestForOne
// - Automatic restart on failures
// - Hierarchical supervision (supervisor-of-supervisors)

use plexspaces_actor::supervisor::{Supervisor, SupervisorEvent, SupervisionStrategy};
use plexspaces_actor::child_spec::{ChildSpec, RestartStrategy, StartFn, StartedChild};
use plexspaces_actor::Actor;
use plexspaces_behavior::MockBehavior;
use plexspaces_core::{ActorError, ActorRef as CoreActorRef, RequestContext};
use plexspaces_mailbox::{Mailbox, MailboxConfig};
use plexspaces_node::NodeBuilder;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

// =============================================================================
// Main
// =============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║           Supervision Tree Example                             ║");
    println!("╚════════════════════════════════════════════════════════════════╝");
    println!();

    // Create node (services auto-initialized)
    let node = Arc::new(NodeBuilder::new("supervision-node").build().await);
    let service_locator = node.service_locator();

    // Create request context with tenant/namespace (REQUIRED - explicit)
    let _ctx = RequestContext::new_without_auth(
        "example-tenant".to_string(),
        "supervision-demo".to_string(),
    );

    // =========================================================================
    // Example 1: Basic Supervisor with OneForOne Strategy
    // =========================================================================
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Example 1: Basic Supervisor (OneForOne Strategy)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    println!("OneForOne: If a child fails, only that child is restarted.");
    println!();

    let (supervisor, mut event_rx) = Supervisor::new(
        "one-for-one-supervisor".to_string(),
        SupervisionStrategy::OneForOne {
            max_restarts: 5,
            within_seconds: 60,
        },
        service_locator.clone(),
    );

    // Add 3 workers
    for i in 1..=3 {
        let worker_id = format!("worker-{}@supervision-node", i);
        let spec = create_worker_spec(&worker_id, RestartStrategy::Permanent);
        
        supervisor.add_child(spec).await?;
        
        // Wait for ChildStarted event
        if let Some(SupervisorEvent::ChildStarted(id)) = event_rx.recv().await {
            println!("  ✓ Worker started: {}", id);
        }
    }

    // Show supervisor stats
    let stats = supervisor.stats().await;
    println!();
    println!("  Supervisor stats:");
    println!("    - Total restarts: {}", stats.total_restarts);
    println!("    - Successful restarts: {}", stats.successful_restarts);
    println!();

    // Shutdown
    let mut supervisor = supervisor;
    supervisor.shutdown().await?;
    println!("  ✓ Supervisor shutdown complete");
    println!();

    // =========================================================================
    // Example 2: Failure Recovery (automatic restart)
    // =========================================================================
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Example 2: Failure Recovery (Automatic Restart)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    println!("Simulating a failure and observing automatic restart...");
    println!();

    let (supervisor, mut event_rx) = Supervisor::new(
        "recovery-supervisor".to_string(),
        SupervisionStrategy::OneForOne {
            max_restarts: 3,
            within_seconds: 10,
        },
        service_locator.clone(),
    );

    // Add a worker
    let worker_id = "crashable-worker@supervision-node".to_string();
    let spec = create_worker_spec(&worker_id, RestartStrategy::Permanent);
    supervisor.add_child(spec).await?;

    // Consume ChildStarted
    let _ = event_rx.recv().await;
    println!("  ✓ Worker started: {}", worker_id);

    // Simulate failure
    println!("  → Simulating crash...");
    supervisor.handle_failure(&worker_id, "simulated crash".to_string(), None).await?;

    // Collect failure/restart events
    let timeout = sleep(Duration::from_millis(500));
    tokio::pin!(timeout);

    loop {
        tokio::select! {
            event = event_rx.recv() => {
                if let Some(event) = event {
                    match event {
                        SupervisorEvent::ChildFailed(id, reason) => {
                            println!("  ✗ Worker failed: {} (reason: {})", id, reason);
                        }
                        SupervisorEvent::ChildRestarted(id, count) => {
                            println!("  ✓ Worker restarted: {} (restart #{})", id, count);
                        }
                        _ => {}
                    }
                }
            }
            _ = &mut timeout => {
                break;
            }
        }
    }

    // Show recovery stats
    let stats = supervisor.stats().await;
    println!();
    println!("  Recovery stats:");
    println!("    - Total restarts: {}", stats.total_restarts);
    println!("    - Successful restarts: {}", stats.successful_restarts);
    println!("    - Failed restarts: {}", stats.failed_restarts);
    println!();

    let mut supervisor = supervisor;
    supervisor.shutdown().await?;
    println!("  ✓ Supervisor shutdown complete");
    println!();

    // =========================================================================
    // Example 3: OneForAll Strategy
    // =========================================================================
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Example 3: OneForAll Strategy");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    println!("OneForAll: If one child fails, ALL children are restarted.");
    println!("Use this when children depend on each other.");
    println!();

    let (supervisor, mut event_rx) = Supervisor::new(
        "one-for-all-supervisor".to_string(),
        SupervisionStrategy::OneForAll {
            max_restarts: 3,
            within_seconds: 60,
        },
        service_locator.clone(),
    );

    // Add workers
    for i in 1..=3 {
        let worker_id = format!("dependent-worker-{}@supervision-node", i);
        let spec = create_worker_spec(&worker_id, RestartStrategy::Permanent);
        supervisor.add_child(spec).await?;
        let _ = event_rx.recv().await;
        println!("  ✓ Worker started: {}", worker_id);
    }
    println!();

    let mut supervisor = supervisor;
    supervisor.shutdown().await?;
    println!("  ✓ Supervisor shutdown complete");
    println!();

    // =========================================================================
    // Example 4: RestForOne Strategy
    // =========================================================================
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Example 4: RestForOne Strategy");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    println!("RestForOne: If a child fails, restart it AND all children started after it.");
    println!("Use this for ordered dependencies (e.g., B depends on A).");
    println!();

    let (supervisor, mut event_rx) = Supervisor::new(
        "rest-for-one-supervisor".to_string(),
        SupervisionStrategy::RestForOne {
            max_restarts: 3,
            within_seconds: 60,
        },
        service_locator.clone(),
    );

    // Add workers in order (C depends on B, B depends on A)
    for name in &["database", "cache", "api"] {
        let worker_id = format!("{}-service@supervision-node", name);
        let spec = create_worker_spec(&worker_id, RestartStrategy::Permanent);
        supervisor.add_child(spec).await?;
        let _ = event_rx.recv().await;
        println!("  ✓ Service started: {} (order matters!)", name);
    }
    println!();

    let mut supervisor = supervisor;
    supervisor.shutdown().await?;
    println!("  ✓ Supervisor shutdown complete");
    println!();

    // =========================================================================
    // Done
    // =========================================================================
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("✅ Supervision Tree Example Complete!");
    println!();
    println!("Key Takeaways:");
    println!("  • OneForOne: Isolate failures (most common)");
    println!("  • OneForAll: For tightly coupled children");
    println!("  • RestForOne: For ordered dependencies");
    println!("  • Permanent: Always restart");
    println!("  • Transient: Restart only on crash (not normal exit)");
    println!("  • Temporary: Never restart");
    println!();

    Ok(())
}

// =============================================================================
// Helper: Create worker ChildSpec
// =============================================================================

/// Create a ChildSpec for a supervised worker
fn create_worker_spec(worker_id: &str, restart: RestartStrategy) -> ChildSpec {
    let id = worker_id.to_string();
    let id_for_factory = id.clone();
    
    // Create async factory for the worker
    let start_fn: StartFn = Arc::new(move || {
        let actor_id = id_for_factory.clone();
        Box::pin(async move {
            // Create mailbox
            let mailbox = Mailbox::new(MailboxConfig::default(), actor_id.clone())
                .await
                .map_err(|e| ActorError::InvalidState(e.to_string()))?;
            
            // Create actor with mock behavior (real apps would use custom behavior)
            let actor = Actor::new(
                actor_id.clone(),
                Box::new(MockBehavior::new()),
                mailbox,
                "example-tenant".to_string(),
                "supervision-demo".to_string(),
                None,
            );
            
            // Create actor reference
            let actor_ref = CoreActorRef::new(actor_id)
                .map_err(|e| ActorError::InvalidState(e.to_string()))?;
            
            Ok(StartedChild::Worker { actor, actor_ref })
        })
    });
    
    ChildSpec::worker(id.clone(), id, start_fn)
        .with_restart(restart)
}
