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

//! Child specification - proto-first runtime binding.
//!
//! ## Design
//! `ChildSpec` wraps `plexspaces_proto::supervision::v1::ChildSpec` (the canonical
//! wire-format data contract) and adds two Rust-only runtime fields:
//! - `actor_id`: resolved `ActorId` (derived from `actor_identity` + namespace + node)
//! - `start_fn`: async factory closure (not serializable)
//!
//! All data fields (role, restart_policy, metadata, facets, shutdown_timeout) are
//! accessed directly on the embedded proto message.

use crate::actor_ref::ActorRef;
use crate::core::{ActorError, ActorId, ActorIdError};
use crate::ActorInstance;
use plexspaces_proto::supervision::v1::ChildSpec as ProtoChildSpec;
pub use plexspaces_proto::supervision::v1::RestartPolicy as ProtoRestartPolicy;
use std::sync::Arc;
use std::time::Duration;

/// Boxed future type for async start functions
pub type BoxFuture<'a, T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send + 'a>>;

/// Child specification — runtime supervision binding.
///
/// Wraps the proto `ChildSpec` data contract and adds Rust-only runtime fields.
/// Use `spec.proto.role`, `spec.proto.restart`, etc. to access data fields directly.
#[derive(Clone)]
pub struct ChildSpec {
    /// Resolved canonical actor identity (derived from proto `actor_identity` + namespace + node).
    pub actor_id: ActorId,

    /// Proto data contract — all data fields live here.
    pub proto: ProtoChildSpec,

    /// Factory function to create/start the child (not serializable, Rust-only).
    pub start_fn: StartFn,
}

/// Convenience accessors that delegate to the embedded proto.
impl ChildSpec {
    /// Role of this child: "worker" for leaf actors, "supervisor" for nested supervisors.
    pub fn role(&self) -> &str {
        &self.proto.role
    }

    /// Restart policy from the proto spec.
    pub fn restart_policy(&self) -> ProtoRestartPolicy {
        ProtoRestartPolicy::try_from(self.proto.restart)
            .unwrap_or(ProtoRestartPolicy::RestartPolicyPermanent)
    }

    /// Shutdown timeout derived from proto duration field.
    pub fn shutdown_timeout(&self) -> Option<Duration> {
        self.proto
            .shutdown_timeout
            .as_ref()
            .map(|d| Duration::from_secs(d.seconds as u64) + Duration::from_nanos(d.nanos as u64))
    }
}

/// How to start a child - async factory function
pub type StartFn =
    Arc<dyn Fn() -> BoxFuture<'static, Result<StartedChild, ActorError>> + Send + Sync>;

/// Result of starting a child
///
/// Large variant size difference is intentional: Supervisor trees are typically
/// small compared to the number of Worker actors; boxing would add a heap indirection
/// on the common path.
#[allow(clippy::large_enum_variant)]
pub enum StartedChild {
    /// Worker child (actor)
    Worker {
        /// The actor instance that was started.
        actor: ActorInstance,
        /// Reference to the running actor.
        actor_ref: ActorRef,
    },
    /// Supervisor child (nested supervisor)
    Supervisor {
        /// The supervisor that was started.
        supervisor: crate::Supervisor,
    },
}

/// Shutdown specification — Erlang semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownSpec {
    /// Abort the child immediately without waiting for graceful shutdown.
    BrutalKill,
    /// Wait up to `Duration` for graceful shutdown, then abort.
    Timeout(Duration),
    /// Wait indefinitely for graceful shutdown (used for supervisor children).
    Infinity,
}

impl ChildSpec {
    /// Create a worker `ChildSpec`.
    pub fn worker(child_actor_id: ActorId, start_fn: StartFn) -> Self {
        Self {
            actor_id: child_actor_id.clone(),
            proto: ProtoChildSpec {
                actor_identity: Some(plexspaces_proto::common::v1::ActorIdentity {
                    name: child_actor_id.name().to_string(),
                    actor_type: child_actor_id.actor_type().to_string(),
                }),
                role: "worker".to_string(),
                restart: ProtoRestartPolicy::RestartPolicyPermanent as i32,
                shutdown_timeout: Some(prost_types::Duration {
                    seconds: 5,
                    nanos: 0,
                }),
                ..Default::default()
            },
            start_fn,
        }
    }

