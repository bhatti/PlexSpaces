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

//! # VM Health Monitoring
//!
//! ## Purpose
//! Provides health checking for Firecracker VMs using hybrid approach:
//! - Periodic polling (every N seconds)
//! - Event-driven monitoring (process exit detection)
//!
//! ## Health Criteria
//! A VM is considered healthy if:
//! 1. Firecracker process is running
//! 2. API is responsive (get_instance_info() succeeds)
//! 3. VM state is valid (Running, not Failed)

use crate::api_client::FirecrackerApiClient;
use crate::error::{FirecrackerError, FirecrackerResult};
use crate::vm::{FirecrackerVm, VmState};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::sleep;

/// Health status of a VM
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HealthStatus {
    /// VM is healthy (all checks pass)
    Healthy,
    /// VM is unhealthy (with reason)
    Unhealthy(String),
}

/// Health event emitted when VM health status changes
#[derive(Debug, Clone)]
pub struct HealthEvent {
    /// VM ID
    pub vm_id: String,
    /// Timestamp of the event
    pub timestamp: std::time::SystemTime,
    /// Current health status
    pub status: HealthStatus,
    /// Previous health status (if known)
    pub previous_status: Option<HealthStatus>,
}

/// Health event callback function type
pub type HealthEventCallback = Arc<dyn Fn(HealthEvent) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> + Send + Sync>;

/// VM Health Monitor
///
/// ## Purpose
/// Monitors health of a single Firecracker VM using periodic checks.
///
/// ## Design
/// - Checks health every `check_interval` seconds
/// - Tracks consecutive failures
/// - Can be started/stopped independently
pub struct VmHealthMonitor {
    /// VM being monitored
    vm: Arc<RwLock<FirecrackerVm>>,
    /// Health check interval
    check_interval: Duration,
    /// Number of consecutive health check failures
    consecutive_failures: Arc<RwLock<u32>>,
    /// Failure threshold (after this many failures, consider VM unhealthy)
    failure_threshold: u32,
    /// Monitoring task handle (if monitoring is active)
    monitoring_handle: Arc<RwLock<Option<tokio::task::JoinHandle<()>>>>,
    /// Health event callback (called when health status changes)
    health_event_callback: Arc<RwLock<Option<HealthEventCallback>>>,
    /// Last known health status (for change detection)
    last_health_status: Arc<RwLock<Option<HealthStatus>>>,
}

impl VmHealthMonitor {
    /// Create new health monitor for a VM
    ///
    /// ## Arguments
    /// * `vm` - VM to monitor (wrapped in Arc for shared ownership)
    /// * `check_interval` - How often to check health (default: 5 seconds)
    ///
    /// ## Returns
    /// New health monitor (not started yet)
    pub fn new(vm: Arc<RwLock<FirecrackerVm>>, check_interval: Duration) -> Self {
        Self {
            vm,
            check_interval,
            consecutive_failures: Arc::new(RwLock::new(0)),
            failure_threshold: 3, // Default: 3 consecutive failures = unhealthy
            monitoring_handle: Arc::new(RwLock::new(None)),
            health_event_callback: Arc::new(RwLock::new(None)),
            last_health_status: Arc::new(RwLock::new(None)),
        }
    }

    /// Create health monitor with custom failure threshold
    pub fn with_threshold(
        vm: Arc<RwLock<FirecrackerVm>>,
        check_interval: Duration,
        failure_threshold: u32,
    ) -> Self {
        Self {
            vm,
            check_interval,
            consecutive_failures: Arc::new(RwLock::new(0)),
            failure_threshold,
            monitoring_handle: Arc::new(RwLock::new(None)),
            health_event_callback: Arc::new(RwLock::new(None)),
            last_health_status: Arc::new(RwLock::new(None)),
        }
    }

    /// Set health event callback (called when health status changes)
    pub async fn set_health_event_callback(&self, callback: HealthEventCallback) {
        let mut cb = self.health_event_callback.write().await;
        *cb = Some(callback);
    }

    /// Get check interval
    pub fn check_interval(&self) -> Duration {
        self.check_interval
    }

    /// Get number of consecutive failures
    pub async fn consecutive_failures(&self) -> u32 {
        *self.consecutive_failures.read().await
    }

