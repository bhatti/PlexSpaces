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

use crate::Actor;
use plexspaces_core::{ActorError, ActorId, ActorIdError, ActorRef};
use plexspaces_proto::supervision::v1::{
    ChildSpec as ProtoChildSpec, ChildType as ProtoChildType,
    RestartStrategy as ProtoRestartStrategy,
};
use prost_types;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

/// Boxed future type for async start functions
pub type BoxFuture<'a, T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send + 'a>>;

/// Child specification — runtime supervision binding
///
/// ## Purpose
/// Defines a child process (actor or supervisor) managed by a supervisor.
/// Carries the resolved [`ActorId`] (internal canonical identity); wire/proto
/// declarations use [`plexspaces_proto::common::v1::ActorIdentity`] plus namespace/node.
///
/// ## Design
/// - Aligns with `proto/plexspaces/v1/supervision/supervision.proto` `ChildSpec`
/// - `start_fn` is Rust-only (not on the wire)
#[derive(Clone)]
pub struct ChildSpec {
    /// Resolved canonical actor (or supervisor process) identity.
    pub actor_id: ActorId,

    /// How to handle child failures
    pub restart_strategy: RestartStrategy,

    /// Shutdown timeout for graceful termination
    /// - None/0 = brutal_kill (immediate)
    /// - Some(ms) = graceful shutdown with timeout
    /// - For supervisors: typically set high or infinity to allow children to shutdown
    pub shutdown_timeout: Option<Duration>,

    /// Worker vs supervisor for **supervision policy** (restart/shutdown paths). Not used as the
    /// actor registry identity — that is always the canonical [`ActorId`] on this struct.
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
pub type StartFn =
    Arc<dyn Fn() -> BoxFuture<'static, Result<StartedChild, ActorError>> + Send + Sync>;

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
    /// Create a new ChildSpec for a supervised **worker** (leaf actor).
    ///
    /// `child_actor_id` is the resolved canonical [`ActorId`] for this child process
    /// (`ActorIdentity.name` + behavior-class `actor_type` + namespace + node). It must
    /// stay distinct from the parent supervisor's **label** string (`Supervisor::new` first argument) and
    /// from every sibling's [`ActorId`]. Supervision policy lives in [`ChildType::Actor`];
    /// registry routing always uses this [`ActorId`], not the word "worker".
    pub fn worker(child_actor_id: ActorId, start_fn: StartFn) -> Self {
        Self {
            actor_id: child_actor_id,
            restart_strategy: RestartStrategy::Permanent,
            shutdown_timeout: Some(Duration::from_secs(5)),
            child_type: ChildType::Actor,
            metadata: HashMap::new(),
            facets: Vec::new(),
            start_fn,
        }
    }

    /// Worker with a synchronous factory (common in tests).
    pub fn worker_sync(
        child_actor_id: ActorId,
        sync_factory: Arc<dyn Fn() -> Result<Actor, ActorError> + Send + Sync>,
        actor_ref: ActorRef,
    ) -> Self {
        let start_fn: StartFn = Arc::new(move || {
            let factory = sync_factory.clone();
            let actor_ref = actor_ref.clone();
            Box::pin(async move {
                let actor = factory()?;
                Ok(StartedChild::Worker { actor, actor_ref })
            })
        });
        Self::worker(child_actor_id, start_fn)
    }

