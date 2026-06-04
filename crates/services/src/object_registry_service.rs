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

//! # Object Registry gRPC Service Handler
//!
//! ## Purpose
//! Implements the `ObjectRegistry` tonic service trait so the registry is
//! accessible over the network from SDKs, WIT components, and peer nodes.
//!
//! ## Key behaviours
//! - `register`: supports `enforce_unique_alias`; returns `existing_grpc_address`
//!   and `existing_object_id` on alias conflict so callers can forward directly.
//! - `lookup`: routes to alias-based lookup when `alias` is set in the request.
//! - `discover`: maps DiscoverRequest fields to the service-trait `discover()`.
//! - `heartbeat` / `batch_heartbeat`: update `last_heartbeat` and reset failure count.
//! - `list_object_types`: returns per-type counts (best-effort via `discover`).

use async_trait::async_trait;
use plexspaces_actor::{DiscoverOptions, ServiceLocator as ServiceLocatorTrait};
use plexspaces_common::{RequestContext, RequestContextExt};
use plexspaces_proto::object_registry::v1::{
    object_registry_server::ObjectRegistry as ObjectRegistryGrpc, BatchHeartbeatRequest,
    BatchHeartbeatResponse, DiscoverRequest, DiscoverResponse, HeartbeatRequest, HeartbeatResponse,
    HealthStatus, ListObjectTypesRequest, ListObjectTypesResponse, LookupRequest, LookupResponse,
    ObjectType, ObjectTypeSummary, RegisterRequest, RegisterResponse, UnregisterRequest,
    UnregisterResponse,
};
use plexspaces_service_traits::{ObjectRegistry, RegisterResult};
use std::sync::Arc;
use tonic::{Request, Response, Status};
use tracing::instrument;

/// gRPC handler for the ObjectRegistry service.
///
/// Wraps the `Arc<dyn ObjectRegistry>` service-trait and the `ServiceLocator`
/// so it can authenticate requests and resolve context.
pub struct ObjectRegistryServiceImpl {
    registry: Arc<dyn ObjectRegistry>,
    service_locator: Arc<dyn ServiceLocatorTrait>,
}

impl ObjectRegistryServiceImpl {
    pub fn new(
        registry: Arc<dyn ObjectRegistry>,
        service_locator: Arc<dyn ServiceLocatorTrait>,
    ) -> Self {
        Self {
            registry,
            service_locator,
        }
    }

    /// Build a `RequestContext` from gRPC metadata, falling back to tenant/namespace
    /// embedded in the request body when headers are absent.
    async fn ctx_from_request(
        &self,
        metadata: &tonic::metadata::MetadataMap,
        body_tenant_id: &str,
        body_namespace: &str,
    ) -> RequestContext {
        let labels = std::collections::HashMap::new();
        match crate::request_context_from_grpc_request(
            metadata,
            &labels,
            &(self.service_locator.clone() as Arc<dyn ServiceLocatorTrait>),
        )
        .await
        {
            Ok(ctx) => {
                // Prefer auth-derived tenant/namespace; fall back to body fields.
                // Preserve is_admin so cross-tenant admin callers work over gRPC.
                let tenant = if ctx.tenant_id().is_empty() {
                    body_tenant_id.to_string()
                } else {
                    ctx.tenant_id().to_string()
                };
                let ns = if ctx.namespace().is_empty() {
                    body_namespace.to_string()
                } else {
                    ctx.namespace().to_string()
                };
                RequestContext::new_without_auth(tenant, ns).with_admin(ctx.is_admin())
            }
            Err(_) => {
                // Auth not configured / header absent — use body fields directly.
                RequestContext::new_without_auth(
                    body_tenant_id.to_string(),
                    body_namespace.to_string(),
                )
            }
        }
    }
}

#[async_trait]
impl ObjectRegistryGrpc for ObjectRegistryServiceImpl {
    /// Register or update an object registration.
    ///
    /// When `enforce_unique_alias` is set and an active registration already holds
    /// the same alias, returns `existing_grpc_address` / `existing_object_id` so the
    /// caller can forward directly without a re-registration.
    #[instrument(skip(self, request), name = "object_registry_grpc_register")]
    async fn register(
        &self,
        request: Request<RegisterRequest>,
    ) -> Result<Response<RegisterResponse>, Status> {
        let (metadata, _, body) = request.into_parts();
        let reg = body
            .registration
            .ok_or_else(|| Status::invalid_argument("registration is required"))?;

        let ctx = self
            .ctx_from_request(&metadata, &reg.tenant_id, &reg.namespace)
            .await;

        if body.enforce_unique_alias {
            match self
                .registry
                .register_with_unique_alias(&ctx, reg.clone(), true)
                .await
            {
                Ok(RegisterResult::Registered) => {
                    return Ok(Response::new(RegisterResponse {
                        registration: Some(reg),
                        created: true,
                        ..Default::default()
                    }));
                }
                Ok(RegisterResult::AlreadyExists {
                    grpc_address,
                    object_id,
                }) => {
                    return Ok(Response::new(RegisterResponse {
                        registration: None,
                        created: false,
                        existing_grpc_address: grpc_address,
                        existing_object_id: object_id,
                    }));
                }
                Err(e) => return Err(Status::internal(e.to_string())),
            }
        }

        match self.registry.register(&ctx, reg.clone()).await {
            Ok(()) => Ok(Response::new(RegisterResponse {
                registration: Some(reg),
                created: true,
                ..Default::default()
            })),
            Err(e) => Err(Status::internal(e.to_string())),
        }
    }

