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

//! Extension trait for `RequestContext` (the prost-generated proto type).
//!
//! ## Purpose
//! Provides ergonomic method-syntax access and smart constructors for
//! `plexspaces_proto::common::v1::RequestContext`, which is now the canonical
//! `RequestContext` type everywhere in PlexSpaces.
//!
//! ## Design
//! - The proto type IS `RequestContext`; this trait adds methods via extension.
//! - `user_id` and `correlation_id` use empty-string semantics (proto style).
//! - All builder methods take `self` and return `RequestContext`.
//! - Static constructors (`new`, `new_without_auth`, `from_auth`, etc.) are
//!   provided as trait methods for callers that need the proto type directly.

use std::collections::HashMap;

use plexspaces_proto::common::v1::RequestContext;

use crate::request_context::RequestContextError;

/// Extension trait for `RequestContext` providing ergonomic accessors and builders.
///
/// Import this trait wherever you call methods on `RequestContext`:
/// ```rust
/// /// ```
pub trait RequestContextExt: Sized {
    // ========== Smart constructors ==========

    /// Create a new `RequestContext` with the given tenant_id, namespace, and auth setting.
    ///
    /// Returns an error if `auth_enabled` is true and `tenant_id` is empty.
    fn new(
        tenant_id: String,
        namespace: String,
        auth_enabled: bool,
    ) -> Result<RequestContext, RequestContextError>;

    /// Create a `RequestContext` with `auth_enabled = false`.
    ///
    /// Use this in tests and non-auth scenarios. For production with auth enabled,
    /// use [`RequestContextExt::new`] to get validation.
    fn new_without_auth(tenant_id: String, namespace: String) -> RequestContext;

    /// Create a `RequestContext` from auth parameters (multi-tenancy entry point).
    ///
    /// If `auth_enabled` is true, `tenant_id` must be `Some`. Otherwise the
    /// `default_tenant_id` fallback is used (or empty string).
    fn from_auth(
        tenant_id: Option<String>,
        namespace: Option<String>,
        user_id: Option<String>,
        admin: bool,
        auth_enabled: bool,
        default_tenant_id: Option<String>,
        default_namespace: Option<String>,
    ) -> Result<RequestContext, RequestContextError>;

    /// Same as [`from_auth`](RequestContextExt::from_auth) but also attaches propagation headers.
    fn from_auth_with_headers(
        tenant_id: Option<String>,
        namespace: Option<String>,
        user_id: Option<String>,
        admin: bool,
        auth_enabled: bool,
        default_tenant_id: Option<String>,
        default_namespace: Option<String>,
        headers: HashMap<String, String>,
    ) -> Result<RequestContext, RequestContextError>;

    // ========== Accessors ==========

    /// Returns `tenant_id` as `&str`.
    fn tenant_id(&self) -> &str;

    /// Returns `namespace` as `&str`.
    fn namespace(&self) -> &str;

    /// Returns `user_id` as `Option<&str>` (`None` when the field is empty).
    fn user_id(&self) -> Option<&str>;

    /// Returns `request_id` as `&str`.
    fn request_id(&self) -> &str;

    /// Returns `correlation_id` as `Option<&str>` (`None` when the field is empty).
    fn correlation_id(&self) -> Option<&str>;

    /// Returns `true` when the admin flag is set.
    fn is_admin(&self) -> bool;

    /// Returns `true` when the internal flag is set.
    fn is_internal(&self) -> bool;

    /// Returns `true` when this context should skip namespace filtering.
    ///
    /// This is true when the context is admin or internal AND the namespace is empty,
    /// which allows cross-namespace administrative/system queries.
    fn should_skip_namespace_filter(&self) -> bool;

    // ========== Header / metadata accessors ==========

    /// Get a propagation header value by name (case-insensitive).
    fn get_header(&self, name: &str) -> Option<&str>;

    /// Returns `true` if the named propagation header exists.
    fn has_header(&self, name: &str) -> bool;

    /// Returns the Bearer token from the `authorization` header (without the `Bearer ` prefix).
    fn bearer_token(&self) -> Option<&str>;