    /// Nested **supervisor** child; `supervisor_actor_id` is the resolved identity of the
    /// supervisor **process** (another [`ActorId`]), not the same string as any supervised
    /// worker's name and not the parent supervisor's opaque label string.
    pub fn supervisor(supervisor_actor_id: ActorId, start_fn: StartFn) -> Self {
        Self {
            actor_id: supervisor_actor_id,
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

    /// Convert to proto ChildSpec (identity only; no `start_fn` on wire).
    pub fn to_proto(&self) -> ProtoChildSpec {
        ProtoChildSpec {
            restart_strategy: self.restart_strategy.to_proto() as i32,
            shutdown_timeout: self.shutdown_timeout.map(|d| prost_types::Duration {
                seconds: d.as_secs() as i64,
                nanos: d.subsec_nanos() as i32,
            }),
            child_type: self.child_type.to_proto() as i32,
            metadata: self.metadata.clone(),
            facets: self.facets.clone(),
            actor_identity: Some(plexspaces_proto::common::v1::ActorIdentity {
                name: self.actor_id.name().to_string(),
                actor_type: self.actor_id.actor_type().to_string(),
            }),
        }
    }

    /// Convert from proto plus deployment context. Caller supplies `start_fn`.
    pub fn from_proto(
        proto: &ProtoChildSpec,
        start_fn: StartFn,
        namespace: &str,
        node_id: &str,
    ) -> Result<Self, ActorIdError> {
        let identity = proto
            .actor_identity
            .as_ref()
            .ok_or(ActorIdError::MissingField("actor_identity"))?;
        let actor_id = ActorId::from_actor_identity(identity, namespace, node_id)?;
        Ok(Self {
            actor_id,
            restart_strategy: RestartStrategy::from_proto(proto.restart_strategy),
            shutdown_timeout: proto.shutdown_timeout.as_ref().map(|d| {
                Duration::from_secs(d.seconds as u64) + Duration::from_nanos(d.nanos as u64)
            }),
            child_type: ChildType::from_proto(proto.child_type),
            metadata: proto.metadata.clone(),
            facets: proto.facets.clone(),
            start_fn,
        })
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
        match ProtoRestartStrategy::try_from(proto)
            .unwrap_or(ProtoRestartStrategy::RestartStrategyUnspecified)
        {
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

    fn test_worker_actor_id() -> ActorId {
        ActorId::new("worker1", "worker", "default", "node1").expect("valid test id")
    }

    #[tokio::test]
    async fn test_child_spec_worker_creation() {
        let start_fn: StartFn =
            Arc::new(|| Box::pin(async move { Err(ActorError::InvalidState("test".to_string())) }));

        let spec = ChildSpec::worker(test_worker_actor_id(), start_fn);

        assert_eq!(spec.actor_id.name(), "worker1");
        assert_eq!(spec.actor_id.actor_type(), "worker");
        assert_eq!(spec.restart_strategy, RestartStrategy::Permanent);
        assert_eq!(spec.child_type, ChildType::Actor);
        assert!(spec.shutdown_timeout.is_some());
    }

    #[tokio::test]
    async fn test_child_spec_supervisor_creation() {
        let start_fn: StartFn =
            Arc::new(|| Box::pin(async move { Err(ActorError::InvalidState("test".to_string())) }));

        let sid =
            ActorId::new("db-supervisor", "supervisor", "default", "node1").expect("valid test id");
        let spec = ChildSpec::supervisor(sid, start_fn);

        assert_eq!(spec.actor_id.name(), "db-supervisor");
        assert_eq!(spec.child_type, ChildType::Supervisor);
        assert!(spec.shutdown_timeout.is_none()); // Infinity for supervisors
    }

    #[tokio::test]
    async fn test_child_spec_with_restart() {
        let start_fn: StartFn =
            Arc::new(|| Box::pin(async move { Err(ActorError::InvalidState("test".to_string())) }));

        let spec = ChildSpec::worker(test_worker_actor_id(), start_fn)
            .with_restart(RestartStrategy::Temporary);

        assert_eq!(spec.restart_strategy, RestartStrategy::Temporary);
    }

    #[tokio::test]
    async fn test_child_spec_with_shutdown() {
        let start_fn: StartFn =
            Arc::new(|| Box::pin(async move { Err(ActorError::InvalidState("test".to_string())) }));

        let spec = ChildSpec::worker(test_worker_actor_id(), start_fn)
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
        assert_eq!(ShutdownSpec::from_duration(None), ShutdownSpec::Infinity);
        assert_eq!(
            ShutdownSpec::from_duration(Some(Duration::from_secs(5))),
            ShutdownSpec::Timeout(Duration::from_secs(5))
        );

        assert_eq!(ShutdownSpec::BrutalKill.to_duration(), Some(Duration::ZERO));
        assert_eq!(ShutdownSpec::Infinity.to_duration(), None);
        assert_eq!(
            ShutdownSpec::Timeout(Duration::from_secs(10)).to_duration(),
            Some(Duration::from_secs(10))
        );
    }

    #[tokio::test]
    async fn test_child_spec_to_proto() {
        let start_fn: StartFn =
            Arc::new(|| Box::pin(async move { Err(ActorError::InvalidState("test".to_string())) }));

        let spec = ChildSpec::worker(test_worker_actor_id(), start_fn)
            .with_restart(RestartStrategy::Transient)
            .with_shutdown(ShutdownSpec::Timeout(Duration::from_secs(10)))
            .with_metadata("start_module".to_string(), "my_module".to_string());

        let proto = spec.to_proto();

        let id = proto.actor_identity.as_ref().expect("identity set");
        assert_eq!(id.name, "worker1");
        assert_eq!(id.actor_type, "worker");
        assert_eq!(
            proto.restart_strategy,
            ProtoRestartStrategy::Transient as i32
        );
        assert_eq!(proto.child_type, ProtoChildType::ChildTypeActor as i32);
        assert!(proto.shutdown_timeout.is_some());
        assert_eq!(proto.shutdown_timeout.unwrap().seconds, 10);
        assert_eq!(
            proto.metadata.get("start_module"),
            Some(&"my_module".to_string())
        );
    }

    #[tokio::test]
    async fn test_child_spec_from_proto() {
        use prost_types::Duration as ProtoDuration;

        let proto = ProtoChildSpec {
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
            actor_identity: Some(plexspaces_proto::common::v1::ActorIdentity {
                name: "worker1".to_string(),
                actor_type: "worker".to_string(),
            }),
        };

        let start_fn: StartFn =
            Arc::new(|| Box::pin(async move { Err(ActorError::InvalidState("test".to_string())) }));

        let spec = ChildSpec::from_proto(&proto, start_fn, "default", "node1").expect("from_proto");

        assert_eq!(spec.actor_id.name(), "worker1");
        assert_eq!(spec.actor_id.actor_type(), "worker");
        assert_eq!(spec.restart_strategy, RestartStrategy::Transient);
        assert_eq!(spec.child_type, ChildType::Actor);
        assert_eq!(spec.shutdown_timeout, Some(Duration::from_secs(10)));
        assert_eq!(
            spec.metadata.get("start_module"),
            Some(&"my_module".to_string())
        );
    }
}