    #[instrument(skip(self, request), name = "object_registry_grpc_unregister")]
    async fn unregister(
        &self,
        request: Request<UnregisterRequest>,
    ) -> Result<Response<UnregisterResponse>, Status> {
        let (metadata, _, body) = request.into_parts();
        let ctx = self
            .ctx_from_request(&metadata, &body.tenant_id, &body.namespace)
            .await;

        let object_type = ObjectType::try_from(body.object_type)
            .unwrap_or(ObjectType::ObjectTypeUnspecified);

        match self
            .registry
            .unregister(&ctx, object_type, &body.object_id)
            .await
        {
            Ok(()) => Ok(Response::new(UnregisterResponse { unregistered: true })),
            Err(e) => {
                let msg = e.to_string().to_lowercase();
                if msg.contains("not found") || msg.contains("does not exist") {
                    Ok(Response::new(UnregisterResponse {
                        unregistered: false,
                    }))
                } else {
                    Err(Status::internal(e.to_string()))
                }
            }
        }
    }

    /// Lookup a single object by ID or by alias.
    ///
    /// When `alias` is non-empty in the request, routes to `lookup_by_alias`;
    /// otherwise uses the standard `lookup` by `object_id`.
    #[instrument(skip(self, request), name = "object_registry_grpc_lookup")]
    async fn lookup(
        &self,
        request: Request<LookupRequest>,
    ) -> Result<Response<LookupResponse>, Status> {
        let (metadata, _, body) = request.into_parts();
        let ctx = self
            .ctx_from_request(&metadata, &body.tenant_id, &body.namespace)
            .await;

        // Alias-based lookup takes precedence.
        if !body.alias.is_empty() {
            return match self.registry.lookup_by_alias(&ctx, &body.alias).await {
                Ok(Some(reg)) => Ok(Response::new(LookupResponse {
                    registration: Some(reg),
                    found: true,
                })),
                Ok(None) => Ok(Response::new(LookupResponse {
                    registration: None,
                    found: false,
                })),
                Err(e) => Err(Status::internal(e.to_string())),
            };
        }

        // Standard object_id lookup.
        let object_type = ObjectType::try_from(body.object_type).ok();
        match self
            .registry
            .lookup(&ctx, &body.object_id, object_type)
            .await
        {
            Ok(Some(reg)) => Ok(Response::new(LookupResponse {
                registration: Some(reg),
                found: true,
            })),
            Ok(None) => Ok(Response::new(LookupResponse {
                registration: None,
                found: false,
            })),
            Err(e) => Err(Status::internal(e.to_string())),
        }
    }

    #[instrument(skip(self, request), name = "object_registry_grpc_discover")]
    async fn discover(
        &self,
        request: Request<DiscoverRequest>,
    ) -> Result<Response<DiscoverResponse>, Status> {
        let (metadata, _, body) = request.into_parts();

        // Derive tenant/namespace: prefer auth headers, fall back to filter fields.
        let ctx = self
            .ctx_from_request(&metadata, &body.tenant_id, &body.namespace)
            .await;

        let object_type = if body.object_type == 0 {
            None
        } else {
            ObjectType::try_from(body.object_type).ok()
        };
        let health_status = if body.health_status == 0 {
            None
        } else {
            HealthStatus::try_from(body.health_status).ok()
        };
        let capabilities = if body.capabilities.is_empty() {
            None
        } else {
            Some(body.capabilities)
        };
        let labels = if body.labels.is_empty() {
            None
        } else {
            Some(body.labels)
        };
        let category = if body.object_category.is_empty() {
            None
        } else {
            Some(body.object_category)
        };

        let page_size = if body.page_size > 0 {
            body.page_size.min(1000) as usize
        } else {
            100
        };
        let offset = 0usize; // page_token-based offset not yet implemented; start from 0

        match self
            .registry
            .discover(
                &ctx,
                DiscoverOptions {
                    object_type,
                    object_category: category,
                    capabilities,
                    labels,
                    health_status,
                    offset,
                    limit: page_size,
                },
            )
            .await
        {
            Ok(registrations) => {
                let has_more = registrations.len() >= page_size;
                let total = registrations.len() as i64;
                Ok(Response::new(DiscoverResponse {
                    registrations,
                    total_count: total,
                    has_more,
                    next_page_token: String::new(),
                }))
            }
            Err(e) => Err(Status::internal(e.to_string())),
        }
    }