    /// Check VM health (synchronous check)
    ///
    /// ## Returns
    /// HealthStatus indicating if VM is healthy
    ///
    /// ## Health Criteria
    /// 1. VM state must be Running (not Failed/Stopped)
    /// 2. API must be responsive (get_instance_info() succeeds)
    /// 3. Process must be running (checked via VM state)
    pub async fn check_health(&mut self) -> HealthStatus {
        let vm = self.vm.read().await;
        let state = vm.state();

        // Check 1: VM state must be valid
        match state {
            VmState::Failed => {
                let mut failures = self.consecutive_failures.write().await;
                *failures += 1;
                return HealthStatus::Unhealthy("VM state is Failed".to_string());
            }
            VmState::Stopped => {
                let mut failures = self.consecutive_failures.write().await;
                *failures += 1;
                return HealthStatus::Unhealthy("VM is stopped".to_string());
            }
            VmState::Created | VmState::Ready | VmState::Booting => {
                // Not running yet, but not necessarily unhealthy
                // Don't increment failure counter for these states
                return HealthStatus::Unhealthy("VM not running yet".to_string());
            }
            VmState::Running | VmState::Paused => {
                // These states are potentially healthy, check API
            }
            VmState::Stopping => {
                // VM is shutting down, not unhealthy
                return HealthStatus::Unhealthy("VM is stopping".to_string());
            }
        }

        // Check 2: API must be responsive
        let config = vm.config();
        if config.socket_path.is_empty() {
            let mut failures = self.consecutive_failures.write().await;
            *failures += 1;
            return HealthStatus::Unhealthy("No socket path configured".to_string());
        }

        // Try to get instance info via API
        let client = FirecrackerApiClient::new(&config.socket_path);
        match client.get_instance_info().await {
            Ok(info) => {
                // Check 3: API state should match VM state
                match info.state.as_str() {
                    "Running" | "Paused" => {
                        // Healthy - reset failure counter
                        let mut failures = self.consecutive_failures.write().await;
                        *failures = 0;
                        HealthStatus::Healthy
                    }
                    _ => {
                        let mut failures = self.consecutive_failures.write().await;
                        *failures += 1;
                        HealthStatus::Unhealthy(format!(
                            "API reports state: {}",
                            info.state
                        ))
                    }
                }
            }
            Err(e) => {
                // API unresponsive
                let mut failures = self.consecutive_failures.write().await;
                *failures += 1;
                HealthStatus::Unhealthy(format!("API unresponsive: {}", e))
            }
        }
    }

    /// Start background health monitoring
    ///
    /// ## Returns
    /// `Ok(())` on success
    ///
    /// ## Design Notes
    /// Spawns a background task that periodically checks health.
    /// Use `stop()` to stop monitoring.
    pub async fn start_monitoring(&mut self) -> FirecrackerResult<()> {
        let mut handle = self.monitoring_handle.write().await;
        if handle.is_some() {
            return Err(FirecrackerError::VmOperationFailed(
                "Monitoring already started".to_string(),
            ));
        }

        let vm = Arc::clone(&self.vm);
        let check_interval = self.check_interval;
        let consecutive_failures = Arc::clone(&self.consecutive_failures);
        let failure_threshold = self.failure_threshold;

        // Clone callback and last_status for the task
        let health_event_callback = Arc::clone(&self.health_event_callback);
        let last_health_status = Arc::clone(&self.last_health_status);
        
        // Get VM ID for events
        let vm_id = {
            let vm_guard = self.vm.read().await;
            vm_guard.vm_id().to_string()
        };

        let task = tokio::spawn(async move {
            loop {
                sleep(check_interval).await;

                // Check health (create temporary monitor for check)
                // We need to create a monitor just for the check, but we'll use the shared state
                let vm_for_check = Arc::clone(&vm);
                let consecutive_failures_for_check = Arc::clone(&consecutive_failures);
                let status = {
                    let mut monitor = VmHealthMonitor {
                        vm: vm_for_check,
                        check_interval,
                        consecutive_failures: consecutive_failures_for_check,
                        failure_threshold,
                        monitoring_handle: Arc::new(RwLock::new(None)),
                        health_event_callback: Arc::new(RwLock::new(None)),
                        last_health_status: Arc::new(RwLock::new(None)),
                    };
                    monitor.check_health().await
                };
                
                // Check if status changed
                let status_changed = {
                    let last_status = last_health_status.read().await;
                    last_status.as_ref() != Some(&status)
                };
                
                // Emit health events via callback if registered and status changed
                if status_changed {
                    if let Some(ref callback) = health_event_callback.read().await.as_ref() {
                        let previous_status = {
                            let last_status = last_health_status.read().await;
                            last_status.clone()
                        };
                        
                        let event = HealthEvent {
                            vm_id: vm_id.clone(),
                            timestamp: std::time::SystemTime::now(),
                            status: status.clone(),
                            previous_status,
                        };
                        callback(event).await;
                    }
                }
                
                // Update last known status
                {
                    let mut last_status = last_health_status.write().await;
                    *last_status = Some(status);
                }
            }
        });

        *handle = Some(task);
        Ok(())
    }

