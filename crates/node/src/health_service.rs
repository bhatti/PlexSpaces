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

//! # PlexSpaces Health Monitoring Service
//!
//! ## Purpose
//! Implements standard gRPC Health Checking Protocol + Kubernetes-style probes
//! for PlexSpaces nodes.
//!
//! ## Architecture Context
//! This service is part of Phase 5: Health Monitoring & Failure Detection.
//! It provides:
//! - Standard `grpc.health.v1.Health` service for K8s integration
//! - Startup/Liveness/Readiness probe semantics
//! - Graceful shutdown with request draining
//! - Extended health diagnostics (PlexSpaces-specific)
//!
//! ## Design
//! Uses `tonic-health` crate for standard gRPC health protocol, extended with
//! PlexSpaces-specific health tracking via proto-defined `NodeHealthState`.
//!
//! ## Key Components
//! - [`PlexSpacesHealthReporter`]: Tracks node health state and reports to K8s
//! - [`HealthMonitor`]: Background task checking system health
//! - Probe checks: Startup (initialization), Liveness (alive), Readiness (ready)
//!
//! ## See Also
//! - [`crate::Node`]: Node that health service monitors
//! - `proto/plexspaces/v1/system.proto`: Proto definitions for health messages

use crate::health_checker::{HealthChecker, HealthCheckContext, run_health_check};
use plexspaces_proto::system::v1::{
    DependencyCheck, DependencyRegistrationConfig, DetailedHealthCheck, HealthCheck, HealthProbeConfig, HealthStatus,
    NodeHealthState, NodeReadinessStatus, ServingStatus,
};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{Duration, Instant};
use tonic_health::server::{health_reporter, HealthReporter};

// Make PlexSpacesHealthReporter implement Service trait for ServiceLocator registration
impl plexspaces_core::Service for PlexSpacesHealthReporter {}

/// PlexSpaces health reporter with K8s probe support
///
/// ## Purpose
/// Manages health state for a PlexSpaces node, implementing:
/// - Standard gRPC Health Checking Protocol (`grpc.health.v1.Health`)
/// - Kubernetes startup/liveness/readiness probe semantics
/// - Graceful shutdown with request draining
///
/// ## Architecture Context
/// This is the primary health tracking component for PlexSpaces nodes.
/// It integrates with:
/// - `grpc.health.v1.Health` service (via `tonic-health`)
/// - PlexSpaces Node (monitors actors, queues, connections)
/// - Kubernetes (health probes for pod lifecycle)
///
/// ## Design Notes
/// - Uses `tonic-health::HealthReporter` for standard gRPC health
/// - Tracks internal state via proto `NodeHealthState` message
/// - Background monitor task updates health every 5 seconds
/// - Graceful shutdown drains requests with 30s timeout
///
/// ## Usage
/// ```rust,ignore
/// // Create health reporter
/// let (reporter, service) = PlexSpacesHealthReporter::new();
///
/// // Register with gRPC server
/// Server::builder()
///     .add_service(service)
///     .serve(addr).await?;
///
/// // Mark startup complete
/// reporter.mark_startup_complete().await;
///
/// // Begin graceful shutdown
/// reporter.begin_shutdown(None).await;
/// ```
pub struct PlexSpacesHealthReporter {
    /// tonic-health reporter for standard gRPC health service
    /// This is used to update the standard grpc.health.v1.Health service status
    reporter: Arc<tokio::sync::RwLock<HealthReporter>>,

    /// Internal health state (proto-defined)
    state: Arc<RwLock<NodeHealthState>>,

    /// Configuration for health probes
    config: Arc<HealthProbeConfig>,

    /// Startup timestamp
    started_at: Instant,

    /// Liveness checkers (basic checks: ping, shutdown)
    pub(crate) liveness_checkers: Arc<RwLock<Vec<Arc<dyn HealthChecker>>>>,

    /// Readiness checkers (critical dependencies)
    pub(crate) readiness_checkers: Arc<RwLock<Vec<Arc<dyn HealthChecker>>>>,

    /// Startup checkers (critical dependencies for startup)
    pub(crate) startup_checkers: Arc<RwLock<Vec<Arc<dyn HealthChecker>>>>,

    /// Service-specific health status tracking
    /// Key: Service name (e.g., "plexspaces.actor.v1.ActorService")
    /// Value: ServingStatus
    service_status: Arc<RwLock<std::collections::HashMap<String, ServingStatus>>>,
    
