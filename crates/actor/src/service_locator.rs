// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! Helper functions for ServiceLocator
//!
//! This module contains helper functions for working with ServiceLocator.
//! The ServiceLocator trait is defined in `service_locator_trait.rs`.
//! The ServiceLocatorImpl implementation is in `plexspaces-services` crate.

use crate::service_locator_trait::ServiceLocator as ServiceLocatorTrait;
use crate::{RequestContext, RequestContextExt};
use std::sync::Arc;

/// Helper function to create RequestContext from gRPC request metadata
///
/// ## Purpose
/// Helper method that extracts tenant_id, namespace, user_id, and admin flag from gRPC request metadata
/// and creates a RequestContext using shared validation from RequestContext::from_auth.
///
/// ## Sources (in order of precedence):
/// 1. `x-tenant-id` in metadata after interceptors run — **not** a client-supplied tenant channel: when auth
///    is enabled, `AuthInterceptor` strips inbound values and repopulates from JWT/mTLS claims only.
/// 2. `x-namespace` header (namespace override or scope hint; may be empty)
/// 3. `x-user-id` header (from JWT middleware, optional)
/// 4. `x-admin` header (from JWT middleware, optional)
/// 5. `tenant_id` in request labels (fallback, only if auth disabled)
/// 6. Default values from NodeConfig in ServiceLocator (if auth disabled)
///
/// ## Arguments
/// * `metadata` - gRPC metadata after the interceptor chain (tenant id must originate from auth, not from raw client metadata)
/// * `labels` - Request labels (for fallback)
/// * `service_locator` - ServiceLocator to get NodeConfig and auth_enabled from SecurityConfig
///
/// ## Returns
/// RequestContext or error if validation fails (e.g. auth enabled but missing tenant_id)
pub async fn request_context_from_grpc_request(
    metadata: &tonic::metadata::MetadataMap,
    labels: &std::collections::HashMap<String, String>,
    service_locator: &Arc<dyn ServiceLocatorTrait>,
) -> Result<RequestContext, plexspaces_common::RequestContextError> {
    // Auth enabled comes from SecurityConfig (disable_auth), not from header presence
    let auth_enabled = !service_locator.is_auth_disabled().await;

    // NOTE: default_tenant_id and default_namespace have been removed from NodeConfig.
    // Tenant comes from auth (JWT/mTLS); namespace from application/actor or request.

    // Extract tenant_id - RequestContext::from_auth will validate based on auth_enabled
    let tenant_id_from_header = metadata
        .get("x-tenant-id")
        .and_then(|v| v.to_str().ok())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let tenant_id_from_labels = labels.get("tenant_id").filter(|s| !s.is_empty()).cloned();

    // Extract namespace - can be empty, RequestContext::from_auth handles defaults
    let namespace_from_header = metadata
        .get("x-namespace")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let namespace_from_labels = labels.get("namespace").cloned();

    // Extract user_id and admin from metadata
    let user_id = metadata
        .get("x-user-id")
        .and_then(|v| v.to_str().ok())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let admin = metadata
        .get("x-admin")
        .and_then(|v| v.to_str().ok())
        .map(|s| s == "true" || s == "1")
        .unwrap_or(false);

    // Extract Bearer token so it can be forwarded in cross-node gRPC calls.
    // The token is forwarded by apply_request_context_to_grpc_metadata and validated
    // by the receiving node's auth interceptor (JWT mode) or accepted by mTLS peers.
    let bearer_token = metadata
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .filter(|s| s.starts_with("Bearer ") || s.starts_with("bearer "))
        .map(|s| s[7..].to_string());

    // Use shared validation from RequestContext::from_auth
    // This validates tenant_id if auth_enabled, otherwise allows empty tenant_id
    // No defaults from NodeConfig - tenant/namespace must come from request
    let ctx = RequestContext::from_auth(
        tenant_id_from_header.or(tenant_id_from_labels),
        namespace_from_header.or(namespace_from_labels),
        user_id,
        admin,
        auth_enabled,
        None, // No default tenant - must come from request/auth
        None, // No default namespace - must come from request
    )?;

    // Store Bearer token in context so cross-node calls can forward it
    if let Some(token) = bearer_token {
        Ok(ctx.with_bearer_token(token))
    } else {
        Ok(ctx)
    }
}

/// Apply [`RequestContext`] to outbound gRPC metadata so a peer can run
/// [`request_context_from_grpc_request`] and recover the same tenant (JWT / auth
/// Propagate request context to outbound gRPC metadata for service-to-service calls.
///
/// Node-to-node calls use mTLS for identity. The receiving node trusts headers
/// from mTLS-authenticated peers, so we propagate the full context including
/// tenant-id for delegated (on-behalf-of-user) requests.
///
/// Header names match the ingress contract used by [`request_context_from_grpc_request`]:
/// `x-tenant-id`, `x-namespace`, `x-user-id`, `x-admin`, and `authorization` when present.
pub fn apply_request_context_to_grpc_metadata(
    ctx: &RequestContext,
    md: &mut tonic::metadata::MetadataMap,
) {
    use tonic::metadata::MetadataValue;
    if !ctx.tenant_id().is_empty() {
        if let Ok(v) = MetadataValue::try_from(ctx.tenant_id()) {
            let _ = md.insert("x-tenant-id", v);
        }
    }
    if !ctx.namespace().is_empty() {
        if let Ok(v) = MetadataValue::try_from(ctx.namespace()) {
            let _ = md.insert("x-namespace", v);
        }
    }
    if let Some(uid) = ctx.user_id() {
        if !uid.is_empty() {
            if let Ok(v) = MetadataValue::try_from(uid) {
                let _ = md.insert("x-user-id", v);
            }
        }
    }
    if ctx.is_admin() {
        let _ = md.insert("x-admin", MetadataValue::from_static("true"));
    }
    if let Some(authz) = ctx.get_header("authorization") {
        if !authz.is_empty() {
            if let Ok(v) = MetadataValue::try_from(authz) {
                let _ = md.insert("authorization", v);
            }
        }
    }
}
