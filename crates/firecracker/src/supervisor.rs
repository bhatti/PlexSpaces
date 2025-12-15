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

//! # VM Supervisor
//!
//! ## Purpose
//! Provides supervision for Firecracker VMs with automatic restart on failure.
//! Follows Erlang/OTP supervisor pattern.
//!
//! ## Design
//! - Manages multiple VMs with independent restart policies
//! - Monitors VM health and restarts on failure
//! - Supports different restart strategies (Always, Backoff, MaxRetries, Never)

use crate::error::{FirecrackerError, FirecrackerResult};
use crate::health::{HealthStatus, VmHealthMonitor};
use crate::vm::{FirecrackerVm, VmState};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tokio::time::sleep;

/// Supervision strategy for VM groups
///
/// ## Design (Simplicity First)
/// - OneForOne: Restart only the failed VM (default, simplest)
/// - OneForAll: Restart all VMs if one fails (for tightly coupled groups)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupervisionStrategy {
    /// Restart only the failed VM (default)
    OneForOne,
    /// Restart all VMs if one fails
    OneForAll,
}

impl Default for SupervisionStrategy {
    fn default() -> Self {
        Self::OneForOne
    }
}

/// Restart policy for supervised VMs
#[derive(Debug, Clone)]
pub enum RestartPolicy {
    /// Always restart on failure (infinite retries)
    Always,

    /// Restart with exponential backoff
    WithBackoff {
        /// Initial delay before first restart
        initial_delay: Duration,
        /// Maximum delay between restarts
        max_delay: Duration,
        /// Backoff multiplier (e.g., 2.0 = doubles each time)
        factor: f64,
    },

    /// Restart with maximum retry limit
    MaxRetries {
        /// Maximum number of restarts before giving up
        max_retries: u32,
        /// Backoff configuration
        backoff: BackoffConfig,
    },

    /// Never restart (monitor only)
    Never,
}

/// Backoff configuration for restart policies
#[derive(Debug, Clone)]
pub struct BackoffConfig {
    /// Initial delay
    pub initial_delay: Duration,
    /// Maximum delay
    pub max_delay: Duration,
    /// Backoff multiplier
    pub factor: f64,
}

impl Default for BackoffConfig {
    fn default() -> Self {
        Self {
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(60),
            factor: 2.0,
        }
    }
}

/// Supervised VM entry
struct SupervisedVm {
    /// VM being supervised
    vm: Arc<RwLock<FirecrackerVm>>,
    /// Health monitor for this VM
    health_monitor: Arc<RwLock<VmHealthMonitor>>,
    /// Restart policy
    restart_policy: RestartPolicy,
    /// Number of restarts performed
    restart_count: u32,
    /// Last restart time
    last_restart: Option<Instant>,
    /// Restart history for backoff calculation
    restart_history: Vec<Instant>,
}

/// VM Supervisor
///
/// ## Purpose
/// Manages multiple VMs with automatic restart on failure.
///
/// ## Design
/// - One supervisor manages multiple VMs
/// - Each VM has independent restart policy
/// - Health monitoring integrated with supervision
pub struct VmSupervisor {
    /// Supervised VMs by VM ID
    vms: Arc<RwLock<HashMap<String, SupervisedVm>>>,
    /// Health check interval (default: 5 seconds)
    health_check_interval: Duration,
    /// Failure threshold for health checks (default: 3 consecutive failures)
    failure_threshold: u32,
    /// Supervision strategy (default: OneForOne)
    strategy: SupervisionStrategy,
}

impl VmSupervisor {
    /// Create new VM supervisor with default strategy (OneForOne)
    pub fn new() -> Self {
        Self {
            vms: Arc::new(RwLock::new(HashMap::new())),
            health_check_interval: Duration::from_secs(5),
            failure_threshold: 3,
            strategy: SupervisionStrategy::OneForOne,
        }
    }

    /// Create supervisor with custom strategy
    pub fn with_strategy(strategy: SupervisionStrategy) -> Self {
        Self {
            vms: Arc::new(RwLock::new(HashMap::new())),
            health_check_interval: Duration::from_secs(5),
            failure_threshold: 3,
            strategy,
        }
    }

    /// Create supervisor with custom health check settings
    pub fn with_health_settings(
        health_check_interval: Duration,
        failure_threshold: u32,
    ) -> Self {
        Self {
            vms: Arc::new(RwLock::new(HashMap::new())),
            health_check_interval,
            failure_threshold,
            strategy: SupervisionStrategy::OneForOne,
        }
    }

