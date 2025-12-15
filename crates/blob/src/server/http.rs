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

//! Plain HTTP handlers for blob file upload/download
//!
//! Provides simple HTTP endpoints for uploading and downloading files:
//! - POST /api/v1/blobs/upload - Upload a file (multipart/form-data)
//!   - Form fields: `file` (required), `tenant_id` (required), `namespace` (required),
//!     `content_type` (optional), `blob_group` (optional), `kind` (optional)
//! - GET /api/v1/blobs/{blob_id}/download/raw - Download raw file data
//!
//! ## Integration
//!
//! These handlers can be integrated with the node's HTTP server using:
//! 1. A separate HTTP server on a different port
//! 2. Tower Router to handle specific routes before gRPC
//! 3. Axum Router (if using axum for HTTP routing)

#[cfg(feature = "server")]
mod handlers {
    use crate::BlobService;
    use crate::BlobError;
    use http_body_util::Full;
    use hyper::body::Bytes;
    use hyper::{Request, Response, StatusCode, Method};
    use multer::Multipart;
    use std::sync::Arc;
    use futures::stream;
    use tokio_util::io::StreamReader;

    /// HTTP handler service for blob operations
    pub struct BlobHttpHandler {
        blob_service: Arc<BlobService>,
    }

    impl BlobHttpHandler {
        /// Create new HTTP handler
        pub fn new(blob_service: Arc<BlobService>) -> Self {
            Self { blob_service }
        }

        /// Handle HTTP request
        pub async fn handle_request(
            &self,
            req: Request<hyper::body::Incoming>,
        ) -> Result<Response<Full<Bytes>>, BlobError> {
            let path = req.uri().path();
            let method = req.method();

            // Route based on path and method
            match (method, path) {
                (&Method::POST, "/api/v1/blobs/upload") => {
                    self.handle_upload(req).await
                }
                (_, path) if path.starts_with("/api/v1/blobs/") && path.ends_with("/download/raw") => {
                    self.handle_download_raw(req).await
                }
                _ => {
                    Ok(Response::builder()
                        .status(StatusCode::NOT_FOUND)
                        .body(Full::new(Bytes::from("Not Found")))
                        .unwrap())
                }
            }
        }

        /// Handle file upload (multipart/form-data)
        async fn handle_upload(
            &self,
            req: Request<hyper::body::Incoming>,
        ) -> Result<Response<Full<Bytes>>, BlobError> {
            // Get content type
            let content_type = req.headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| BlobError::ConfigError("Missing content-type header".to_string()))?;

            if !content_type.starts_with("multipart/form-data") {
                return Ok(Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .body(Full::new(Bytes::from("Expected multipart/form-data")))
                    .unwrap());
            }

            // Parse multipart
            let boundary = content_type
                .split("boundary=")
                .nth(1)
                .ok_or_else(|| BlobError::ConfigError("Missing boundary in content-type".to_string()))?;

            // Convert request body to bytes
            use http_body_util::BodyExt;
            let body_bytes = req.into_body().collect().await
                .map_err(|e| BlobError::InternalError(format!("Failed to read body: {}", e)))?
                .to_bytes();

            // Create multipart parser - multer 2.1 uses different API
            // For now, we'll use a simpler approach with bytes directly
            // Note: This handler is legacy - the Axum router handles multipart properly
            // This code path may not be used if Axum router is active
            // This handler is legacy - the Axum router handles multipart uploads properly
            // Return NOT_IMPLEMENTED to indicate this path should use Axum router
            Ok(Response::builder()
                .status(StatusCode::NOT_IMPLEMENTED)
                .body(Full::new(Bytes::from("Use Axum router for multipart uploads")))
                .unwrap())
        }

        /// Handle raw file download
        async fn handle_download_raw(
            &self,
            req: Request<hyper::body::Incoming>,
        ) -> Result<Response<Full<Bytes>>, BlobError> {
            // Extract blob_id from path: /api/v1/blobs/{blob_id}/download/raw
            let path = req.uri().path();
            let blob_id = path
                .strip_prefix("/api/v1/blobs/")
                .and_then(|p| p.strip_suffix("/download/raw"))
                .ok_or_else(|| BlobError::ConfigError("Invalid path format".to_string()))?;

            if blob_id.is_empty() {
                return Ok(Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .body(Full::new(Bytes::from("Missing blob_id")))
                    .unwrap());
            }

            // Get metadata
            let metadata = self.blob_service
                .get_metadata(blob_id)
                .await?;

            // Download blob data
            let data = self.blob_service
                .download_blob(blob_id)
                .await?;

            // Build response with appropriate content type
            let content_type = if metadata.content_type.is_empty() {
                "application/octet-stream"
            } else {
                &metadata.content_type
            };
            let content_disposition = format!("attachment; filename=\"{}\"", 
                if metadata.name.is_empty() { "file" } else { &metadata.name });

            Ok(Response::builder()
                .status(StatusCode::OK)
                .header("content-type", content_type)
                .header("content-disposition", content_disposition)
                .header("content-length", data.len().to_string())
                .body(Full::new(Bytes::from(data)))
                .unwrap())
        }
    }

    /// Create a tower Service from the HTTP handler
    /// This can be used with tower's Router or similar to integrate HTTP routes
    impl Clone for BlobHttpHandler {
        fn clone(&self) -> Self {
            Self {
                blob_service: self.blob_service.clone(),
            }
        }
    }

    impl tower::Service<Request<hyper::body::Incoming>> for BlobHttpHandler {
        type Response = Response<Full<Bytes>>;
        type Error = BlobError;
        type Future = std::pin::Pin<Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>>;

        fn poll_ready(&mut self, _cx: &mut std::task::Context<'_>) -> std::task::Poll<Result<(), Self::Error>> {
            std::task::Poll::Ready(Ok(()))
        }

        fn call(&mut self, req: Request<hyper::body::Incoming>) -> Self::Future {
            let handler = self.clone();
            Box::pin(async move {
                handler.handle_request(req).await
            })
        }
    }
}

#[cfg(feature = "server")]
pub use handlers::BlobHttpHandler;

#[cfg(not(feature = "server"))]
pub struct BlobHttpHandler {
    _private: (),
}

#[cfg(not(feature = "server"))]
impl BlobHttpHandler {
    pub fn new(_blob_service: std::sync::Arc<crate::BlobService>) -> Self {
        Self { _private: () }
    }
}