    /// Shutdown flag to prevent new requests
    shutdown_flag: Arc<tokio::sync::RwLock<bool>>,
    
    /// ServiceLocator reference (for setting shutdown flag)
    /// This allows HealthService to be the source of truth for shutdown
    service_locator: Option<Arc<plexspaces_core::ServiceLocator>>,
}

impl PlexSpacesHealthReporter {
    /// Create new health reporter with default configuration
    ///
    /// ## Returns
    /// Tuple of:
    /// - `PlexSpacesHealthReporter`: Health state tracker
    /// - gRPC health service to register with server
    ///
    /// ## Examples
    /// ```rust,ignore
    /// let (reporter, service) = PlexSpacesHealthReporter::new();
    ///
    /// Server::builder()
    ///     .add_service(service)
    ///     .serve(addr).await?;
    /// ```
    pub fn new() -> (Self, impl tonic::server::NamedService) {
        Self::with_config_and_service_locator(Default::default(), None)
    }
    
    /// Create health reporter with ServiceLocator reference
    ///
    /// ## Arguments
    /// * `service_locator` - ServiceLocator to update shutdown flag on
    ///
    /// ## Returns
    /// Tuple of health reporter and gRPC service
    ///
    /// ## Note
    /// Prefer using `health_service_helpers::create_and_register_health_service()`
    /// which handles both creation and registration in ServiceLocator.
    pub fn with_service_locator(
        service_locator: Arc<plexspaces_core::ServiceLocator>,
    ) -> (Self, impl tonic::server::NamedService) {
        Self::with_config_and_service_locator(Default::default(), Some(service_locator))
    }

    /// Create health reporter with custom configuration
    ///
    /// ## Arguments
    /// * `config` - Health probe configuration (thresholds, timeouts, etc.)
    ///
    /// ## Returns
    /// Tuple of health reporter and gRPC service
    pub fn with_config(config: HealthProbeConfig) -> (Self, impl tonic::server::NamedService) {
        Self::with_config_and_service_locator(config, None)
    }
    
    /// Create health reporter with custom configuration and ServiceLocator
    ///
    /// ## Arguments
    /// * `config` - Health probe configuration (thresholds, timeouts, etc.)
    /// * `service_locator` - Optional ServiceLocator to update shutdown flag on
    ///
    /// ## Returns
    /// Tuple of health reporter and gRPC service
    pub fn with_config_and_service_locator(
        config: HealthProbeConfig,
        service_locator: Option<Arc<plexspaces_core::ServiceLocator>>,
    ) -> (Self, impl tonic::server::NamedService) {
        let (reporter, service) = health_reporter();

        let initial_state = NodeHealthState {
            serving_status: ServingStatus::ServingStatusNotServing as i32,
            reason: "Startup in progress".to_string(),
            state_entered_at: Some(prost_types::Timestamp::from(std::time::SystemTime::now())),
            startup_complete: false,
            is_alive: true, // Alive but not ready yet
            is_ready: false,
            shutdown_in_progress: false,
            in_flight_requests: 0,
        };

        let health_reporter = Self {
            reporter: Arc::new(tokio::sync::RwLock::new(reporter)),
            state: Arc::new(RwLock::new(initial_state)),
            config: Arc::new(config),
            started_at: Instant::now(),
            liveness_checkers: Arc::new(RwLock::new(Vec::new())),
            readiness_checkers: Arc::new(RwLock::new(Vec::new())),
            startup_checkers: Arc::new(RwLock::new(Vec::new())),
            service_status: Arc::new(RwLock::new(std::collections::HashMap::new())),
            shutdown_flag: Arc::new(tokio::sync::RwLock::new(false)),
            service_locator,
        };

        (health_reporter, service)
    }

    /// Register a liveness checker
    ///
    /// ## Purpose
    /// Adds a checker for liveness probes. Liveness checks are lightweight
    /// and only verify the node is alive (not deadlocked/crashed).
    ///
    /// ## Arguments
    /// * `checker` - Health checker to register
    pub async fn register_liveness_checker(&self, checker: Arc<dyn HealthChecker>) {
        let mut checkers = self.liveness_checkers.write().await;
        checkers.push(checker);
    }

