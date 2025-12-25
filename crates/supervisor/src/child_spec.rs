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

//! Child specification - unified child process definition (proto-first design)
//!
//! ## Purpose
//! Defines a child process (actor or supervisor) managed by a supervisor.
//! This type mirrors `proto/plexspaces/v1/supervision/supervision.proto` ChildSpec exactly.
//!
//! ## Erlang/OTP Equivalent
//! Maps to Erlang's child_spec:
//! ```erlang
//! #{id => ChildId,
//!   start => {Module, Function, Args},
//!   restart => permanent | temporary | transient,
//!   shutdown => brutal_kill | Timeout | infinity,
//!   type => worker | supervisor}
//! ```
//!
//! ## Proto-First Design
//! This struct is defined in `proto/plexspaces/v1/supervision/supervision.proto`
//! and generated via `buf generate`. This module provides Rust-friendly wrappers
//! and conversion methods.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use plexspaces_actor::{Actor, ActorRef};
use plexspaces_core::ActorError;
use plexspaces_proto::supervision::v1::{ChildSpec as ProtoChildSpec, ChildType as ProtoChildType, RestartStrategy as ProtoRestartStrategy};
use prost_types;

/// Boxed future type for async start functions
pub type BoxFuture<'a, T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send + 'a>>;

/// Child specification - mirrors proto ChildSpec exactly
///
/// ## Purpose
/// Defines a child process (actor or supervisor) managed by a supervisor.
/// This is the unified specification that replaces ActorSpec.
///
/// ## Design
/// - Proto-first: Defined in `proto/plexspaces/v1/supervision/supervision.proto`
/// - Erlang/OTP semantics: Maps directly to Erlang child_spec
/// - Type-safe: Uses enums for restart strategy and child type
#[derive(Clone)]
pub struct ChildSpec {
    /// Unique identifier for this child within the supervisor
    /// This is the child's local name, like "worker1" or "db_supervisor"
    pub child_id: String,
    
    /// ID of the actor or supervisor to supervise
    /// - For actors: actor ID (e.g., "worker1@localhost")
    /// - For supervisors: supervisor ID (e.g., "db-supervisor")
    /// The supervisor monitors this process regardless of its type
    pub actor_or_supervisor_id: String,
    
    /// How to handle child failures
    pub restart_strategy: RestartStrategy,
    
    /// Shutdown timeout for graceful termination
    /// - None/0 = brutal_kill (immediate)
    /// - Some(ms) = graceful shutdown with timeout
    /// - For supervisors: typically set high or infinity to allow children to shutdown
    pub shutdown_timeout: Option<Duration>,
    
    /// Child type: actor (worker) or supervisor
    pub child_type: ChildType,
    
    /// Metadata for child configuration
    /// Can include:
    /// - "start_module": Module name for recreation
    /// - "start_function": Function to call
    /// - "supervisor_strategy": For CHILD_TYPE_SUPERVISOR, its strategy
    pub metadata: HashMap<String, String>,
    
    /// Facet configuration (for automatic attachment during actor creation)
    /// Facets are attached in priority order (high priority first) before actor.init() is called
    /// All facets are automatically restored during supervisor restart
    /// Phase 1: Unified Lifecycle - Multiple facets support
    pub facets: Vec<plexspaces_proto::common::v1::Facet>,
    
    /// Factory function to create/start the child
    /// Returns StartedChild which can be either an Actor or Supervisor
    pub start_fn: StartFn,
}

/// How to start a child - factory function
///
/// ## Purpose
/// Factory function that creates and starts a child (actor or supervisor).
/// Returns a StartedChild enum that can be either an Actor or Supervisor.
///
/// ## Erlang Equivalent
/// Maps to Erlang's `start => {Module, Function, Args}` in child_spec.
pub type StartFn = Arc<dyn Fn() -> BoxFuture<'static, Result<StartedChild, ActorError>> + Send + Sync>;

/// Result of starting a child
///
/// ## Purpose
/// Represents a successfully started child, which can be either:
/// - A worker (Actor with ActorRef)
/// - A supervisor (Supervisor)
///
/// ## Erlang Equivalent
/// In Erlang, `start_link/1` returns `{ok, Pid}` for both workers and supervisors.
/// This enum provides type safety while maintaining the unified interface.
pub enum StartedChild {
    /// Worker child (actor)
    Worker {
        /// The actor instance
        actor: Actor,
        /// Actor reference for messaging
        actor_ref: ActorRef,
    },
    /// Supervisor child (nested supervisor)
    Supervisor {
        /// The supervisor instance
        supervisor: crate::Supervisor,
    },
}

