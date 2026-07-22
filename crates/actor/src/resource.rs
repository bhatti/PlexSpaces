// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! Resource-Aware Actor Scheduling - Preventing exhaustion and enabling intelligent placement.
//!
//! Data contracts (`ResourceProfile`, `ResourceContract`, `ResourceUsage`, `ActorHealth`) are
//! proto-generated. This module re-exports those types and provides Rust-only extension methods.

use plexspaces_proto::actor::v1::{ActorHealthStatus, ResourceViolationCode};
use std::time::Duration;

// ============================================================================
// Re-exports from proto (canonical data contracts)
// ============================================================================

pub use plexspaces_proto::actor::v1::{
    ActorHealth, ActorHealthStatus as ProtoActorHealthStatus, ResourceContract, ResourceProfile,
    ResourceUsage,
};

// ============================================================================
// ResourceViolation — stays in Rust (thiserror ADT, non-serializable payloads)
// ============================================================================

/// Resource limit exceeded by an actor at runtime.
#[derive(Debug, thiserror::Error)]
pub enum ResourceViolation {
    /// CPU utilization exceeds the contract ceiling.
    #[error("CPU usage exceeded: allowed {allowed}%, actual {actual}%")]
    CpuExceeded {
        /// Configured CPU percent ceiling.
        allowed: f32,
        /// Measured CPU percent at time of violation.
        actual: f32,
    },

    /// Memory usage exceeds the contract ceiling.
    #[error("Memory usage exceeded: allowed {allowed} bytes, actual {actual} bytes")]
    MemoryExceeded {
        /// Configured memory ceiling in bytes.
        allowed: u64,
        /// Measured memory usage in bytes at time of violation.
        actual: u64,
    },

    /// I/O operations per second exceed the contract ceiling.
    #[error("I/O operations exceeded: allowed {allowed}/s, actual {actual}/s")]
    IoExceeded {
        /// Configured I/O ops/s ceiling.
        allowed: u32,
        /// Measured I/O ops/s at time of violation.
        actual: u32,
    },

    /// Network bandwidth exceeds the contract ceiling.
    #[error("Network bandwidth exceeded: allowed {allowed} Mbps, actual {actual} Mbps")]
    NetworkExceeded {
        /// Configured bandwidth ceiling in Mbps.
        allowed: u32,
        /// Measured bandwidth in Mbps at time of violation.
        actual: u32,
    },
}

impl ResourceViolation {
    /// Returns the proto `ResourceViolationCode` enum value for this violation.
    pub fn code(&self) -> ResourceViolationCode {
        match self {
            ResourceViolation::CpuExceeded { .. } => {
                ResourceViolationCode::ResourceViolationCodeCpuExceeded
            }
            ResourceViolation::MemoryExceeded { .. } => {
                ResourceViolationCode::ResourceViolationCodeMemoryExceeded
            }
            ResourceViolation::IoExceeded { .. } => {
                ResourceViolationCode::ResourceViolationCodeIoExceeded
            }
            ResourceViolation::NetworkExceeded { .. } => {
                ResourceViolationCode::ResourceViolationCodeNetworkExceeded
            }
        }
    }
}

// ============================================================================
// Extension trait: ResourceContractExt — factory methods and validation
// ============================================================================

/// Factory methods and usage validation for `ResourceContract`.
pub trait ResourceContractExt {
    /// Returns a preset contract suitable for CPU-bound actors (80% CPU, 1 GB RAM).
    fn cpu_intensive() -> ResourceContract;
    /// Returns a preset contract suitable for I/O-bound actors (20% CPU, 10 000 IOPS).
    fn io_intensive() -> ResourceContract;
    /// Returns a preset contract suitable for network-bound actors (30% CPU, 100 Mbps).
    fn network_intensive() -> ResourceContract;
    /// Checks `usage` against this contract; returns `Err(ResourceViolation)` on first breach.
    fn validate_usage(&self, usage: &ResourceUsage) -> Result<(), ResourceViolation>;
}

impl ResourceContractExt for ResourceContract {
    fn cpu_intensive() -> Self {
        ResourceContract {
            max_cpu_percent: 80.0,
            max_memory_bytes: 1024 * 1024 * 1024, // 1GB
            max_io_ops_per_sec: Some(100),
            guaranteed_bandwidth_mbps: None,
            max_execution_time: Some(prost_types::Duration {
                seconds: 300,
                nanos: 0,
            }),
        }
    }

    fn io_intensive() -> Self {
        ResourceContract {
            max_cpu_percent: 20.0,
            max_memory_bytes: 256 * 1024 * 1024, // 256MB
            max_io_ops_per_sec: Some(10000),
            guaranteed_bandwidth_mbps: None,
            max_execution_time: Some(prost_types::Duration {
                seconds: 60,
                nanos: 0,
            }),
        }
    }

    fn network_intensive() -> Self {
        ResourceContract {
            max_cpu_percent: 30.0,
            max_memory_bytes: 512 * 1024 * 1024, // 512MB
            max_io_ops_per_sec: Some(1000),
            guaranteed_bandwidth_mbps: Some(100),
            max_execution_time: Some(prost_types::Duration {
                seconds: 120,
                nanos: 0,
            }),
        }
    }

