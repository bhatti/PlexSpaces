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
/// // Create context from request
/// let ctx = RequestContext::new("tenant-123".to_string())
///     .with_namespace("production".to_string())
///     .with_user_id("user-456".to_string());
///
/// // Pass to repository/service
/// let result = repository.get(&ctx, "resource-id").await?;
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestContext {
    /// Tenant ID (REQUIRED for all operations)
    pub tenant_id: String,
    
    /// Namespace within tenant (defaults to "default")
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
}

impl RequestContext {
    /// Create a new RequestContext with required tenant_id
    ///
    /// ## Arguments
    /// * `tenant_id` - Tenant identifier (required)
    ///
    /// ## Returns
    /// New RequestContext with defaults:
    /// - namespace: "default"
    /// - request_id: Generated ULID
    /// - timestamp: Current time
    ///
    /// ## Example
    /// ```rust
    /// use plexspaces_core::RequestContext;
    /// let ctx = RequestContext::new("tenant-123".to_string());
    /// assert_eq!(ctx.tenant_id(), "tenant-123");
    /// assert_eq!(ctx.namespace(), "default");
    /// ```
    pub fn new(tenant_id: String) -> Self {
        let now = Utc::now();
        Self {
            tenant_id,
            namespace: "default".to_string(),
            user_id: None,
            request_id: Ulid::new().to_string(),
            correlation_id: None,
            timestamp: Timestamp {
                seconds: now.timestamp(),
                nanos: now.timestamp_subsec_nanos() as i32,
            },
            metadata: HashMap::new(),
        }
    }

    /// Create RequestContext from proto message
    ///
    /// ## Arguments
    /// * `proto` - RequestContext proto message
    ///
    /// ## Returns
    /// RequestContext or error if tenant_id is missing
    pub fn from_proto(proto: &plexspaces_proto::v1::common::RequestContext) -> Result<Self, RequestContextError> {
        if proto.tenant_id.is_empty() {
            return Err(RequestContextError::MissingTenantId);
        }

        Ok(Self {
            tenant_id: proto.tenant_id.clone(),
            namespace: if proto.namespace.is_empty() {
                "default".to_string()
            } else {
                proto.namespace.clone()
            },
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
                let now = Utc::now();
                Timestamp {
                    seconds: now.timestamp(),
                    nanos: now.timestamp_subsec_nanos() as i32,
                }
            }),
            metadata: proto.metadata.clone(),
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
        let ctx = RequestContext::new("tenant-123".to_string());
        
        assert_eq!(ctx.tenant_id(), "tenant-123");
        assert_eq!(ctx.namespace(), "default");
        assert_eq!(ctx.user_id(), None);
        assert!(!ctx.request_id().is_empty());
        assert_eq!(ctx.correlation_id(), None);
    }

    #[test]
    fn test_with_namespace() {
        let ctx = RequestContext::new("tenant-123".to_string())
            .with_namespace("production".to_string());
        
        assert_eq!(ctx.namespace(), "production");
    }

    #[test]
    fn test_with_user_id() {
        let ctx = RequestContext::new("tenant-123".to_string())
            .with_user_id("user-456".to_string());
        
        assert_eq!(ctx.user_id(), Some("user-456"));
    }

    #[test]
    fn test_with_correlation_id() {
        let ctx = RequestContext::new("tenant-123".to_string())
            .with_correlation_id("corr-789".to_string());
        
        assert_eq!(ctx.correlation_id(), Some("corr-789"));
    }

    #[test]
    fn test_with_metadata() {
        let ctx = RequestContext::new("tenant-123".to_string())
            .with_metadata("key1".to_string(), "value1".to_string())
            .with_metadata("key2".to_string(), "value2".to_string());
        
        assert_eq!(ctx.get_metadata("key1"), Some(&"value1".to_string()));
        assert_eq!(ctx.get_metadata("key2"), Some(&"value2".to_string()));
        assert!(ctx.has_metadata("key1"));
        assert!(!ctx.has_metadata("key3"));
    }

    #[test]
    fn test_builder_chain() {
        let ctx = RequestContext::new("tenant-123".to_string())
            .with_namespace("production".to_string())
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
            timestamp: Some(Timestamp {
                seconds: 1234567890,
                nanos: 0,
            }),
            metadata: {
                let mut map = HashMap::new();
                map.insert("key1".to_string(), "value1".to_string());
                map
            },
        };

        let ctx = RequestContext::from_proto(&proto).unwrap();
        
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
            timestamp: None,
            metadata: HashMap::new(),
        };

        let result = RequestContext::from_proto(&proto);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RequestContextError::MissingTenantId));
    }

    #[test]
    fn test_from_proto_defaults() {
        let proto = plexspaces_proto::v1::common::RequestContext {
            tenant_id: "tenant-123".to_string(),
            namespace: "".to_string(), // Empty should default to "default"
            user_id: "".to_string(),   // Empty should be None
            request_id: "".to_string(), // Empty should generate new ULID
            correlation_id: "".to_string(), // Empty should be None
            timestamp: None, // None should use current time
            metadata: HashMap::new(),
        };

        let ctx = RequestContext::from_proto(&proto).unwrap();
        
        assert_eq!(ctx.tenant_id(), "tenant-123");
        assert_eq!(ctx.namespace(), "default");
        assert_eq!(ctx.user_id(), None);
        assert!(!ctx.request_id().is_empty()); // Should generate new ULID
        assert_eq!(ctx.correlation_id(), None);
    }

    #[test]
    fn test_to_proto() {
        let ctx = RequestContext::new("tenant-123".to_string())
            .with_namespace("production".to_string())
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
        let original = RequestContext::new("tenant-123".to_string())
            .with_namespace("production".to_string())
            .with_user_id("user-456".to_string())
            .with_correlation_id("corr-789".to_string())
            .with_metadata("key1".to_string(), "value1".to_string());

        let proto = original.to_proto();
        let restored = RequestContext::from_proto(&proto).unwrap();
        
        assert_eq!(original.tenant_id(), restored.tenant_id());
        assert_eq!(original.namespace(), restored.namespace());
        assert_eq!(original.user_id(), restored.user_id());
        assert_eq!(original.correlation_id(), restored.correlation_id());
        assert_eq!(original.get_metadata("key1"), restored.get_metadata("key1"));
    }

    #[test]
    fn test_clone() {
        let ctx1 = RequestContext::new("tenant-123".to_string())
            .with_namespace("production".to_string())
            .with_user_id("user-456".to_string());

        let ctx2 = ctx1.clone();
        
        assert_eq!(ctx1.tenant_id(), ctx2.tenant_id());
        assert_eq!(ctx1.namespace(), ctx2.namespace());
        assert_eq!(ctx1.user_id(), ctx2.user_id());
    }
}