    #[instrument(skip(self, request), name = "object_registry_grpc_heartbeat")]
    async fn heartbeat(
        &self,
        request: Request<HeartbeatRequest>,
    ) -> Result<Response<HeartbeatResponse>, Status> {
        let (metadata, _, body) = request.into_parts();
        let ctx = self
            .ctx_from_request(&metadata, &body.tenant_id, &body.namespace)
            .await;

        let object_type = ObjectType::try_from(body.object_type)
            .unwrap_or(ObjectType::ObjectTypeUnspecified);

        match self.registry.heartbeat(&ctx, object_type, &body.object_id).await {
            Ok(()) => Ok(Response::new(HeartbeatResponse {
                accepted: true,
                registration: None,
            })),
            Err(e) => {
                let msg = e.to_string().to_lowercase();
                if msg.contains("not found") || msg.contains("does not exist") {
                    Ok(Response::new(HeartbeatResponse {
                        accepted: false,
                        registration: None,
                    }))
                } else {
                    Err(Status::internal(e.to_string()))
                }
            }
        }
    }

    #[instrument(skip(self, request), name = "object_registry_grpc_batch_heartbeat")]
    async fn batch_heartbeat(
        &self,
        request: Request<BatchHeartbeatRequest>,
    ) -> Result<Response<BatchHeartbeatResponse>, Status> {
        let (metadata, _, body) = request.into_parts();
        let mut results = Vec::with_capacity(body.heartbeats.len());
        let mut success_count = 0i32;
        let mut failure_count = 0i32;

        for hb in body.heartbeats {
            let ctx = self
                .ctx_from_request(&metadata, &hb.tenant_id, &hb.namespace)
                .await;
            let object_type =
                ObjectType::try_from(hb.object_type).unwrap_or(ObjectType::ObjectTypeUnspecified);
            let accepted = self
                .registry
                .heartbeat(&ctx, object_type, &hb.object_id)
                .await
                .is_ok();
            if accepted {
                success_count += 1;
            } else {
                failure_count += 1;
            }
            results.push(HeartbeatResponse {
                accepted,
                registration: None,
            });
        }

        Ok(Response::new(BatchHeartbeatResponse {
            results,
            success_count,
            failure_count,
        }))
    }

