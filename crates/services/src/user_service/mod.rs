// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! OAuth User Service implementation.
//!
//! ## Purpose
//! Implements all UserService RPCs: OIDC login flow, user/tenant listing,
//! and long-lived API token management.
//!
//! ## Architecture
//! UserServiceImpl (gRPC handler)
//!   → UserRepository      (users table)
//!   → TenantRepository    (tenants table)
//!   → ApiTokenRepository  (api_tokens table)

pub mod oidc;
mod repository;
mod tenant_repository;
pub mod api_token_repository;

use plexspaces_actor::ServiceLocator;
use plexspaces_common::request_context_ext::RequestContextExt;
use plexspaces_proto::security::v1::{
    user_service_server::UserService,
    CreateApiTokenRequest, CreateApiTokenResponse,
    DeleteApiTokenRequest, DeleteApiTokenResponse,
    GetOrCreateByEmailRequest, GetOrCreateByEmailResponse,
    ListApiTokensRequest, ListApiTokensResponse,
    ListTenantsRequest, ListTenantsResponse,
    ListUsersRequest, ListUsersResponse,
    UpdateUserRequest, UpdateUserResponse,
};
use plexspaces_proto::common::v1::PageResponse;
pub use repository::{SqlUserRepository, UserRepository};
pub use tenant_repository::{SqlTenantRepository, TenantRepository, TenantRepositoryError};
pub use api_token_repository::{
    ApiTokenRepository, ApiTokenRepositoryError, SqlApiTokenRepository,
};
use std::sync::Arc;
use tonic::{Request, Response, Status};

use crate::request_context_from_grpc_request;

/// User service gRPC implementation.
pub struct UserServiceImpl {
    user_repo: Arc<dyn UserRepository>,
    tenant_repo: Arc<dyn TenantRepository>,
    token_repo: Arc<dyn ApiTokenRepository>,
    service_locator: Arc<dyn ServiceLocator>,
}

impl UserServiceImpl {
    pub fn new(
        user_repo: Arc<dyn UserRepository>,
        tenant_repo: Arc<dyn TenantRepository>,
        token_repo: Arc<dyn ApiTokenRepository>,
        service_locator: Arc<dyn ServiceLocator>,
    ) -> Self {
        Self {
            user_repo,
            tenant_repo,
            token_repo,
            service_locator,
        }
    }

    /// Expose the tenant repository so the OIDC handler can call get_or_create_by_slug.
    pub fn tenant_repo(&self) -> Arc<dyn TenantRepository> {
        self.tenant_repo.clone()
    }

    /// Expose the API token repository so the HTTP gateway can validate psx_ tokens.
    pub fn token_repo(&self) -> Arc<dyn ApiTokenRepository> {
        self.token_repo.clone()
    }
}

// ─── Helper ──────────────────────────────────────────────────────────────────

fn build_page_response(total: i32, offset: i32, limit: i32) -> PageResponse {
    PageResponse {
        request_id: ulid::Ulid::new().to_string(),
        total_size: total,
        offset,
        limit,
        has_next: offset + limit < total,
    }
}

// ─── gRPC implementation ─────────────────────────────────────────────────────

#[tonic::async_trait]
impl UserService for UserServiceImpl {
    // ── OIDC login flow ──────────────────────────────────────────────────────

    async fn get_or_create_by_email(
        &self,
        request: Request<GetOrCreateByEmailRequest>,
    ) -> Result<Response<GetOrCreateByEmailResponse>, Status> {
        let _ctx = request_context_from_grpc_request(
            request.metadata(),
            &std::collections::HashMap::new(),
            &self.service_locator,
        )
        .await
        .map_err(|e| Status::unauthenticated(e.to_string()))?;

        let req = request.into_inner();

        if req.email.is_empty() {
            return Err(Status::invalid_argument("email is required"));
        }
        if req.tenant_id.is_empty() {
            return Err(Status::invalid_argument("tenant_id is required"));
        }

        let (user, created) = self
            .user_repo
            .get_or_create_by_email(&req)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        if created {
            metrics::counter!("plexspaces_auth_logins_total", "status" => "created", "provider" => req.provider.clone()).increment(1);
            tracing::info!(email = %req.email, tenant_id = %req.tenant_id, "auth.user.created");
        } else {
            metrics::counter!("plexspaces_auth_logins_total", "status" => "existing", "provider" => req.provider.clone()).increment(1);
            tracing::info!(email = %req.email, "auth.login.success");
        }

        Ok(Response::new(GetOrCreateByEmailResponse {
            request_id: req.request_id.clone(),
            user: Some(user),
            created,
        }))
    }

