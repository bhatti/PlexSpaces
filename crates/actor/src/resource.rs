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

//! Resource-Aware Actor Scheduling - Preventing exhaustion and enabling intelligent placement
//!
//! ## Purpose
//! Implements Quickwit-inspired resource contracts for PlexSpaces actors, enabling intelligent
//! placement decisions, preventing resource exhaustion, and maintaining cluster health through
//! proactive monitoring and enforcement of resource limits.
//!
//! ## Design Philosophy
//! Following Quickwit's resource-aware approach, PlexSpaces provides:
//! - **Declared Resource Profiles**: Actors declare expected resource usage upfront
//! - **Contract Enforcement**: Runtime validates usage against declared contracts
//! - **Health Monitoring**: Continuous tracking of actor health and resource consumption
//! - **Intelligent Placement**: Schedulers use resource profiles for bin-packing actors to nodes
//! - **Violation Actions**: Configurable responses to resource limit breaches
//!
//! ## Key Concepts
//!
//! ### Resource Profile (What kind of actor is this?)
//! Categorizes actors by primary resource consumption pattern:
//! - **CpuIntensive**: Compute-heavy (video encoding, crypto, ML inference)
//! - **MemoryIntensive**: Large state (caching, in-memory aggregation)
//! - **IoIntensive**: Disk-heavy (database, file processing)
//! - **NetworkIntensive**: API calls, streaming, external integrations
//! - **Balanced**: Mixed workload (default)
//!
//! ### Resource Contract (What limits apply?)
//! Declares resource requirements and maximum limits:
//! - **max_cpu_percent**: CPU usage cap (0-100%)
//! - **max_memory_bytes**: Memory usage cap
//! - **max_io_ops_per_sec**: I/O operations cap
//! - **guaranteed_bandwidth_mbps**: Network bandwidth guarantee
//! - **max_execution_time**: Per-message timeout
//!
//! ### Resource Usage (What is currently consumed?)
//! Tracks real-time resource consumption:
//! - **cpu_percent**: Current CPU usage
//! - **memory_bytes**: Current memory usage
//! - **io_ops_per_sec**: Current I/O rate
//! - **network_mbps**: Current network bandwidth
//!
//! ### Actor Health (Is the actor functioning normally?)
//! Monitors actor health based on resource usage and message processing:
//! - **Healthy**: Normal operation, resources within limits
//! - **Degraded**: Elevated resource usage but still functional
//! - **Stuck**: Not processing messages (possible deadlock/hang)
//! - **Failed**: Actor crashed or resource limits repeatedly exceeded
//!
//! ## Architecture Integration
//!
//! Resource management integrates with PlexSpaces core:
//! - **ActorConfig**: ResourceContract attached to actor configuration
//! - **Scheduler**: Uses ResourceProfile for node placement decisions
//! - **Supervisor**: Monitors ActorHealth, restarts Failed/Stuck actors
//! - **Metrics**: ResourceUsage exported to Prometheus/Grafana
//! - **Multi-Tenancy (future)**: ResourceQuota per IsolationContext
//!
//! ## Examples
//!
//! ### CPU-Intensive Actor (Video Encoder)
//! ```rust,ignore
//! use plexspaces::actor::resource::{ResourceContract, ResourceProfile};
//!
//! let actor_config = ActorConfig {
//!     resource_profile: ResourceProfile::CpuIntensive,
//!     resource_contract: ResourceContract::cpu_intensive(),
//!     ..Default::default()
//! };
//!
//! // Contract: 80% CPU, 1GB memory, 300s timeout
//! // Scheduler will place on node with available CPU capacity
//! let actor = spawn_actor("video-encoder", actor_config).await?;
//! ```
//!
//! ### Memory-Intensive Actor (Cache)
//! ```rust,ignore
//! let contract = ResourceContract {
//!     max_cpu_percent: 10.0,        // Low CPU
//!     max_memory_bytes: 4 * 1024 * 1024 * 1024,  // 4GB memory
//!     max_io_ops_per_sec: Some(1000),
//!     max_execution_time: Some(Duration::from_secs(10)),
//!     ..Default::default()
//! };
//!
//! let actor_config = ActorConfig {
//!     resource_profile: ResourceProfile::MemoryIntensive,
//!     resource_contract: contract,
//!     ..Default::default()
//! };
//!
//! // Scheduler will place on node with sufficient free memory
//! let cache_actor = spawn_actor("user-cache", actor_config).await?;
//! ```
//!
//! ### I/O-Intensive Actor (Database Connector)
//! ```rust,ignore
//! let contract = ResourceContract::io_intensive();
//! // Contract: 20% CPU, 256MB memory, 10000 IOPS
//!
//! let actor_config = ActorConfig {
//!     resource_profile: ResourceProfile::IoIntensive,
//!     resource_contract: contract,
//!     ..Default::default()
//! };
//!
//! // Scheduler may prefer nodes with fast local SSDs
//! let db_actor = spawn_actor("db-connector", actor_config).await?;
//! ```
//!
//! ### Network-Intensive Actor (API Gateway)
//! ```rust,ignore
//! let contract = ResourceContract::network_intensive();
//! // Contract: 30% CPU, 512MB memory, 100 Mbps bandwidth
//!
//! let actor_config = ActorConfig {
//!     resource_profile: ResourceProfile::NetworkIntensive,
//!     resource_contract: contract,
//!     ..Default::default()
//! };
//!
//! // Scheduler may prefer nodes with high network bandwidth
//! let gateway = spawn_actor("api-gateway", actor_config).await?;
//! ```
//!
//! ## Resource Validation and Enforcement
//!
//! ### Validating Resource Usage
//! ```rust,ignore
//! let contract = ResourceContract::default();
//! let usage = ResourceUsage {
//!     cpu_percent: 5.0,
//!     memory_bytes: 50 * 1024 * 1024,  // 50MB
//!     io_ops_per_sec: 100,
//!     network_mbps: 10,
//! };
//!
//! // Check if usage is within contract
//! match contract.validate_usage(&usage) {
//!     Ok(()) => println!("Actor within resource limits"),
//!     Err(ResourceViolation::CpuExceeded { allowed, actual }) => {
//!         println!("CPU limit exceeded: {}% > {}%", actual, allowed);
//!         // Take action: throttle, backpressure, or terminate
//!     },
//!     Err(e) => println!("Resource violation: {}", e),
//! }
//! ```
//!
//! ### Health Monitoring (Stuck Detection)
//! ```rust,ignore
//! use plexspaces::actor::resource::ActorHealth;
//!
//! // Check if actor is stuck (not processing messages)
//! let last_message_elapsed = Duration::from_secs(60);
//! let stuck_threshold = Duration::from_secs(30);
//!
//! let health = ActorHealth::check_stuck(last_message_elapsed, stuck_threshold);
//!
//! match health {
//!     ActorHealth::Healthy => { /* continue */ },
//!     ActorHealth::Stuck { since } => {
//!         println!("Actor stuck for {:?}, restarting...", since);
//!         supervisor.restart_child(actor_id).await?;
//!     },
//!     ActorHealth::Failed { reason } => {
//!         println!("Actor failed: {}", reason);
//!     },
//!     _ => { /* handle degraded */ }
//! }
//! ```
//!
//! ## Design Decisions
//!
//! ### Why separate ResourceProfile and ResourceContract?
//! - **Profile**: Describes actor category (cpu/memory/io/network) for placement hints
//! - **Contract**: Specifies exact limits for runtime enforcement
//! - **Orthogonal**: Profile informs scheduling, Contract enforces limits
//! - **Example**: CpuIntensive profile + custom contract with lower limits for testing
//!
//! ### Why predefined contracts (cpu_intensive(), io_intensive())?
//! - **Convenience**: Common patterns pre-configured
//! - **Best Practices**: Prevent overly permissive defaults
//! - **Customizable**: Users can override any field
//! - **Evolution**: Defaults can be tuned based on production data
//!
//! ### Why validate_usage() returns Result instead of bool?
//! - **Rich Errors**: Detailed violation information (which limit, allowed vs actual)
//! - **Actionable**: Error type guides enforcement action (log, throttle, terminate)
//! - **Debuggable**: Clear error messages for troubleshooting
//!
//! ### Why Optional for some contract fields?
//! - **Flexibility**: Not all actors need all limits
//! - **Example**: Pure compute actor may not care about network bandwidth
//! - **Backwards Compatibility**: Can add new limits without breaking existing code
//!
//! ## Comparison to Other Systems
//!
//! | Feature | PlexSpaces | Quickwit | Kubernetes Pods | Erlang Processes |
//! |---------|------------|----------|-----------------|------------------|
//! | **Resource profiles** | ✅ 5 types | ✅ Custom | ⚠️ Via node affinity | ❌ No |
//! | **Contracts upfront** | ✅ Yes | ✅ Yes | ✅ Requests/Limits | ❌ No |
//! | **Runtime enforcement** | ✅ Yes | ✅ Yes | ✅ cgroups | ⚠️ Process limits |
//! | **Health checks** | ✅ Built-in | ✅ Built-in | ✅ Liveness/Readiness | ⚠️ Monitors |
//! | **Stuck detection** | ✅ Yes | ✅ Yes | ⚠️ Manual | ✅ Process flags |
//!
//! ## Performance Characteristics
//!
//! - **Profile Assignment**: O(1) - enum copy
//! - **Contract Validation**: O(1) - simple field comparisons
//! - **Health Check**: O(1) - duration comparison
//! - **Memory Overhead**: ~128 bytes per actor (profile + contract + usage)
//!
//! ## Future Enhancements
//!
//! ### 1. Dynamic Resource Adjustment
//! ```rust,ignore
//! // Automatically adjust limits based on actual usage patterns
//! contract.auto_tune(historical_usage).await?;
//! ```
//!
//! ### 2. Resource Quotas per Tenant (Multi-Tenancy)
//! ```rust,ignore
//! // Tenant-level resource limits (from proto)
//! let quota = ResourceQuota {
//!     max_actors: 1000,
//!     max_memory_mb: 8192,
//!     max_cpu_percent: 50.0,
//!     rate_limit_msg_per_sec: 10000,
//! };
//! ```
//!
//! ### 3. Predictive Scaling
//! ```rust,ignore
//! // Predict resource needs based on message rate trends
//! let predicted = predict_resource_needs(&usage_history)?;
//! scheduler.scale_up_if_needed(predicted).await?;
//! ```
//!
//! ### 4. Circuit Breakers for Resource Violations
//! ```rust,ignore
//! // Temporarily stop sending messages if actor is overloaded
//! if contract.validate_usage(&usage).is_err() {
//!     circuit_breaker.trip(actor_id).await?;
//! }
//! ```

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Resource profile for an actor
/// Indicates what type of resources the actor primarily consumes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ResourceProfile {
    /// CPU-intensive workload (computation, crypto, compression)
    CpuIntensive,
    /// Memory-intensive workload (caching, large state)
    MemoryIntensive,
    /// I/O-intensive workload (disk operations, database)
    IoIntensive,
    /// Network-intensive workload (API calls, streaming)
    NetworkIntensive,
    /// Balanced workload (default)
    #[default]
    Balanced,
}

