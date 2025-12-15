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

//! # Graceful Shutdown Coordinator
//!
//! ## Purpose
//! Orchestrates graceful shutdown of PlexSpaces nodes, following the VictoriaMetrics
//! pattern for proper signal handling and coordinated cleanup.
//!
//! ## Architecture Context
//! This module provides application-level shutdown coordination:
//! - Listens for OS signals (SIGTERM, SIGINT, SIGHUP)
//! - Coordinates shutdown across all node components
//! - Works with HealthReporter for request draining
//! - Ensures proper cleanup order
//!
//! ## Design
//! Inspired by https://victoriametrics.com/blog/go-graceful-shutdown/
//!
//! Uses proto-defined types from `system.proto`:
//! - `ShutdownSignal`: OS signal mapping
//! - `ShutdownPhase`: State machine tracking
//! - `ShutdownStatus`: Detailed observability
//!
//! ## Usage
//! ```rust,ignore
//! use plexspaces_node::{Node, ShutdownCoordinator};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Create node
//!     let node = Node::new(config).await?;
//!
//!     // Create shutdown coordinator
//!     let coordinator = ShutdownCoordinator::new();
//!
//!     // Spawn signal handler
//!     let shutdown_rx = coordinator.listen_for_signals();
//!
//!     // Start node
//!     node.start().await?;
//!
//!     // Wait for shutdown signal
//!     let signal = shutdown_rx.await.unwrap();
//!     eprintln!("Received signal: {:?}", signal);
//!
//!     // Graceful shutdown
//!     coordinator.shutdown(&node).await?;
//!
//!     Ok(())
//! }
//! ```

use plexspaces_proto::system::v1::{
    ShutdownPhase, ShutdownPhaseStatus, ShutdownSignal, ShutdownStatus,
};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::Instant;

/// Graceful shutdown coordinator
///
/// ## Purpose
/// Coordinates graceful shutdown of a PlexSpaces node with proper signal handling.
///
/// ## Design Notes
/// - Follows VictoriaMetrics pattern for signal handling
/// - Integrates with K8s graceful shutdown (terminationGracePeriodSeconds)
/// - Ensures all components shut down in correct order
/// - Tracks shutdown progress using proto ShutdownStatus
///
/// ## Shutdown Sequence
/// 1. Receive signal (SIGTERM/SIGINT/SIGHUP)
/// 2. Set health to NOT_SERVING (K8s removes from load balancer)
/// 3. Drain in-flight requests (timeout: 30s)
/// 4. Stop accepting new actors
/// 5. Stop all running actors (graceful)
/// 6. Close gRPC connections
/// 7. Shutdown TupleSpace
/// 8. Final cleanup
/// 9. Exit with code 0
pub struct ShutdownCoordinator {
    /// Current shutdown status (proto-defined)
    status: Arc<Mutex<ShutdownStatus>>,
    /// Shutdown start time (for elapsed tracking)
    shutdown_started: Arc<Mutex<Option<Instant>>>,
}

impl ShutdownCoordinator {
    /// Create new shutdown coordinator
    pub fn new() -> Self {
        let initial_status = ShutdownStatus {
            phase: ShutdownPhase::ShutdownPhaseRunning as i32,
            signal: ShutdownSignal::ShutdownSignalUnspecified as i32,
            shutdown_started_at: None,
            elapsed: None,
            health_not_serving_status: None,
            draining_status: None,
            stopping_actors_status: None,
            closing_connections_status: None,
            shutting_down_tuplespace_status: None,
            final_cleanup_status: None,
            completed: false,
            error: String::new(),
        };

        Self {
            status: Arc::new(Mutex::new(initial_status)),
            shutdown_started: Arc::new(Mutex::new(None)),
        }
    }

    /// Listen for OS signals and return shutdown receiver
    ///
    /// ## Purpose
    /// Spawns background task listening for SIGTERM, SIGINT, SIGHUP.
    /// Returns a oneshot receiver that fires when shutdown signal received.
    ///
    /// ## Returns
    /// Receiver that completes when shutdown signal received
    ///
    /// ## Examples
    /// ```rust,ignore
    /// let coordinator = ShutdownCoordinator::new();
    /// let shutdown_rx = coordinator.listen_for_signals();
    ///
    /// // Start node...
    ///
    /// // Wait for shutdown
    /// let signal = shutdown_rx.await.unwrap();
    /// coordinator.shutdown(&node).await?;
    /// ```
    pub fn listen_for_signals(&self) -> tokio::sync::oneshot::Receiver<ShutdownSignal> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let status = self.status.clone();

