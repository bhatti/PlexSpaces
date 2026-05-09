// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! ObjectRegistry trait for service discovery.

use async_trait::async_trait;
use plexspaces_common::RequestContext;

pub type ObjectRegistration = plexspaces_proto::object_registry::v1::ObjectRegistration;

/// Trait for object registry (service discovery).
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
}