    // ── User management ──────────────────────────────────────────────────────

    async fn update_user(
        &self,
        request: Request<UpdateUserRequest>,
    ) -> Result<Response<UpdateUserResponse>, Status> {
        let ctx = request_context_from_grpc_request(
            request.metadata(),
            &std::collections::HashMap::new(),
            &self.service_locator,
        )
        .await
        .map_err(|e| Status::unauthenticated(e.to_string()))?;

        // Only admins may update other users; non-admins may only update themselves.
        let req = request.into_inner();

        if req.user_id.is_empty() {
            return Err(Status::invalid_argument("user_id is required"));
        }

        let caller_is_owner = ctx.user_id().map_or(false, |uid| uid == req.user_id);
        if !ctx.is_admin() && !caller_is_owner {
            return Err(Status::permission_denied(
                "Only an admin or the account owner may update this user",
            ));
        }

        // Non-admins cannot promote themselves to admin.
        if !ctx.is_admin() {
            if let Some(true) = req.admin {
                return Err(Status::permission_denied(
                    "Only an admin may change admin status",
                ));
            }
        }

        let user = self
            .user_repo
            .update_user(&req)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(UpdateUserResponse { request_id: req.request_id.clone(), user: Some(user) }))
    }

    async fn list_users(
        &self,
        request: Request<ListUsersRequest>,
    ) -> Result<Response<ListUsersResponse>, Status> {
        let ctx = request_context_from_grpc_request(
            request.metadata(),
            &std::collections::HashMap::new(),
            &self.service_locator,
        )
        .await
        .map_err(|e| Status::unauthenticated(e.to_string()))?;

        let req = request.into_inner();
        let page = req.page.unwrap_or_default();
        let offset = page.offset.max(0);
        let limit = if page.limit <= 0 { 50 } else { page.limit.min(1000) };

        // Admins may pass tenant_id_filter to query a specific tenant, or leave it empty for all.
        // Non-admins are always constrained to their own tenant.
        let tenant_filter: Option<String> = if ctx.is_admin() {
            if req.tenant_id_filter.is_empty() {
                None // admin + no filter → all tenants
            } else {
                Some(req.tenant_id_filter)
            }
        } else {
            Some(ctx.tenant_id().to_string())
        };

        let (users, total) = self
            .user_repo
            .list_users(tenant_filter.as_deref(), offset, limit)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(ListUsersResponse {
            request_id: req.request_id.clone(),
            users,
            page: Some(build_page_response(total, offset, limit)),
        }))
    }

    // ── Tenant management ────────────────────────────────────────────────────

    async fn list_tenants(
        &self,
        request: Request<ListTenantsRequest>,
    ) -> Result<Response<ListTenantsResponse>, Status> {
        let ctx = request_context_from_grpc_request(
            request.metadata(),
            &std::collections::HashMap::new(),
            &self.service_locator,
        )
        .await
        .map_err(|e| Status::unauthenticated(e.to_string()))?;

        let req = request.into_inner();
        let page = req.page.unwrap_or_default();
        let offset = page.offset.max(0);
        let limit = if page.limit <= 0 { 50 } else { page.limit.min(1000) };

        if ctx.is_admin() {
            let (tenants, total) = self
                .tenant_repo
                .list_tenants(offset, limit)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;

            Ok(Response::new(ListTenantsResponse {
                request_id: req.request_id.clone(),
                tenants,
                page: Some(build_page_response(total, offset, limit)),
            }))
        } else {
            // Non-admins see only their own tenant.
            let tenant = self
                .tenant_repo
                .get_tenant(ctx.tenant_id())
                .await
                .map_err(|e| Status::internal(e.to_string()))?;

            let tenants: Vec<_> = tenant.into_iter().collect();
            let total = tenants.len() as i32;
            Ok(Response::new(ListTenantsResponse {
                request_id: req.request_id.clone(),
                tenants,
                page: Some(build_page_response(total, 0, limit)),
            }))
        }
    }

    async fn create_api_token(
        &self,
        request: Request<CreateApiTokenRequest>,
    ) -> Result<Response<CreateApiTokenResponse>, Status> {
        let ctx = request_context_from_grpc_request(
            request.metadata(),
            &std::collections::HashMap::new(),
            &self.service_locator,
        )
        .await
        .map_err(|e| Status::unauthenticated(e.to_string()))?;

        let user_id = ctx
            .user_id()
            .ok_or_else(|| Status::unauthenticated("user_id missing from token"))?
            .to_string();

        let req = request.into_inner();

        if req.name.is_empty() {
            return Err(Status::invalid_argument("name is required"));
        }

        let scopes: Vec<String> = if req.scopes.is_empty() {
            vec!["read".into(), "write".into()]
        } else {
            req.scopes.clone()
        };

        let ttl_secs = req.ttl.as_ref().map(|d| d.seconds).unwrap_or(90 * 24 * 3600);
        let expires_at = Some(chrono::Utc::now().timestamp() + ttl_secs);

        let token_id = ulid::Ulid::new().to_string();

        let token = self
            .token_repo
            .create(
                &token_id,
                &user_id,
                ctx.tenant_id(),
                &req.name,
                &scopes,
                expires_at,
                ctx.is_admin(),
            )
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        metrics::counter!("plexspaces_api_tokens_created_total").increment(1);
        tracing::info!(user_id = %user_id, token_id = %token.token_id, name = %req.name, "auth.api_token.created");

        // The plaintext JWT is generated by the caller (HTTP route handler)
        // which has access to the signing key. The gRPC layer returns token_id
        // so the caller can embed it as the JWT jti claim.
        Ok(Response::new(CreateApiTokenResponse {
            request_id: req.request_id.clone(),
            token: Some(token),
            plaintext: token_id,
        }))
    }

    async fn delete_api_token(
        &self,
        request: Request<DeleteApiTokenRequest>,
    ) -> Result<Response<DeleteApiTokenResponse>, Status> {
        let ctx = request_context_from_grpc_request(
            request.metadata(),
            &std::collections::HashMap::new(),
            &self.service_locator,
        )
        .await
        .map_err(|e| Status::unauthenticated(e.to_string()))?;

        let user_id = ctx
            .user_id()
            .ok_or_else(|| Status::unauthenticated("user_id missing from token"))?
            .to_string();

        let req = request.into_inner();

        if req.token_id.is_empty() {
            return Err(Status::invalid_argument("token_id is required"));
        }

        self.token_repo
            .revoke(&req.token_id, &user_id, ctx.is_admin())
            .await
            .map_err(|e| match e {
                api_token_repository::ApiTokenRepositoryError::NotFound(msg) => {
                    Status::not_found(msg)
                }
                api_token_repository::ApiTokenRepositoryError::PermissionDenied(msg) => {
                    Status::permission_denied(msg)
                }
                api_token_repository::ApiTokenRepositoryError::Database(msg) => {
                    Status::internal(msg)
                }
            })?;

        metrics::counter!("plexspaces_api_tokens_revoked_total").increment(1);
        tracing::info!(user_id = %user_id, token_id = %req.token_id, "auth.api_token.revoked");

        Ok(Response::new(DeleteApiTokenResponse { request_id: req.request_id.clone() }))
    }

    async fn list_api_tokens(
        &self,
        request: Request<ListApiTokensRequest>,
    ) -> Result<Response<ListApiTokensResponse>, Status> {
        let ctx = request_context_from_grpc_request(
            request.metadata(),
            &std::collections::HashMap::new(),
            &self.service_locator,
        )
        .await
        .map_err(|e| Status::unauthenticated(e.to_string()))?;

        let user_id = ctx
            .user_id()
            .ok_or_else(|| Status::unauthenticated("user_id missing from token"))?
            .to_string();

        let req = request.into_inner();
        let page = req.page.unwrap_or_default();
        let offset = page.offset.max(0);
        let limit = if page.limit <= 0 { 50 } else { page.limit.min(1000) };

        let (tokens, total) = self
            .token_repo
            .list_for_user(&user_id, offset, limit)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(ListApiTokensResponse {
            request_id: req.request_id.clone(),
            tokens,
            page: Some(build_page_response(total, offset, limit)),
        }))
    }
}
