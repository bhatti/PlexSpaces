// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
//! RequestContext propagation tests (multi-tenancy).
//!
//! Verifies that request_context_from_grpc_request (single source of truth)
//! correctly extracts tenant_id and namespace from gRPC metadata and propagates
//! them via RequestContext::from_auth. No hardcoded tenant/namespace.

use plexspaces_core::request_context_from_grpc_request;
use plexspaces_node::service_locator_helpers::create_default_service_locator;
use std::collections::HashMap;
use std::sync::Arc;
use tonic::metadata::MetadataValue;

#[tokio::test]
async fn test_request_context_from_grpc_metadata_tenant_and_namespace() {
    let locator = create_default_service_locator(None, None).await;
    let sl: Arc<dyn plexspaces_core::ServiceLocator> = locator;

    let mut metadata = tonic::metadata::MetadataMap::new();
    metadata.insert(
        "x-tenant-id",
        MetadataValue::try_from("tenant-from-jwt").unwrap(),
    );
    metadata.insert(
        "x-namespace",
        MetadataValue::try_from("app-namespace").unwrap(),
    );

    let labels = HashMap::new();
    let ctx = request_context_from_grpc_request(&metadata, &labels, &sl)
        .await
        .unwrap();

    assert_eq!(ctx.tenant_id(), "tenant-from-jwt");
    assert_eq!(ctx.namespace(), "app-namespace");
}

#[tokio::test]
async fn test_request_context_from_grpc_metadata_empty_fails_when_auth_enabled() {
    let locator = create_default_service_locator(None, None).await;
    let sl: Arc<dyn plexspaces_core::ServiceLocator> = locator;

    let metadata = tonic::metadata::MetadataMap::new();
    let labels = HashMap::new();

    let result = request_context_from_grpc_request(&metadata, &labels, &sl).await;

    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        plexspaces_common::RequestContextError::MissingTenantId
    ));
}

#[tokio::test]
async fn test_request_context_from_grpc_labels_fallback() {
    let locator = create_default_service_locator(None, None).await;
    let sl: Arc<dyn plexspaces_core::ServiceLocator> = locator;

    let metadata = tonic::metadata::MetadataMap::new();
    let mut labels = HashMap::new();
    labels.insert("tenant_id".to_string(), "tenant-from-labels".to_string());
    labels.insert("namespace".to_string(), "ns-from-labels".to_string());

    let ctx = request_context_from_grpc_request(&metadata, &labels, &sl)
        .await
        .unwrap();

    assert_eq!(ctx.tenant_id(), "tenant-from-labels");
    assert_eq!(ctx.namespace(), "ns-from-labels");
}