/// Resource contract for an actor
/// Declares resource requirements and limits upfront
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceContract {
    /// Maximum CPU usage as percentage (0.0 - 100.0)
    pub max_cpu_percent: f32,
    /// Maximum memory usage in bytes
    pub max_memory_bytes: usize,
    /// Maximum I/O operations per second
    pub max_io_ops_per_sec: Option<u32>,
    /// Guaranteed network bandwidth in Mbps
    pub guaranteed_bandwidth_mbps: Option<u32>,
    /// Maximum execution time per message
    pub max_execution_time: Option<Duration>,
}

impl Default for ResourceContract {
    fn default() -> Self {
        ResourceContract {
            max_cpu_percent: 10.0,               // 10% CPU
            max_memory_bytes: 100 * 1024 * 1024, // 100MB
            max_io_ops_per_sec: None,
            guaranteed_bandwidth_mbps: None,
            max_execution_time: Some(Duration::from_secs(30)),
        }
    }
}

impl ResourceContract {
    /// Create a contract for CPU-intensive work
    pub fn cpu_intensive() -> Self {
        ResourceContract {
            max_cpu_percent: 80.0,
            max_memory_bytes: 1024 * 1024 * 1024, // 1GB
            max_io_ops_per_sec: Some(100),
            guaranteed_bandwidth_mbps: None,
            max_execution_time: Some(Duration::from_secs(300)),
        }
    }