        tokio::spawn(async move {
            use tokio::signal::unix::{signal, SignalKind};

            // Create signal handlers
            let mut sigterm =
                signal(SignalKind::terminate()).expect("Failed to create SIGTERM handler");
            let mut sigint =
                signal(SignalKind::interrupt()).expect("Failed to create SIGINT handler");
            let mut sighup = signal(SignalKind::hangup()).expect("Failed to create SIGHUP handler");

            let shutdown_signal = tokio::select! {
                _ = sigterm.recv() => {
                    eprintln!("📡 Received SIGTERM, initiating graceful shutdown...");
                    ShutdownSignal::ShutdownSignalSigterm
                }
                _ = sigint.recv() => {
                    eprintln!("📡 Received SIGINT (Ctrl+C), initiating graceful shutdown...");
                    ShutdownSignal::ShutdownSignalSigint
                }
                _ = sighup.recv() => {
                    eprintln!("📡 Received SIGHUP, initiating graceful shutdown...");
                    ShutdownSignal::ShutdownSignalSighup
                }
            };

            // Store signal in status (clone before using twice)
            let signal_clone1 = shutdown_signal.clone();
            let signal_clone2 = shutdown_signal.clone();
            let signal_value = signal_clone1 as i32;
            let mut status = status.lock().await;
            status.signal = signal_value;

            // Notify main thread
            let _ = tx.send(signal_clone2);
        });