    /// Returns an API key stored as a query parameter (`apikey-query:<name>`).
    fn api_key_query(&self, param_name: &str) -> Option<&str>;

    /// Returns all actual HTTP headers (excludes internal `apikey-query:*` entries).
    fn http_headers(&self) -> HashMap<&str, &str>;

    /// Returns all API key query parameters as `(param_name, key_value)` pairs.
    fn api_key_query_params(&self) -> HashMap<&str, &str>;

    /// Get a metadata value.
    fn get_metadata(&self, key: &str) -> Option<&String>;

    /// Returns `true` if the metadata map contains the given key.
    fn has_metadata(&self, key: &str) -> bool;

    // ========== Builders ==========

    /// Set namespace (builder pattern).
    fn with_namespace(self, namespace: String) -> RequestContext;

    /// Set user_id (builder pattern).
    fn with_user_id(self, user_id: String) -> RequestContext;

    /// Set correlation_id (builder pattern).
    fn with_correlation_id(self, correlation_id: String) -> RequestContext;

    /// Add a metadata entry (builder pattern).
    fn with_metadata(self, key: String, value: String) -> RequestContext;

    /// Set a propagation header; name is lowercased per HTTP/2 convention (builder pattern).
    fn with_header(self, name: String, value: String) -> RequestContext;

    /// Attach a Bearer token (`authorization: Bearer <token>`) (builder pattern).
    fn with_bearer_token(self, token: String) -> RequestContext;

    /// Attach an API key via header (builder pattern).
    fn with_api_key_header(self, header_name: String, key: String) -> RequestContext;

    /// Attach an API key via query parameter (stored as `apikey-query:<name>`) (builder pattern).
    fn with_api_key_query(self, param_name: String, key: String) -> RequestContext;

    /// Set multiple headers at once (builder pattern).
    fn with_headers(self, headers: HashMap<String, String>) -> RequestContext;

    /// Set the admin flag (builder pattern).
    fn with_admin(self, admin: bool) -> RequestContext;

    /// Set the internal flag (builder pattern).
    fn with_internal(self, internal: bool) -> RequestContext;

    /// Returns a clone of this context (identity method for sites that used the old `to_proto()`).
    ///
    /// Since `RequestContext` IS the proto type, this is just a clone.
    fn to_proto(&self) -> RequestContext;
}

impl RequestContextExt for RequestContext {
    fn new(
        tenant_id: String,
        namespace: String,
        auth_enabled: bool,
    ) -> Result<RequestContext, RequestContextError> {
        if auth_enabled && tenant_id.is_empty() {
            return Err(RequestContextError::MissingTenantId);
        }
        Ok(make_context(tenant_id, namespace, auth_enabled))
    }

    fn new_without_auth(tenant_id: String, namespace: String) -> RequestContext {
        make_context(tenant_id, namespace, false)
    }

    fn from_auth(
        tenant_id: Option<String>,
        namespace: Option<String>,
        user_id: Option<String>,
        admin: bool,
        auth_enabled: bool,
        default_tenant_id: Option<String>,
        _default_namespace: Option<String>,
    ) -> Result<RequestContext, RequestContextError> {
        let effective_tenant_id = if auth_enabled {
            tenant_id.ok_or(RequestContextError::MissingTenantId)?
        } else {
            tenant_id.or(default_tenant_id).unwrap_or_default()
        };
        let effective_namespace = namespace.unwrap_or_default();

        let ctx = <RequestContext as RequestContextExt>::new(
            effective_tenant_id,
            effective_namespace,
            auth_enabled,
        )?
        .with_admin(admin);
        let ctx = if let Some(uid) = user_id {
            ctx.with_user_id(uid)
        } else {
            ctx
        };
        Ok(ctx)
    }

    fn from_auth_with_headers(
        tenant_id: Option<String>,
        namespace: Option<String>,
        user_id: Option<String>,
        admin: bool,
        auth_enabled: bool,
        default_tenant_id: Option<String>,
        default_namespace: Option<String>,
        headers: HashMap<String, String>,
    ) -> Result<RequestContext, RequestContextError> {
        let ctx = <RequestContext as RequestContextExt>::from_auth(
            tenant_id,
            namespace,
            user_id,
            admin,
            auth_enabled,
            default_tenant_id,
            default_namespace,
        )?;
        Ok(ctx.with_headers(headers))
    }