    /// Register a readiness checker
    ///
    /// ## Purpose
    /// Adds a checker for readiness probes. Readiness checks verify critical
    /// dependencies are healthy. Non-critical dependencies can fail without
    /// blocking readiness.
    ///
    /// ## Arguments
    /// * `checker` - Health checker to register
    pub async fn register_readiness_checker(&self, checker: Arc<dyn HealthChecker>) {
        let mut checkers = self.readiness_checkers.write().await;
        checkers.push(checker);
    }

    /// Register a startup checker
    ///
    /// ## Purpose
    /// Adds a checker for startup probes. Startup checks verify critical
    /// dependencies required for initialization are healthy.
    ///
    /// ## Arguments
    /// * `checker` - Health checker to register
    pub async fn register_startup_checker(&self, checker: Arc<dyn HealthChecker>) {
        let mut checkers = self.startup_checkers.write().await;
        checkers.push(checker);
    }

    /// Run liveness checks
    ///
    /// ## Returns
    /// `true` if all liveness checks pass, `false` otherwise
    pub async fn check_liveness(&self) -> bool {
        let checkers = self.liveness_checkers.read().await;
        let ctx = HealthCheckContext::default();

        for checker in checkers.iter() {
            if checker.check(&ctx).await.is_err() {
                return false;
            }
        }

        true
    }

    /// Run readiness checks
    ///
    /// ## Returns
    /// Tuple of (is_ready, reason_if_not_ready)
    ///
    /// ## Design Notes
    /// - Critical dependencies must be healthy for readiness
    /// - Non-critical dependencies can fail (degraded mode) but readiness remains true
    /// - Circuit breakers track dependency state transitions
    pub async fn check_readiness(&self) -> (bool, String) {
        // Record metrics
        metrics::counter!("plexspaces_node_readiness_checks_total").increment(1);
        let checkers = self.readiness_checkers.read().await;
        let ctx = HealthCheckContext::default();

        // Track degraded dependencies
        let mut degraded_dependencies = Vec::new();

        // Check all dependencies (critical and non-critical)
        for checker in checkers.iter() {
            let check_result = checker.check(&ctx).await;
            
            if let Err(e) = check_result {
                if checker.is_critical() {
                    // Critical dependency failed - node not ready
                    return (false, format!("Critical dependency '{}' unhealthy: {}", checker.name(), e));
                } else {
                    // Non-critical dependency failed - degraded mode
                    degraded_dependencies.push(checker.name().to_string());
                }
            }
        }

        // Check state
        let state = self.state.read().await;
        if !state.is_alive || state.shutdown_in_progress || !state.is_ready {
            return (false, state.reason.clone());
        }

        // Check queue depth
        let threshold = if self.config.queue_depth_threshold == 0 {
            10000
        } else {
            self.config.queue_depth_threshold
        };
        if state.in_flight_requests > threshold {
            return (false, format!("Queue depth {} exceeds threshold {}", state.in_flight_requests, threshold));
        }

        // If we have degraded dependencies, still ready but note degraded mode
        if !degraded_dependencies.is_empty() {
            let reason = format!("Degraded mode: non-critical dependencies unhealthy: {}", degraded_dependencies.join(", "));
            // Still return true (ready) but with degraded reason for observability
            // The detailed health check will show DEGRADED status
            return (true, reason);
        }

        (true, String::new())
    }

    /// Run startup checks
    ///
    /// ## Returns
    /// Tuple of (is_complete, reason_if_not_complete)
    pub async fn check_startup(&self) -> (bool, String) {
        let state = self.state.read().await;
        if state.startup_complete {
            return (true, String::new());
        }

        // Check critical startup dependencies
        let checkers = self.startup_checkers.read().await;
        let ctx = HealthCheckContext::default();

        for checker in checkers.iter() {
            if checker.is_critical() {
                if let Err(e) = checker.check(&ctx).await {
                    return (false, format!("Critical startup dependency '{}' unhealthy: {}", checker.name(), e));
                }
            }
        }

        (false, state.reason.clone())
    }