/// Restart strategy - how to handle child failures
///
/// ## Purpose
/// Determines when and how to restart a child after it terminates.
///
/// ## Erlang Equivalent
/// Maps to Erlang's restart policy in child_spec:
/// - `permanent`: Always restart
/// - `transient`: Restart only on abnormal exit
/// - `temporary`: Never restart
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartStrategy {
    /// Always restart (default for most actors)
    Permanent,
    /// Restart only on abnormal exit (not normal or shutdown)
    Transient,
    /// Never restart (one-shot processes)
    Temporary,
}

/// Child type - worker or supervisor
///
/// ## Purpose
/// Distinguishes between worker actors and nested supervisors.
///
/// ## Erlang Equivalent
/// Maps to Erlang's `type => worker | supervisor` in child_spec.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChildType {
    /// Worker process (actor)
    Actor,
    /// Supervisor process (manages other children)
    Supervisor,
}

/// Shutdown specification - Erlang semantics
///
/// ## Purpose
/// Defines how to shutdown a child gracefully.
///
/// ## Erlang Equivalent
/// Maps to Erlang's `shutdown => brutal_kill | Timeout | infinity` in child_spec.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownSpec {
    /// Kill immediately (no graceful shutdown)
    BrutalKill,
    /// Graceful shutdown with timeout
    Timeout(Duration),
    /// Wait indefinitely (for supervisors)
    Infinity,
}

impl ChildSpec {
    /// Create a new ChildSpec for a worker (actor)
    ///
    /// ## Arguments
    /// * `child_id` - Unique identifier within supervisor
    /// * `actor_id` - Actor ID to supervise
    /// * `start_fn` - Factory function to create/start the actor
    ///
    /// ## Example
    /// ```rust,ignore
    /// let spec = ChildSpec::worker("worker1", "worker1@node1", Arc::new(|| {
    ///     Box::pin(async move {
    ///         let actor = Actor::new(...);
    ///         let actor_ref = ActorRef::local(...);
    ///         Ok(StartedChild::Worker { actor, actor_ref })
    ///     })
    /// }));
    /// ```
    pub fn worker(
        child_id: String,
        actor_id: String,
        start_fn: StartFn,
    ) -> Self {
        Self {
            child_id,
            actor_or_supervisor_id: actor_id,
            restart_strategy: RestartStrategy::Permanent,
            shutdown_timeout: Some(Duration::from_secs(5)),
            child_type: ChildType::Actor,
            metadata: HashMap::new(),
            facets: Vec::new(),
            start_fn,
        }
    }

    /// Create a new ChildSpec for a supervisor
    ///
    /// ## Arguments
    /// * `child_id` - Unique identifier within supervisor
    /// * `supervisor_id` - Supervisor ID to supervise
    /// * `start_fn` - Factory function to create/start the supervisor
    ///
    /// ## Example
    /// ```rust,ignore
    /// let spec = ChildSpec::supervisor("db-supervisor", "db-supervisor", Arc::new(|| {
    ///     Box::pin(async move {
    ///         let supervisor = Supervisor::new(...);
    ///         Ok(StartedChild::Supervisor { supervisor })
    ///     })
    /// }));
    /// ```
    pub fn supervisor(
        child_id: String,
        supervisor_id: String,
        start_fn: StartFn,
    ) -> Self {
        Self {
            child_id,
            actor_or_supervisor_id: supervisor_id,
            restart_strategy: RestartStrategy::Permanent,
            shutdown_timeout: None, // Infinity for supervisors
            child_type: ChildType::Supervisor,
            metadata: HashMap::new(),
            facets: Vec::new(),
            start_fn,
        }
    }

    /// Set restart strategy
    pub fn with_restart(mut self, strategy: RestartStrategy) -> Self {
        self.restart_strategy = strategy;
        self
    }

    /// Set shutdown timeout
    pub fn with_shutdown(mut self, shutdown: ShutdownSpec) -> Self {
        self.shutdown_timeout = match shutdown {
            ShutdownSpec::BrutalKill => Some(Duration::ZERO),
            ShutdownSpec::Timeout(duration) => Some(duration),
            ShutdownSpec::Infinity => None,
        };
        self
    }

