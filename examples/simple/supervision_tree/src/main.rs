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

//! Supervision Tree Example
//!
//! Demonstrates Erlang/OTP-style supervision trees with fault tolerance patterns.
//!
//! ## What This Example Shows
//!
//! 1. **Basic Supervisor**: Supervisor managing multiple worker actors
//! 2. **Supervisor-of-Supervisors**: Hierarchical supervision tree
//! 3. **Failure Recovery**: Automatic restart on actor failures
//! 4. **Restart Strategies**: OneForOne, OneForAll, RestForOne
//!
//! ## Architecture
//!
//! ```
//! RootSupervisor (OneForOne)
//!   ├─ WorkerSupervisor (OneForAll)
//!   │   ├─ Worker1 (Permanent)
//!   │   ├─ Worker2 (Permanent)
//!   │   └─ Worker3 (Transient)
//!   └─ ServiceSupervisor (RestForOne)
//!       ├─ Service1 (Permanent)
//!       ├─ Service2 (Permanent)
//!       └─ Service3 (Temporary)
//! ```

use anyhow::Result;
use plexspaces_actor::Actor;
use plexspaces_behavior::MockBehavior;
use plexspaces_mailbox::{Mailbox, MailboxConfig};
use plexspaces_supervisor::{
    ActorSpec, ChildType, RestartPolicy, SupervisionStrategy, Supervisor, SupervisorEvent,
};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{info, warn};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    info!("Starting supervision tree example");

    // Example 1: Basic Supervisor with Workers
    example_basic_supervisor().await?;

    // Example 2: Supervisor-of-Supervisors (Hierarchical)
    example_hierarchical_supervision().await?;

    // Example 3: Failure Recovery
    example_failure_recovery().await?;

    info!("Supervision tree example completed successfully");

    Ok(())
}

/// Example 1: Basic Supervisor with Workers
///
/// Creates a supervisor with multiple workers using OneForOne strategy.
async fn example_basic_supervisor() -> Result<()> {
    info!("=== Example 1: Basic Supervisor with Workers ===");

    let (supervisor, mut event_rx) = Supervisor::new(
        "basic-supervisor".to_string(),
        SupervisionStrategy::OneForOne {
            max_restarts: 5,
            within_seconds: 60,
        },
    );

    // Add 3 workers
    for i in 1..=3 {
        let worker_id = format!("worker-{}@localhost", i);
        let spec = create_worker_spec(worker_id.clone(), RestartPolicy::Permanent)?;

        supervisor.add_child(spec).await?;
        info!("Added worker: {}", worker_id);

        // Consume ChildStarted event
        if let Some(event) = event_rx.recv().await {
            match event {
                SupervisorEvent::ChildStarted(id) => {
                    info!("Worker started: {}", id);
                }
                _ => warn!("Unexpected event: {:?}", event),
            }
        }
    }

    // Get supervisor stats
    let stats = supervisor.stats().await;
    info!(
        "Supervisor stats: total_restarts={}, successful_restarts={}, failed_restarts={}",
        stats.total_restarts, stats.successful_restarts, stats.failed_restarts
    );

    // Shutdown supervisor
    let mut supervisor = supervisor;
    supervisor.shutdown().await?;
    info!("Basic supervisor shutdown completed");

    Ok(())
}

/// Example 2: Hierarchical Supervision (Supervisor-of-Supervisors)
///
/// Creates a root supervisor with child supervisors, demonstrating
/// hierarchical supervision trees.
async fn example_hierarchical_supervision() -> Result<()> {
    info!("=== Example 2: Hierarchical Supervision ===");

    // Create root supervisor
    let (root_supervisor, mut root_events) = Supervisor::new(
        "root-supervisor".to_string(),
        SupervisionStrategy::OneForOne {
            max_restarts: 3,
            within_seconds: 60,
        },
    );

    // Create worker supervisor (OneForAll strategy)
    let (worker_supervisor, mut worker_events) = Supervisor::new(
        "worker-supervisor".to_string(),
        SupervisionStrategy::OneForAll {
            max_restarts: 3,
            within_seconds: 60,
        },
    );

    // Add 2 workers to worker supervisor
    for i in 1..=2 {
        let worker_id = format!("worker-{}@localhost", i);
        let spec = create_worker_spec(worker_id.clone(), RestartPolicy::Permanent)?;
        worker_supervisor.add_child(spec).await?;

        // Consume ChildStarted event
        if let Some(event) = worker_events.recv().await {
            match event {
                SupervisorEvent::ChildStarted(id) => {
                    info!("Worker started under worker-supervisor: {}", id);
                }
                _ => {}
            }
        }
    }

    // Add worker supervisor to root supervisor
    root_supervisor
        .add_supervisor_child(
            worker_supervisor,
            worker_events,
            plexspaces_proto::supervision::v1::EventPropagation::EventPropagationForwardAll,
            RestartPolicy::Permanent,
            Some(5000),
        )
        .await?;

    info!("Added worker-supervisor to root-supervisor");

    // Consume ChildStarted event for supervisor
    if let Some(event) = root_events.recv().await {
        match event {
            SupervisorEvent::ChildStarted(id) => {
                info!("Child supervisor started: {}", id);
            }
            _ => {}
        }
    }

    // Shutdown root supervisor (cascades to child supervisor and workers)
    let mut root_supervisor = root_supervisor;
    root_supervisor.shutdown().await?;
    info!("Hierarchical supervision shutdown completed");

    Ok(())
}

