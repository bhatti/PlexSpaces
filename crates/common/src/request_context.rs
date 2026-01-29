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

//! Request Context (Go-style context.Context)
//!
//! ## Purpose
//! Provides request-scoped context similar to Go's context.Context.
//! Carries tenant isolation, tracing, and request metadata through the call chain.
//!
//! ## Design Philosophy
//! - **Tenant Isolation**: tenant_id from auth (JWT/mTLS); empty when auth disabled.
//! - **Namespace**: from application/actor or request; never hardcoded.
//! - **Tracing**: request_id and correlation_id for distributed tracing.
//! - **Immutable**: Context is passed by reference; no mutation in callees.
//!
//! ## Propagation (single source of truth)
//! - **Entry points**: HTTP sets `x-tenant-id` and `x-namespace` from path/JWT; gRPC uses
//!   `plexspaces_core::request_context_from_grpc_request(metadata, labels, service_locator)`.
//! - **All service methods** take `ctx: &RequestContext` and use `ctx.tenant_id()` /
//!   `ctx.namespace()`; no hardcoded tenant or namespace in business logic.
//! - **System operations** (node registration, heartbeats) use
//!   `ServiceLocator::request_context_for_system_operations()` (empty tenant, optional namespace).

use std::collections::HashMap;
use chrono::Utc;
use ulid::Ulid;
use prost_types::Timestamp;

/// Request context (Go-style context.Context)
///
/// ## Purpose
/// Carries tenant isolation, tracing, and request metadata through call chain.
/// Similar to Go's context.Context but with explicit tenant isolation.
///
/// ## Usage Pattern
/// ```rust
/// // Create context from request (tenant_id and namespace are REQUIRED)
/// let ctx = RequestContext::new("tenant-123".to_string(), "production".to_string(), false)?;
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestContext {
    /// Tenant ID (REQUIRED for all operations)
    pub tenant_id: String,
    
    /// Namespace within tenant (optional, can be empty)
    ///
    /// Used for further isolation within a tenant. Can be empty string.
    /// For admin/internal contexts with empty namespace, repository lookups
    /// bypass namespace filtering to allow cross-namespace queries.
    pub namespace: String,
    
    /// User ID (from JWT, optional)
    pub user_id: Option<String>,
    
    /// Request ID (for tracing)
    pub request_id: String,
    
    /// Correlation ID (for distributed tracing)
    pub correlation_id: Option<String>,
    
    /// Request timestamp
    pub timestamp: Timestamp,
    
    /// Metadata (extensible key-value pairs)
    pub metadata: HashMap<String, String>,
    
    /// Admin flag (from JWT, optional)
    ///
    /// When true, indicates the user has admin privileges.
    /// Admin users with empty namespace can bypass namespace filtering for
    /// administrative operations (see should_skip_namespace_filter()).
    pub admin: bool,
    
    /// Internal flag (for system operations)
    ///
    /// When true, indicates this is an internal system operation.
    /// Internal operations bypass authn/authz and tenant filtering.
    /// Internal contexts with empty namespace can bypass namespace filtering
    /// for system operations (see should_skip_namespace_filter()).
    pub internal: bool,
    
    /// Auth enabled flag (from SecurityConfig)
    ///
    /// When true, indicates authentication is enabled.
    /// If auth is enabled and tenant_id is empty, RequestContext creation will fail.
    /// If auth is disabled, tenant_id can be empty.
    pub auth_enabled: bool,
}

