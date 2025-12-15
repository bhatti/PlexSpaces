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

//! Helper functions for working with proto types

use chrono::{DateTime, Utc};
use prost_types::Timestamp;
use plexspaces_proto::storage::v1::BlobMetadata;

/// Convert chrono DateTime to prost Timestamp
pub fn datetime_to_timestamp(dt: DateTime<Utc>) -> Timestamp {
    Timestamp {
        seconds: dt.timestamp(),
        nanos: dt.timestamp_subsec_nanos() as i32,
    }
}

/// Convert prost Timestamp to chrono DateTime
pub fn timestamp_to_datetime(ts: Option<Timestamp>) -> Option<DateTime<Utc>> {
    ts.map(|t| {
        DateTime::from_timestamp(t.seconds, t.nanos as u32)
            .unwrap_or_else(|| Utc::now())
    })
}

/// Get storage path for blob
/// Format: /plexspaces/{tenant_id}/{namespace}/{blob_id}
pub fn get_storage_path(metadata: &BlobMetadata, prefix: &str) -> String {
    let normalized_prefix = prefix.trim_end_matches('/');
    format!(
        "{}/{}/{}/{}",
        normalized_prefix,
        metadata.tenant_id,
        metadata.namespace,
        metadata.blob_id
    )
}

/// Check if blob is expired
pub fn is_expired(metadata: &BlobMetadata) -> bool {
    if let Some(expires_at) = &metadata.expires_at {
        if let Some(expires_dt) = timestamp_to_datetime(Some(expires_at.clone())) {
            return Utc::now() > expires_dt;
        }
    }
    false
}

/// Validate blob metadata
pub fn validate_metadata(metadata: &BlobMetadata) -> Result<(), String> {
    if metadata.blob_id.is_empty() {
        return Err("blob_id is required".to_string());
    }
    if metadata.tenant_id.is_empty() {
        return Err("tenant_id is required".to_string());
    }
    if metadata.namespace.is_empty() {
        return Err("namespace is required".to_string());
    }
    if metadata.name.is_empty() {
        return Err("name is required".to_string());
    }
    if metadata.sha256.len() != 64 {
        return Err("sha256 must be 64 hex characters".to_string());
    }
    if metadata.content_length < 0 {
        return Err("content_length must be non-negative".to_string());
    }
    Ok(())
}