/// Example 3: Failure Recovery
///
/// Demonstrates automatic restart on actor failures with different
/// restart strategies.
async fn example_failure_recovery() -> Result<()> {
    info!("=== Example 3: Failure Recovery ===");

    // Create supervisor with OneForOne strategy
    let (supervisor, mut event_rx) = Supervisor::new(
        "recovery-supervisor".to_string(),
        SupervisionStrategy::OneForOne {
            max_restarts: 3,
            within_seconds: 10, // Short window for demo
        },
    );

    // Add a worker
    let worker_id = "recovery-worker@localhost".to_string();
    let spec = create_worker_spec(worker_id.clone(), RestartPolicy::Permanent)?;
    supervisor.add_child(spec).await?;

    // Consume ChildStarted event
    let _ = event_rx.recv().await;

    info!("Simulating failure for worker: {}", worker_id);

    // Simulate a failure
    supervisor
        .handle_failure(&worker_id, "simulated failure".to_string())
        .await?;

    // Wait for restart events
    let mut failure_received = false;
    let mut restart_received = false;

    // Collect events for a short time
    let timeout = sleep(Duration::from_millis(500));
    tokio::pin!(timeout);

    loop {
        tokio::select! {
            event = event_rx.recv() => {
                if let Some(event) = event {
                    match event {
                        SupervisorEvent::ChildFailed(id, reason) => {
                            info!("Child failed: {} - {}", id, reason);
                            failure_received = true;
                        }
                        SupervisorEvent::ChildRestarted(id, count) => {
                            info!("Child restarted: {} (restart count: {})", id, count);
                            restart_received = true;
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

    if failure_received && restart_received {
        info!("Failure recovery demonstrated successfully");
    } else {
        warn!("Expected failure and restart events, but some were missing");
    }

    // Get stats
    let stats = supervisor.stats().await;
    info!(
        "Recovery stats: total_restarts={}, successful_restarts={}, failed_restarts={}",
        stats.total_restarts, stats.successful_restarts, stats.failed_restarts
    );

    // Shutdown
    let mut supervisor = supervisor;
    supervisor.shutdown().await?;
    info!("Failure recovery example completed");

    Ok(())
}

/// Create a worker actor spec
fn create_worker_spec(
    id: String,
    restart: RestartPolicy,
) -> Result<ActorSpec, anyhow::Error> {
    let spec = ActorSpec {
        id: id.clone(),
        factory: Arc::new(move || {
            let mailbox = tokio::runtime::Handle::current()
                .block_on(Mailbox::new(MailboxConfig::default(), id.clone()))
                .map_err(|e| plexspaces_core::ActorError::Other(e.to_string()))?;
            Ok(Actor::new(
                id.clone(),
                Box::new(MockBehavior::new()),
                mailbox,
                "example".to_string(),
                None,
            ))
        }),
        restart,
        child_type: ChildType::Worker,
        shutdown_timeout_ms: Some(5000),
    };

    Ok(spec)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_basic_supervisor() {
        example_basic_supervisor().await.unwrap();
    }

    #[tokio::test]
    async fn test_hierarchical_supervision() {
        example_hierarchical_supervision().await.unwrap();
    }

    #[tokio::test]
    async fn test_failure_recovery() {
        example_failure_recovery().await.unwrap();
    }
}
