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

//! Tests for helper functions

use plexspaces_blob::helpers::*;
use plexspaces_proto::storage::v1::BlobMetadata;
use chrono::{Duration, Utc};
use prost_types::Timestamp;

#[test]
fn test_datetime_to_timestamp() {
    let dt = Utc::now();
    let ts = datetime_to_timestamp(dt);
    
    assert!(ts.seconds > 0);
    assert!(ts.nanos >= 0);
}

#[test]
fn test_timestamp_to_datetime() {
    let dt = Utc::now();
    let ts = datetime_to_timestamp(dt);
    
    let converted = timestamp_to_datetime(Some(ts));
    assert!(converted.is_some());
    
    let converted_dt = converted.unwrap();
    // Should be within 1 second (allowing for rounding)
    assert!((converted_dt.timestamp() - dt.timestamp()).abs() <= 1);
}

#[test]
fn test_timestamp_to_datetime_none() {
    assert!(timestamp_to_datetime(None).is_none());
}

#[test]
fn test_get_storage_path() {
    let metadata = BlobMetadata {
        blob_id: "blob-123".to_string(),
        tenant_id: "tenant-1".to_string(),
        namespace: "ns-1".to_string(),
        name: "test.txt".to_string(),
        sha256: "a".repeat(64),
        content_type: String::new(),
        content_length: 100,
        etag: String::new(),
        blob_group: String::new(),
        kind: String::new(),
        metadata: std::collections::HashMap::new(),
        tags: std::collections::HashMap::new(),
        expires_at: None,
        created_at: None,
        updated_at: None,
    };

    let path = get_storage_path(&metadata, "/plexspaces");
    assert_eq!(path, "/plexspaces/tenant-1/ns-1/blob-123");

    let path2 = get_storage_path(&metadata, "/plexspaces/");
    assert_eq!(path2, "/plexspaces/tenant-1/ns-1/blob-123");
}

#[test]
fn test_is_expired() {
    let mut metadata = BlobMetadata {
        blob_id: "blob-1".to_string(),
        tenant_id: "tenant-1".to_string(),
        namespace: "ns-1".to_string(),
        name: "test.txt".to_string(),
        sha256: "a".repeat(64),
        content_type: String::new(),
        content_length: 100,
        etag: String::new(),
        blob_group: String::new(),
        kind: String::new(),
        metadata: std::collections::HashMap::new(),
        tags: std::collections::HashMap::new(),
        expires_at: None,
        created_at: None,
        updated_at: None,
    };

    // Not expired (no expiration)
    assert!(!is_expired(&metadata));

    // Expired
    metadata.expires_at = Some(datetime_to_timestamp(Utc::now() - Duration::hours(1)));
    assert!(is_expired(&metadata));

    // Not expired (future)
    metadata.expires_at = Some(datetime_to_timestamp(Utc::now() + Duration::hours(1)));
    assert!(!is_expired(&metadata));
}

#[test]
fn test_validate_metadata() {
    let metadata = BlobMetadata {
        blob_id: "blob-1".to_string(),
        tenant_id: "tenant-1".to_string(),
        namespace: "ns-1".to_string(),
        name: "test.txt".to_string(),
        sha256: "a".repeat(64),
        content_type: String::new(),
        content_length: 100,
        etag: String::new(),
        blob_group: String::new(),
        kind: String::new(),
        metadata: std::collections::HashMap::new(),
        tags: std::collections::HashMap::new(),
        expires_at: None,
        created_at: None,
        updated_at: None,
    };

    assert!(validate_metadata(&metadata).is_ok());
}

#[test]
fn test_validate_metadata_empty_blob_id() {
    let mut metadata = BlobMetadata {
        blob_id: String::new(),
        tenant_id: "tenant-1".to_string(),
        namespace: "ns-1".to_string(),
        name: "test.txt".to_string(),
        sha256: "a".repeat(64),
        content_type: String::new(),
        content_length: 100,
        etag: String::new(),
        blob_group: String::new(),
        kind: String::new(),
        metadata: std::collections::HashMap::new(),
        tags: std::collections::HashMap::new(),
        expires_at: None,
        created_at: None,
        updated_at: None,
    };

    assert!(validate_metadata(&metadata).is_err());
}

#[test]
fn test_validate_metadata_invalid_sha256() {
    let metadata = BlobMetadata {
        blob_id: "blob-1".to_string(),
        tenant_id: "tenant-1".to_string(),
        namespace: "ns-1".to_string(),
        name: "test.txt".to_string(),
        sha256: "invalid".to_string(),
        content_type: String::new(),
        content_length: 100,
        etag: String::new(),
        blob_group: String::new(),
        kind: String::new(),
        metadata: std::collections::HashMap::new(),
        tags: std::collections::HashMap::new(),
        expires_at: None,
        created_at: None,
        updated_at: None,
    };

    assert!(validate_metadata(&metadata).is_err());
}

#[test]
fn test_validate_metadata_negative_length() {
    let metadata = BlobMetadata {
        blob_id: "blob-1".to_string(),
        tenant_id: "tenant-1".to_string(),
        namespace: "ns-1".to_string(),
        name: "test.txt".to_string(),
        sha256: "a".repeat(64),
        content_type: String::new(),
        content_length: -1,
        etag: String::new(),
        blob_group: String::new(),
        kind: String::new(),
        metadata: std::collections::HashMap::new(),
        tags: std::collections::HashMap::new(),
        expires_at: None,
        created_at: None,
        updated_at: None,
    };

    assert!(validate_metadata(&metadata).is_err());
}
