// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! TracingGate — runtime toggle for per-actor tracing.
//!
//! Defined in the actor crate so `ActorContext` can gate span creation without
//! a dependency on the services crate (which would be circular).
//! `TracingControlServiceImpl` in `plexspaces-services` implements this trait.

/// Decides whether tracing spans should be created for a given actor.
///
/// Default state: all actors disabled (zero overhead). Operators enable
/// selectively via `POST /api/v1/admin/tracing/enable` or the gRPC
/// `TracingControlService.EnableTracing` RPC.
pub trait TracingGate: Send + Sync {
    /// Returns `true` if tracing spans should be created for this actor instance.
    ///
    /// Checked once at the start of the actor task and per-message when enabled.
    /// Implementation must be O(1) / lock-free (DashMap read).
    fn is_enabled(&self, actor_type: &str, actor_id: &str) -> bool;
}