    /// Worker with a synchronous factory (common in tests).
    pub fn worker_sync(
        child_actor_id: ActorId,
        sync_factory: Arc<dyn Fn() -> Result<ActorInstance, ActorError> + Send + Sync>,
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

    /// Create a supervisor `ChildSpec` (nested supervisor).
    pub fn supervisor(supervisor_actor_id: ActorId, start_fn: StartFn) -> Self {
        Self {
            actor_id: supervisor_actor_id.clone(),
            proto: ProtoChildSpec {
                actor_identity: Some(plexspaces_proto::common::v1::ActorIdentity {
                    name: supervisor_actor_id.name().to_string(),
                    actor_type: supervisor_actor_id.actor_type().to_string(),
                }),
                role: "supervisor".to_string(),
                restart: ProtoRestartPolicy::RestartPolicyPermanent as i32,
                shutdown_timeout: None, // Infinity for supervisors
                ..Default::default()
            },
            start_fn,
        }
    }

    /// Set restart policy.
    pub fn with_restart(mut self, policy: ProtoRestartPolicy) -> Self {
        self.proto.restart = policy as i32;
        self
    }

    /// Set shutdown timeout.
    pub fn with_shutdown(mut self, shutdown: ShutdownSpec) -> Self {
        self.proto.shutdown_timeout = match shutdown {
            ShutdownSpec::BrutalKill => Some(prost_types::Duration {
                seconds: 0,
                nanos: 0,
            }),
            ShutdownSpec::Timeout(d) => Some(prost_types::Duration {
                seconds: d.as_secs() as i64,
                nanos: d.subsec_nanos() as i32,
            }),
            ShutdownSpec::Infinity => None,
        };
        self
    }

    /// Add metadata entry.
    pub fn with_metadata(mut self, key: String, value: String) -> Self {
        self.proto.metadata.insert(key, value);
        self
    }

    /// Add a facet.
    pub fn with_facet(mut self, facet: plexspaces_proto::common::v1::Facet) -> Self {
        self.proto.facets.push(facet);
        self
    }

    /// Add multiple facets.
    pub fn with_facets(mut self, facets: Vec<plexspaces_proto::common::v1::Facet>) -> Self {
        self.proto.facets.extend(facets);
        self
    }

    /// Return the embedded proto (wire representation — no `start_fn`).
    pub fn to_proto(&self) -> &ProtoChildSpec {
        &self.proto
    }

    /// Construct from proto plus deployment context. Caller supplies `start_fn`.
    pub fn from_proto(
        proto: ProtoChildSpec,
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
            proto,
            start_fn,
        })
    }
}

impl ShutdownSpec {
    /// Convert an `Option<Duration>` to a `ShutdownSpec`.
    ///
    /// - `None` → `Infinity`
    /// - `Some(Duration::ZERO)` → `BrutalKill`
    /// - `Some(d)` → `Timeout(d)`
    pub fn from_duration(timeout: Option<Duration>) -> Self {
        match timeout {
            None => ShutdownSpec::Infinity,
            Some(d) if d.is_zero() => ShutdownSpec::BrutalKill,
            Some(d) => ShutdownSpec::Timeout(d),
        }
    }

