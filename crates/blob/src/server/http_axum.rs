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

//! Axum HTTP handlers for blob file upload/download
//!
//! Provides simple HTTP endpoints for uploading and downloading files:
//! - POST /api/v1/blobs/upload - Upload a file (multipart/form-data)
//! - GET /api/v1/blobs/{blob_id}/download/raw - Download raw file data

use crate::{BlobService, BlobError};
use axum::{
    extract::{Multipart, Path},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Router,
};
use std::collections::HashMap;
use std::sync::Arc;

/// Create Axum router for blob HTTP endpoints
pub fn create_blob_router(blob_service: Arc<BlobService>) -> Router {
    Router::new()
        .route("/api/v1/blobs/upload", post(handle_upload))
        .route("/api/v1/blobs/:blob_id/download/raw", get(handle_download_raw))
        .with_state(blob_service)
}

/// Handle file upload (multipart/form-data)
async fn handle_upload(
    axum::extract::State(blob_service): axum::extract::State<Arc<BlobService>>,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, BlobError> {
    let mut file_data: Option<Vec<u8>> = None;
    let mut file_name: Option<String> = None;
    let mut tenant_id: Option<String> = None;
    let mut namespace: Option<String> = None;
    let mut content_type: Option<String> = None;
    let mut blob_group: Option<String> = None;
    let mut kind: Option<String> = None;

    // Parse multipart form
    while let Some(field) = multipart.next_field().await
        .map_err(|e| BlobError::InternalError(format!("Failed to parse multipart: {}", e)))?
    {
        let field_name = field.name().unwrap_or("").to_string();
        
        match field_name.as_str() {
            "file" => {
                // Get file name and content type from field
                if let Some(name) = field.file_name() {
                    file_name = Some(name.to_string());
                }
                if let Some(ct) = field.content_type() {
                    if content_type.is_none() {
                        content_type = Some(ct.to_string());
                    }
                }
                
                // Read file data
                let data = field.bytes().await
                    .map_err(|e| BlobError::InternalError(format!("Failed to read file data: {}", e)))?;
                file_data = Some(data.to_vec());
            }
            "tenant_id" => {
                let value = field.text().await
                    .map_err(|e| BlobError::InternalError(format!("Failed to read tenant_id: {}", e)))?;
                tenant_id = Some(value);
            }
            "namespace" => {
                let value = field.text().await
                    .map_err(|e| BlobError::InternalError(format!("Failed to read namespace: {}", e)))?;
                namespace = Some(value);
            }
            "content_type" => {
                let value = field.text().await
                    .map_err(|e| BlobError::InternalError(format!("Failed to read content_type: {}", e)))?;
                if content_type.is_none() {
                    content_type = Some(value);
                }
            }
            "blob_group" => {
                let value = field.text().await
                    .map_err(|e| BlobError::InternalError(format!("Failed to read blob_group: {}", e)))?;
                blob_group = Some(value);
            }
            "kind" => {
                let value = field.text().await
                    .map_err(|e| BlobError::InternalError(format!("Failed to read kind: {}", e)))?;
                kind = Some(value);
            }
            _ => {
                // Ignore unknown fields
            }
        }
    }

    // Validate required fields
    let file_data = file_data.ok_or_else(|| BlobError::InvalidInput("Missing file field".to_string()))?;
    let file_name = file_name.unwrap_or_else(|| "uploaded_file".to_string());
    let tenant_id = tenant_id.ok_or_else(|| BlobError::InvalidInput("Missing tenant_id field".to_string()))?;
    let namespace = namespace.ok_or_else(|| BlobError::InvalidInput("Missing namespace field".to_string()))?;

    // Upload blob
    let metadata = blob_service
        .upload_blob(
            &tenant_id,
            &namespace,
            &file_name,
            file_data,
            content_type,
            blob_group,
            kind,
            HashMap::new(),
            HashMap::new(),
            None,
        )
        .await?;

    // Return JSON response
    let response = serde_json::json!({
        "blob_id": metadata.blob_id,
        "tenant_id": metadata.tenant_id,
        "namespace": metadata.namespace,
        "name": metadata.name,
        "content_type": metadata.content_type,
        "content_length": metadata.content_length,
        "sha256": metadata.sha256,
        "created_at": metadata.created_at.map(|ts| {
            format!("{}.{:09}Z", ts.seconds, ts.nanos)
        }),
    });

    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        serde_json::to_string(&response)
            .map_err(|e| BlobError::InternalError(format!("Failed to serialize response: {}", e)))?
    ))
}

/// Handle raw file download
async fn handle_download_raw(
    axum::extract::State(blob_service): axum::extract::State<Arc<BlobService>>,
    Path(blob_id): Path<String>,
) -> Result<impl IntoResponse, BlobError> {
    // Get metadata
    let metadata = blob_service
        .get_metadata(&blob_id)
        .await?;

    // Download blob data
    let data = blob_service
        .download_blob(&blob_id)
        .await?;

    // Build response with appropriate headers
    let content_type = if metadata.content_type.is_empty() {
        "application/octet-stream".to_string()
    } else {
        metadata.content_type.clone()
    };
    
    let content_disposition = format!(
        "attachment; filename=\"{}\"",
        if metadata.name.is_empty() { "file" } else { &metadata.name }
    );

    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, content_type),
            (header::CONTENT_DISPOSITION, content_disposition),
            (header::CONTENT_LENGTH, data.len().to_string()),
        ],
        data,
    ))
}

/// Convert BlobError to HTTP response
impl IntoResponse for BlobError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            BlobError::NotFound(_) => (StatusCode::NOT_FOUND, self.to_string()),
            BlobError::InvalidInput(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            BlobError::ConfigError(_) => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
            BlobError::StorageError(_) => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
            BlobError::InternalError(_) => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
            BlobError::RepositoryError(_) => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
            BlobError::SqlError(_) => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
            BlobError::IoError(_) => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
            BlobError::ObjectStoreError(_) => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
            BlobError::SerializationError(_) => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
        };

        let body = serde_json::json!({
            "error": message,
        });

        (
            status,
            [(header::CONTENT_TYPE, "application/json")],
            serde_json::to_string(&body).unwrap_or_else(|_| "{\"error\":\"Internal error\"}".to_string()),
        )
            .into_response()
    }
}
