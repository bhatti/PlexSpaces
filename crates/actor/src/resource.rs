// SPDX-License-Identifier: AGPL-3.0-or-later
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

/// Resource violation error
#[derive(Debug, thiserror::Error)]
pub enum ResourceViolation {
    #[error("CPU usage exceeded: allowed {allowed}%, actual {actual}%")]
    CpuExceeded { allowed: f32, actual: f32 },

    #[error("Memory usage exceeded: allowed {allowed} bytes, actual {actual} bytes")]
    MemoryExceeded { allowed: u64, actual: u64 },

    #[error("I/O operations exceeded: allowed {allowed}/s, actual {actual}/s")]
    IoExceeded { allowed: u32, actual: u32 },

    #[error("Network bandwidth exceeded: allowed {allowed} Mbps, actual {actual} Mbps")]
    NetworkExceeded { allowed: u32, actual: u32 },
}

impl ResourceViolation {
    pub fn code(&self) -> ResourceViolationCode {
        match self {
            ResourceViolation::CpuExceeded { .. } => ResourceViolationCode::ResourceViolationCodeCpuExceeded,
            ResourceViolation::MemoryExceeded { .. } => ResourceViolationCode::ResourceViolationCodeMemoryExceeded,
            ResourceViolation::IoExceeded { .. } => ResourceViolationCode::ResourceViolationCodeIoExceeded,
            ResourceViolation::NetworkExceeded { .. } => ResourceViolationCode::ResourceViolationCodeNetworkExceeded,
        }
    }
}

// ============================================================================
// Extension trait: ResourceContractExt — factory methods and validation
// ============================================================================

pub trait ResourceContractExt {
    fn cpu_intensive() -> ResourceContract;
    fn io_intensive() -> ResourceContract;
    fn network_intensive() -> ResourceContract;
    fn validate_usage(&self, usage: &ResourceUsage) -> Result<(), ResourceViolation>;
}

impl ResourceContractExt for ResourceContract {
    fn cpu_intensive() -> Self {
        ResourceContract {
            max_cpu_percent: 80.0,
            max_memory_bytes: 1024 * 1024 * 1024, // 1GB
            max_io_ops_per_sec: Some(100),
            guaranteed_bandwidth_mbps: None,
            max_execution_time: Some(prost_types::Duration { seconds: 300, nanos: 0 }),
        }
    }

    fn io_intensive() -> Self {
        ResourceContract {
            max_cpu_percent: 20.0,
            max_memory_bytes: 256 * 1024 * 1024, // 256MB
            max_io_ops_per_sec: Some(10000),
            guaranteed_bandwidth_mbps: None,
            max_execution_time: Some(prost_types::Duration { seconds: 60, nanos: 0 }),
        }
    }

    fn network_intensive() -> Self {
        ResourceContract {
            max_cpu_percent: 30.0,
            max_memory_bytes: 512 * 1024 * 1024, // 512MB
            max_io_ops_per_sec: Some(1000),
            guaranteed_bandwidth_mbps: Some(100),
            max_execution_time: Some(prost_types::Duration { seconds: 120, nanos: 0 }),
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

pub trait ActorHealthExt {
    fn healthy() -> ActorHealth;
    fn stuck(since: Duration) -> ActorHealth;
    fn check_stuck(last_message_elapsed: Duration, threshold: Duration) -> ActorHealth;
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
        let v = ResourceViolation::CpuExceeded { allowed: 10.0, actual: 50.0 };
        assert_eq!(v.code(), ResourceViolationCode::ResourceViolationCodeCpuExceeded);

        let v = ResourceViolation::MemoryExceeded { allowed: 100, actual: 200 };
        assert_eq!(v.code(), ResourceViolationCode::ResourceViolationCodeMemoryExceeded);
    }
}
