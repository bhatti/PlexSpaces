// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
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
    use crate::BlobError;
    use crate::BlobService;
    use http_body_util::{BodyExt, Full};
    use hyper::body::Bytes;
    use hyper::{Method, Request, Response, StatusCode};
    use multer::Multipart;
    use plexspaces_actor::{RequestContext, RequestContextExt};
    use std::sync::Arc;

    /// HTTP handler service for blob operations
    pub struct BlobHttpHandler {
        blob_service: Arc<BlobService>,
    }

    impl BlobHttpHandler {
        /// Create new HTTP handler
        pub fn new(blob_service: Arc<BlobService>) -> Self {
            Self { blob_service }
        }

        /// Extract RequestContext from HTTP request headers
        ///
        /// Extracts tenant_id from:
        /// 1. `x-tenant-id` header (set by JWT middleware)
        /// 2. Form field `tenant_id` (fallback for multipart uploads)
        /// 3. Error if not found (production should always have JWT)
        fn extract_context_from_headers<B>(req: &Request<B>) -> Result<RequestContext, BlobError> {
            let headers = req.headers();

            // Extract tenant_id from headers (set by JWT middleware)
            let tenant_id = headers
                .get("x-tenant-id")
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| {
                    BlobError::InvalidInput(
                        "Missing x-tenant-id header. JWT authentication required.".to_string(),
                    )
                })?;

            // namespace is REQUIRED - must be provided in header or use default from config
            let namespace = headers
                .get("x-namespace")
                .and_then(|v| v.to_str().ok())
                .filter(|s| !s.is_empty())
                .unwrap_or(""); // Default namespace (can be empty)

            let user_id = headers.get("x-user-id").and_then(|v| v.to_str().ok());

            let mut ctx =
                RequestContext::new_without_auth(tenant_id.to_string(), namespace.to_string());

            if let Some(uid) = user_id {
                ctx = ctx.with_user_id(uid.to_string());
            }

            Ok(ctx)
        }

        /// Extract RequestContext from multipart form (fallback for uploads)
        fn extract_context_from_form(tenant_id: &str, namespace: &str) -> RequestContext {
            RequestContext::new_without_auth(tenant_id.to_string(), namespace.to_string())
        }

        /// Handle HTTP request
        pub async fn handle_request<B>(
            &self,
            req: Request<B>,
        ) -> Result<Response<Full<Bytes>>, BlobError>
        where
            B: hyper::body::Body<Data = Bytes> + Send + 'static,
            B::Error: std::error::Error + Send + Sync + 'static,
        {
            let path = req.uri().path();
            let method = req.method();

            // Route based on path and method
            match (method, path) {
                (&Method::POST, "/api/v1/blobs/upload") => self.handle_upload(req).await,
                (_, path)
                    if path.starts_with("/api/v1/blobs/") && path.ends_with("/download/raw") =>
                {
                    self.handle_download_raw(req).await
                }
                _ => Ok(Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(Full::new(Bytes::from("Not Found")))
                    .unwrap()),
            }
        }

        /// Handle file upload (multipart/form-data)
        async fn handle_upload<B>(
            &self,
            req: Request<B>,
        ) -> Result<Response<Full<Bytes>>, BlobError>
        where
            B: hyper::body::Body<Data = Bytes> + Send + 'static,
            B::Error: std::error::Error + Send + Sync + 'static,
        {
            // Extract content-type header and boundary before moving req
            let boundary = {
                let content_type = req
                    .headers()
                    .get("content-type")
                    .and_then(|v| v.to_str().ok())
                    .ok_or_else(|| {
                        BlobError::InvalidInput("Missing content-type header".to_string())
                    })?;

                content_type
                    .strip_prefix("multipart/form-data; boundary=")
                    .ok_or_else(|| {
                        BlobError::InvalidInput("Invalid content-type for multipart".to_string())
                    })?
                    .to_string()
            };

            // Collect request body (now we can move req since we've extracted what we need)
            let (_parts, body) = req.into_parts();
            let body_bytes = BodyExt::collect(body)
                .await
                .map_err(|e| {
                    BlobError::InternalError(format!("Failed to read request body: {}", e))
                })?
                .to_bytes();

            // Create multipart parser from bytes
            // multer 2.1 expects a Stream<Item = Result<O, E>>, so we create one from the bytes
            use futures::stream;
            use std::io;
            // Create a stream that yields the bytes as a single chunk
            let bytes_stream =
                stream::once(async move { Ok::<bytes::Bytes, io::Error>(body_bytes) });
            let mut multipart = Multipart::new(bytes_stream, boundary.as_str());

            // Extract form fields
            let mut file_data: Option<Vec<u8>> = None;
            let mut file_name: Option<String> = None;
            let mut tenant_id: Option<String> = None;
            let mut namespace: Option<String> = None;
            let mut content_type_field: Option<String> = None;
            let mut blob_group: Option<String> = None;
            let mut kind: Option<String> = None;

            while let Some(field) = multipart.next_field().await.map_err(|e| {
                BlobError::InternalError(format!("Failed to parse multipart: {}", e))
            })? {
                let field_name = field.name().unwrap_or("").to_string();

                match field_name.as_str() {
                    "file" => {
                        if let Some(name) = field.file_name() {
                            file_name = Some(name.to_string());
                        }
                        if let Some(ct) = field.content_type() {
                            if content_type_field.is_none() {
                                content_type_field = Some(ct.to_string());
                            }
                        }
                        let data = field.bytes().await.map_err(|e| {
                            BlobError::InternalError(format!("Failed to read file data: {}", e))
                        })?;
                        file_data = Some(data.to_vec());
                    }
                    "tenant_id" => {
                        let value = field.text().await.map_err(|e| {
                            BlobError::InternalError(format!("Failed to read tenant_id: {}", e))
                        })?;
                        tenant_id = Some(value);
                    }
                    "namespace" => {
                        let value = field.text().await.map_err(|e| {
                            BlobError::InternalError(format!("Failed to read namespace: {}", e))
                        })?;
                        namespace = Some(value);
                    }
                    "content_type" => {
                        let value = field.text().await.map_err(|e| {
                            BlobError::InternalError(format!("Failed to read content_type: {}", e))
                        })?;
                        if content_type_field.is_none() {
                            content_type_field = Some(value);
                        }
                    }
                    "blob_group" => {
                        let value = field.text().await.map_err(|e| {
                            BlobError::InternalError(format!("Failed to read blob_group: {}", e))
                        })?;
                        blob_group = Some(value);
                    }
                    "kind" => {
                        let value = field.text().await.map_err(|e| {
                            BlobError::InternalError(format!("Failed to read kind: {}", e))
                        })?;
                        kind = Some(value);
                    }
                    _ => {}
                }
            }

            // Validate required fields
            let file_data = file_data
                .ok_or_else(|| BlobError::InvalidInput("Missing file field".to_string()))?;
            let tenant_id = tenant_id
                .ok_or_else(|| BlobError::InvalidInput("Missing tenant_id field".to_string()))?;
            let namespace = namespace
                .ok_or_else(|| BlobError::InvalidInput("Missing namespace field".to_string()))?;
            let file_name =
                file_name.ok_or_else(|| BlobError::InvalidInput("Missing filename".to_string()))?;

            // Create RequestContext from form fields
            let ctx = Self::extract_context_from_form(&tenant_id, &namespace);

            // Upload blob
            let metadata = self
                .blob_service
                .upload_blob(
                    &ctx,
                    crate::UploadBlobParams {
                        name: file_name,
                        data: file_data,
                        content_type: content_type_field,
                        blob_group,
                        kind,
                        ..Default::default()
                    },
                )
                .await?;

            // Return JSON response with metadata
            let response_json = serde_json::json!({
                "blob_id": metadata.blob_id,
                "tenant_id": metadata.tenant_id,
                "namespace": metadata.namespace,
                "name": metadata.name,
                "content_type": metadata.content_type,
                "content_length": metadata.content_length,
                "sha256": metadata.sha256,
            });

            Ok(Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/json")
                .body(Full::new(Bytes::from(
                    serde_json::to_string(&response_json).unwrap(),
                )))
                .unwrap())
        }

        /// Handle raw file download
        async fn handle_download_raw<B>(
            &self,
            req: Request<B>,
        ) -> Result<Response<Full<Bytes>>, BlobError>
        where
            B: hyper::body::Body<Data = Bytes> + Send + 'static,
            B::Error: std::error::Error + Send + Sync + 'static,
        {
            // Extract RequestContext from headers
            let ctx = Self::extract_context_from_headers(&req)?;

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

            // Get metadata (automatically filtered by tenant_id)
            let metadata = self.blob_service.get_metadata(&ctx, blob_id).await?;

            // Download blob data (automatically filtered by tenant_id)
            let data = self.blob_service.download_blob(&ctx, blob_id).await?;

            // Build response with appropriate content type
            let content_type = if metadata.content_type.is_empty() {
                "application/octet-stream"
            } else {
                &metadata.content_type
            };
            let content_disposition = format!(
                "attachment; filename=\"{}\"",
                if metadata.name.is_empty() {
                    "file"
                } else {
                    &metadata.name
                }
            );

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
        type Future = std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>,
        >;

        fn poll_ready(
            &mut self,
            _cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), Self::Error>> {
            std::task::Poll::Ready(Ok(()))
        }

        fn call(&mut self, req: Request<hyper::body::Incoming>) -> Self::Future {
            let handler = self.clone();
            Box::pin(async move { handler.handle_request(req).await })
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