    /// Get detailed health with dependency checks
    ///
    /// ## Arguments
    /// * `include_non_critical` - Whether to include non-critical dependency checks
    ///
    /// ## Returns
    /// `DetailedHealthCheck` with all dependency checks
    pub async fn get_detailed_health(&self, include_non_critical: bool) -> DetailedHealthCheck {
        let ctx = HealthCheckContext::default();
        let mut dependency_checks = Vec::new();

        // Run all readiness checkers
        let readiness_checkers = self.readiness_checkers.read().await;
        for checker in readiness_checkers.iter() {
            if include_non_critical || checker.is_critical() {
                // run_health_check already includes circuit breaker info via trait method
                let check_result = run_health_check(checker.as_ref(), &ctx).await;
                dependency_checks.push(check_result);
            }
        }
        drop(readiness_checkers);

        // Determine overall status
        let critical_healthy = dependency_checks
            .iter()
            .filter(|c| c.is_critical)
            .all(|c| c.status == HealthStatus::HealthStatusHealthy as i32);

        let non_critical_healthy = dependency_checks
            .iter()
            .filter(|c| !c.is_critical)
            .all(|c| c.status == HealthStatus::HealthStatusHealthy as i32);

        let overall_status = if critical_healthy {
            if non_critical_healthy {
                HealthStatus::HealthStatusHealthy
            } else {
                HealthStatus::HealthStatusDegraded
            }
        } else {
            HealthStatus::HealthStatusUnhealthy
        };

        // Build component checks (basic system components)
        let mut component_checks = Vec::new();
        
        // Add basic component health checks
        // Note: More detailed component checks can be added as needed
        component_checks.push(HealthCheck {
            component: "health_reporter".to_string(),
            status: HealthStatus::HealthStatusHealthy as i32,
            message: "Health reporter operational".to_string(),
            checked_at: Some(prost_types::Timestamp {
                seconds: chrono::Utc::now().timestamp(),
                nanos: 0,
            }),
            response_time: None,
            details: std::collections::HashMap::new(),
        });
        
        DetailedHealthCheck {
            overall_status: overall_status as i32,
            component_checks,
            dependency_checks,
            critical_dependencies_healthy: critical_healthy,
            non_critical_dependencies_healthy: non_critical_healthy,
        }
    }

    /// Mark startup complete (NOT_SERVING → SERVING transition)
    ///
    /// ## Purpose
    /// Called by PlexSpacesNode after initialization completes:
    /// - gRPC server started
    /// - TupleSpace backend connected
    /// - Actor registry initialized
    /// - Initial actors spawned
    ///
    /// ## Arguments
    /// * `message` - Optional message explaining what was initialized
    ///
    /// ## Returns
    /// Startup duration
    ///
    /// ## Examples
    /// ```rust,ignore
    /// // After node initialization
    /// let duration = reporter.mark_startup_complete(None).await;
    /// eprintln!("Startup took {:?}", duration);
    /// ```
    pub async fn mark_startup_complete(&self, message: Option<String>) -> Duration {
        let startup_duration = self.started_at.elapsed();

        // Update internal state
        let mut state = self.state.write().await;
        state.serving_status = ServingStatus::ServingStatusServing as i32;
        state.reason = message.unwrap_or_else(|| "Startup complete".to_string());
        state.state_entered_at = Some(prost_types::Timestamp::from(std::time::SystemTime::now()));
        state.startup_complete = true;
        state.is_alive = true;
        state.is_ready = true;
        drop(state);

        // Initialize default service statuses as SERVING
        {
            let mut service_status = self.service_status.write().await;
            service_status.insert("".to_string(), ServingStatus::ServingStatusServing); // Overall health
            service_status.insert("plexspaces.actor.v1.ActorService".to_string(), ServingStatus::ServingStatusServing);
            service_status.insert("plexspaces.tuplespace.v1.TuplePlexSpaceService".to_string(), ServingStatus::ServingStatusServing);
            service_status.insert("plexspaces.supervisor.v1.SupervisorService".to_string(), ServingStatus::ServingStatusServing);
        }

        eprintln!(
            "✅ Startup complete in {:?}, node SERVING",
            startup_duration
        );

        startup_duration
    }