        rx
    }

    /// Simulate signal reception for testing (internal use)
    ///
    /// ## Purpose
    /// Allows testing signal handling without sending actual OS signals.
    /// This is used by tests to trigger shutdown without killing the test process.
    #[cfg(test)]
    pub(crate) async fn simulate_signal(&self, signal: ShutdownSignal) {
        let mut status = self.status.lock().await;
        status.signal = signal as i32;
    }

    /// Execute graceful shutdown sequence
    ///
    /// ## Purpose
    /// Orchestrates complete shutdown of PlexSpaces node following the
    /// proto-defined shutdown sequence.
    ///
    /// ## Arguments
    /// * `node` - Node to shut down (must implement ShutdownTarget)
    ///
    /// ## Returns
    /// `Ok(())` on successful shutdown, `Err` if timeout or failure
    ///
    /// ## Errors
    /// - Timeout during any phase
    /// - Component shutdown failure
    ///
    /// ## Examples
    /// ```rust,ignore
    /// coordinator.shutdown(&node).await?;
    /// ```
    pub async fn shutdown<N>(&self, node: &N) -> Result<(), ShutdownError>
    where
        N: ShutdownTarget,
    {
        // Mark shutdown started
        let now = std::time::SystemTime::now();
        *self.shutdown_started.lock().await = Some(Instant::now());

        let mut status = self.status.lock().await;
        status.shutdown_started_at = Some(prost_types::Timestamp::from(now));
        drop(status);

        eprintln!("🛑 Starting graceful shutdown sequence...");

        // Phase 1: Set health to NOT_SERVING
        self.execute_phase(
            ShutdownPhase::ShutdownPhaseHealthNotServing,
            "Setting health to NOT_SERVING (K8s removes from load balancer)",
            || async { node.set_health_not_serving().await },
        )
        .await?;

        // Phase 2: Drain requests
        self.execute_phase(
            ShutdownPhase::ShutdownPhaseDraining,
            "Draining in-flight requests",
            || async { node.drain_requests().await },
        )
        .await?;

        // Phase 3: Stop actors
        self.execute_phase(
            ShutdownPhase::ShutdownPhaseStoppingActors,
            "Stopping all actors",
            || async { node.stop_all_actors().await },
        )
        .await?;

        // Phase 4: Close connections
        self.execute_phase(
            ShutdownPhase::ShutdownPhaseClosingConnections,
            "Closing gRPC connections",
            || async { node.close_all_connections().await },
        )
        .await?;

        // Phase 5: Shutdown TupleSpace
        self.execute_phase(
            ShutdownPhase::ShutdownPhaseShuttingDownTuplespace,
            "Shutting down TupleSpace",
            || async { node.shutdown_tuplespace().await },
        )
        .await?;

        // Phase 6: Final cleanup
        self.execute_phase(ShutdownPhase::ShutdownPhaseFinalCleanup, "Final cleanup", || async {
            node.final_cleanup().await
        })
        .await?;

        // Mark complete
        let mut status = self.status.lock().await;
        status.phase = ShutdownPhase::ShutdownPhaseComplete as i32;
        status.completed = true;
        status.elapsed = self.get_elapsed_proto().await;
        drop(status);

        let elapsed = self.shutdown_started.lock().await.unwrap().elapsed();
        eprintln!("✅ Graceful shutdown complete in {:?}", elapsed);

        Ok(())
    }

    /// Execute a single shutdown phase
    async fn execute_phase<F, Fut>(
        &self,
        phase: ShutdownPhase,
        description: &str,
        operation: F,
    ) -> Result<(), ShutdownError>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<(), String>>,
    {
        // Clone phase before using it multiple times (enums implement Clone but not Copy)
        let phase_clone1 = phase.clone();
        let phase_clone2 = phase.clone();
        let phase_clone3 = phase.clone();
        let phase_value = phase_clone1 as i32;
        let phase_num = self.get_phase_number(phase_clone3);
        eprintln!("  {}/7 {}", phase_num, description);

        // Update current phase
        let mut status = self.status.lock().await;
        status.phase = phase_value;
        drop(status);

        // Create phase status
        let start_time = Instant::now();
        let start_timestamp = std::time::SystemTime::now();

        // Execute operation
        let result = operation().await;

        // Record phase completion
        let end_timestamp = std::time::SystemTime::now();
        let duration = start_time.elapsed();

        let phase_status = ShutdownPhaseStatus {
            phase: phase_value,
            completed: result.is_ok(),
            started_at: Some(prost_types::Timestamp::from(start_timestamp)),
            completed_at: Some(prost_types::Timestamp::from(end_timestamp)),
            duration: Some(prost_types::Duration::try_from(duration).unwrap_or_default()),
            error: result.as_ref().err().cloned().unwrap_or_default(),
            details: None,
        };

        // Store phase status (use second clone for match)
        let mut status = self.status.lock().await;
        match phase_clone2 {
            ShutdownPhase::ShutdownPhaseHealthNotServing => {
                status.health_not_serving_status = Some(phase_status);
            }
            ShutdownPhase::ShutdownPhaseDraining => {
                status.draining_status = Some(phase_status);
            }
            ShutdownPhase::ShutdownPhaseStoppingActors => {
                status.stopping_actors_status = Some(phase_status);
            }
            ShutdownPhase::ShutdownPhaseClosingConnections => {
                status.closing_connections_status = Some(phase_status);
            }
            ShutdownPhase::ShutdownPhaseShuttingDownTuplespace => {
                status.shutting_down_tuplespace_status = Some(phase_status);
            }
            ShutdownPhase::ShutdownPhaseFinalCleanup => {
                status.final_cleanup_status = Some(phase_status);
            }
            _ => {}
        }
        drop(status);

        result.map_err(|e| ShutdownError::PhaseFailed {
            phase: format!("{:?}", phase_clone2),
            error: e,
        })
    }

    /// Get phase number for display (1-7)
    fn get_phase_number(&self, phase: ShutdownPhase) -> u8 {
        match phase {
            ShutdownPhase::ShutdownPhaseHealthNotServing => 1,
            ShutdownPhase::ShutdownPhaseDraining => 2,
            ShutdownPhase::ShutdownPhaseStoppingActors => 3,
            ShutdownPhase::ShutdownPhaseClosingConnections => 4,
            ShutdownPhase::ShutdownPhaseShuttingDownTuplespace => 5,
            ShutdownPhase::ShutdownPhaseFinalCleanup => 6,
            ShutdownPhase::ShutdownPhaseComplete => 7,
            _ => 0,
        }
    }

    /// Get current shutdown status (proto message)
    ///
    /// ## Purpose
    /// Returns proto ShutdownStatus for observability/monitoring.
    ///
    /// ## Returns
    /// Complete shutdown status with all phase details
    pub async fn get_status(&self) -> ShutdownStatus {
        let mut status = self.status.lock().await.clone();
        status.elapsed = self.get_elapsed_proto().await;
        status
    }

    /// Get shutdown elapsed time as proto Duration
    async fn get_elapsed_proto(&self) -> Option<prost_types::Duration> {
        self.shutdown_started.lock().await.map(|start| {
            let elapsed = start.elapsed();
            prost_types::Duration::try_from(elapsed).unwrap_or_default()
        })
    }
}

impl Default for ShutdownCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