    /// Create a contract for I/O-intensive work
    pub fn io_intensive() -> Self {
        ResourceContract {
            max_cpu_percent: 20.0,
            max_memory_bytes: 256 * 1024 * 1024, // 256MB
            max_io_ops_per_sec: Some(10000),
            guaranteed_bandwidth_mbps: None,
            max_execution_time: Some(Duration::from_secs(60)),
        }
    }

    /// Create a contract for network-intensive work
    pub fn network_intensive() -> Self {
        ResourceContract {
            max_cpu_percent: 30.0,
            max_memory_bytes: 512 * 1024 * 1024, // 512MB
            max_io_ops_per_sec: Some(1000),
            guaranteed_bandwidth_mbps: Some(100),
            max_execution_time: Some(Duration::from_secs(120)),
        }
    }

    /// Validate if current usage is within contract
    pub fn validate_usage(&self, usage: &ResourceUsage) -> Result<(), ResourceViolation> {
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

/// Current resource usage of an actor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceUsage {
    /// Current CPU usage percentage
    pub cpu_percent: f32,
    /// Current memory usage in bytes
    pub memory_bytes: usize,
    /// Current I/O operations per second
    pub io_ops_per_sec: u32,
    /// Current network bandwidth in Mbps
    pub network_mbps: u32,
}

impl Default for ResourceUsage {
    fn default() -> Self {
        ResourceUsage {
            cpu_percent: 0.0,
            memory_bytes: 0,
            io_ops_per_sec: 0,
            network_mbps: 0,
        }
    }
}

/// Resource violation error
#[derive(Debug, thiserror::Error)]
pub enum ResourceViolation {
    #[error("CPU usage exceeded: allowed {allowed}%, actual {actual}%")]
    CpuExceeded { allowed: f32, actual: f32 },