    #[instrument(skip(self, request), name = "object_registry_grpc_list_object_types")]
    async fn list_object_types(
        &self,
        request: Request<ListObjectTypesRequest>,
    ) -> Result<Response<ListObjectTypesResponse>, Status> {
        let (metadata, _, body) = request.into_parts();
        let ctx = self
            .ctx_from_request(&metadata, &body.tenant_id, &body.namespace)
            .await;

        // TODO: replace with a COUNT GROUP BY repository query to avoid full table scan.
        let all = match self
            .registry
            .discover(
                &ctx,
                DiscoverOptions {
                    limit: 10_000,
                    ..Default::default()
                },
            )
            .await
        {
            Ok(r) => r,
            Err(e) => return Err(Status::internal(e.to_string())),
        };

        let mut type_counts: std::collections::HashMap<i32, i64> = std::collections::HashMap::new();
        for reg in &all {
            *type_counts.entry(reg.object_type).or_default() += 1;
        }

        let summaries = type_counts
            .into_iter()
            .map(|(ot, count)| ObjectTypeSummary {
                object_type: ot,
                count,
                health_counts: Default::default(),
            })
            .collect::<Vec<_>>();

        let total_count = all.len() as i64;
        Ok(Response::new(ListObjectTypesResponse {
            summaries,
            total_count,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plexspaces_object_registry::{ObjectRegistryImpl, SqliteObjectRegistryRepository};
    use plexspaces_proto::object_registry::v1::{ObjectRegistration, ObjectType};
    use std::sync::Arc;

    async fn make_service() -> ObjectRegistryServiceImpl {
        let repo = Arc::new(
            SqliteObjectRegistryRepository::new(":memory:")
                .await
                .unwrap(),
        );
        let registry = Arc::new(ObjectRegistryImpl::new(repo));
        let locator = Arc::new(crate::service_locator::ServiceLocatorImpl::new());
        ObjectRegistryServiceImpl::new(
            registry as Arc<dyn ObjectRegistry>,
            locator as Arc<dyn ServiceLocatorTrait>,
        )
    }

    fn reg(id: &str, tenant: &str, ns: &str) -> ObjectRegistration {
        ObjectRegistration {
            object_id: id.to_string(),
            object_type: ObjectType::ObjectTypeActor as i32,
            grpc_address: "http://test:8000".to_string(),
            tenant_id: tenant.to_string(),
            namespace: ns.to_string(),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn test_register_and_lookup() {
        let svc = make_service().await;
        let registration = reg("actor-1", "t1", "ns1");

        let req = Request::new(RegisterRequest {
            registration: Some(registration.clone()),
            enforce_unique_alias: false,
            ..Default::default()
        });
        let resp = svc.register(req).await.unwrap().into_inner();
        assert!(resp.created);

        let lookup = Request::new(LookupRequest {
            object_id: "actor-1".to_string(),
            tenant_id: "t1".to_string(),
            namespace: "ns1".to_string(),
            ..Default::default()
        });
        let found = svc.lookup(lookup).await.unwrap().into_inner();
        assert!(found.found);
        assert_eq!(found.registration.unwrap().object_id, "actor-1");
    }

    #[tokio::test]
    async fn test_lookup_by_alias() {
        let svc = make_service().await;
        let mut registration = reg("actor-alias", "t1", "ns1");
        registration.alias = "Counter:worker:ns1:t1".to_string();

        let req = Request::new(RegisterRequest {
            registration: Some(registration),
            enforce_unique_alias: false,
            ..Default::default()
        });
        svc.register(req).await.unwrap();

        let lookup = Request::new(LookupRequest {
            alias: "Counter:worker:ns1:t1".to_string(),
            tenant_id: "t1".to_string(),
            namespace: "ns1".to_string(),
            ..Default::default()
        });
        let found = svc.lookup(lookup).await.unwrap().into_inner();
        assert!(found.found);
        assert_eq!(found.registration.unwrap().object_id, "actor-alias");
    }

    #[tokio::test]
    async fn test_register_enforce_unique_alias_conflict() {
        let svc = make_service().await;
        let mut r1 = reg("actor-a", "t1", "ns1");
        r1.alias = "Counter:w:ns1:t1".to_string();
        r1.health_status = plexspaces_proto::object_registry::v1::HealthStatus::HealthStatusHealthy as i32;

        svc.register(Request::new(RegisterRequest {
            registration: Some(r1),
            enforce_unique_alias: true,
            ..Default::default()
        }))
        .await
        .unwrap();

        let mut r2 = reg("actor-b", "t1", "ns1");
        r2.alias = "Counter:w:ns1:t1".to_string();

        let resp = svc
            .register(Request::new(RegisterRequest {
                registration: Some(r2),
                enforce_unique_alias: true,
                ..Default::default()
            }))
            .await
            .unwrap()
            .into_inner();

        assert!(!resp.created);
        assert_eq!(resp.existing_object_id, "actor-a");
    }

    #[tokio::test]
    async fn test_unregister() {
        let svc = make_service().await;
        svc.register(Request::new(RegisterRequest {
            registration: Some(reg("actor-del", "t1", "ns1")),
            ..Default::default()
        }))
        .await
        .unwrap();

        let resp = svc
            .unregister(Request::new(UnregisterRequest {
                object_id: "actor-del".to_string(),
                object_type: ObjectType::ObjectTypeActor as i32,
                tenant_id: "t1".to_string(),
                namespace: "ns1".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(resp.unregistered);

        // Lookup should now return not found.
        let found = svc
            .lookup(Request::new(LookupRequest {
                object_id: "actor-del".to_string(),
                tenant_id: "t1".to_string(),
                namespace: "ns1".to_string(),
                ..Default::default()
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(!found.found);
    }

    #[tokio::test]
    async fn test_heartbeat_accepted() {
        let svc = make_service().await;
        svc.register(Request::new(RegisterRequest {
            registration: Some(reg("actor-hb", "t1", "ns1")),
            ..Default::default()
        }))
        .await
        .unwrap();

        let resp = svc
            .heartbeat(Request::new(HeartbeatRequest {
                object_id: "actor-hb".to_string(),
                object_type: ObjectType::ObjectTypeActor as i32,
                tenant_id: "t1".to_string(),
                namespace: "ns1".to_string(),
                ..Default::default()
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(resp.accepted);
    }

    #[tokio::test]
    async fn test_discover() {
        let svc = make_service().await;
        for i in 0..3 {
            svc.register(Request::new(RegisterRequest {
                registration: Some(reg(&format!("actor-{}", i), "t1", "ns1")),
                ..Default::default()
            }))
            .await
            .unwrap();
        }

        let resp = svc
            .discover(Request::new(DiscoverRequest {
                object_type: ObjectType::ObjectTypeActor as i32,
                tenant_id: "t1".to_string(),
                namespace: "ns1".to_string(),
                page_size: 10,
                ..Default::default()
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(resp.registrations.len(), 3);
    }
}