impl RequestContext {
    /// Create a new RequestContext with required tenant_id and namespace
    ///
    /// ## Arguments
    /// * `tenant_id` - Tenant identifier (required if auth_enabled, empty if auth disabled)
    /// * `namespace` - Namespace identifier (can be empty)
    /// * `auth_enabled` - Whether authentication is enabled (from SecurityConfig)
    ///
    /// ## Returns
    /// New RequestContext or error if validation fails
    ///
    /// ## Validation
    /// - If auth_enabled is true and tenant_id is empty, returns error
    /// - If auth_enabled is false, tenant_id can be empty
    /// - namespace can always be empty
    pub fn new(
        tenant_id: String,
        namespace: String,
        auth_enabled: bool,
    ) -> Result<Self, RequestContextError> {
        // Validate: if auth is enabled, tenant_id must not be empty
        if auth_enabled && tenant_id.is_empty() {
            return Err(RequestContextError::MissingTenantId);
        }
        
        let now = Utc::now();
        Ok(Self {
            tenant_id,
            namespace,
            user_id: None,
            request_id: Ulid::new().to_string(),
            correlation_id: None,
            timestamp: Timestamp {
                seconds: now.timestamp(),
                nanos: now.timestamp_subsec_nanos() as i32,
            },
            metadata: HashMap::new(),
            admin: false,
            internal: false,
            auth_enabled,
        })
    }
    
    /// Create a new RequestContext (convenience method for backward compatibility)
    ///
    /// ## Note
    /// This assumes auth is disabled. For production, use `new()` with explicit auth_enabled.
    pub fn new_without_auth(tenant_id: String, namespace: String) -> Self {
        Self::new(tenant_id, namespace, false).unwrap()
    }

    /// Create RequestContext from proto message
    ///
    /// ## Arguments
    /// * `proto` - RequestContext proto message
    /// * `auth_enabled` - Whether authentication is enabled (from SecurityConfig)
    ///
    /// ## Returns
    /// RequestContext or error if validation fails
    ///
    /// ## Validation
    /// - If auth_enabled is true and tenant_id is empty, returns error
    /// - If auth_enabled is false, tenant_id can be empty
    /// - namespace can always be empty (defaults to empty string)
    pub fn from_proto(
        proto: &plexspaces_proto::v1::common::RequestContext,
        auth_enabled: bool,
    ) -> Result<Self, RequestContextError> {
        // Validate: if auth is enabled, tenant_id must not be empty
        if auth_enabled && proto.tenant_id.is_empty() {
            return Err(RequestContextError::MissingTenantId);
        }

        // Namespace can be empty - default to empty string
        let namespace = proto.namespace.clone();

        let now = Utc::now();
        Ok(Self {
            tenant_id: proto.tenant_id.clone(),
            namespace,
            user_id: if proto.user_id.is_empty() {
                None
            } else {
                Some(proto.user_id.clone())
            },
            request_id: if proto.request_id.is_empty() {
                Ulid::new().to_string()
            } else {
                proto.request_id.clone()
            },
            correlation_id: if proto.correlation_id.is_empty() {
                None
            } else {
                Some(proto.correlation_id.clone())
            },
            timestamp: proto.timestamp.clone().unwrap_or_else(|| {
                Timestamp {
                    seconds: now.timestamp(),
                    nanos: now.timestamp_subsec_nanos() as i32,
                }
            }),
            metadata: proto.metadata.clone(),
            admin: proto.admin,
            internal: proto.internal,
            auth_enabled,
        })
    }

    /// Convert to proto message
    pub fn to_proto(&self) -> plexspaces_proto::v1::common::RequestContext {
        plexspaces_proto::v1::common::RequestContext {
            tenant_id: self.tenant_id.clone(),
            namespace: self.namespace.clone(),
            user_id: self.user_id.clone().unwrap_or_default(),
            request_id: self.request_id.clone(),
            correlation_id: self.correlation_id.clone().unwrap_or_default(),
            timestamp: Some(self.timestamp.clone()),
            metadata: self.metadata.clone(),
            admin: self.admin,
            internal: self.internal,
            auth_enabled: self.auth_enabled,
        }
    }

    /// Set namespace (builder pattern)
    pub fn with_namespace(mut self, namespace: String) -> Self {
        self.namespace = namespace;
        self
    }

    /// Set user_id (builder pattern)
    pub fn with_user_id(mut self, user_id: String) -> Self {
        self.user_id = Some(user_id);
        self
    }

    /// Set correlation_id (builder pattern)
    pub fn with_correlation_id(mut self, correlation_id: String) -> Self {
        self.correlation_id = Some(correlation_id);
        self
    }

    /// Add metadata (builder pattern)
    pub fn with_metadata(mut self, key: String, value: String) -> Self {
        self.metadata.insert(key, value);
        self
    }