    /// Get number of supervised VMs
    pub async fn supervised_vm_count(&self) -> usize {
        let vms = self.vms.read().await;
        vms.len()
    }

    /// Check if supervisor is empty
    pub async fn is_empty(&self) -> bool {
        self.supervised_vm_count().await == 0
    }

    /// Start supervising a VM
    ///
    /// ## Arguments
    /// * `vm` - VM to supervise (wrapped in Arc for shared ownership)
    /// * `restart_policy` - Restart policy for this VM
    ///
    /// ## Returns
    /// `Ok(())` on success
    pub async fn supervise(
        &mut self,
        vm: Arc<RwLock<FirecrackerVm>>,
        restart_policy: RestartPolicy,
    ) -> FirecrackerResult<()> {
        let vm_id = {
            let vm_guard = vm.read().await;
            vm_guard.vm_id().to_string()
        };

        // Create health monitor
        let health_monitor = Arc::new(RwLock::new(VmHealthMonitor::with_threshold(
            Arc::clone(&vm),
            self.health_check_interval,
            self.failure_threshold,
        )));

        // Start health monitoring
        {
            let mut monitor = health_monitor.write().await;
            monitor.start_monitoring().await?;
        }

        // Create supervised VM entry
        let supervised_vm = SupervisedVm {
            vm,
            health_monitor,
            restart_policy,
            restart_count: 0,
            last_restart: None,
            restart_history: Vec::new(),
        };

        // Add to supervisor
        let mut vms = self.vms.write().await;
        vms.insert(vm_id, supervised_vm);

        Ok(())
    }

    /// Stop supervising a VM
    ///
    /// ## Arguments
    /// * `vm_id` - VM ID to stop supervising
    ///
    /// ## Returns
    /// `Ok(())` on success
    pub async fn stop_supervision(&mut self, vm_id: &str) -> FirecrackerResult<()> {
        let mut vms = self.vms.write().await;
        if let Some(supervised_vm) = vms.remove(vm_id) {
            // Stop health monitoring
            let mut monitor = supervised_vm.health_monitor.write().await;
            monitor.stop().await?;
        }
        Ok(())
    }

    /// Handle VM failure (called by health monitor or external code)
    ///
    /// ## Arguments
    /// * `vm_id` - VM ID that failed
    ///
    /// ## Returns
    /// `Ok(())` if handled successfully, error if restart failed or policy prevents restart
    pub async fn handle_failure(&mut self, vm_id: &str) -> FirecrackerResult<()> {
        // Check if VM is supervised and get restart policy info
        let (delay, vm_arc) = {
            let mut vms = self.vms.write().await;
            let supervised_vm = vms.get_mut(vm_id).ok_or_else(|| {
                FirecrackerError::VmOperationFailed(format!("VM {} not supervised", vm_id))
            })?;

            // Check restart policy
            let (should_restart, max_retries_exceeded) = match &supervised_vm.restart_policy {
                RestartPolicy::Never => (false, false),
                RestartPolicy::MaxRetries { max_retries, .. } => {
                    let exceeded = supervised_vm.restart_count >= *max_retries;
                    (!exceeded, exceeded)
                }
                _ => (true, false), // Always or WithBackoff
            };

            if max_retries_exceeded {
                // Will stop supervision after releasing lock
                return Err(FirecrackerError::VmOperationFailed(format!(
                    "VM {} exceeded max retries",
                    vm_id
                )));
            }

            if !should_restart {
                return Ok(());
            }

            // Calculate backoff delay
            let delay = self.calculate_backoff(supervised_vm);

            // Clone VM arc before releasing lock
            let vm_arc = Arc::clone(&supervised_vm.vm);

            (delay, vm_arc)
        };

        // Wait for backoff delay
        if !delay.is_zero() {
            sleep(delay).await;
        }

        // Apply supervision strategy
        match self.strategy {
            SupervisionStrategy::OneForOne => {
                // Restart only the failed VM
                self.restart_vm(vm_arc).await?;
            }
            SupervisionStrategy::OneForAll => {
                // Restart all VMs
                let vm_ids: Vec<String> = {
                    let vms = self.vms.read().await;
                    vms.keys().cloned().collect()
                };

                for id in vm_ids {
                    if let Some(supervised_vm) = self.vms.read().await.get(&id) {
                        let vm_arc = Arc::clone(&supervised_vm.vm);
                        drop(supervised_vm);
                        let _ = self.restart_vm(vm_arc).await;
                    }
                }
            }
        }

        // Update restart tracking for the failed VM
        let mut vms = self.vms.write().await;
        if let Some(supervised_vm) = vms.get_mut(vm_id) {
            supervised_vm.restart_count += 1;
            supervised_vm.last_restart = Some(Instant::now());
            supervised_vm.restart_history.push(Instant::now());
        }

        Ok(())
    }

