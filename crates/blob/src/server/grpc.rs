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
}

#[tonic::async_trait]
impl BlobServiceTrait for BlobServiceImpl {
    type DownloadBlobStream = tokio_stream::wrappers::ReceiverStream<Result<DownloadBlobResponse, Status>>;
    /// Upload a blob
    async fn upload_blob(
        &self,
        request: Request<UploadBlobRequest>,
    ) -> Result<Response<UploadBlobResponse>, Status> {
        let req = request.into_inner();

        // Validate required fields
        if req.tenant_id.is_empty() {
            return Err(Status::invalid_argument("tenant_id is required"));
        }
        if req.namespace.is_empty() {
            return Err(Status::invalid_argument("namespace is required"));
        }
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

        // Upload blob
        let metadata = self
            .blob_service
            .upload_blob(
                &req.tenant_id,
                &req.namespace,
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
        let req = request.into_inner();

        if req.blob_id.is_empty() {
            return Err(Status::invalid_argument("blob_id is required"));
        }

        // Get metadata first
        let metadata = self
            .blob_service
            .get_metadata(&req.blob_id)
            .await
            .map_err(|e| Status::not_found(format!("Blob not found: {}", e)))?;

        // Download blob data
        let data = self
            .blob_service
            .download_blob(&req.blob_id)
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
        let req = request.into_inner();

        if req.blob_id.is_empty() {
            return Err(Status::invalid_argument("blob_id is required"));
        }

        let metadata = self
            .blob_service
            .get_metadata(&req.blob_id)
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
        let req = request.into_inner();

        if req.tenant_id.is_empty() {
            return Err(Status::invalid_argument("tenant_id is required"));
        }
        if req.namespace.is_empty() {
            return Err(Status::invalid_argument("namespace is required"));
        }

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
        let page_size = req
            .page
            .as_ref()
            .map(|p| p.page_size)
            .unwrap_or(100)
            .max(1)
            .min(1000) as i64;
        let page = 1; // TODO: Parse from page_token

        let (blobs, total_count) = self
            .blob_service
            .list_blobs(&req.tenant_id, &req.namespace, &filters, page_size, page)
            .await
            .map_err(|e| Status::internal(format!("Failed to list blobs: {}", e)))?;

        // Build page response
        use plexspaces_proto::common::v1::PageResponse;
        let page_response = PageResponse {
            next_page_token: String::new(), // TODO: Generate page token
            total_size: total_count as i32,
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
        let req = request.into_inner();

        if req.blob_id.is_empty() {
            return Err(Status::invalid_argument("blob_id is required"));
        }

        self.blob_service
            .delete_blob(&req.blob_id)
            .await
            .map_err(|e| Status::internal(format!("Failed to delete blob: {}", e)))?;

        Ok(Response::new(DeleteBlobResponse {}))
    }

    /// Generate presigned URL
    async fn generate_presigned_url(
        &self,
        request: Request<GeneratePresignedUrlRequest>,
    ) -> Result<Response<GeneratePresignedUrlResponse>, Status> {
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

        // Generate presigned URL
        let url = self.blob_service
            .generate_presigned_url(&req.blob_id, &req.operation, expires_after)
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