    /// Set admin flag (builder pattern)
    pub fn with_admin(mut self, admin: bool) -> Self {
        self.admin = admin;
        self
    }

    /// Check if context has admin privileges
    pub fn is_admin(&self) -> bool {
        self.admin
    }

    /// Set internal flag (builder pattern)
    pub fn with_internal(mut self, internal: bool) -> Self {
        self.internal = internal;
        self
    }

    /// Check if context is for internal operations
    pub fn is_internal(&self) -> bool {
        self.internal
    }

    /// Check if namespace filtering should be skipped for this context.
    ///
    /// ## Purpose
    /// Returns true if this is an admin context with an empty namespace.
    /// When true, repository lookup methods should skip namespace filtering to allow
    /// cross-namespace queries for administrative operations.
    ///
    /// ## Usage
    /// Used by repository implementations to determine whether to include namespace
    /// in WHERE clauses or composite keys.
    ///
    /// ## Examples
    /// ```rust
    /// # use plexspaces_common::RequestContext;
    /// let admin_ctx = RequestContext::new_without_auth("tenant1".to_string(), String::new())
    ///     .with_admin(true);
    /// assert!(admin_ctx.should_skip_namespace_filter());
    ///
    /// let normal_ctx = RequestContext::new_without_auth("tenant1".to_string(), "ns1".to_string());
    /// assert!(!normal_ctx.should_skip_namespace_filter());
    /// ```
    pub fn should_skip_namespace_filter(&self) -> bool {
        (self.admin || self.internal) && self.namespace.is_empty()
    }

    /// Get tenant_id
    pub fn tenant_id(&self) -> &str {
        &self.tenant_id
    }

    /// Get namespace
    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    /// Get user_id
    pub fn user_id(&self) -> Option<&str> {
        self.user_id.as_deref()
    }

    /// Get request_id
    pub fn request_id(&self) -> &str {
        &self.request_id
    }

    /// Get correlation_id
    pub fn correlation_id(&self) -> Option<&str> {
        self.correlation_id.as_deref()
    }

    /// Get metadata value
    pub fn get_metadata(&self, key: &str) -> Option<&String> {
        self.metadata.get(key)
    }

    /// Check if context has metadata key
    pub fn has_metadata(&self, key: &str) -> bool {
        self.metadata.contains_key(key)
    }


    /// Create RequestContext from auth config and tenant/namespace
    ///
    /// ## Purpose
    /// Creates RequestContext with validation based on auth configuration.
    /// If auth is enabled and tenant_id is missing, returns an error.
    /// If auth is disabled, uses default_tenant_id from config (required, no defaults).
    ///
    /// ## Arguments
    /// * `tenant_id` - Tenant ID (from JWT or request, required)
    /// * `namespace` - Namespace (from request, required)
    /// * `user_id` - User ID (from JWT, optional)
    /// * `admin` - Admin flag (from JWT, optional)
    /// * `auth_enabled` - Whether authentication is enabled
    /// * `default_tenant_id` - Default tenant ID when auth is disabled (required if auth disabled)
    /// * `default_namespace` - Unused; namespace must come from user request (kept for API compatibility).
    ///
    /// ## Returns
    /// RequestContext or error if validation fails
    ///
    /// ## Note
    /// Tenant may fall back to config when auth is disabled. Namespace must come from user request
    /// and is never substituted with config.
    pub fn from_auth(
        tenant_id: Option<String>,
        namespace: Option<String>,
        user_id: Option<String>,
        admin: bool,
        auth_enabled: bool,
        default_tenant_id: Option<String>,
        _default_namespace: Option<String>,
    ) -> Result<Self, RequestContextError> {
        // Validate tenant_id: if auth is enabled, tenant_id must be provided
        // If auth is disabled, use default_tenant_id (can be empty)
        let effective_tenant_id = if auth_enabled {
            tenant_id.ok_or_else(|| {
                RequestContextError::MissingTenantId
            })?
        } else {
            // If auth disabled, tenant_id can be empty (use default or empty)
            tenant_id.or(default_tenant_id).unwrap_or_default()
        };

        // Namespace must come from user request; do not substitute with config
        let effective_namespace = namespace.unwrap_or_default();

        let mut ctx = Self::new(effective_tenant_id, effective_namespace, auth_enabled)?
            .with_admin(admin);

        if let Some(uid) = user_id {
            ctx = ctx.with_user_id(uid);
        }

        Ok(ctx)
    }
}

