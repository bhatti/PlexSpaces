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

//! Tests for blob config validation

use plexspaces_blob::BlobConfigExt;
use plexspaces_proto::storage::v1::BlobConfig as ProtoBlobConfig;
use std::env;

#[test]
fn test_config_validation_success() {
    let config = ProtoBlobConfig {
        backend: "minio".to_string(),
        bucket: "test-bucket".to_string(),
        endpoint: "http://localhost:9000".to_string(),
        region: String::new(),
        access_key_id: String::new(),
        secret_access_key: String::new(),
        use_ssl: false,
        prefix: "/plexspaces".to_string(),
        gcp_service_account_json: String::new(),
        azure_account_name: String::new(),
        azure_account_key: String::new(),
    };

    assert!(config.validate().is_ok());
}

#[test]
fn test_config_validation_invalid_backend() {
    let config = ProtoBlobConfig {
        backend: "invalid".to_string(),
        bucket: "test".to_string(),
        endpoint: String::new(),
        region: String::new(),
        access_key_id: String::new(),
        secret_access_key: String::new(),
        use_ssl: false,
        prefix: String::new(),
        gcp_service_account_json: String::new(),
        azure_account_name: String::new(),
        azure_account_key: String::new(),
    };

    assert!(config.validate().is_err());
}

#[test]
fn test_config_validation_empty_bucket() {
    let config = ProtoBlobConfig {
        backend: "s3".to_string(),
        bucket: String::new(),
        endpoint: String::new(),
        region: "us-east-1".to_string(),
        access_key_id: String::new(),
        secret_access_key: String::new(),
        use_ssl: false,
        prefix: String::new(),
        gcp_service_account_json: String::new(),
        azure_account_name: String::new(),
        azure_account_key: String::new(),
    };

    assert!(config.validate().is_err());
}

#[test]
fn test_config_validation_minio_no_endpoint() {
    let config = ProtoBlobConfig {
        backend: "minio".to_string(),
        bucket: "test".to_string(),
        endpoint: String::new(),
        region: String::new(),
        access_key_id: String::new(),
        secret_access_key: String::new(),
        use_ssl: false,
        prefix: String::new(),
        gcp_service_account_json: String::new(),
        azure_account_name: String::new(),
        azure_account_key: String::new(),
    };

    assert!(config.validate().is_err());
}

#[test]
fn test_config_validation_s3_no_region() {
    let config = ProtoBlobConfig {
        backend: "s3".to_string(),
        bucket: "test".to_string(),
        endpoint: String::new(),
        region: String::new(),
        access_key_id: String::new(),
        secret_access_key: String::new(),
        use_ssl: false,
        prefix: String::new(),
        gcp_service_account_json: String::new(),
        azure_account_name: String::new(),
        azure_account_key: String::new(),
    };

    assert!(config.validate().is_err());
}

#[test]
fn test_config_get_access_key_from_env() {
    env::set_var("BLOB_ACCESS_KEY_ID", "test-key");
    env::set_var("BLOB_SECRET_ACCESS_KEY", "test-secret");

    let config = ProtoBlobConfig {
        backend: "minio".to_string(),
        bucket: "test".to_string(),
        endpoint: "http://localhost:9000".to_string(),
        region: String::new(),
        access_key_id: String::new(),
        secret_access_key: String::new(),
        use_ssl: false,
        prefix: String::new(),
        gcp_service_account_json: String::new(),
        azure_account_name: String::new(),
        azure_account_key: String::new(),
    };

    assert_eq!(config.get_access_key_id(), Some("test-key".to_string()));
    assert_eq!(config.get_secret_access_key(), Some("test-secret".to_string()));

    env::remove_var("BLOB_ACCESS_KEY_ID");
    env::remove_var("BLOB_SECRET_ACCESS_KEY");
}

#[test]
fn test_config_get_access_key_from_config() {
    let config = ProtoBlobConfig {
        backend: "minio".to_string(),
        bucket: "test".to_string(),
        endpoint: "http://localhost:9000".to_string(),
        region: String::new(),
        access_key_id: "config-key".to_string(),
        secret_access_key: "config-secret".to_string(),
        use_ssl: false,
        prefix: String::new(),
        gcp_service_account_json: String::new(),
        azure_account_name: String::new(),
        azure_account_key: String::new(),
    };

    assert_eq!(config.get_access_key_id(), Some("config-key".to_string()));
    assert_eq!(config.get_secret_access_key(), Some("config-secret".to_string()));
}