    fn validate_usage(&self, usage: &ResourceUsage) -> Result<(), ResourceViolation> {
        if usage.cpu_percent > self.max_cpu_percent {
            return Err(ResourceViolation::CpuExceeded {
                allowed: self.max_cpu_percent,
                actual: usage.cpu_percent,
            });
        }

        if usage.memory_bytes > self.max_memory_bytes {
            return Err(ResourceViolation::MemoryExceeded {
                allowed: self.max_memory_bytes,
                actual: usage.memory_bytes,
            });
        }

        if let Some(max_io) = self.max_io_ops_per_sec {
            if usage.io_ops_per_sec > max_io {
                return Err(ResourceViolation::IoExceeded {
                    allowed: max_io,
                    actual: usage.io_ops_per_sec,
                });
            }
        }

        Ok(())
    }
}

// ============================================================================
// Extension trait: ActorHealthExt — factory method for stuck detection
// ============================================================================

/// Factory methods and stuck-detection helpers for `ActorHealth`.
pub trait ActorHealthExt {
    /// Returns a healthy `ActorHealth` with no stuck_since or failure_reason.
    fn healthy() -> ActorHealth;
    /// Returns a stuck `ActorHealth` with `stuck_since` set to `since`.
    fn stuck(since: Duration) -> ActorHealth;
    /// Returns `healthy()` when `last_message_elapsed <= threshold`, otherwise `stuck(elapsed)`.
    fn check_stuck(last_message_elapsed: Duration, threshold: Duration) -> ActorHealth;
    /// Returns true iff `status == Healthy`.
    fn is_healthy(&self) -> bool;
}

impl ActorHealthExt for ActorHealth {
    fn healthy() -> ActorHealth {
        ActorHealth {
            status: ActorHealthStatus::ActorHealthStatusHealthy as i32,
            stuck_since: None,
            failure_reason: None,
        }
    }

    fn stuck(since: Duration) -> ActorHealth {
        ActorHealth {
            status: ActorHealthStatus::ActorHealthStatusStuck as i32,
            stuck_since: Some(prost_types::Duration {
                seconds: since.as_secs() as i64,
                nanos: since.subsec_nanos() as i32,
            }),
            failure_reason: None,
        }
    }

    fn check_stuck(last_message_elapsed: Duration, threshold: Duration) -> ActorHealth {
        if last_message_elapsed > threshold {
            ActorHealth::stuck(last_message_elapsed)
        } else {
            ActorHealth::healthy()
        }
    }

    fn is_healthy(&self) -> bool {
        ActorHealthStatus::try_from(self.status)
            .map(|s| s == ActorHealthStatus::ActorHealthStatusHealthy)
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resource::{ActorHealthExt, ResourceContractExt};

    #[test]
    fn test_resource_contract_validation() {
        let contract = ResourceContract::default();
        let mut usage = ResourceUsage::default();

        // Should pass with default usage
        assert!(contract.validate_usage(&usage).is_ok());

        // Should fail with excessive CPU
        usage.cpu_percent = 50.0;
        let result = contract.validate_usage(&usage);
        assert!(matches!(result, Err(ResourceViolation::CpuExceeded { .. })));

        // Should fail with excessive memory
        usage.cpu_percent = 0.0;
        usage.memory_bytes = 200 * 1024 * 1024;
        let result = contract.validate_usage(&usage);
        assert!(matches!(
            result,
            Err(ResourceViolation::MemoryExceeded { .. })
        ));
    }

    #[test]
    fn test_actor_health_stuck_detection() {
        let health = ActorHealth::check_stuck(Duration::from_secs(10), Duration::from_secs(30));
        assert!(health.is_healthy());

        let health = ActorHealth::check_stuck(Duration::from_secs(60), Duration::from_secs(30));
        assert_eq!(
            ActorHealthStatus::try_from(health.status).unwrap(),
            ActorHealthStatus::ActorHealthStatusStuck
        );
        assert!(health.stuck_since.is_some());
    }

    #[test]
    fn test_specialized_contracts() {
        let cpu_contract = ResourceContract::cpu_intensive();
        assert!(cpu_contract.max_cpu_percent > 50.0);

        let io_contract = ResourceContract::io_intensive();
        assert!(io_contract.max_io_ops_per_sec.unwrap() > 5000);

        let net_contract = ResourceContract::network_intensive();
        assert!(net_contract.guaranteed_bandwidth_mbps.is_some());
    }

    #[test]
    fn test_resource_violation_codes() {
        let v = ResourceViolation::CpuExceeded {
            allowed: 10.0,
            actual: 50.0,
        };
        assert_eq!(
            v.code(),
            ResourceViolationCode::ResourceViolationCodeCpuExceeded
        );

        let v = ResourceViolation::MemoryExceeded {
            allowed: 100,
            actual: 200,
        };
        assert_eq!(
            v.code(),
            ResourceViolationCode::ResourceViolationCodeMemoryExceeded
        );
    }
}