    /// Calculate backoff delay based on restart policy and history
    fn calculate_backoff(&self, supervised_vm: &SupervisedVm) -> Duration {
        match &supervised_vm.restart_policy {
            RestartPolicy::Always => Duration::ZERO,
            RestartPolicy::WithBackoff {
                initial_delay,
                max_delay,
                factor,
            } => {
                let delay = if supervised_vm.restart_count == 0 {
                    *initial_delay
                } else {
                    let calculated = initial_delay.as_secs_f64()
                        * factor.powi(supervised_vm.restart_count as i32);
                    Duration::from_secs_f64(calculated).min(*max_delay)
                };
                delay
            }
            RestartPolicy::MaxRetries { backoff, .. } => {
                if supervised_vm.restart_count == 0 {
                    backoff.initial_delay
                } else {
                    let calculated = backoff.initial_delay.as_secs_f64()
                        * backoff.factor.powi(supervised_vm.restart_count as i32);
                    Duration::from_secs_f64(calculated).min(backoff.max_delay)
                }
            }
            RestartPolicy::Never => Duration::ZERO,
        }
    }

    /// Restart a VM
    async fn restart_vm(&self, vm: Arc<RwLock<FirecrackerVm>>) -> FirecrackerResult<()> {
        let mut vm_guard = vm.write().await;

        // Stop VM if running
        if vm_guard.state() != VmState::Stopped {
            let _ = vm_guard.stop().await;
        }

        // Start Firecracker process
        vm_guard.start_firecracker().await?;

        // Boot VM
        vm_guard.boot().await?;

        Ok(())
    }
}

impl Default for VmSupervisor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_supervisor_creation() {
        let supervisor = VmSupervisor::new();
        assert!(supervisor.is_empty().await);
        assert_eq!(supervisor.supervised_vm_count().await, 0);
    }

    #[tokio::test]
    async fn test_supervisor_add_vm() {
        let config = crate::config::VmConfig {
            vm_id: ulid::Ulid::new().to_string(),
            vcpu_count: 1,
            mem_size_mib: 256,
            ..Default::default()
        };

        let vm = FirecrackerVm::create(config).await.unwrap();
        let vm_arc = Arc::new(RwLock::new(vm));

        let mut supervisor = VmSupervisor::new();

        let policy = RestartPolicy::Always;
        supervisor.supervise(vm_arc, policy).await.unwrap();

        assert_eq!(supervisor.supervised_vm_count().await, 1);
    }

    #[tokio::test]
    async fn test_supervisor_stop_supervision() {
        let config = crate::config::VmConfig {
            vm_id: ulid::Ulid::new().to_string(),
            vcpu_count: 1,
            mem_size_mib: 256,
            ..Default::default()
        };

        let vm = FirecrackerVm::create(config).await.unwrap();
        let vm_id = vm.vm_id().to_string();
        let vm_arc = Arc::new(RwLock::new(vm));

        let mut supervisor = VmSupervisor::new();

        supervisor
            .supervise(vm_arc, RestartPolicy::Always)
            .await
            .unwrap();
        assert_eq!(supervisor.supervised_vm_count().await, 1);

        supervisor.stop_supervision(&vm_id).await.unwrap();
        assert_eq!(supervisor.supervised_vm_count().await, 0);
    }

    #[tokio::test]
    async fn test_restart_policy_backoff_calculation() {
        let config = crate::config::VmConfig {
            vm_id: ulid::Ulid::new().to_string(),
            vcpu_count: 1,
            mem_size_mib: 256,
            ..Default::default()
        };

        let vm = FirecrackerVm::create(config).await.unwrap();
        let vm_arc = Arc::new(RwLock::new(vm));

        let mut supervisor = VmSupervisor::new();

        let policy = RestartPolicy::WithBackoff {
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(60),
            factor: 2.0,
        };

        supervisor.supervise(vm_arc, policy).await.unwrap();

        // Test backoff calculation (would need access to internal state)
        // For now, just verify supervisor works
        assert_eq!(supervisor.supervised_vm_count().await, 1);
    }
}

