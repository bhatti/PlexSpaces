// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Tests for `ActorRegistry::resolve_actor_id`
//!
//! Verifies all three addressing formats:
//! 1. Canonical `name//type::namespace@node` — parsed directly, zero registry traffic
//! 2. `actor_type:name` — live O(1) lookup, then virtual definition fallback
//! 3. Bare type — live O(1) lookup, then virtual fallback, random among multiple

use plexspaces_core::{
    actor_context::ObjectRegistry, ActorId, ActorRegistry, Message, MessageSender, RequestContext,
};
use plexspaces_object_registry::{ObjectRegistryImpl, SqliteObjectRegistryRepository};
use std::sync::Arc;
use ulid::Ulid;

// ── Minimal MessageSender stub ────────────────────────────────────────────────

struct MockSender {
    actor_id: ActorId,
    namespace: String,
}

impl MockSender {
    fn new(actor_id: ActorId) -> Self {
        let namespace = actor_id.namespace().to_string();
        Self { actor_id, namespace }
    }
}

#[async_trait::async_trait]
impl MessageSender for MockSender {
    async fn tell(
        &self,
        _ctx: &RequestContext,
        _msg: Message,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }

    fn actor_id(&self) -> Option<String> {
        Some(self.actor_id.to_string())
    }

    fn namespace(&self) -> Option<&str> {
        Some(&self.namespace)
    }

    fn actor_type(&self) -> Option<String> {
        Some(self.actor_id.actor_type().to_string())
    }

    async fn set_actor_type(&self, _: Option<String>) {}

    fn local_state_handle(&self) -> Option<Arc<dyn plexspaces_core::ActorStateHandle>> {
        None
    }

    async fn set_local_state_handle(
        &self,
        _: Option<Arc<dyn plexspaces_core::ActorStateHandle>>,
    ) {
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// ── ObjectRegistry adapter (mirrors actor_registry_no_mailbox_exposure_tests) ─

struct ObjectRegistryAdapter {
    inner: Arc<ObjectRegistryImpl>,
}

#[async_trait::async_trait]
impl ObjectRegistry for ObjectRegistryAdapter {
    async fn lookup(
        &self,
        ctx: &RequestContext,
        object_id: &str,
        object_type: Option<plexspaces_proto::object_registry::v1::ObjectType>,
    ) -> Result<
        Option<plexspaces_proto::object_registry::v1::ObjectRegistration>,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        let obj_type = object_type.unwrap_or(
            plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeUnspecified,
        );
        self.inner.lookup(ctx, obj_type, object_id).await.map_err(
            |e| Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
                as Box<dyn std::error::Error + Send + Sync>,
        )
    }

    async fn lookup_full(
        &self,
        ctx: &RequestContext,
        object_type: plexspaces_proto::object_registry::v1::ObjectType,
        object_id: &str,
    ) -> Result<
        Option<plexspaces_proto::object_registry::v1::ObjectRegistration>,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        self.inner
            .lookup_full(ctx, object_type, object_id)
            .await
            .map_err(|e| {
                Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
                    as Box<dyn std::error::Error + Send + Sync>
            })
    }