    // ========== Accessors ==========

    fn tenant_id(&self) -> &str {
        &self.tenant_id
    }

    fn namespace(&self) -> &str {
        &self.namespace
    }

    fn user_id(&self) -> Option<&str> {
        if self.user_id.is_empty() {
            None
        } else {
            Some(self.user_id.as_str())
        }
    }

    fn request_id(&self) -> &str {
        &self.request_id
    }

    fn correlation_id(&self) -> Option<&str> {
        if self.correlation_id.is_empty() {
            None
        } else {
            Some(self.correlation_id.as_str())
        }
    }

    fn is_admin(&self) -> bool {
        self.admin
    }

    fn is_internal(&self) -> bool {
        self.internal
    }

    fn should_skip_namespace_filter(&self) -> bool {
        (self.admin || self.internal) && self.namespace.is_empty()
    }

    // ========== Header / metadata accessors ==========

    fn get_header(&self, name: &str) -> Option<&str> {
        self.headers.get(&name.to_lowercase()).map(|s| s.as_str())
    }

    fn has_header(&self, name: &str) -> bool {
        self.headers.contains_key(&name.to_lowercase())
    }

    fn bearer_token(&self) -> Option<&str> {
        self.headers
            .get("authorization")
            .and_then(|v| v.strip_prefix("Bearer "))
    }

    fn api_key_query(&self, param_name: &str) -> Option<&str> {
        self.headers
            .get(&format!("apikey-query:{}", param_name))
            .map(|s| s.as_str())
    }

    fn http_headers(&self) -> HashMap<&str, &str> {
        self.headers
            .iter()
            .filter(|(k, _)| !k.starts_with("apikey-query:"))
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect()
    }

    fn api_key_query_params(&self) -> HashMap<&str, &str> {
        self.headers
            .iter()
            .filter_map(|(k, v)| {
                k.strip_prefix("apikey-query:")
                    .map(|name| (name, v.as_str()))
            })
            .collect()
    }

    fn get_metadata(&self, key: &str) -> Option<&String> {
        self.metadata.get(key)
    }

    fn has_metadata(&self, key: &str) -> bool {
        self.metadata.contains_key(key)
    }

    // ========== Builders ==========

    fn with_namespace(mut self, namespace: String) -> RequestContext {
        self.namespace = namespace;
        self
    }

    fn with_user_id(mut self, user_id: String) -> RequestContext {
        self.user_id = user_id;
        self
    }

    fn with_correlation_id(mut self, correlation_id: String) -> RequestContext {
        self.correlation_id = correlation_id;
        self
    }

    fn with_metadata(mut self, key: String, value: String) -> RequestContext {
        self.metadata.insert(key, value);
        self
    }

    fn with_header(mut self, name: String, value: String) -> RequestContext {
        self.headers.insert(name.to_lowercase(), value);
        self
    }

    fn with_bearer_token(self, token: String) -> RequestContext {
        self.with_header("authorization".to_string(), format!("Bearer {}", token))
    }

    fn with_api_key_header(self, header_name: String, key: String) -> RequestContext {
        self.with_header(header_name, key)
    }

    fn with_api_key_query(self, param_name: String, key: String) -> RequestContext {
        self.with_header(format!("apikey-query:{}", param_name), key)
    }

    fn with_headers(mut self, headers: HashMap<String, String>) -> RequestContext {
        for (k, v) in headers {
            self.headers.insert(k.to_lowercase(), v);
        }
        self
    }

    fn with_admin(mut self, admin: bool) -> RequestContext {
        self.admin = admin;
        self
    }

    fn with_internal(mut self, internal: bool) -> RequestContext {
        self.internal = internal;
        self
    }

    fn to_proto(&self) -> RequestContext {
        self.clone()
    }
}