    #[error("Memory usage exceeded: allowed {allowed} bytes, actual {actual} bytes")]
    MemoryExceeded { allowed: usize, actual: usize },

    #[error("I/O operations exceeded: allowed {allowed}/s, actual {actual}/s")]
    IoExceeded { allowed: u32, actual: u32 },

    #[error("Network bandwidth exceeded: allowed {allowed} Mbps, actual {actual} Mbps")]
    NetworkExceeded { allowed: u32, actual: u32 },
}

/// Runtime type for actor execution
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum RuntimeType {
    /// Default Tokio runtime
    #[default]
    Default,
    /// CPU-optimized runtime (more worker threads)
    CpuOptimized,
    /// I/O-optimized runtime (more blocking threads)
    IoOptimized,
    /// Isolated runtime (security/stability)
    Isolated,
}

/// Actor health status (Quickwit-inspired)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActorHealth {
    /// Actor is healthy and processing messages
    Healthy,
    /// Actor is degraded but still running
    Degraded,
    /// Actor appears stuck (not processing messages)
    Stuck { since: Duration },
    /// Actor has failed
    Failed { reason: String },
}

impl ActorHealth {
    /// Check if actor is stuck based on last message time
    pub fn check_stuck(last_message_elapsed: Duration, threshold: Duration) -> Self {
        if last_message_elapsed > threshold {
            ActorHealth::Stuck {
                since: last_message_elapsed,
            }
        } else {
            ActorHealth::Healthy
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        usage.cpu_percent = 5.0;
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
        assert_eq!(health, ActorHealth::Healthy);

        let health = ActorHealth::check_stuck(Duration::from_secs(60), Duration::from_secs(30));
        assert!(matches!(health, ActorHealth::Stuck { .. }));
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
}
