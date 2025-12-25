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

//! gRPC Blob Service Implementation
//!
//! ## Purpose
//! Implements the BlobService gRPC interface for blob storage operations.
//! This enables clients to upload, download, and manage blobs via gRPC and HTTP/REST.

use chrono::{Duration, Utc};
use crate::{BlobService, repository::ListFilters, helpers::datetime_to_timestamp};
use plexspaces_proto::storage::v1::{
    blob_service_server::BlobService as BlobServiceTrait,
    DeleteBlobRequest, DeleteBlobResponse, DownloadBlobRequest, DownloadBlobResponse,
    GeneratePresignedUrlRequest, GeneratePresignedUrlResponse, GetBlobMetadataRequest,
    GetBlobMetadataResponse, ListBlobsRequest, ListBlobsResponse, UploadBlobRequest,
    UploadBlobResponse,
};
use plexspaces_core::RequestContext;
use std::sync::Arc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

/// gRPC service implementation for blob storage operations
pub struct BlobServiceImpl {
    /// Blob service for actual operations
    blob_service: Arc<BlobService>,
}

impl BlobServiceImpl {
    /// Create new blob service implementation
    pub fn new(blob_service: Arc<BlobService>) -> Self {
        Self { blob_service }
    }

    /// Extract RequestContext from gRPC request metadata
    /// 
    /// ## Security Note
    /// When JWT authentication is enabled, the AuthInterceptor removes any user-provided
    /// x-tenant-id, x-user-id, and x-user-roles headers and sets them ONLY from JWT claims.
    /// This prevents header injection attacks. These headers must come from JWT, not from client.
    /// 
    /// Extracts tenant_id from:
    /// 1. `x-tenant-id` header (set by JWT middleware, NOT from client)
    /// 2. `tenant_id` in request labels (fallback, only if auth disabled)
    /// 3. Error if not found (production should always have JWT)
    fn extract_context<T>(request: &Request<T>) -> Result<RequestContext, Status> {
        let metadata = request.metadata();
        
        // Extract tenant_id from headers (set by JWT middleware, not from client)
        // AuthInterceptor ensures these headers come from JWT, not user input
        let tenant_id = metadata.get("x-tenant-id")
            .and_then(|v| v.to_str().ok())
            .filter(|s| !s.is_empty())  // Reject empty strings
            .or_else(|| {
                // Fallback: check request labels (for backward compatibility when auth disabled)
                // This is the HTTP API boundary exception
                None  // For now, require x-tenant-id header from JWT
            })
            .ok_or_else(|| Status::unauthenticated("Missing x-tenant-id header. JWT authentication required."))?;
        
        // namespace is required - must be provided in header or use default from config
        let namespace = metadata.get("x-namespace")
            .and_then(|v| v.to_str().ok())
            .filter(|s| !s.is_empty())
            .unwrap_or(""); // Default namespace (can be empty)
        
        // Extract user_id from headers (set by JWT middleware, not from client)
        let user_id = metadata.get("x-user-id")
            .and_then(|v| v.to_str().ok())
            .filter(|s| !s.is_empty());
        
        let mut ctx = RequestContext::new_without_auth(tenant_id.to_string(), namespace.to_string());
        
        if let Some(uid) = user_id {
            ctx = ctx.with_user_id(uid.to_string());
        }
        
        Ok(ctx)
    }
}