    async fn discover(
        &self,
        ctx: &RequestContext,
        object_type: Option<plexspaces_proto::object_registry::v1::ObjectType>,
        name_pattern: Option<String>,
        tags: Option<Vec<String>>,
        metadata: Option<Vec<String>>,
        health_status: Option<plexspaces_proto::object_registry::v1::HealthStatus>,
        offset: usize,
        limit: usize,
    ) -> Result<
        Vec<plexspaces_proto::object_registry::v1::ObjectRegistration>,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        self.inner
            .discover(ctx, object_type, name_pattern, tags, metadata, health_status, offset, limit)
            .await
            .map_err(|e| {
                Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
                    as Box<dyn std::error::Error + Send + Sync>
            })
    }

    async fn register(
        &self,
        ctx: &RequestContext,
        registration: plexspaces_proto::object_registry::v1::ObjectRegistration,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.inner.register(ctx, registration).await.map_err(|e| {
            Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
                as Box<dyn std::error::Error + Send + Sync>
        })
    }

    async fn unregister(
        &self,
        ctx: &RequestContext,
        object_type: plexspaces_proto::object_registry::v1::ObjectType,
        object_id: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.inner
            .unregister(ctx, object_type, object_id)
            .await
            .map_err(|e| {
                Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
                    as Box<dyn std::error::Error + Send + Sync>
            })
    }

    async fn heartbeat(
        &self,
        ctx: &RequestContext,
        object_type: plexspaces_proto::object_registry::v1::ObjectType,
        object_id: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.inner
            .heartbeat(ctx, object_type, object_id)
            .await
            .map_err(|e| {
                Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
                    as Box<dyn std::error::Error + Send + Sync>
            })
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

async fn make_registry(node_id: &str) -> Arc<ActorRegistry> {
    let object_repo = Arc::new(
        SqliteObjectRegistryRepository::new(":memory:")
            .await
            .unwrap(),
    );
    let object_registry_impl = Arc::new(ObjectRegistryImpl::new(object_repo));
    let object_registry: Arc<dyn ObjectRegistry> =
        Arc::new(ObjectRegistryAdapter { inner: object_registry_impl });
    Arc::new(ActorRegistry::new(object_registry, node_id.to_string()))
}

fn unique_ctx(namespace: &str) -> RequestContext {
    RequestContext::new_without_auth(format!("tenant-{}", Ulid::new()), namespace.to_string())
}

async fn register_live(registry: &Arc<ActorRegistry>, ctx: &RequestContext, actor_id: ActorId) {
    let actor_type = actor_id.actor_type().to_string();
    let sender: Arc<dyn MessageSender> = Arc::new(MockSender::new(actor_id.clone()));
    registry
        .register_actor(ctx, actor_id, sender, actor_type, None, None, None)
        .await;
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn resolve_canonical_format_live_actor() {
    let registry = make_registry("node-1").await;
    let ns = "default";
    let ctx = unique_ctx(ns);
    let actor_id = ActorId::new("alerts", "channel", ns, "node-1").unwrap();
    register_live(&registry, &ctx, actor_id.clone()).await;

    let resolved = registry
        .resolve_actor_id(&ctx, &actor_id.to_string())
        .await
        .unwrap();
    assert_eq!(resolved.to_string(), actor_id.to_string());
}

#[tokio::test]
async fn resolve_canonical_format_bypasses_registry_lookup() {
    // Even with no live actor the canonical path must parse without error
    let registry = make_registry("node-42").await;
    let ns = "prod";
    let ctx = unique_ctx(ns);
    let canonical = format!("myactor//mytype::{}@node-42", ns);

    let resolved = registry.resolve_actor_id(&ctx, &canonical).await.unwrap();
    assert_eq!(resolved.name(), "myactor");
    assert_eq!(resolved.actor_type(), "mytype");
    assert_eq!(resolved.node_id(), "node-42");
}

#[tokio::test]
async fn resolve_type_colon_name_hits_live_actor() {
    let registry = make_registry("node-1").await;
    let ns = "default";
    let ctx = unique_ctx(ns);
    let actor_id = ActorId::new("alerts", "channel", ns, "node-1").unwrap();
    register_live(&registry, &ctx, actor_id.clone()).await;

    let resolved = registry
        .resolve_actor_id(&ctx, "channel:alerts")
        .await
        .unwrap();
    assert_eq!(resolved.name(), "alerts");
    assert_eq!(resolved.actor_type(), "channel");
    assert_eq!(resolved.namespace(), ns);
}

#[tokio::test]
async fn resolve_type_colon_name_no_live_builds_canonical() {
    // No actor registered — fallback must still build a valid canonical ID
    let registry = make_registry("node-1").await;
    let ns = "ns1";
    let ctx = unique_ctx(ns);

    let resolved = registry
        .resolve_actor_id(&ctx, "channel:alerts")
        .await
        .unwrap();
    assert_eq!(resolved.name(), "alerts");
    assert_eq!(resolved.actor_type(), "channel");
    assert_eq!(resolved.namespace(), ns);
    assert_eq!(resolved.node_id(), "node-1");
}

#[tokio::test]
async fn resolve_bare_type_single_live_actor() {
    let registry = make_registry("node-1").await;
    // Use the same ctx for registration and lookup so tenant_id matches in the type-index.
    let ctx = unique_ctx("default");
    let ns = ctx.namespace().to_string();
    let actor_id = ActorId::new("channel", "channel", &ns, "node-1").unwrap();
    register_live(&registry, &ctx, actor_id.clone()).await;

    let resolved = registry.resolve_actor_id(&ctx, "channel").await.unwrap();
    assert_eq!(resolved.actor_type(), "channel");
    assert_eq!(resolved.namespace(), ns);
}

#[tokio::test]
async fn resolve_bare_type_multiple_live_actors_picks_one() {
    let registry = make_registry("node-1").await;
    // Use the same ctx for registration and lookup so tenant_id matches in the type-index.
    let ctx = unique_ctx("default");
    let ns = ctx.namespace().to_string();

    for name in ["ch1", "ch2", "ch3"] {
        let id = ActorId::new(name, "channel", &ns, "node-1").unwrap();
        register_live(&registry, &ctx, id).await;
    }

    // Must return without error and the result must be one of the registered actors
    let resolved = registry.resolve_actor_id(&ctx, "channel").await.unwrap();
    assert_eq!(resolved.actor_type(), "channel");
    assert!(
        ["ch1", "ch2", "ch3"].contains(&resolved.name()),
        "unexpected name: {}",
        resolved.name()
    );
}

#[tokio::test]
async fn resolve_empty_type_returns_error() {
    let registry = make_registry("node-1").await;
    let ctx = unique_ctx("default");
    assert!(registry.resolve_actor_id(&ctx, ":alerts").await.is_err());
}

#[tokio::test]
async fn resolve_empty_name_returns_error() {
    let registry = make_registry("node-1").await;
    let ctx = unique_ctx("default");
    assert!(registry.resolve_actor_id(&ctx, "channel:").await.is_err());
}

#[tokio::test]
async fn resolve_bare_type_no_live_no_virtual_returns_error() {
    let registry = make_registry("node-1").await;
    let ctx = unique_ctx("default");

    let result = registry.resolve_actor_id(&ctx, "nonexistent-type").await;
    assert!(result.is_err(), "Expected error for unknown bare type");
}

#[tokio::test]
async fn resolve_does_not_cross_namespace_boundaries() {
    let registry = make_registry("node-1").await;
    let ns_a = "ns-a";
    let ns_b = "ns-b";

    let ctx_a = unique_ctx(ns_a);
    let ctx_b = unique_ctx(ns_b);

    let actor_a = ActorId::new("alerts", "channel", ns_a, "node-1").unwrap();
    register_live(&registry, &ctx_a, actor_a.clone()).await;

    // ctx_b has a different namespace — should not see ns_a's actor via live lookup
    // and should fall back to building a canonical ID in ns_b
    let resolved = registry
        .resolve_actor_id(&ctx_b, "channel:alerts")
        .await
        .unwrap();
    assert_eq!(resolved.namespace(), ns_b, "Must not cross namespace");
}
