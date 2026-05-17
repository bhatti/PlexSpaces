// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Blob storage service trait.

use async_trait::async_trait;
use plexspaces_common::RequestContext;
use std::collections::HashMap;

/// Blob storage operations for uploading, downloading, and managing binary blobs.
#[async_trait]
pub trait BlobServiceTrait: Send + Sync {
    async fn upload(
        &self,
        ctx: &RequestContext,
        name: &str,
        data: Vec<u8>,
        content_type: Option<String>,
        metadata: HashMap<String, String>,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>>;

    async fn download(
        &self,
        ctx: &RequestContext,
        blob_id: &str,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>>;

    async fn download_by_name(
        &self,
        ctx: &RequestContext,
        name: &str,
    ) -> Result<Option<Vec<u8>>, Box<dyn std::error::Error + Send + Sync>>;

    async fn delete(
        &self,
        ctx: &RequestContext,
        blob_id: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;

    async fn delete_by_name(
        &self,
        ctx: &RequestContext,
        name: &str,
    ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>>;

    async fn exists(
        &self,
        ctx: &RequestContext,
        blob_id: &str,
    ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>>;

    async fn list(
        &self,
        ctx: &RequestContext,
        prefix: &str,
        limit: usize,
    ) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>>;

    /// List blobs with rich metadata for dashboard display.
    /// Returns (page_of_metadata, has_next).
    async fn list_metadata(
        &self,
        ctx: &RequestContext,
        prefix: &str,
        kind_filter: Option<&str>,
        offset: usize,
        limit: usize,
    ) -> Result<
        (Vec<plexspaces_proto::storage::v1::BlobMetadata>, bool),
        Box<dyn std::error::Error + Send + Sync>,
    >;

    /// Generate a presigned URL for direct client access to a blob.
    ///
    /// Returns `Ok(None)` when the configured storage backend does not support presigned
    /// URLs (e.g. local filesystem). Returns `Ok(Some(url))` on success.
    async fn generate_presigned_url(
        &self,
        ctx: &RequestContext,
        blob_id: &str,
        operation: &str,
        expires_after: std::time::Duration,
    ) -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>>;

    fn as_any(self: std::sync::Arc<Self>) -> std::sync::Arc<dyn std::any::Any + Send + Sync>;
}
