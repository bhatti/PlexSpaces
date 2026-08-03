// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! # Actor test helpers
//!
//! Helpers for creating test actor IDs and request contexts.

use plexspaces_common::{RequestContext, RequestContextExt};
use plexspaces_service_traits::ActorId;
use ulid::Ulid;

/// Creates an ActorId suitable for tests.
///
/// Uses a fresh ULID as the actor name to guarantee uniqueness across parallel tests.
pub fn create_test_actor_id(actor_type: &str, namespace: &str, node_id: &str) -> ActorId {
    ActorId::new(&Ulid::new().to_string(), actor_type, namespace, node_id)
        .expect("test actor id construction must not fail")
}

/// Creates a named ActorId (stable name, useful for lookup tests).
pub fn create_named_actor_id(
    name: &str,
    actor_type: &str,
    namespace: &str,
    node_id: &str,
) -> ActorId {
    ActorId::new(name, actor_type, namespace, node_id)
        .expect("test actor id construction must not fail")
}

/// Creates a RequestContext for tests with the given tenant and namespace.
///
/// Never uses `RequestContext::internal()` — always creates a properly scoped context
/// per CLAUDE.md Rule #6.
pub fn create_test_request_context(tenant_id: &str, namespace: &str) -> RequestContext {
    RequestContext::new_without_auth(tenant_id.to_string(), namespace.to_string())
}

/// Creates a RequestContext with default test tenant/namespace values.
pub fn default_test_context() -> RequestContext {
    create_test_request_context("test-tenant", "test-namespace")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_test_actor_id_unique() {
        let a1 = create_test_actor_id("counter", "ns", "node1");
        let a2 = create_test_actor_id("counter", "ns", "node1");
        assert_ne!(
            a1.name(),
            a2.name(),
            "each call must produce a unique actor id"
        );
    }

    #[test]
    fn test_named_actor_id_stable() {
        let a1 = create_named_actor_id("my-actor", "counter", "ns", "node1");
        let a2 = create_named_actor_id("my-actor", "counter", "ns", "node1");
        assert_eq!(a1.name(), a2.name());
    }

    #[test]
    fn test_request_context_tenant() {
        use plexspaces_common::RequestContextExt;
        let ctx = create_test_request_context("tenant-abc", "ns-xyz");
        assert_eq!(ctx.tenant_id(), "tenant-abc");
        assert_eq!(ctx.namespace(), "ns-xyz");
    }
}