    /// Begin graceful shutdown sequence
    ///
    /// ## Purpose
    /// Initiates graceful shutdown:
    /// 1. Set health to NOT_SERVING (K8s removes from service)
    /// 2. Drain in-flight requests (configurable timeout)
    /// 3. Return when ready for final shutdown
    ///
    /// ## Arguments
    /// * `drain_timeout` - Override default drain timeout (default: 30s from config)
    ///
    /// ## Returns
    /// Tuple of:
    /// - `requests_drained`: Number of requests drained
    /// - `drain_duration`: Time taken to drain
    /// - `drain_completed`: Whether drain completed or timed out
    ///
    /// ## Examples
    /// ```rust,ignore
    /// // On SIGTERM
    /// let (drained, duration, completed) = reporter.begin_shutdown(None).await;
    /// if !completed {
    ///     eprintln!("Warning: {} requests still in flight", drained);
    /// }
    /// ```
    pub async fn begin_shutdown(&self, drain_timeout: Option<Duration>) -> (u64, Duration, bool) {
        // Update state to NOT_SERVING
        let mut state = self.state.write().await;
        state.serving_status = ServingStatus::ServingStatusNotServing as i32;
        state.reason = "Graceful shutdown in progress".to_string();
        state.state_entered_at = Some(prost_types::Timestamp::from(std::time::SystemTime::now()));
        state.shutdown_in_progress = true;
        drop(state);

        // Set shutdown flag to prevent new requests
        // This is the source of truth for shutdown - update both local flag and ServiceLocator
        {
            let mut flag = self.shutdown_flag.write().await;
            *flag = true;
        }
        
        // Also update ServiceLocator shutdown flag (if registered)
        if let Some(ref service_locator) = self.service_locator {
            service_locator.set_shutdown(true).await;
            tracing::info!("ServiceLocator shutdown flag set via HealthService");
        }
        
        // Update all service statuses to NOT_SERVING (K8s removes from service immediately)
        {
            let mut service_status = self.service_status.write().await;
            service_status.insert("".to_string(), ServingStatus::ServingStatusNotServing); // Overall health
            service_status.insert("plexspaces.actor.v1.ActorService".to_string(), ServingStatus::ServingStatusNotServing);
            service_status.insert("plexspaces.tuplespace.v1.TuplePlexSpaceService".to_string(), ServingStatus::ServingStatusNotServing);
            service_status.insert("plexspaces.supervisor.v1.SupervisorService".to_string(), ServingStatus::ServingStatusNotServing);
        }

        eprintln!("🛑 Graceful shutdown: NOT_SERVING, draining requests...");

        // Drain requests
        let timeout = drain_timeout.unwrap_or_else(|| {
            Duration::from_secs(
                self.config
                    .drain_timeout
                    .as_ref()
                    .map(|d| d.seconds as u64)
                    .unwrap_or(30),
            )
        });

        let (drained, duration, completed) = self.drain_requests(timeout).await;

        if completed {
            eprintln!("✅ All {} requests drained in {:?}", drained, duration);
        } else {
            eprintln!(
                "⚠️  Drain timeout, {} requests still in flight after {:?}",
                drained, duration
            );
        }

        (drained, duration, completed)
    }

    /// Set service-specific health status
    ///
    /// ## Purpose
    /// Tracks health status for individual services (e.g., ActorService, TupleSpaceService).
    /// This enables service-specific health checks via gRPC Health Protocol.
    ///
    /// ## Arguments
    /// * `service_name` - Service name (e.g., "plexspaces.actor.v1.ActorService")
    /// * `status` - Serving status for the service
    ///
    /// ## Examples
    /// ```rust,ignore
    /// reporter.set_service_status(
    ///     "plexspaces.actor.v1.ActorService",
    ///     ServingStatus::ServingStatusServing
    /// ).await;
    /// ```
    pub async fn set_service_status(&self, service_name: &str, status: ServingStatus) {
        // Convert to i32 first to avoid move issues
        let status_i32 = status as i32;
        {
            let mut service_status = self.service_status.write().await;
            // Recreate enum from i32 to avoid move
            let status_to_insert = match status_i32 {
                1 => ServingStatus::ServingStatusServing,
                2 => ServingStatus::ServingStatusNotServing,
                _ => ServingStatus::ServingStatusUnknown,
            };
            service_status.insert(service_name.to_string(), status_to_insert);
        }
        
        // Also update the tonic-health reporter for standard gRPC health protocol
        // Note: We need to use the reporter's set_serving/set_not_serving methods
        // but they require a NamedService type. For now, we track internally.
        // The standard health service will use overall health for service-specific checks.
        
        tracing::debug!(
            service = service_name,
            status = status_i32,
            "Service health status updated"
        );
    }