/// Error during shutdown
#[derive(Debug, thiserror::Error)]
pub enum ShutdownError {
    /// Phase execution failed
    #[error("Shutdown phase {phase} failed: {error}")]
    /// Phase failed during shutdown
    PhaseFailed { 
        /// Name of the phase that failed
        phase: String, 
        /// Error message
        error: String 
    },

    /// Timeout during shutdown
    #[error("Shutdown timeout")]
    Timeout,

    /// Other error
    #[error("Shutdown error: {0}")]
    Other(String),
}

/// Trait for types that support graceful shutdown
///
/// ## Purpose
/// Abstracts shutdown operations to allow testing and different node implementations.
pub trait ShutdownTarget: Send + Sync {
    /// Set health to NOT_SERVING
    fn set_health_not_serving(
        &self,
    ) -> impl std::future::Future<Output = Result<(), String>> + Send;

    /// Drain in-flight requests
    fn drain_requests(&self) -> impl std::future::Future<Output = Result<(), String>> + Send;

    /// Stop all actors gracefully
    fn stop_all_actors(&self) -> impl std::future::Future<Output = Result<(), String>> + Send;

    /// Close all gRPC connections
    fn close_all_connections(&self)
        -> impl std::future::Future<Output = Result<(), String>> + Send;

    /// Shutdown TupleSpace
    fn shutdown_tuplespace(&self) -> impl std::future::Future<Output = Result<(), String>> + Send;

    /// Final cleanup
    fn final_cleanup(&self) -> impl std::future::Future<Output = Result<(), String>> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockNode {
        fail_phase: Option<ShutdownPhase>,
    }

    impl MockNode {
        fn new() -> Self {
            Self { fail_phase: None }
        }

        fn with_failure(phase: ShutdownPhase) -> Self {
            Self {
                fail_phase: Some(phase),
            }
        }
    }

    impl ShutdownTarget for MockNode {
        async fn set_health_not_serving(&self) -> Result<(), String> {
            tokio::time::sleep(Duration::from_millis(10)).await;
            if self.fail_phase == Some(ShutdownPhase::ShutdownPhaseHealthNotServing) {
                return Err("Health service failed".to_string());
            }
            Ok(())
        }

        async fn drain_requests(&self) -> Result<(), String> {
            tokio::time::sleep(Duration::from_millis(10)).await;
            if self.fail_phase == Some(ShutdownPhase::ShutdownPhaseDraining) {
                return Err("Drain timeout".to_string());
            }
            Ok(())
        }

        async fn stop_all_actors(&self) -> Result<(), String> {
            tokio::time::sleep(Duration::from_millis(10)).await;
            if self.fail_phase == Some(ShutdownPhase::ShutdownPhaseStoppingActors) {
                return Err("Actor stop failed".to_string());
            }
            Ok(())
        }

        async fn close_all_connections(&self) -> Result<(), String> {
            tokio::time::sleep(Duration::from_millis(10)).await;
            if self.fail_phase == Some(ShutdownPhase::ShutdownPhaseClosingConnections) {
                return Err("Connection close failed".to_string());
            }
            Ok(())
        }

        async fn shutdown_tuplespace(&self) -> Result<(), String> {
            tokio::time::sleep(Duration::from_millis(10)).await;
            if self.fail_phase == Some(ShutdownPhase::ShutdownPhaseShuttingDownTuplespace) {
                return Err("TupleSpace shutdown failed".to_string());
            }
            Ok(())
        }