#[tonic::async_trait]
impl BlobServiceTrait for BlobServiceImpl {
    type DownloadBlobStream = tokio_stream::wrappers::ReceiverStream<Result<DownloadBlobResponse, Status>>;
    /// Upload a blob
    async fn upload_blob(
        &self,
        request: Request<UploadBlobRequest>,
    ) -> Result<Response<UploadBlobResponse>, Status> {
        // Extract RequestContext from metadata (tenant_id from JWT middleware)
        let ctx = Self::extract_context(&request)?;
        let req = request.into_inner();

        // Validate required fields
        if req.name.is_empty() {
            return Err(Status::invalid_argument("name is required"));
        }
        if req.data.is_empty() {
            return Err(Status::invalid_argument("data cannot be empty"));
        }

        // Convert expires_after Duration to chrono Duration
        let expires_after = req.expires_after.map(|d| {
            Duration::seconds(d.seconds) + Duration::nanoseconds(d.nanos as i64)
        });

        // Upload blob (tenant_id and namespace from RequestContext)
        let metadata = self
            .blob_service
            .upload_blob(
                &ctx,
                &req.name,
                req.data,
                if req.content_type.is_empty() {
                    None
                } else {
                    Some(req.content_type)
                },
                if req.blob_group.is_empty() {
                    None
                } else {
                    Some(req.blob_group)
                },
                if req.kind.is_empty() {
                    None
                } else {
                    Some(req.kind)
                },
                req.metadata,
                req.tags,
                expires_after,
            )
            .await
            .map_err(|e| Status::internal(format!("Failed to upload blob: {}", e)))?;

        Ok(Response::new(UploadBlobResponse {
            metadata: Some(metadata),
        }))
    }

    /// Download a blob (streaming)
    async fn download_blob(
        &self,
        request: Request<DownloadBlobRequest>,
    ) -> Result<Response<Self::DownloadBlobStream>, Status> {
        // Extract RequestContext from metadata (tenant_id from JWT middleware)
        let ctx = Self::extract_context(&request)?;
        let req = request.into_inner();

        if req.blob_id.is_empty() {
            return Err(Status::invalid_argument("blob_id is required"));
        }

        // Get metadata first (automatically filtered by tenant_id)
        let metadata = self
            .blob_service
            .get_metadata(&ctx, &req.blob_id)
            .await
            .map_err(|e| Status::not_found(format!("Blob not found: {}", e)))?;

        // Download blob data (automatically filtered by tenant_id)
        let data = self
            .blob_service
            .download_blob(&ctx, &req.blob_id)
            .await
            .map_err(|e| Status::internal(format!("Failed to download blob: {}", e)))?;

        // Create streaming response
        // First send metadata, then data in chunks
        let (tx, rx) = tokio::sync::mpsc::channel(10);

        // Spawn task to send data
        let metadata_clone = metadata.clone();
        tokio::spawn(async move {
            // Send metadata first
            if tx
                .send(Ok(DownloadBlobResponse {
                    metadata: Some(metadata_clone),
                    data: vec![],
                }))
                .await
                .is_err()
            {
                return;
            }

            // Send data in chunks (64KB chunks)
            const CHUNK_SIZE: usize = 64 * 1024;
            for chunk in data.chunks(CHUNK_SIZE) {
                if tx
                    .send(Ok(DownloadBlobResponse {
                        metadata: None,
                        data: chunk.to_vec(),
                    }))
                    .await
                    .is_err()
                {
                    break;
                }
            }
        });

        // Create streaming response
        let stream = ReceiverStream::new(rx);
        Ok(Response::new(stream))
    }

    /// Get blob metadata
    async fn get_blob_metadata(
        &self,
        request: Request<GetBlobMetadataRequest>,
    ) -> Result<Response<GetBlobMetadataResponse>, Status> {
        // Extract RequestContext from metadata (tenant_id from JWT middleware)
        let ctx = Self::extract_context(&request)?;
        let req = request.into_inner();

        if req.blob_id.is_empty() {
            return Err(Status::invalid_argument("blob_id is required"));
        }

        // Get metadata (automatically filtered by tenant_id)
        let metadata = self
            .blob_service
            .get_metadata(&ctx, &req.blob_id)
            .await
            .map_err(|e| Status::not_found(format!("Blob not found: {}", e)))?;

        Ok(Response::new(GetBlobMetadataResponse {
            metadata: Some(metadata),
        }))
    }