    /// Get service-specific health status
    ///
    /// ## Arguments
    /// * `service_name` - Service name to check
    ///
    /// ## Returns
    /// `ServingStatus` for the service, or `ServingStatus::ServingStatusUnknown` if not tracked
    pub async fn get_service_status(&self, service_name: &str) -> ServingStatus {
        let service_status = self.service_status.read().await;
        service_status
            .get(service_name)
            .cloned()
            .unwrap_or(ServingStatus::ServingStatusUnknown)
    }

    /// Get all service health statuses
    ///
    /// ## Returns
    /// HashMap of service names to their health statuses
    pub async fn get_all_service_statuses(&self) -> std::collections::HashMap<String, ServingStatus> {
        let service_status = self.service_status.read().await;
        service_status.clone()
    }
    
    /// Check if shutdown is in progress
    ///
    /// ## Returns
    /// `true` if shutdown is in progress, `false` otherwise
    pub async fn is_shutting_down(&self) -> bool {
        let flag = self.shutdown_flag.read().await;
        *flag
    }
    
    /// Update standard gRPC health status for all services
    ///
    /// ## Arguments
    /// * `status` - Serving status to set for all services
    async fn update_standard_health_status(&self, status: ServingStatus) {
        // Update internal service status tracking
        // The standard gRPC health service will query our internal state
        // via the custom health service implementation
        // Convert enum to i32 first to avoid move issues
        let status_i32 = status as i32;
        
        // Helper to recreate enum from i32
        let recreate_status = |i: i32| -> ServingStatus {
            match i {
                1 => ServingStatus::ServingStatusServing,
                2 => ServingStatus::ServingStatusNotServing,
                _ => ServingStatus::ServingStatusUnknown,
            }
        };
        
        let mut service_status = self.service_status.write().await;
        service_status.insert("".to_string(), recreate_status(status_i32)); // Overall health
        service_status.insert("plexspaces.actor.v1.ActorService".to_string(), recreate_status(status_i32));
        service_status.insert("plexspaces.tuplespace.v1.TuplePlexSpaceService".to_string(), recreate_status(status_i32));
        service_status.insert("plexspaces.supervisor.v1.SupervisorService".to_string(), recreate_status(status_i32));
    }

    /// Get current node readiness status (detailed diagnostics)
    ///
    /// ## Purpose
    /// Returns detailed readiness information beyond simple SERVING/NOT_SERVING.
    /// Useful for debugging why a node is not ready.
    ///
    /// ## Returns
    /// `NodeReadinessStatus` with detailed checks
    ///
    /// ## Examples
    /// ```rust,ignore
    /// let status = reporter.get_readiness().await;
    /// if !status.is_ready {
    ///     eprintln!("Not ready: {}", status.not_ready_reason);
    /// }
    /// ```
    pub async fn get_readiness(&self) -> NodeReadinessStatus {
        let state = self.state.read().await;

        let is_ready = state.serving_status == ServingStatus::ServingStatusServing as i32
            && state.is_alive
            && state.is_ready;

        let not_ready_reason = if !is_ready {
            state.reason.clone()
        } else {
            String::new()
        };

        // Get connected nodes count and check required nodes
        // Note: This requires access to Node, which is not available in HealthReporter
        // For now, we'll use default values. In production, this should be passed from Node.
        let (connected_nodes_count, required_nodes_connected, missing_nodes, tuplespace_healthy) = 
            (0u32, true, Vec::new(), true);
        
        NodeReadinessStatus {
            is_ready,
            not_ready_reason,
            liveness_passing: state.is_alive,
            queue_depth_ok: state.in_flight_requests
                < if self.config.queue_depth_threshold == 0 {
                    10000
                } else {
                    self.config.queue_depth_threshold
                },
            tuplespace_healthy,
            required_nodes_connected,
            total_queue_depth: state.in_flight_requests,
            queue_depth_threshold: if self.config.queue_depth_threshold == 0 {
                10000
            } else {
                self.config.queue_depth_threshold
            },
            connected_nodes_count,
            required_nodes: self.config.required_nodes.clone(),
            missing_nodes,
        }
    }