/// Internal helper: construct a `RequestContext` with a fresh request_id and timestamp.
fn make_context(tenant_id: String, namespace: String, auth_enabled: bool) -> RequestContext {
    use chrono::Utc;
    use ulid::Ulid;

    let now = Utc::now();
    RequestContext {
        tenant_id,
        namespace,
        user_id: String::new(),
        request_id: Ulid::new().to_string(),
        correlation_id: String::new(),
        timestamp: Some(prost_types::Timestamp {
            seconds: now.timestamp(),
            nanos: now.timestamp_subsec_nanos() as i32,
        }),
        metadata: HashMap::new(),
        headers: HashMap::new(),
        admin: false,
        internal: false,
        auth_enabled,
    }
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
        let ctx =
            RequestContext::new_without_auth("tenant-123".to_string(), "production".to_string());
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
        let ctx =
            RequestContext::new_without_auth("tenant-123".to_string(), "production".to_string())
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
    fn test_new_auth_enabled_missing_tenant_id_includes_hint() {
        let result = RequestContext::new(String::new(), "ns".to_string(), true);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("PLEXSPACES_DISABLE_AUTH"),
            "MissingTenantId must include auth hint: {}",
            err
        );
    }

    #[test]
    fn test_from_proto_defaults() {
        // With the proto type as the canonical type, "from_proto" is just constructing the struct.
        // Validate that empty user_id/correlation_id => None via accessors.
        let ctx = RequestContext {
            tenant_id: "tenant-123".to_string(),
            namespace: "".to_string(),
            user_id: "".to_string(),
            request_id: "req-1".to_string(),
            correlation_id: "".to_string(),
            timestamp: None,
            metadata: HashMap::new(),
            headers: HashMap::new(),
            admin: false,
            internal: false,
            auth_enabled: false,
        };

        assert_eq!(ctx.tenant_id(), "tenant-123");
        assert_eq!(ctx.namespace(), "");
        assert_eq!(ctx.user_id(), None);
        assert_eq!(ctx.request_id(), "req-1");
        assert_eq!(ctx.correlation_id(), None);
    }

    #[test]
    fn test_to_proto() {
        let ctx =
            RequestContext::new_without_auth("tenant-123".to_string(), "production".to_string())
                .with_user_id("user-456".to_string())
                .with_correlation_id("corr-789".to_string())
                .with_metadata("key1".to_string(), "value1".to_string());

        let proto = ctx.to_proto();

        assert_eq!(proto.tenant_id, "tenant-123");
        assert_eq!(proto.namespace, "production");
        assert_eq!(proto.user_id, "user-456");
        assert_eq!(proto.correlation_id, "corr-789");
        assert_eq!(proto.metadata.get("key1"), Some(&"value1".to_string()));
    }

    #[test]
    fn test_should_skip_namespace_filter() {
        let admin_ctx =
            RequestContext::new_without_auth("tenant1".to_string(), String::new()).with_admin(true);
        assert!(admin_ctx.should_skip_namespace_filter());

        let internal_ctx = RequestContext::new_without_auth("tenant1".to_string(), String::new())
            .with_internal(true);
        assert!(internal_ctx.should_skip_namespace_filter());

        let admin_with_ns =
            RequestContext::new_without_auth("tenant1".to_string(), "ns1".to_string())
                .with_admin(true);
        assert!(!admin_with_ns.should_skip_namespace_filter());

        let internal_with_ns =
            RequestContext::new_without_auth("tenant1".to_string(), "ns1".to_string())
                .with_internal(true);
        assert!(!internal_with_ns.should_skip_namespace_filter());

        let normal_ctx = RequestContext::new_without_auth("tenant1".to_string(), "ns1".to_string());
        assert!(!normal_ctx.should_skip_namespace_filter());

        let normal_empty_ns =
            RequestContext::new_without_auth("tenant1".to_string(), String::new());
        assert!(!normal_empty_ns.should_skip_namespace_filter());
    }

    #[test]
    fn test_clone() {
        let ctx1 =
            RequestContext::new_without_auth("tenant-123".to_string(), "production".to_string())
                .with_user_id("user-456".to_string());
        let ctx2 = ctx1.clone();
        assert_eq!(ctx1.tenant_id(), ctx2.tenant_id());
        assert_eq!(ctx1.namespace(), ctx2.namespace());
        assert_eq!(ctx1.user_id(), ctx2.user_id());
    }

    #[test]
    fn test_with_bearer_token() {
        let ctx = RequestContext::new_without_auth("t1".to_string(), "ns".to_string())
            .with_bearer_token("eyJtoken".to_string());

        assert_eq!(ctx.bearer_token(), Some("eyJtoken"));
        assert_eq!(ctx.get_header("authorization"), Some("Bearer eyJtoken"));
    }

    #[test]
    fn test_with_api_key_header() {
        let ctx = RequestContext::new_without_auth("t1".to_string(), "ns".to_string())
            .with_api_key_header("x-api-key".to_string(), "sk-abc123".to_string());

        assert_eq!(ctx.get_header("x-api-key"), Some("sk-abc123"));
        assert!(ctx.has_header("x-api-key"));
        assert!(!ctx.has_header("x-nonexistent"));
    }

    #[test]
    fn test_with_api_key_query() {
        let ctx = RequestContext::new_without_auth("t1".to_string(), "ns".to_string())
            .with_api_key_query("api_key".to_string(), "sk-xyz".to_string());

        assert_eq!(ctx.api_key_query("api_key"), Some("sk-xyz"));
        assert_eq!(ctx.api_key_query("other"), None);
    }

    #[test]
    fn test_with_header() {
        let ctx = RequestContext::new_without_auth("t1".to_string(), "ns".to_string())
            .with_header("X-Custom-Header".to_string(), "value".to_string());

        assert_eq!(ctx.get_header("x-custom-header"), Some("value"));
        assert_eq!(ctx.get_header("X-Custom-Header"), Some("value"));
    }

    #[test]
    fn test_with_headers_bulk() {
        let mut headers = HashMap::new();
        headers.insert("Authorization".to_string(), "Bearer tok".to_string());
        headers.insert("X-API-Key".to_string(), "key123".to_string());

        let ctx = RequestContext::new_without_auth("t1".to_string(), "ns".to_string())
            .with_headers(headers);

        assert_eq!(ctx.bearer_token(), Some("tok"));
        assert_eq!(ctx.get_header("x-api-key"), Some("key123"));
    }

    #[test]
    fn test_http_headers_excludes_query_keys() {
        let ctx = RequestContext::new_without_auth("t1".to_string(), "ns".to_string())
            .with_bearer_token("tok".to_string())
            .with_api_key_header("x-api-key".to_string(), "key1".to_string())
            .with_api_key_query("api_key".to_string(), "key2".to_string());

        let http_hdrs = ctx.http_headers();
        assert_eq!(http_hdrs.get("authorization"), Some(&"Bearer tok"));
        assert_eq!(http_hdrs.get("x-api-key"), Some(&"key1"));
        assert!(!http_hdrs.contains_key("apikey-query:api_key"));

        let query_params = ctx.api_key_query_params();
        assert_eq!(query_params.get("api_key"), Some(&"key2"));
    }

    #[test]
    fn test_from_auth_with_headers() {
        let mut headers = HashMap::new();
        headers.insert("authorization".to_string(), "Bearer tok123".to_string());
        headers.insert("x-custom".to_string(), "val".to_string());

        let ctx = RequestContext::from_auth_with_headers(
            Some("t1".to_string()),
            Some("ns".to_string()),
            Some("user1".to_string()),
            false,
            false,
            None,
            None,
            headers,
        )
        .unwrap();

        assert_eq!(ctx.tenant_id(), "t1");
        assert_eq!(ctx.bearer_token(), Some("tok123"));
        assert_eq!(ctx.get_header("x-custom"), Some("val"));
    }

    #[test]
    fn test_from_auth_auth_disabled_empty_tenant_allowed() {
        let ctx = RequestContext::from_auth(None, None, None, false, false, None, None).unwrap();
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
        let result =
            RequestContext::from_auth(None, Some("ns".to_string()), None, false, true, None, None);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            RequestContextError::MissingTenantId
        ));
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
        let ctx =
            RequestContext::from_auth(Some("t1".to_string()), None, None, false, false, None, None)
                .unwrap();
        assert_eq!(ctx.namespace(), "");
    }
}
