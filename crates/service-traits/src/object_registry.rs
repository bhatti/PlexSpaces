// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! ObjectRegistry trait for service discovery.

use async_trait::async_trait;
use plexspaces_common::RequestContext;

pub type ObjectRegistration = plexspaces_proto::object_registry::v1::ObjectRegistration;
pub type HealthStatus = plexspaces_proto::object_registry::v1::HealthStatus;

/// Result of a `register_with_unique_alias` call.
#[derive(Debug, Clone)]
pub enum RegisterResult {
    /// Registration succeeded.
    Registered,
    /// Alias conflict with an active instance; contains routing info for forwarding.
    AlreadyExists {
        /// gRPC address of the existing active instance.
        grpc_address: String,
        /// object_id of the existing active instance.
        object_id: String,
    },
}

/// Trait for object registry (service discovery).
// TODO(crate-21): Replace the 7-arg discover() signature with a DiscoverOptions struct.
// The allow below is temporary until that refactor is done across all 14 implementations.
#[allow(clippy::too_many_arguments)]
#[async_trait]
pub trait ObjectRegistry: Send + Sync {
    async fn lookup(
        &self,
        ctx: &RequestContext,
        object_id: &str,
        object_type: Option<plexspaces_proto::object_registry::v1::ObjectType>,
    ) -> Result<Option<ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>>;

    async fn lookup_full(
        &self,
        ctx: &RequestContext,
        object_type: plexspaces_proto::object_registry::v1::ObjectType,
        object_id: &str,
    ) -> Result<Option<ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>>;

    async fn register(
        &self,
        ctx: &RequestContext,
        registration: ObjectRegistration,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;

    async fn discover(
        &self,
        ctx: &RequestContext,
        object_type: Option<plexspaces_proto::object_registry::v1::ObjectType>,
        object_category: Option<String>,
        capabilities: Option<Vec<String>>,
        labels: Option<Vec<String>>,
        health_status: Option<plexspaces_proto::object_registry::v1::HealthStatus>,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>>;

    async fn list_tenant_ids_by_object_type(
        &self,
        ctx: &RequestContext,
        object_type: plexspaces_proto::object_registry::v1::ObjectType,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
        let registrations = self
            .discover(ctx, Some(object_type), None, None, None, None, 0, 10_000)
            .await?;
        let mut tenant_ids = std::collections::BTreeSet::new();
        for registration in registrations {
            if !registration.tenant_id.is_empty() {
                tenant_ids.insert(registration.tenant_id);
            }
        }
        Ok(tenant_ids.into_iter().skip(offset).take(limit).collect())
    }

    async fn count_tenant_ids_by_object_type(
        &self,
        ctx: &RequestContext,
        object_type: plexspaces_proto::object_registry::v1::ObjectType,
    ) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
        let registrations = self
            .discover(ctx, Some(object_type), None, None, None, None, 0, 10_000)
            .await?;
        let mut tenant_ids = std::collections::BTreeSet::new();
        for registration in registrations {
            if !registration.tenant_id.is_empty() {
                tenant_ids.insert(registration.tenant_id);
            }
        }
        Ok(tenant_ids.len())
    }

    async fn unregister(
        &self,
        ctx: &RequestContext,
        object_type: plexspaces_proto::object_registry::v1::ObjectType,
        object_id: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;

    async fn unregister_all(
        &self,
        ctx: &RequestContext,
    ) -> Result<u64, Box<dyn std::error::Error + Send + Sync>> {
        let registrations = self
            .discover(ctx, None, None, None, None, None, 0, 10_000)
            .await?;
        let mut removed = 0_u64;
        for registration in registrations {
            let object_type = plexspaces_proto::object_registry::v1::ObjectType::try_from(
                registration.object_type,
            )
            .unwrap_or(plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeActor);
            self.unregister(ctx, object_type, &registration.object_id)
                .await?;
            removed += 1;
        }
        Ok(removed)
    }

    async fn heartbeat(
        &self,
        ctx: &RequestContext,
        object_type: plexspaces_proto::object_registry::v1::ObjectType,
        object_id: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;

    /// Lookup an object by alias (identity-based placement key).
    ///
    /// Returns `Ok(None)` if no registration holds the alias.
    /// Default implementation returns `Ok(None)` for adapters that do not support alias lookup.
    async fn lookup_by_alias(
        &self,
        _ctx: &RequestContext,
        _alias: &str,
    ) -> Result<Option<ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
        Ok(None)
    }

    /// Register with unique alias enforcement (Orleans grain directory pattern).
    ///
    /// If `enforce_unique` is true, an active registration with the same alias blocks
    /// the call and returns `RegisterResult::AlreadyExists` with routing info.
    /// Default implementation ignores alias and delegates to `register()`.
    async fn register_with_unique_alias(
        &self,
        ctx: &RequestContext,
        registration: ObjectRegistration,
        _enforce_unique: bool,
    ) -> Result<RegisterResult, Box<dyn std::error::Error + Send + Sync>> {
        self.register(ctx, registration).await?;
        Ok(RegisterResult::Registered)
    }

    /// Record a missed heartbeat for an object.
    ///
    /// Increments the failure count and transitions health:
    /// - count < max → DEGRADED
    /// - count >= max → DEAD (cascades to node objects when the object is a NODE)
    /// Default implementation returns DEGRADED without persisting (for adapters that lack this).
    async fn record_heartbeat_failure(
        &self,
        _ctx: &RequestContext,
        _object_id: &str,
    ) -> Result<HealthStatus, Box<dyn std::error::Error + Send + Sync>> {
        Ok(HealthStatus::HealthStatusDegraded)
    }

    /// Mark all HEALTHY/DEGRADED/STARTING objects on `node_id` as DEAD.
    ///
    /// Called when SWIM detects that a node has permanently left the cluster.
    /// Default implementation is a no-op for adapters that do not support this.
    async fn mark_objects_dead_by_node(
        &self,
        _ctx: &RequestContext,
        _node_id: &str,
    ) -> Result<u64, Box<dyn std::error::Error + Send + Sync>> {
        Ok(0)
    }

    /// Find registrations whose `last_heartbeat` is older than `threshold_seconds` ago.
    ///
    /// Only returns objects in HEALTHY or DEGRADED state (not already DEAD/STOPPING).
    /// An empty `ctx.tenant_id()` with `is_admin=true` performs a cross-tenant scan.
    /// Default implementation returns an empty list for adapters that do not persist heartbeats.
    async fn find_stale_heartbeats(
        &self,
        _ctx: &RequestContext,
        _threshold_seconds: i64,
        _limit: usize,
    ) -> Result<Vec<ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
        Ok(vec![])
    }
}