    /// Add metadata
    pub fn with_metadata(mut self, key: String, value: String) -> Self {
        self.metadata.insert(key, value);
        self
    }

    /// Add facet configuration (Phase 1: Unified Lifecycle)
    ///
    /// ## Purpose
    /// Adds a facet to be automatically attached when the actor is created.
    /// Facets are attached in priority order (high priority first) before actor.init() is called.
    ///
    /// ## Arguments
    /// * `facet` - Facet configuration from proto
    ///
    /// ## Returns
    /// Self for method chaining
    pub fn with_facet(mut self, facet: plexspaces_proto::common::v1::Facet) -> Self {
        self.facets.push(facet);
        self
    }

    /// Add multiple facets (Phase 1: Unified Lifecycle)
    ///
    /// ## Purpose
    /// Adds multiple facets to be automatically attached when the actor is created.
    /// Facets should be provided in priority order (high priority first).
    ///
    /// ## Arguments
    /// * `facets` - Vector of facet configurations
    ///
    /// ## Returns
    /// Self for method chaining
    pub fn with_facets(mut self, facets: Vec<plexspaces_proto::common::v1::Facet>) -> Self {
        self.facets.extend(facets);
        self
    }

    /// Convert to proto ChildSpec
    pub fn to_proto(&self) -> ProtoChildSpec {
        ProtoChildSpec {
            child_id: self.child_id.clone(),
            actor_or_supervisor_id: self.actor_or_supervisor_id.clone(),
            restart_strategy: self.restart_strategy.to_proto() as i32,
            shutdown_timeout: self.shutdown_timeout.map(|d| prost_types::Duration {
                seconds: d.as_secs() as i64,
                nanos: d.subsec_nanos() as i32,
            }),
            child_type: self.child_type.to_proto() as i32,
            metadata: self.metadata.clone(),
            facets: self.facets.clone(),
        }
    }

    /// Convert from proto ChildSpec
    ///
    /// ## Note
    /// This method cannot reconstruct the `start_fn` since it's not serializable.
    /// The caller must provide the start_fn separately.
    pub fn from_proto(proto: &ProtoChildSpec, start_fn: StartFn) -> Self {
        Self {
            child_id: proto.child_id.clone(),
            actor_or_supervisor_id: proto.actor_or_supervisor_id.clone(),
            restart_strategy: RestartStrategy::from_proto(proto.restart_strategy),
            shutdown_timeout: proto.shutdown_timeout.as_ref().map(|d| {
                Duration::from_secs(d.seconds as u64) + Duration::from_nanos(d.nanos as u64)
            }),
            child_type: ChildType::from_proto(proto.child_type),
            metadata: proto.metadata.clone(),
            facets: proto.facets.clone(),
            start_fn,
        }
    }
}

impl RestartStrategy {
    /// Convert to proto RestartStrategy enum
    pub fn to_proto(&self) -> ProtoRestartStrategy {
        match self {
            RestartStrategy::Permanent => ProtoRestartStrategy::Permanent,
            RestartStrategy::Transient => ProtoRestartStrategy::Transient,
            RestartStrategy::Temporary => ProtoRestartStrategy::Temporary,
        }
    }

    /// Convert from proto RestartStrategy enum
    pub fn from_proto(proto: i32) -> Self {
        match ProtoRestartStrategy::try_from(proto).unwrap_or(ProtoRestartStrategy::RestartStrategyUnspecified) {
            ProtoRestartStrategy::Permanent => RestartStrategy::Permanent,
            ProtoRestartStrategy::Transient => RestartStrategy::Transient,
            ProtoRestartStrategy::Temporary => RestartStrategy::Temporary,
            ProtoRestartStrategy::RestartStrategyUnspecified => RestartStrategy::Permanent, // Default
        }
    }
}

impl ChildType {
    /// Convert to proto ChildType enum
    pub fn to_proto(&self) -> ProtoChildType {
        match self {
            ChildType::Actor => ProtoChildType::ChildTypeActor,
            ChildType::Supervisor => ProtoChildType::ChildTypeSupervisor,
        }
    }

    /// Convert from proto ChildType enum
    pub fn from_proto(proto: i32) -> Self {
        match ProtoChildType::try_from(proto).unwrap_or(ProtoChildType::ChildTypeUnspecified) {
            ProtoChildType::ChildTypeActor => ChildType::Actor,
            ProtoChildType::ChildTypeSupervisor => ChildType::Supervisor,
            ProtoChildType::ChildTypeUnspecified => ChildType::Actor, // Default
        }
    }
}

