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

//! Lifecycle Event Observability Example
//!
//! ## Purpose
//! Demonstrates the JavaNOW-inspired channel/subscriber pattern for actor lifecycle
//! event notification. Shows how observability backends (Prometheus, StatsD,
//! OpenTelemetry) can subscribe to and process lifecycle events for metrics collection.
//!
//! ## Architecture
//! ```
//! Node (Publisher)
//!   |
//!   |-- Lifecycle Events -->|
//!   |                       |
//!   |                       +--> Prometheus Exporter (metrics::counter!)
//!   |                       |
//!   |                       +--> StatsD Forwarder (UDP batching)
//!   |                       |
//!   |                       +--> OpenTelemetry (distributed tracing)
//! ```
//!
//! ## Usage
//! ```bash
//! cargo run --example lifecycle_metrics
//! ```
//!
//! ## Expected Output
//! ```
//! [Prometheus Exporter] Actor spawned: worker-1@node1
//! [Prometheus Exporter] Actor spawned: worker-2@node1
//! [Prometheus Exporter] Actor spawned: worker-3@node1
//! [Prometheus Exporter] Actor terminated: worker-1@node1 (reason: normal)
//! [Prometheus Exporter] Actor terminated: worker-2@node1 (reason: normal)
//! [Prometheus Exporter] Actor terminated: worker-3@node1 (reason: normal)
//!
//! Final Metrics:
//! - Actors spawned: 3
//! - Actors terminated: 3
//! - Active actors: 0
//! ```

use plexspaces_behavior::MockBehavior;
use plexspaces_node::{NodeBuilder};
use plexspaces_proto::ActorLifecycleEvent;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;

/// Prometheus-style metrics exporter (simplified)
///
/// In a real system, this would use the `metrics` crate or Prometheus client library
/// to export metrics via HTTP /metrics endpoint.
struct PrometheusExporter {
    actor_spawn_total: Arc<AtomicU64>,
    actor_terminated_total: Arc<AtomicU64>,
    actor_failed_total: Arc<AtomicU64>,
}

impl PrometheusExporter {
    fn new() -> Self {
        Self {
            actor_spawn_total: Arc::new(AtomicU64::new(0)),
            actor_terminated_total: Arc::new(AtomicU64::new(0)),
            actor_failed_total: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Process lifecycle events and convert to metrics
    ///
    /// ## Real-World Equivalent
    /// ```ignore
    /// metrics::counter!("plexspaces_actor_spawn_total", "node_id" => &node_id).increment(1);
    /// metrics::gauge!("plexspaces_actor_active").set(active_count as f64);
    /// metrics::counter!("plexspaces_actor_error_total", "reason" => &error).increment(1);
    /// ```
    async fn process_events(&self, mut event_rx: mpsc::UnboundedReceiver<ActorLifecycleEvent>) {
        use plexspaces_proto::actor_lifecycle_event::EventType;

        while let Some(event) = event_rx.recv().await {
            match event.event_type {
                Some(EventType::Created(_)) => {
                    // Actor spawned - increment counter
                    self.actor_spawn_total.fetch_add(1, Ordering::Relaxed);
                    println!("[Prometheus Exporter] Actor spawned: {}", event.actor_id);
                }
                Some(EventType::Terminated(terminated)) => {
                    // Actor terminated - increment counter
                    self.actor_terminated_total.fetch_add(1, Ordering::Relaxed);
                    println!(
                        "[Prometheus Exporter] Actor terminated: {} (reason: {})",
                        event.actor_id, terminated.reason
                    );
                }
                Some(EventType::Failed(failed)) => {
                    // Actor failed - increment error counter
                    self.actor_failed_total.fetch_add(1, Ordering::Relaxed);
                    println!(
                        "[Prometheus Exporter] Actor failed: {} (error: {})",
                        event.actor_id, failed.error
                    );
                }
                _ => {
                    // Other lifecycle events (Activated, Deactivating, etc.)
                    // Could be used for additional metrics or tracing
                }
            }
        }
    }

    fn print_metrics(&self) {
        println!("\n=== Final Metrics (Prometheus Format) ===");
        println!(
            "plexspaces_actor_spawn_total: {}",
            self.actor_spawn_total.load(Ordering::Relaxed)
        );
        println!(
            "plexspaces_actor_terminated_total: {}",
            self.actor_terminated_total.load(Ordering::Relaxed)
        );
        println!(
            "plexspaces_actor_failed_total: {}",
            self.actor_failed_total.load(Ordering::Relaxed)
        );
        println!(
            "plexspaces_actor_active: {}",
            self.actor_spawn_total.load(Ordering::Relaxed) as i64
                - self.actor_terminated_total.load(Ordering::Relaxed) as i64
        );
        println!("=========================================\n");
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("PlexSpaces Lifecycle Event Observability Example\n");
    println!("This example demonstrates JavaNOW-inspired pub/sub for actor lifecycle events.\n");

    // Create node using NodeBuilder
    let node = Arc::new(NodeBuilder::new("node1").build());

    // Create Prometheus exporter
    let prometheus = Arc::new(PrometheusExporter::new());

    // Create subscription channel
    let (event_tx, event_rx) = mpsc::unbounded_channel();

    // Subscribe Prometheus exporter to lifecycle events
    node.subscribe_lifecycle_events(event_tx).await;

    // Spawn background task to process events
    let prometheus_clone = prometheus.clone();
    let metrics_handle = tokio::spawn(async move {
        prometheus_clone.process_events(event_rx).await;
    });

    // Spawn several actors
    println!("Spawning 3 worker actors...\n");

    use plexspaces_actor::ActorBuilder;
    let mut actors = Vec::new();
    for i in 1..=3 {
        let actor_ref = ActorBuilder::new(Box::new(MockBehavior::new()))
            .with_id(format!("worker-{}@{}", i, node.id().as_str()))
            .spawn(node.service_locator().clone())
            .await?;
        actors.push(actor_ref);
    }

    // Let actors run briefly
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Terminate all actors (drop their refs)
    println!("\nTerminating actors...\n");
    drop(actors);

    // Wait for lifecycle events to be processed
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    // Print final metrics
    prometheus.print_metrics();

    // Unsubscribe and shutdown
    node.unsubscribe_lifecycle_events().await;
    metrics_handle.abort();

    println!("Example completed successfully!");

    Ok(())
}