/// Hint appended to auth errors so users know how to fix or disable auth for testing.
/// Use when returning 401/Unauthenticated so clients get actionable guidance.
pub const AUTH_REQUIRED_HINT: &str =
    " Authentication required: provide a valid JWT in Authorization header (HTTP) or use mTLS (gRPC). For local testing, set PLEXSPACES_DISABLE_AUTH=1.";

/// RequestContext errors
#[derive(Debug, thiserror::Error)]
pub enum RequestContextError {
    /// Missing required tenant_id (when auth is enabled)
    #[error("Missing required tenant_id in RequestContext.{AUTH_REQUIRED_HINT}")]
    MissingTenantId,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_request_context() {
        let ctx = RequestContext::new_without_auth("tenant-123".to_string(), "default".to_string());
        
        assert_eq!(ctx.tenant_id(), "tenant-123");
        assert_eq!(ctx.namespace(), "default");
        assert_eq!(ctx.user_id(), None);
        assert!(!ctx.request_id().is_empty());
        assert_eq!(ctx.correlation_id(), None);
    }

    #[test]
    fn test_with_namespace() {
        let ctx = RequestContext::new_without_auth("tenant-123".to_string(), "production".to_string());
        
        assert_eq!(ctx.namespace(), "production");
    }

    #[test]
    fn test_with_user_id() {
        let ctx = RequestContext::new_without_auth("tenant-123".to_string(), "".to_string())
            .with_user_id("user-456".to_string());
        
        assert_eq!(ctx.user_id(), Some("user-456"));
    }

    #[test]
    fn test_with_correlation_id() {
        let ctx = RequestContext::new_without_auth("tenant-123".to_string(), "".to_string())
            .with_correlation_id("corr-789".to_string());
        
        assert_eq!(ctx.correlation_id(), Some("corr-789"));
    }

    #[test]
    fn test_with_metadata() {
        let ctx = RequestContext::new_without_auth("tenant-123".to_string(), "".to_string())
            .with_metadata("key1".to_string(), "value1".to_string())
            .with_metadata("key2".to_string(), "value2".to_string());
        
        assert_eq!(ctx.get_metadata("key1"), Some(&"value1".to_string()));
        assert_eq!(ctx.get_metadata("key2"), Some(&"value2".to_string()));
        assert!(ctx.has_metadata("key1"));
        assert!(!ctx.has_metadata("key3"));
    }

    #[test]
    fn test_builder_chain() {
        let ctx = RequestContext::new_without_auth("tenant-123".to_string(), "production".to_string())
            .with_user_id("user-456".to_string())
            .with_correlation_id("corr-789".to_string())
            .with_metadata("source".to_string(), "api".to_string());
        
        assert_eq!(ctx.tenant_id(), "tenant-123");
        assert_eq!(ctx.namespace(), "production");
        assert_eq!(ctx.user_id(), Some("user-456"));
        assert_eq!(ctx.correlation_id(), Some("corr-789"));
        assert_eq!(ctx.get_metadata("source"), Some(&"api".to_string()));
    }

    #[test]
    fn test_from_proto_success() {
        let proto = plexspaces_proto::v1::common::RequestContext {
            tenant_id: "tenant-123".to_string(),
            namespace: "production".to_string(),
            user_id: "user-456".to_string(),
            request_id: "req-123".to_string(),
            correlation_id: "corr-789".to_string(),
            auth_enabled: false,
            timestamp: Some(Timestamp {
                seconds: 1234567890,
                nanos: 0,
            }),
            metadata: {
                let mut map = HashMap::new();
                map.insert("key1".to_string(), "value1".to_string());
                map
            },
            admin: false,
            internal: false,
        };

        let ctx = RequestContext::from_proto(&proto, false).unwrap();
        
        assert_eq!(ctx.tenant_id(), "tenant-123");
        assert_eq!(ctx.namespace(), "production");
        assert_eq!(ctx.user_id(), Some("user-456"));
        assert_eq!(ctx.request_id(), "req-123");
        assert_eq!(ctx.correlation_id(), Some("corr-789"));
        assert_eq!(ctx.get_metadata("key1"), Some(&"value1".to_string()));
    }

