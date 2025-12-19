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
//! - **Tenant Isolation**: tenant_id is REQUIRED for all operations
//! - **Tracing**: request_id and correlation_id for distributed tracing
//! - **Extensible**: metadata map for additional context
//! - **Immutable**: Context should be passed by reference, not mutated

use std::collections::HashMap;
use std::sync::Arc;
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
    /// let ctx = RequestContext::new("tenant-123".to_string(), "production".to_string())
    ///     .with_user_id("user-456".to_string());
    ///
    /// // Pass to repository/service
    /// let result = repository.get(&ctx, "resource-id").await?;
    /// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestContext {
    /// Tenant ID (REQUIRED for all operations)
    pub tenant_id: String,
    
    /// Namespace within tenant (REQUIRED, no defaults)
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
    /// Admin users can bypass tenant filtering for administrative operations.
    pub admin: bool,
    
    /// Internal flag (for system operations)
    ///
    /// When true, indicates this is an internal system operation.
    /// Internal operations bypass authn/authz and tenant filtering.
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
    ///
    /// ## Example
    /// ```rust
    /// use plexspaces_core::RequestContext;
    /// let ctx = RequestContext::new("tenant-123".to_string(), "production".to_string(), false)?;
    /// assert_eq!(ctx.tenant_id(), "tenant-123");
    /// assert_eq!(ctx.namespace(), "production");
    /// ```
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

    /// Create RequestContext for internal/system operations
    ///
    /// ## Purpose
    /// Creates a RequestContext for internal operations that bypass authn/authz and tenant filtering.
    /// Uses "internal" as tenant_id and "system" as namespace.
    ///
    /// ## Usage
    /// Use this for system-level operations like heartbeats, node registration, etc.
    pub fn internal() -> Self {
        Self::new("internal".to_string(), "system".to_string(), false)
            .unwrap()
            .with_admin(true)
            .with_internal(true)
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
    /// * `default_namespace` - Default namespace (required if not provided)
    ///
    /// ## Returns
    /// RequestContext or error if validation fails
    ///
    /// ## Note
    /// tenant_id and namespace are REQUIRED - no defaults. If auth is disabled,
    /// default_tenant_id and default_namespace must be provided in config.
    pub fn from_auth(
        tenant_id: Option<String>,
        namespace: Option<String>,
        user_id: Option<String>,
        admin: bool,
        auth_enabled: bool,
        default_tenant_id: Option<String>,
        default_namespace: Option<String>,
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

        // Namespace can be empty - use provided, default, or empty string
        let effective_namespace = namespace
            .or(default_namespace)
            .unwrap_or_default();

        let mut ctx = Self::new(effective_tenant_id, effective_namespace, auth_enabled)?
            .with_admin(admin);

        if let Some(uid) = user_id {
            ctx = ctx.with_user_id(uid);
        }

        Ok(ctx)
    }
    
    /// Create RequestContext from gRPC request metadata
    ///
    /// ## Purpose
    /// Helper method that extracts tenant_id, namespace, user_id, and admin flag from gRPC request metadata
    /// and creates a RequestContext using shared validation from RequestContext::from_auth.
    ///
    /// ## Sources (in order of precedence):
    /// 1. `x-tenant-id` header (from JWT middleware)
    /// 2. `x-namespace` header (from request, can be empty)
    /// 3. `x-user-id` header (from JWT middleware, optional)
    /// 4. `x-admin` header (from JWT middleware, optional, indicates admin privileges)
    /// 5. `tenant_id` in request labels (fallback, only if auth disabled)
    /// 6. Default values from NodeConfig in ServiceLocator (if auth disabled)
    ///
    /// ## Arguments
    /// * `metadata` - gRPC request metadata
    /// * `labels` - Request labels (for fallback)
    /// * `service_locator` - ServiceLocator to get NodeConfig
    ///
    /// ## Returns
    /// RequestContext or error if validation fails (validation happens in RequestContext::from_auth)
    /// Create RequestContext from gRPC request metadata
    ///
    /// ## Note
    /// This method requires tonic::metadata::MetadataMap support.
    /// For non-gRPC use cases, use `RequestContext::from_auth` directly.
    pub async fn from_grpc_request(
        metadata: &tonic::metadata::MetadataMap,
        labels: &std::collections::HashMap<String, String>,
        service_locator: &Arc<crate::ServiceLocator>,
    ) -> Result<Self, RequestContextError> {
        // Get NodeConfig from ServiceLocator
        let node_config = service_locator.get_node_config().await;
        
        // Get auth_enabled from SecurityConfig (check runtime config)
        // For now, infer from x-tenant-id header presence, but should come from SecurityConfig.disable_auth
        // TODO: Get from RuntimeConfig.security.disable_auth
        let auth_enabled = metadata.get("x-tenant-id")
            .and_then(|v| v.to_str().ok())
            .filter(|s| !s.is_empty())
            .is_some();
        
        // Get defaults from NodeConfig
        let default_tenant_id = node_config.as_ref()
            .map(|c| c.default_tenant_id.clone());
        let default_namespace = node_config.as_ref()
            .map(|c| c.default_namespace.clone());
        
        // Extract tenant_id - RequestContext::from_auth will validate based on auth_enabled
        let tenant_id_from_header = metadata.get("x-tenant-id")
            .and_then(|v| v.to_str().ok())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());
        let tenant_id_from_labels = labels.get("tenant_id")
            .filter(|s| !s.is_empty())
            .map(|s| s.clone());
        
        // Extract namespace - can be empty, RequestContext::from_auth handles defaults
        let namespace_from_header = metadata.get("x-namespace")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        let namespace_from_labels = labels.get("namespace")
            .map(|s| s.clone());
        
        // Extract user_id and admin from metadata
        let user_id = metadata.get("x-user-id")
            .and_then(|v| v.to_str().ok())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());
        let admin = metadata.get("x-admin")
            .and_then(|v| v.to_str().ok())
            .map(|s| s == "true" || s == "1")
            .unwrap_or(false);
        
        // Use shared validation from RequestContext::from_auth
        // This validates tenant_id if auth_enabled, otherwise allows empty tenant_id
        Self::from_auth(
            tenant_id_from_header.or(tenant_id_from_labels),
            namespace_from_header.or(namespace_from_labels),
            user_id,
            admin,
            auth_enabled,
            default_tenant_id,
            default_namespace,
        )
    }
}

/// Trait for accessing gRPC metadata (to avoid direct dependency on tonic)
trait GrpcMetadata {
    fn get_str(&self, key: &str) -> Option<&str>;
}

impl GrpcMetadata for tonic::metadata::MetadataMap {
    fn get_str(&self, key: &str) -> Option<&str> {
        self.get(key)
            .and_then(|v| v.to_str().ok())
    }
}

/// RequestContext errors
#[derive(Debug, thiserror::Error)]
pub enum RequestContextError {
    /// Missing required tenant_id
    #[error("Missing required tenant_id in RequestContext")]
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
        assert!(matches!(result.unwrap_err(), RequestContextError::MissingTenantId));
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
}