impl ShutdownSpec {
    /// Convert from Duration option (proto semantics)
    ///
    /// ## Arguments
    /// * `timeout` - Optional shutdown timeout
    ///   - None = Infinity
    ///   - Some(Duration::ZERO) = BrutalKill
    ///   - Some(duration) = Timeout(duration)
    pub fn from_duration(timeout: Option<Duration>) -> Self {
        match timeout {
            None => ShutdownSpec::Infinity,
            Some(d) if d.is_zero() => ShutdownSpec::BrutalKill,
            Some(d) => ShutdownSpec::Timeout(d),
        }
    }

    /// Convert to Duration option (proto semantics)
    pub fn to_duration(&self) -> Option<Duration> {
        match self {
            ShutdownSpec::BrutalKill => Some(Duration::ZERO),
            ShutdownSpec::Timeout(d) => Some(*d),
            ShutdownSpec::Infinity => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_child_spec_worker_creation() {
        let start_fn: StartFn = Arc::new(|| {
            Box::pin(async move {
                Err(ActorError::InvalidState("test".to_string()))
            })
        });
        
        let spec = ChildSpec::worker("worker1".to_string(), "worker1@node1".to_string(), start_fn);
        
        assert_eq!(spec.child_id, "worker1");
        assert_eq!(spec.actor_or_supervisor_id, "worker1@node1");
        assert_eq!(spec.restart_strategy, RestartStrategy::Permanent);
        assert_eq!(spec.child_type, ChildType::Actor);
        assert!(spec.shutdown_timeout.is_some());
    }

    #[tokio::test]
    async fn test_child_spec_supervisor_creation() {
        let start_fn: StartFn = Arc::new(|| {
            Box::pin(async move {
                Err(ActorError::InvalidState("test".to_string()))
            })
        });
        
        let spec = ChildSpec::supervisor("db-supervisor".to_string(), "db-supervisor".to_string(), start_fn);
        
        assert_eq!(spec.child_id, "db-supervisor");
        assert_eq!(spec.actor_or_supervisor_id, "db-supervisor");
        assert_eq!(spec.child_type, ChildType::Supervisor);
        assert!(spec.shutdown_timeout.is_none()); // Infinity for supervisors
    }

    #[tokio::test]
    async fn test_child_spec_with_restart() {
        let start_fn: StartFn = Arc::new(|| {
            Box::pin(async move {
                Err(ActorError::InvalidState("test".to_string()))
            })
        });
        
        let spec = ChildSpec::worker("worker1".to_string(), "worker1@node1".to_string(), start_fn)
            .with_restart(RestartStrategy::Temporary);
        
        assert_eq!(spec.restart_strategy, RestartStrategy::Temporary);
    }

    #[tokio::test]
    async fn test_child_spec_with_shutdown() {
        let start_fn: StartFn = Arc::new(|| {
            Box::pin(async move {
                Err(ActorError::InvalidState("test".to_string()))
            })
        });
        
        let spec = ChildSpec::worker("worker1".to_string(), "worker1@node1".to_string(), start_fn)
            .with_shutdown(ShutdownSpec::BrutalKill);
        
        assert_eq!(spec.shutdown_timeout, Some(Duration::ZERO));
        
        let spec2 = spec.with_shutdown(ShutdownSpec::Infinity);
        assert_eq!(spec2.shutdown_timeout, None);
        
        let spec3 = spec2.with_shutdown(ShutdownSpec::Timeout(Duration::from_secs(10)));
        assert_eq!(spec3.shutdown_timeout, Some(Duration::from_secs(10)));
    }

    #[tokio::test]
    async fn test_restart_strategy_proto_conversion() {
        assert_eq!(
            RestartStrategy::Permanent.to_proto() as i32,
            ProtoRestartStrategy::Permanent as i32
        );
        assert_eq!(
            RestartStrategy::from_proto(ProtoRestartStrategy::Permanent as i32),
            RestartStrategy::Permanent
        );
        
        assert_eq!(
            RestartStrategy::Transient.to_proto() as i32,
            ProtoRestartStrategy::Transient as i32
        );
        assert_eq!(
            RestartStrategy::from_proto(ProtoRestartStrategy::Transient as i32),
            RestartStrategy::Transient
        );
        
        assert_eq!(
            RestartStrategy::Temporary.to_proto() as i32,
            ProtoRestartStrategy::Temporary as i32
        );
        assert_eq!(
            RestartStrategy::from_proto(ProtoRestartStrategy::Temporary as i32),
            RestartStrategy::Temporary
        );
    }

    #[tokio::test]
    async fn test_child_type_proto_conversion() {
        assert_eq!(
            ChildType::Actor.to_proto() as i32,
            ProtoChildType::ChildTypeActor as i32
        );
        assert_eq!(
            ChildType::from_proto(ProtoChildType::ChildTypeActor as i32),
            ChildType::Actor
        );
        
        assert_eq!(
            ChildType::Supervisor.to_proto() as i32,
            ProtoChildType::ChildTypeSupervisor as i32
        );
        assert_eq!(
            ChildType::from_proto(ProtoChildType::ChildTypeSupervisor as i32),
            ChildType::Supervisor
        );
    }

    #[tokio::test]
    async fn test_shutdown_spec_conversion() {
        assert_eq!(
            ShutdownSpec::from_duration(Some(Duration::ZERO)),
            ShutdownSpec::BrutalKill
        );
        assert_eq!(
            ShutdownSpec::from_duration(None),
            ShutdownSpec::Infinity
        );
        assert_eq!(
            ShutdownSpec::from_duration(Some(Duration::from_secs(5))),
            ShutdownSpec::Timeout(Duration::from_secs(5))
        );
        
        assert_eq!(
            ShutdownSpec::BrutalKill.to_duration(),
            Some(Duration::ZERO)
        );
        assert_eq!(
            ShutdownSpec::Infinity.to_duration(),
            None
        );
        assert_eq!(
            ShutdownSpec::Timeout(Duration::from_secs(10)).to_duration(),
            Some(Duration::from_secs(10))
        );
    }

    #[tokio::test]
    async fn test_child_spec_to_proto() {
        let start_fn: StartFn = Arc::new(|| {
            Box::pin(async move {
                Err(ActorError::InvalidState("test".to_string()))
            })
        });
        
        let spec = ChildSpec::worker("worker1".to_string(), "worker1@node1".to_string(), start_fn)
            .with_restart(RestartStrategy::Transient)
            .with_shutdown(ShutdownSpec::Timeout(Duration::from_secs(10)))
            .with_metadata("start_module".to_string(), "my_module".to_string());
        
        let proto = spec.to_proto();
        
        assert_eq!(proto.child_id, "worker1");
        assert_eq!(proto.actor_or_supervisor_id, "worker1@node1");
        assert_eq!(proto.restart_strategy, ProtoRestartStrategy::Transient as i32);
        assert_eq!(proto.child_type, ProtoChildType::ChildTypeActor as i32);
        assert!(proto.shutdown_timeout.is_some());
        assert_eq!(proto.shutdown_timeout.unwrap().seconds, 10);
        assert_eq!(proto.metadata.get("start_module"), Some(&"my_module".to_string()));
    }

    #[tokio::test]
    async fn test_child_spec_from_proto() {
        use prost_types::Duration as ProtoDuration;
        
        let proto = ProtoChildSpec {
            child_id: "worker1".to_string(),
            actor_or_supervisor_id: "worker1@node1".to_string(),
            restart_strategy: ProtoRestartStrategy::Transient as i32,
            shutdown_timeout: Some(ProtoDuration {
                seconds: 10,
                nanos: 0,
            }),
            child_type: ProtoChildType::ChildTypeActor as i32,
            metadata: {
                let mut m = HashMap::new();
                m.insert("start_module".to_string(), "my_module".to_string());
                m
            },
            facets: Vec::new(),
        };
        
        let start_fn: StartFn = Arc::new(|| {
            Box::pin(async move {
                Err(ActorError::InvalidState("test".to_string()))
            })
        });
        
        let spec = ChildSpec::from_proto(&proto, start_fn);
        
        assert_eq!(spec.child_id, "worker1");
        assert_eq!(spec.actor_or_supervisor_id, "worker1@node1");
        assert_eq!(spec.restart_strategy, RestartStrategy::Transient);
        assert_eq!(spec.child_type, ChildType::Actor);
        assert_eq!(spec.shutdown_timeout, Some(Duration::from_secs(10)));
        assert_eq!(spec.metadata.get("start_module"), Some(&"my_module".to_string()));
    }
}