        async fn final_cleanup(&self) -> Result<(), String> {
            tokio::time::sleep(Duration::from_millis(10)).await;
            if self.fail_phase == Some(ShutdownPhase::ShutdownPhaseFinalCleanup) {
                return Err("Cleanup failed".to_string());
            }
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_successful_shutdown() {
        let coordinator = ShutdownCoordinator::new();
        let node = MockNode::new();

        let result = coordinator.shutdown(&node).await;
        assert!(result.is_ok());

        let status = coordinator.get_status().await;
        assert_eq!(status.phase, ShutdownPhase::ShutdownPhaseComplete as i32);
        assert!(status.completed);
        assert!(status.error.is_empty());
        assert!(status.elapsed.is_some());
    }

    #[tokio::test]
    async fn test_shutdown_phase_tracking() {
        let coordinator = ShutdownCoordinator::new();
        let node = MockNode::new();

        coordinator.shutdown(&node).await.unwrap();

        let status = coordinator.get_status().await;

        // All phases should have completed
        assert!(status.health_not_serving_status.is_some());
        assert!(status.draining_status.is_some());
        assert!(status.stopping_actors_status.is_some());
        assert!(status.closing_connections_status.is_some());
        assert!(status.shutting_down_tuplespace_status.is_some());
        assert!(status.final_cleanup_status.is_some());

        // All phases should be marked completed
        assert!(status.health_not_serving_status.unwrap().completed);
        assert!(status.draining_status.unwrap().completed);
    }

    #[tokio::test]
    async fn test_shutdown_failure_in_drain_phase() {
        let coordinator = ShutdownCoordinator::new();
        let node = MockNode::with_failure(ShutdownPhase::ShutdownPhaseDraining);

        let result = coordinator.shutdown(&node).await;
        assert!(result.is_err());

        let status = coordinator.get_status().await;
        assert!(!status.completed);

        // First phase should succeed
        assert!(status.health_not_serving_status.is_some());
        assert!(status.health_not_serving_status.unwrap().completed);

        // Drain phase should fail
        assert!(status.draining_status.is_some());
        let drain_status = status.draining_status.unwrap();
        assert!(!drain_status.completed);
        assert_eq!(drain_status.error, "Drain timeout");
    }

    #[tokio::test]
    async fn test_signal_simulation() {
        // Test signal handling using simulate_signal (doesn't kill test process)
        let coordinator = ShutdownCoordinator::new();

        // Simulate SIGTERM
        coordinator.simulate_signal(ShutdownSignal::ShutdownSignalSigterm).await;

        let status = coordinator.get_status().await;
        assert_eq!(status.signal, ShutdownSignal::ShutdownSignalSigterm as i32);
    }

    #[tokio::test]
    async fn test_different_signals() {
        // Test each signal type
        for (signal, expected) in [
            (ShutdownSignal::ShutdownSignalSigterm, ShutdownSignal::ShutdownSignalSigterm as i32),
            (ShutdownSignal::ShutdownSignalSigint, ShutdownSignal::ShutdownSignalSigint as i32),
            (ShutdownSignal::ShutdownSignalSighup, ShutdownSignal::ShutdownSignalSighup as i32),
        ] {
            let coordinator = ShutdownCoordinator::new();
            coordinator.simulate_signal(signal).await;

            let status = coordinator.get_status().await;
            assert_eq!(status.signal, expected);
        }
    }

    #[tokio::test]
    async fn test_shutdown_with_all_phases_successful() {
        let coordinator = ShutdownCoordinator::new();
        let node = MockNode::new();

        coordinator.shutdown(&node).await.unwrap();

        let status = coordinator.get_status().await;

        // All phase statuses should be present
        assert!(status.health_not_serving_status.is_some());
        assert!(status.draining_status.is_some());
        assert!(status.stopping_actors_status.is_some());
        assert!(status.closing_connections_status.is_some());
        assert!(status.shutting_down_tuplespace_status.is_some());
        assert!(status.final_cleanup_status.is_some());

        // All should be completed
        assert!(status.health_not_serving_status.unwrap().completed);
        assert!(status.draining_status.unwrap().completed);
    }

    #[tokio::test]
    async fn test_phase_error_details() {
        let coordinator = ShutdownCoordinator::new();
        let node = MockNode::with_failure(ShutdownPhase::ShutdownPhaseStoppingActors);

        let result = coordinator.shutdown(&node).await;
        assert!(result.is_err());

        let status = coordinator.get_status().await;

        // Check that the failed phase has error details
        let stopping_status = status.stopping_actors_status.unwrap();
        assert!(!stopping_status.completed);
        assert_eq!(stopping_status.error, "Actor stop failed");
    }

    #[tokio::test]
    async fn test_default_coordinator() {
        // Test Default trait
        let coordinator = ShutdownCoordinator::default();

        let status = coordinator.get_status().await;
        assert_eq!(status.phase, ShutdownPhase::ShutdownPhaseRunning as i32);
        assert!(!status.completed);
    }

    #[tokio::test]
    async fn test_shutdown_elapsed_time() {
        let coordinator = ShutdownCoordinator::new();
        let node = MockNode::new();

        coordinator.shutdown(&node).await.unwrap();

        let status = coordinator.get_status().await;
        assert!(status.elapsed.is_some());

        let elapsed = status.elapsed.unwrap();
        assert!(elapsed.seconds > 0 || elapsed.nanos > 0);
    }
}
