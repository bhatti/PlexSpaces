// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Tests for blob config validation

use plexspaces_blob::BlobConfigExt;
use plexspaces_proto::storage::v1::BlobConfig as ProtoBlobConfig;
use std::env;

fn make_config(backend: &str, bucket: &str, endpoint: &str, region: &str) -> ProtoBlobConfig {
    ProtoBlobConfig {
        backend: backend.to_string(),
        bucket: bucket.to_string(),
        endpoint: endpoint.to_string(),
        region: region.to_string(),
        access_key_id: String::new(),
        secret_access_key: String::new(),
        use_ssl: false,
        prefix: "/plexspaces".to_string(),
        gcp_service_account_json: String::new(),
        azure_account_name: String::new(),
        azure_account_key: String::new(),
    }
}

#[test]
fn test_config_validation_embedded_no_endpoint_required() {
    // embedded backend: endpoint is optional (populated at runtime by EmbeddedObjectStore)
    let config = make_config("embedded", "test-bucket", "", "");
    assert!(config.validate().is_ok());
}

#[test]
fn test_config_validation_embedded_with_endpoint() {
    let config = make_config("embedded", "test-bucket", "http://localhost:9000", "");
    assert!(config.validate().is_ok());
}

#[test]
fn test_config_validation_s3_success() {
    let config = make_config("s3", "test-bucket", "", "us-east-1");
    assert!(config.validate().is_ok());
}

#[test]
fn test_config_validation_invalid_backend() {
    let config = make_config("invalid", "test", "", "");
    assert!(config.validate().is_err());
}

#[test]
fn test_config_validation_empty_bucket_for_s3() {
    let config = make_config("s3", "", "", "us-east-1");
    assert!(config.validate().is_err());
}

#[test]
fn test_config_validation_s3_no_region() {
    let config = make_config("s3", "test", "", "");
    assert!(config.validate().is_err());
}

#[test]
fn test_config_get_access_key_from_env() {
    env::set_var("BLOB_ACCESS_KEY_ID", "test-key");
    env::set_var("BLOB_SECRET_ACCESS_KEY", "test-secret");

    let config = make_config("embedded", "test", "http://localhost:9000", "");
    assert_eq!(config.get_access_key_id(), Some("test-key".to_string()));
    assert_eq!(config.get_secret_access_key(), Some("test-secret".to_string()));

    env::remove_var("BLOB_ACCESS_KEY_ID");
    env::remove_var("BLOB_SECRET_ACCESS_KEY");
}

#[test]
fn test_config_get_access_key_from_config() {
    let mut config = make_config("embedded", "test", "http://localhost:9000", "");
    config.access_key_id = "config-key".to_string();
    config.secret_access_key = "config-secret".to_string();

    assert_eq!(config.get_access_key_id(), Some("config-key".to_string()));
    assert_eq!(config.get_secret_access_key(), Some("config-secret".to_string()));
}