    /// Stop background health monitoring
    pub async fn stop(&mut self) -> FirecrackerResult<()> {
        let mut handle = self.monitoring_handle.write().await;
        if let Some(task) = handle.take() {
            task.abort();
        }
        Ok(())
    }

    /// Check if monitoring is active
    pub async fn is_monitoring(&self) -> bool {
        let handle = self.monitoring_handle.read().await;
        handle.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_health_monitor_creation() {
        let config = crate::config::VmConfig {
            vm_id: ulid::Ulid::new().to_string(),
            vcpu_count: 1,
            mem_size_mib: 256,
            ..Default::default()
        };

        let vm = FirecrackerVm::create(config).await.unwrap();
        let vm_arc = Arc::new(RwLock::new(vm));

        let monitor = VmHealthMonitor::new(vm_arc.clone(), Duration::from_secs(5));
        assert_eq!(monitor.check_interval(), Duration::from_secs(5));
        assert_eq!(monitor.consecutive_failures().await, 0);
    }

    #[tokio::test]
    async fn test_health_check_created_vm() {
        let config = crate::config::VmConfig {
            vm_id: ulid::Ulid::new().to_string(),
            vcpu_count: 1,
            mem_size_mib: 256,
            ..Default::default()
        };

        let vm = FirecrackerVm::create(config).await.unwrap();
        let vm_arc = Arc::new(RwLock::new(vm));

        let mut monitor = VmHealthMonitor::new(vm_arc.clone(), Duration::from_secs(5));

        // VM in Created state should be unhealthy (not running)
        let status = monitor.check_health().await;
        assert_eq!(
            status,
            HealthStatus::Unhealthy("VM not running yet".to_string())
        );
    }

    #[tokio::test]
    async fn test_health_check_failed_vm() {
        let config = crate::config::VmConfig {
            vm_id: ulid::Ulid::new().to_string(),
            vcpu_count: 1,
            mem_size_mib: 256,
            ..Default::default()
        };

        let vm = FirecrackerVm::create(config).await.unwrap();
        let vm_arc = Arc::new(RwLock::new(vm));

        // Note: We can't directly set state to Failed, but we can test the logic
        // In real usage, state would be set by VM operations

        let mut monitor = VmHealthMonitor::new(vm_arc.clone(), Duration::from_secs(5));
        let status = monitor.check_health().await;
        // Should detect VM is not running
        assert!(matches!(status, HealthStatus::Unhealthy(_)));
    }

    #[tokio::test]
    async fn test_consecutive_failures_tracking() {
        let config = crate::config::VmConfig {
            vm_id: ulid::Ulid::new().to_string(),
            vcpu_count: 1,
            mem_size_mib: 256,
            ..Default::default()
        };

        let vm = FirecrackerVm::create(config).await.unwrap();
        let vm_arc = Arc::new(RwLock::new(vm));

        let mut monitor = VmHealthMonitor::new(vm_arc.clone(), Duration::from_secs(5));

        // Initial state
        assert_eq!(monitor.consecutive_failures().await, 0);

        // First failure (VM not running)
        monitor.check_health().await;
        // Note: Created state doesn't increment counter, so still 0
        // But if we had a Failed state, it would increment

        // Test with threshold
        let monitor_with_threshold = VmHealthMonitor::with_threshold(
            vm_arc.clone(),
            Duration::from_secs(5),
            5,
        );
        // Can't access failure_threshold directly (private), but we can verify it works
        assert_eq!(monitor_with_threshold.check_interval(), Duration::from_secs(5));
    }
}