    #[test]
    fn test_from_proto_missing_tenant_id() {
        let proto = plexspaces_proto::v1::common::RequestContext {
            tenant_id: "".to_string(),
            namespace: "production".to_string(),
            user_id: "".to_string(),
            request_id: "".to_string(),
            correlation_id: "".to_string(),
            auth_enabled: false,
            timestamp: None,
            metadata: HashMap::new(),
            admin: false,
            internal: false,
        };

        let result = RequestContext::from_proto(&proto, true);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, RequestContextError::MissingTenantId));
        assert!(err.to_string().contains("PLEXSPACES_DISABLE_AUTH"), "Auth error must include hint: {}", err);
    }

    #[test]
    fn test_new_auth_enabled_missing_tenant_id_includes_hint() {
        let result = RequestContext::new(String::new(), "ns".to_string(), true);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("PLEXSPACES_DISABLE_AUTH"), "MissingTenantId must include auth hint: {}", err);
    }

    #[test]
    fn test_from_proto_defaults() {
        let proto = plexspaces_proto::v1::common::RequestContext {
            tenant_id: "tenant-123".to_string(),
            namespace: "".to_string(), // Empty namespace remains empty
            user_id: "".to_string(),   // Empty should be None
            request_id: "".to_string(), // Empty should generate new ULID
            correlation_id: "".to_string(), // Empty should be None
            timestamp: None, // None should use current time
            metadata: HashMap::new(),
            admin: false,
            internal: false,
            auth_enabled: false,
        };

        let ctx = RequestContext::from_proto(&proto, false).unwrap();
        
        assert_eq!(ctx.tenant_id(), "tenant-123");
        assert_eq!(ctx.namespace(), "");
        assert_eq!(ctx.user_id(), None);
        assert!(!ctx.request_id().is_empty()); // Should generate new ULID
        assert_eq!(ctx.correlation_id(), None);
    }

    #[test]
    fn test_to_proto() {
        let ctx = RequestContext::new_without_auth("tenant-123".to_string(), "production".to_string())
            .with_user_id("user-456".to_string())
            .with_correlation_id("corr-789".to_string())
            .with_metadata("key1".to_string(), "value1".to_string());

        let proto = ctx.to_proto();
        
        assert_eq!(proto.tenant_id, "tenant-123");
        assert_eq!(proto.namespace, "production");
        assert_eq!(proto.user_id, "user-456");
        assert_eq!(proto.request_id, ctx.request_id());
        assert_eq!(proto.correlation_id, "corr-789");
        assert_eq!(proto.metadata.get("key1"), Some(&"value1".to_string()));
    }

    #[test]
    fn test_should_skip_namespace_filter() {
        // Admin context with empty namespace should skip filter
        let admin_ctx = RequestContext::new_without_auth("tenant1".to_string(), String::new())
            .with_admin(true);
        assert!(admin_ctx.should_skip_namespace_filter(), "Admin with empty namespace should skip filter");

        // Internal context with empty namespace should skip filter
        let internal_ctx = RequestContext::new_without_auth("tenant1".to_string(), String::new())
            .with_internal(true);
        assert!(internal_ctx.should_skip_namespace_filter(), "Internal with empty namespace should skip filter");

        // Admin context with non-empty namespace should NOT skip filter
        let admin_with_ns = RequestContext::new_without_auth("tenant1".to_string(), "ns1".to_string())
            .with_admin(true);
        assert!(!admin_with_ns.should_skip_namespace_filter(), "Admin with namespace should NOT skip filter");

        // Internal context with non-empty namespace should NOT skip filter
        let internal_with_ns = RequestContext::new_without_auth("tenant1".to_string(), "ns1".to_string())
            .with_internal(true);
        assert!(!internal_with_ns.should_skip_namespace_filter(), "Internal with namespace should NOT skip filter");

        // Normal context should NOT skip filter
        let normal_ctx = RequestContext::new_without_auth("tenant1".to_string(), "ns1".to_string());
        assert!(!normal_ctx.should_skip_namespace_filter(), "Normal context should NOT skip filter");

        // Normal context with empty namespace should NOT skip filter (not admin/internal)
        let normal_empty_ns = RequestContext::new_without_auth("tenant1".to_string(), String::new());
        assert!(!normal_empty_ns.should_skip_namespace_filter(), "Normal context with empty namespace should NOT skip filter");
    }

    #[test]
    fn test_to_proto_roundtrip() {
        let original = RequestContext::new_without_auth("tenant-123".to_string(), "production".to_string())
            .with_user_id("user-456".to_string())
            .with_correlation_id("corr-789".to_string())
            .with_metadata("key1".to_string(), "value1".to_string());

        let proto = original.to_proto();
        let restored = RequestContext::from_proto(&proto, false).unwrap();
        
        assert_eq!(original.tenant_id(), restored.tenant_id());
        assert_eq!(original.namespace(), restored.namespace());
        assert_eq!(original.user_id(), restored.user_id());
        assert_eq!(original.correlation_id(), restored.correlation_id());
        assert_eq!(original.get_metadata("key1"), restored.get_metadata("key1"));
    }

    #[test]
    fn test_clone() {
        let ctx1 = RequestContext::new_without_auth("tenant-123".to_string(), "production".to_string())
            .with_user_id("user-456".to_string());

        let ctx2 = ctx1.clone();
        
        assert_eq!(ctx1.tenant_id(), ctx2.tenant_id());
        assert_eq!(ctx1.namespace(), ctx2.namespace());
        assert_eq!(ctx1.user_id(), ctx2.user_id());
    }

    // ========== from_auth propagation (multi-tenancy) ==========

    #[test]
    fn test_from_auth_auth_disabled_empty_tenant_allowed() {
        let ctx = RequestContext::from_auth(
            None,
            None,
            None,
            false,
            false,
            None,
            None,
        )
        .unwrap();
        assert_eq!(ctx.tenant_id(), "");
        assert_eq!(ctx.namespace(), "");
    }

    #[test]
    fn test_from_auth_auth_disabled_tenant_from_request() {
        let ctx = RequestContext::from_auth(
            Some("tenant-from-jwt".to_string()),
            Some("ns-from-request".to_string()),
            None,
            false,
            false,
            None,
            None,
        )
        .unwrap();
        assert_eq!(ctx.tenant_id(), "tenant-from-jwt");
        assert_eq!(ctx.namespace(), "ns-from-request");
    }

    #[test]
    fn test_from_auth_auth_enabled_missing_tenant_fails() {
        let result = RequestContext::from_auth(
            None,
            Some("ns".to_string()),
            None,
            false,
            true,
            None,
            None,
        );
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RequestContextError::MissingTenantId));
    }

    #[test]
    fn test_from_auth_auth_enabled_tenant_required() {
        let ctx = RequestContext::from_auth(
            Some("tenant-required".to_string()),
            Some("ns".to_string()),
            None,
            false,
            true,
            None,
            None,
        )
        .unwrap();
        assert_eq!(ctx.tenant_id(), "tenant-required");
        assert_eq!(ctx.namespace(), "ns");
    }

    #[test]
    fn test_from_auth_namespace_from_request_only() {
        let ctx = RequestContext::from_auth(
            Some("t1".to_string()),
            Some("app-namespace".to_string()),
            None,
            false,
            false,
            Some("default-tenant".to_string()),
            Some("default-ns".to_string()),
        )
        .unwrap();
        assert_eq!(ctx.namespace(), "app-namespace");
    }

    #[test]
    fn test_from_auth_namespace_empty_defaults_to_empty() {
        let ctx = RequestContext::from_auth(
            Some("t1".to_string()),
            None,
            None,
            false,
            false,
            None,
            None,
        )
        .unwrap();
        assert_eq!(ctx.namespace(), "");
    }
}