    /// List blobs
    async fn list_blobs(
        &self,
        request: Request<ListBlobsRequest>,
    ) -> Result<Response<ListBlobsResponse>, Status> {
        // Extract RequestContext from metadata (tenant_id from JWT middleware)
        let ctx = Self::extract_context(&request)?;
        let req = request.into_inner();

        // Build filters
        let filters = ListFilters {
            name_prefix: if req.name_prefix.is_empty() {
                None
            } else {
                Some(req.name_prefix)
            },
            blob_group: if req.blob_group.is_empty() {
                None
            } else {
                Some(req.blob_group)
            },
            kind: if req.kind.is_empty() {
                None
            } else {
                Some(req.kind)
            },
            sha256: if req.sha256.is_empty() {
                None
            } else {
                Some(req.sha256)
            },
        };

        // Get pagination params
        let offset = req
            .page
            .as_ref()
            .map(|p| p.offset.max(0))
            .unwrap_or(0) as i64;
        let limit = req
            .page
            .as_ref()
            .map(|p| p.limit.max(1).min(1000))
            .unwrap_or(100) as i64;
        let page = (offset / limit) as i64 + 1;

        // List blobs (tenant_id and namespace from RequestContext)
        let (blobs, total_count) = self
            .blob_service
            .list_blobs(&ctx, &filters, limit, page)
            .await
            .map_err(|e| Status::internal(format!("Failed to list blobs: {}", e)))?;

        // Build page response
        use plexspaces_proto::common::v1::PageResponse;
        let has_next = (offset + limit) < total_count;
        let page_response = PageResponse {
            total_size: total_count as i32,
            offset: offset as i32,
            limit: limit as i32,
            has_next,
        };

        Ok(Response::new(ListBlobsResponse {
            blobs,
            page: Some(page_response),
        }))
    }

    /// Delete a blob
    async fn delete_blob(
        &self,
        request: Request<DeleteBlobRequest>,
    ) -> Result<Response<DeleteBlobResponse>, Status> {
        // Extract RequestContext from metadata (tenant_id from JWT middleware)
        let ctx = Self::extract_context(&request)?;
        let req = request.into_inner();

        if req.blob_id.is_empty() {
            return Err(Status::invalid_argument("blob_id is required"));
        }

        // Delete blob (automatically filtered by tenant_id)
        self.blob_service
            .delete_blob(&ctx, &req.blob_id)
            .await
            .map_err(|e| Status::internal(format!("Failed to delete blob: {}", e)))?;

        Ok(Response::new(DeleteBlobResponse {}))
    }

    /// Generate presigned URL
    async fn generate_presigned_url(
        &self,
        request: Request<GeneratePresignedUrlRequest>,
    ) -> Result<Response<GeneratePresignedUrlResponse>, Status> {
        // Extract RequestContext from metadata (tenant_id from JWT middleware)
        let ctx = Self::extract_context(&request)?;
        let req = request.into_inner();

        if req.blob_id.is_empty() {
            return Err(Status::invalid_argument("blob_id is required"));
        }
        if req.operation.is_empty() {
            return Err(Status::invalid_argument("operation is required"));
        }

        let expires_after = req
            .expires_after
            .map(|d| Duration::seconds(d.seconds) + Duration::nanoseconds(d.nanos as i64))
            .unwrap_or_else(|| Duration::hours(1));

        // Generate presigned URL (automatically filtered by tenant_id)
        let url = self.blob_service
            .generate_presigned_url(&ctx, &req.blob_id, &req.operation, expires_after)
            .await
            .map_err(|e| Status::internal(format!("Failed to generate presigned URL: {}", e)))?;

        // Calculate expiration time
        let expires_at = Some(crate::helpers::datetime_to_timestamp(Utc::now() + expires_after));

        Ok(Response::new(GeneratePresignedUrlResponse {
            url,
            expires_at,
        }))
    }
}