    /// Convert back to `Option<Duration>` for use in timeout APIs.
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
        assert_eq!(
            spec.restart_policy(),
            ProtoRestartPolicy::RestartPolicyPermanent
        );
        assert_eq!(spec.role(), "worker");
        assert!(spec.proto.shutdown_timeout.is_some());
    }

    #[tokio::test]
    async fn test_child_spec_supervisor_creation() {
        let start_fn: StartFn =
            Arc::new(|| Box::pin(async move { Err(ActorError::InvalidState("test".to_string())) }));

        let sid =
            ActorId::new("db-supervisor", "supervisor", "default", "node1").expect("valid test id");
        let spec = ChildSpec::supervisor(sid, start_fn);

        assert_eq!(spec.actor_id.name(), "db-supervisor");
        assert_eq!(spec.role(), "supervisor");
        assert!(spec.proto.shutdown_timeout.is_none()); // Infinity for supervisors
    }

    #[tokio::test]
    async fn test_child_spec_with_restart() {
        let start_fn: StartFn =
            Arc::new(|| Box::pin(async move { Err(ActorError::InvalidState("test".to_string())) }));

        let spec = ChildSpec::worker(test_worker_actor_id(), start_fn)
            .with_restart(ProtoRestartPolicy::RestartPolicyTemporary);

        assert_eq!(
            spec.restart_policy(),
            ProtoRestartPolicy::RestartPolicyTemporary
        );
    }

    #[tokio::test]
    async fn test_child_spec_with_shutdown() {
        let start_fn: StartFn =
            Arc::new(|| Box::pin(async move { Err(ActorError::InvalidState("test".to_string())) }));

        let spec = ChildSpec::worker(test_worker_actor_id(), start_fn)
            .with_shutdown(ShutdownSpec::BrutalKill);
        assert_eq!(spec.shutdown_timeout(), Some(Duration::ZERO));

        let spec2 = spec.with_shutdown(ShutdownSpec::Infinity);
        assert_eq!(spec2.shutdown_timeout(), None);

        let spec3 = spec2.with_shutdown(ShutdownSpec::Timeout(Duration::from_secs(10)));
        assert_eq!(spec3.shutdown_timeout(), Some(Duration::from_secs(10)));
    }

    #[tokio::test]
    async fn test_restart_policy_in_child_spec() {
        let start_fn: StartFn =
            Arc::new(|| Box::pin(async move { Err(ActorError::InvalidState("test".to_string())) }));
        let spec = ChildSpec::worker(test_worker_actor_id(), start_fn)
            .with_restart(ProtoRestartPolicy::RestartPolicyTransient);
        assert_eq!(
            spec.restart_policy(),
            ProtoRestartPolicy::RestartPolicyTransient
        );

        let proto = spec.to_proto();
        assert_eq!(
            proto.restart,
            ProtoRestartPolicy::RestartPolicyTransient as i32
        );
    }

    #[tokio::test]
    async fn test_child_role_values() {
        let spec_worker = ChildSpec::worker(
            test_worker_actor_id(),
            Arc::new(|| Box::pin(async move { Err(ActorError::InvalidState("test".to_string())) })),
        );
        assert_eq!(spec_worker.role(), "worker");

        let sid = ActorId::new("sup", "supervisor", "default", "node1").expect("valid");
        let spec_sup = ChildSpec::supervisor(
            sid,
            Arc::new(|| Box::pin(async move { Err(ActorError::InvalidState("test".to_string())) })),
        );
        assert_eq!(spec_sup.role(), "supervisor");
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
            .with_restart(ProtoRestartPolicy::RestartPolicyTransient)
            .with_shutdown(ShutdownSpec::Timeout(Duration::from_secs(10)))
            .with_metadata("start_module".to_string(), "my_module".to_string());

        let proto = spec.to_proto();

        let id = proto.actor_identity.as_ref().expect("identity set");
        assert_eq!(id.name, "worker1");
        assert_eq!(id.actor_type, "worker");
        assert_eq!(
            proto.restart,
            ProtoRestartPolicy::RestartPolicyTransient as i32
        );
        assert_eq!(proto.role, "worker");
        assert!(proto.shutdown_timeout.is_some());
        assert_eq!(proto.shutdown_timeout.as_ref().unwrap().seconds, 10);
        assert_eq!(
            proto.metadata.get("start_module"),
            Some(&"my_module".to_string())
        );
    }

    #[tokio::test]
    async fn test_child_spec_from_proto() {
        use std::collections::HashMap;

        let proto = ProtoChildSpec {
            restart: ProtoRestartPolicy::RestartPolicyTransient as i32,
            shutdown_timeout: Some(prost_types::Duration {
                seconds: 10,
                nanos: 0,
            }),
            role: "worker".to_string(),
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
            ..Default::default()
        };

        let start_fn: StartFn =
            Arc::new(|| Box::pin(async move { Err(ActorError::InvalidState("test".to_string())) }));

        let spec = ChildSpec::from_proto(proto, start_fn, "default", "node1").expect("from_proto");

        assert_eq!(spec.actor_id.name(), "worker1");
        assert_eq!(spec.actor_id.actor_type(), "worker");
        assert_eq!(
            spec.restart_policy(),
            ProtoRestartPolicy::RestartPolicyTransient
        );
        assert_eq!(spec.role(), "worker");
        assert_eq!(spec.shutdown_timeout(), Some(Duration::from_secs(10)));
        assert_eq!(
            spec.proto.metadata.get("start_module"),
            Some(&"my_module".to_string())
        );
    }
}