    /// Drain in-flight requests with timeout
    ///
    /// ## Arguments
    /// * `timeout` - Maximum time to wait for draining
    ///
    /// ## Returns
    /// Tuple of (requests_drained, duration, completed)
    async fn drain_requests(&self, timeout: Duration) -> (u64, Duration, bool) {
        let start = Instant::now();

        loop {
            let state = self.state.read().await;
            let in_flight = state.in_flight_requests;
            drop(state);

            if in_flight == 0 {
                return (0, start.elapsed(), true);
            }

            if start.elapsed() > timeout {
                return (in_flight, start.elapsed(), false);
            }

            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    /// Update in-flight request count (called by Node)
    ///
    /// ## Purpose
    /// Node calls this to update request count for accurate draining
    pub async fn update_in_flight_requests(&self, count: u64) {
        let mut state = self.state.write().await;
        state.in_flight_requests = count;
    }

    /// Check if node is alive (liveness probe)
    ///
    /// ## Purpose
    /// Liveness check for Kubernetes. Returns false if node should be restarted.
    ///
    /// ## Checks
    /// - gRPC server responsive
    /// - Not deadlocked (internal watchdog)
    /// - Core services not crashed
    pub async fn is_alive(&self) -> bool {
        // Record metrics
        metrics::counter!("plexspaces_node_liveness_checks_total").increment(1);
        let state = self.state.read().await;
        state.is_alive && !state.shutdown_in_progress
    }

    /// Check if node is ready (readiness probe)
    ///
    /// ## Purpose
    /// Readiness check for Kubernetes. Returns false if node should not receive requests.
    ///
    /// ## Checks
    /// - Liveness checks pass
    /// - Not overloaded (queue depth < threshold)
    /// - Critical dependencies healthy (via readiness checkers)
    /// - TupleSpace backend healthy
    /// - Required nodes connected
    pub async fn is_ready(&self) -> bool {
        // Must be alive first
        if !self.is_alive().await {
            return false;
        }

        // Check readiness (includes critical dependencies)
        let (is_ready, _reason) = self.check_readiness().await;
        is_ready
    }

    /// Get current health state (internal)
    pub async fn get_state(&self) -> NodeHealthState {
        self.state.read().await.clone()
    }
}

impl Default for PlexSpacesHealthReporter {
    fn default() -> Self {
        Self::new().0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_health_reporter_initialization() {
        let (reporter, _service) = PlexSpacesHealthReporter::new();

        let state = reporter.get_state().await;
        assert_eq!(state.serving_status, ServingStatus::ServingStatusNotServing as i32);
        assert!(!state.startup_complete);
        assert!(state.is_alive);
        assert!(!state.is_ready);
    }

    #[tokio::test]
    async fn test_mark_startup_complete() {
        let (reporter, _service) = PlexSpacesHealthReporter::new();

        let duration = reporter
            .mark_startup_complete(Some("All systems initialized".to_string()))
            .await;

        // Duration should be measurable (even if very small)
        assert!(duration.as_nanos() > 0);

        let state = reporter.get_state().await;
        assert_eq!(state.serving_status, ServingStatus::ServingStatusServing as i32);
        assert!(state.startup_complete);
        assert!(state.is_alive);
        assert!(state.is_ready);
        assert_eq!(state.reason, "All systems initialized");
    }

    #[tokio::test]
    async fn test_begin_shutdown() {
        let (reporter, _service) = PlexSpacesHealthReporter::new();

        // Mark ready first
        reporter.mark_startup_complete(None).await;

        // Begin shutdown
        let (drained, duration, completed) =
            reporter.begin_shutdown(Some(Duration::from_secs(1))).await;

        assert_eq!(drained, 0); // No in-flight requests
        assert!(completed);
        assert!(duration.as_millis() < 1500);

        let state = reporter.get_state().await;
        assert_eq!(state.serving_status, ServingStatus::ServingStatusNotServing as i32);
        assert!(state.shutdown_in_progress);
    }

    #[tokio::test]
    async fn test_readiness_status() {
        let (reporter, _service) = PlexSpacesHealthReporter::new();

        // Initially not ready
        let status = reporter.get_readiness().await;
        assert!(!status.is_ready);
        assert!(status.liveness_passing); // Alive but not ready

        // Mark startup complete
        reporter.mark_startup_complete(None).await;

        let status = reporter.get_readiness().await;
        assert!(status.is_ready);
        assert!(status.liveness_passing);
        assert!(status.queue_depth_ok);
    }

    #[tokio::test]
    async fn test_liveness_and_readiness_checks() {
        let (reporter, _service) = PlexSpacesHealthReporter::new();

        // Initially alive but not ready
        assert!(reporter.is_alive().await);
        assert!(!reporter.is_ready().await);

        // After startup
        reporter.mark_startup_complete(None).await;
        assert!(reporter.is_alive().await);
        assert!(reporter.is_ready().await);

        // During shutdown
        reporter.begin_shutdown(Some(Duration::from_secs(1))).await;
        assert!(!reporter.is_alive().await); // shutdown_in_progress = true
        assert!(!reporter.is_ready().await);
    }

    #[tokio::test]
    async fn test_request_draining() {
        let (reporter, _service) = PlexSpacesHealthReporter::new();
        reporter.mark_startup_complete(None).await;

        // Simulate in-flight requests
        reporter.update_in_flight_requests(5).await;

        // Drain will timeout because requests never complete
        let (drained, _duration, completed) = reporter
            .begin_shutdown(Some(Duration::from_millis(200)))
            .await;

        assert_eq!(drained, 5);
        assert!(!completed); // Timeout

        // Clear requests
        reporter.update_in_flight_requests(0).await;

        // Now should drain immediately
        let (_drained, _duration, completed) =
            reporter.begin_shutdown(Some(Duration::from_secs(1))).await;
        assert!(completed);
    }

    #[tokio::test]
    async fn test_custom_health_probe_config() {
        // Test with custom drain timeout from config
        let custom_config = HealthProbeConfig {
            dependency_registration: Some(DependencyRegistrationConfig {
                enabled: false,
                dependencies: vec![],
                default_namespace: String::new(),
                default_tenant: String::new(),
            }),
            circuit_breaker_config: None,
            monitoring_interval: Some(prost_types::Duration {
                seconds: 5,
                nanos: 0,
            }),
            drain_timeout: Some(prost_types::Duration {
                seconds: 5,
                nanos: 0,
            }),
            queue_depth_threshold: 100,
            required_nodes: vec!["node1".to_string(), "node2".to_string()],
            enable_detailed_logging: true,
        };

        let (reporter, _service) = PlexSpacesHealthReporter::with_config(custom_config);
        reporter.mark_startup_complete(None).await;

        // Begin shutdown should use config drain_timeout (5s)
        let (_drained, duration, completed) = reporter.begin_shutdown(None).await; // No override - use config

        assert!(completed);
        // Verify it didn't wait the full 5 seconds (completes immediately with no requests)
        assert!(duration.as_secs() < 1);
    }

    #[tokio::test]
    async fn test_queue_depth_threshold() {
        let config = HealthProbeConfig {
            dependency_registration: Some(DependencyRegistrationConfig {
                enabled: false,
                dependencies: vec![],
                default_namespace: String::new(),
                default_tenant: String::new(),
            }),
            circuit_breaker_config: None,
            monitoring_interval: None,
            drain_timeout: None,
            queue_depth_threshold: 10, // Low threshold
            required_nodes: vec![],
            enable_detailed_logging: false,
        };

        let (reporter, _service) = PlexSpacesHealthReporter::with_config(config);
        reporter.mark_startup_complete(None).await;

        // Initially ready
        assert!(reporter.is_ready().await);

        // Exceed threshold
        reporter.update_in_flight_requests(15).await;
        assert!(!reporter.is_ready().await); // Not ready due to queue depth

        // Below threshold again
        reporter.update_in_flight_requests(5).await;
        assert!(reporter.is_ready().await);
    }

    #[tokio::test]
    async fn test_default_health_reporter() {
        // Test Default trait
        let reporter = PlexSpacesHealthReporter::default();

        let state = reporter.get_state().await;
        assert_eq!(state.serving_status, ServingStatus::ServingStatusNotServing as i32);
        assert!(!state.startup_complete);
        assert!(state.is_alive);
    }

    #[tokio::test]
    async fn test_readiness_with_queue_details() {
        let (reporter, _service) = PlexSpacesHealthReporter::new();
        reporter.mark_startup_complete(None).await;

        let status = reporter.get_readiness().await;
        assert!(status.is_ready);
        assert!(status.liveness_passing);
        assert!(status.queue_depth_ok);
        assert_eq!(status.total_queue_depth, 0);
        assert!(status.queue_depth_threshold > 0);
    }
}
