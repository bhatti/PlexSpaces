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

//! HTTP Router for handling custom HTTP routes alongside gRPC services
//!
//! This module provides routing for:
//! - Pure HTTP routes (e.g., blob upload/download)
//! - gRPC-Gateway routes (gRPC APIs via HTTP)
//! - Native gRPC routes
//!
//! Uses tower Layer to intercept HTTP requests and route to custom handlers.

use axum::{
    body::Body,
    extract::Multipart,
    http::StatusCode,
    response::{Json, Response},
    routing::{get, post},
    Router,
};
use serde::Serialize;
use std::sync::Arc;
use tower::{Layer, Service};
use hyper::{Request, StatusCode as HyperStatusCode};
use http_body_util::{Full, BodyExt};
use hyper::body::Bytes;
use url::form_urlencoded;

/// JSON-serializable blob metadata response
#[derive(Serialize)]
struct BlobMetadataJson {
    blob_id: String,
    tenant_id: String,
    namespace: String,
    name: String,
    sha256: String,
    content_type: String,
    content_length: i64,
    etag: String,
    blob_group: String,
    kind: String,
    metadata: std::collections::HashMap<String, String>,
    tags: std::collections::HashMap<String, String>,
    expires_at: Option<String>,
    created_at: Option<String>,
    updated_at: Option<String>,
}

impl From<plexspaces_proto::storage::v1::BlobMetadata> for BlobMetadataJson {
    fn from(meta: plexspaces_proto::storage::v1::BlobMetadata) -> Self {
        use prost_types::Timestamp;
        use chrono::{DateTime, Utc};
        
        let expires_at = meta.expires_at.as_ref().map(|ts| {
            let dt = DateTime::<Utc>::from_timestamp(ts.seconds, ts.nanos as u32)
                .unwrap_or_else(Utc::now);
            dt.to_rfc3339()
        });
        
        let created_at = meta.created_at.as_ref().map(|ts| {
            let dt = DateTime::<Utc>::from_timestamp(ts.seconds, ts.nanos as u32)
                .unwrap_or_else(Utc::now);
            dt.to_rfc3339()
        });
        
        let updated_at = meta.updated_at.as_ref().map(|ts| {
            let dt = DateTime::<Utc>::from_timestamp(ts.seconds, ts.nanos as u32)
                .unwrap_or_else(Utc::now);
            dt.to_rfc3339()
        });
        
        Self {
            blob_id: meta.blob_id,
            tenant_id: meta.tenant_id,
            namespace: meta.namespace,
            name: meta.name,
            sha256: meta.sha256,
            content_type: meta.content_type,
            content_length: meta.content_length,
            etag: meta.etag,
            blob_group: meta.blob_group,
            kind: meta.kind,
            metadata: meta.metadata,
            tags: meta.tags,
            expires_at,
            created_at,
            updated_at,
        }
    }
}

/// HTTP router layer that intercepts requests and routes to custom handlers
#[derive(Clone)]
pub struct HttpRouterLayer {
    blob_service: Option<Arc<plexspaces_blob::BlobService>>,
    service_locator: Option<Arc<plexspaces_core::ServiceLocator>>,
    node_id: Option<String>,
}

impl HttpRouterLayer {
    /// Create new HTTP router layer
    pub fn new() -> Self {
        Self {
            blob_service: None,
            service_locator: None,
            node_id: None,
        }
    }

    /// Register blob service for HTTP upload/download routes
    pub fn with_blob_service(mut self, blob_service: Arc<plexspaces_blob::BlobService>) -> Self {
        self.blob_service = Some(blob_service);
        self
    }

    /// Register service locator and node ID for actor invocation routes
    pub fn with_actor_service(mut self, service_locator: Arc<plexspaces_core::ServiceLocator>, node_id: String) -> Self {
        self.service_locator = Some(service_locator);
        self.node_id = Some(node_id);
        self
    }
}

impl Default for HttpRouterLayer {
    fn default() -> Self {
        Self::new()
    }
}

impl<S> Layer<S> for HttpRouterLayer
where
    S: Service<Request<hyper::body::Incoming>, Response = Response<hyper::body::Incoming>> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Send + 'static + std::fmt::Display,
{
    type Service = HttpRouterService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        HttpRouterService {
            blob_service: self.blob_service.clone(),
            service_locator: self.service_locator.clone(),
            node_id: self.node_id.clone(),
            inner,
        }
    }
}

/// HTTP router service that routes requests to appropriate handlers
pub struct HttpRouterService<S> {
    blob_service: Option<Arc<plexspaces_blob::BlobService>>,
    service_locator: Option<Arc<plexspaces_core::ServiceLocator>>,
    node_id: Option<String>,
    inner: S,
}

impl<S> Service<Request<hyper::body::Incoming>> for HttpRouterService<S>
where
    S: Service<Request<hyper::body::Incoming>, Response = Response<hyper::body::Incoming>> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Send + 'static + std::fmt::Display,
{
    type Response = Response<hyper::body::Incoming>;
    type Error = S::Error;
    type Future = std::pin::Pin<Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut std::task::Context<'_>) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<hyper::body::Incoming>) -> Self::Future {
        let blob_service = self.blob_service.clone();
        let service_locator = self.service_locator.clone();
        let node_id = self.node_id.clone();
        let mut inner = self.inner.clone();
        Box::pin(async move {
            let path = req.uri().path();
            let method = req.method();
            
            // Route actor invocation HTTP endpoints (gRPC-Gateway style)
            // Pattern: /api/v1/actors/{tenant_id}/{namespace}/{actor_type} or /api/v1/actors/{namespace}/{actor_type}
            if let (Some(service_locator), Some(node_id)) = (&service_locator, &node_id) {
                if path.starts_with("/api/v1/actors/") {
                    // Parse path: /api/v1/actors/{tenant_id}/{namespace}/{actor_type} or /api/v1/actors/{namespace}/{actor_type}
                    let path_parts: Vec<&str> = path.strip_prefix("/api/v1/actors/")
                        .unwrap_or("")
                        .split('/')
                        .collect();
                    
                    if path_parts.len() >= 2 {
                        // Extract query params first (needed for fallback)
                        let query_params: std::collections::HashMap<String, String> = req.uri()
                            .query()
                            .map(|q| {
                                form_urlencoded::parse(q.as_bytes())
                                    .into_owned()
                                    .collect()
                            })
                            .unwrap_or_default();
                        
                        let (tenant_id, namespace, actor_type) = if path_parts.len() == 3 {
                            // /api/v1/actors/{tenant_id}/{namespace}/{actor_type}
                            (path_parts[0].to_string(), path_parts[1].to_string(), path_parts[2].to_string())
                        } else if path_parts.len() == 2 {
                            // /api/v1/actors/{namespace}/{actor_type} - tenant_id required in header or query param
                            // Try to get tenant_id from headers (set by JWT middleware) or query params
                            let tenant_id = req.headers()
                                .get("x-tenant-id")
                                .and_then(|v| v.to_str().ok())
                                .or_else(|| query_params.get("tenant_id").map(|s| s.as_str()))
                                .ok_or_else(|| {
                                    // Return error response if tenant_id not found
                                    let error_body = format!("tenant_id is required. Either include it in the path as /api/v1/actors/{{tenant_id}}/{{namespace}}/{{actor_type}}, provide x-tenant-id header, or add tenant_id query parameter.");
                                    return Ok(Response::builder()
                                        .status(StatusCode::BAD_REQUEST)
                                        .body(hyper::body::Incoming::from(error_body))
                                        .unwrap());
                                })?;
                            (tenant_id.to_string(), path_parts[0].to_string(), path_parts[1].to_string())
                        } else {
                            // Single part - require tenant_id and namespace in headers/query
                            let tenant_id = req.headers()
                                .get("x-tenant-id")
                                .and_then(|v| v.to_str().ok())
                                .or_else(|| query_params.get("tenant_id").map(|s| s.as_str()))
                                .ok_or_else(|| {
                                    let error_body = "tenant_id is required. Provide x-tenant-id header or tenant_id query parameter.";
                                    return Ok(Response::builder()
                                        .status(StatusCode::BAD_REQUEST)
                                        .body(hyper::body::Incoming::from(error_body))
                                        .unwrap());
                                })?;
                            let namespace = req.headers()
                                .get("x-namespace")
                                .and_then(|v| v.to_str().ok())
                                .or_else(|| query_params.get("namespace").map(|s| s.as_str()))
                                .unwrap_or(""); // namespace can be empty
                            (tenant_id.to_string(), namespace.to_string(), path_parts[0].to_string())
                        };
                        
                        // Extract query parameters for GET requests (already extracted above, reuse)
                        let query_params: std::collections::HashMap<String, String> = query_params;
                        
                        // Create InvokeActorRequest (tenant_id comes from auth, not request)
                        use plexspaces_proto::actor::v1::InvokeActorRequest;
                        let mut invoke_req = InvokeActorRequest {
                            namespace,
                            actor_type,
                            http_method: method.as_str().to_string(),
                            payload: vec![],
                            headers: std::collections::HashMap::new(),
                            query_params,
                            path: path.to_string(),
                            subpath: String::new(),
                        };
                        
                        // For POST/PUT, read body as payload
                        if method == &hyper::Method::POST || method == &hyper::Method::PUT {
                            let (parts, body) = req.into_parts();
                            match http_body_util::BodyExt::collect(body).await {
                                Ok(collected) => {
                                    invoke_req.payload = collected.to_bytes().to_vec();
                                    // Extract headers
                                    for (key, value) in parts.headers.iter() {
                                        if let Ok(value_str) = value.to_str() {
                                            invoke_req.headers.insert(
                                                key.as_str().to_string(),
                                                value_str.to_string()
                                            );
                                        }
                                    }
                                }
                                Err(e) => {
                                    let err_json = serde_json::json!({
                                        "code": 400,
                                        "message": format!("Failed to read request body: {}", e)
                                    });
                                    let resp = Response::builder()
                                        .status(HyperStatusCode::BAD_REQUEST)
                                        .header("content-type", "application/json")
                                        .body(hyper::body::Incoming::from(serde_json::to_string(&err_json).unwrap().into_bytes()))
                                        .unwrap();
                                    return Ok(resp);
                                }
                            }
                        } else {
                            // For GET/DELETE, extract headers without consuming body
                            for (key, value) in req.headers().iter() {
                                if let Ok(value_str) = value.to_str() {
                                    invoke_req.headers.insert(
                                        key.as_str().to_string(),
                                        value_str.to_string()
                                    );
                                }
                            }
                        }
                        
                        // Call InvokeActor via ActorService
                        use plexspaces_actor_service::ActorServiceImpl;
                        let actor_service = ActorServiceImpl::new(service_locator.clone(), node_id.clone());
                        use tonic::Request as TonicRequest;
                        let grpc_req = TonicRequest::new(invoke_req);
                        
                        match actor_service.invoke_actor(grpc_req).await {
                            Ok(grpc_resp) => {
                                let resp_inner = grpc_resp.into_inner();
                                // Convert InvokeActorResponse to JSON
                                use plexspaces_proto::actor::v1::InvokeActorResponse;
                                let json_resp = serde_json::json!({
                                    "success": resp_inner.success,
                                    "payload": if resp_inner.payload.is_empty() {
                                        serde_json::Value::Null
                                    } else {
                                        // Try to decode base64 or use as string
                                        match String::from_utf8(resp_inner.payload.clone()) {
                                            Ok(s) => serde_json::Value::String(s),
                                            Err(_) => {
                                                use base64::{Engine as _, engine::general_purpose};
                                                serde_json::Value::String(general_purpose::STANDARD.encode(&resp_inner.payload))
                                            }
                                        }
                                    },
                                    "headers": resp_inner.headers,
                                    "actor_id": resp_inner.actor_id,
                                    "error_message": resp_inner.error_message,
                                });
                                
                                let status = if resp_inner.success {
                                    HyperStatusCode::OK
                                } else {
                                    HyperStatusCode::from_u16(500).unwrap_or(HyperStatusCode::INTERNAL_SERVER_ERROR)
                                };
                                
                                let resp = Response::builder()
                                    .status(status)
                                    .header("content-type", "application/json")
                                    .body(hyper::body::Incoming::from(serde_json::to_string(&json_resp).unwrap().into_bytes()))
                                    .unwrap();
                                return Ok(resp);
                            }
                            Err(status) => {
                                let err_json = serde_json::json!({
                                    "code": status.code() as u16,
                                    "message": status.message()
                                });
                                let http_status = match status.code() {
                                    tonic::Code::NotFound => HyperStatusCode::NOT_FOUND,
                                    tonic::Code::InvalidArgument => HyperStatusCode::BAD_REQUEST,
                                    tonic::Code::PermissionDenied => HyperStatusCode::FORBIDDEN,
                                    _ => HyperStatusCode::INTERNAL_SERVER_ERROR,
                                };
                                let resp = Response::builder()
                                    .status(http_status)
                                    .header("content-type", "application/json")
                                    .body(hyper::body::Incoming::from(serde_json::to_string(&err_json).unwrap().into_bytes()))
                                    .unwrap();
                                return Ok(resp);
                            }
                        }
                    }
                }
            }

            // Route blob HTTP endpoints - these need custom handling
            if let Some(blob_service) = &blob_service {
                match (method, path) {
                    (&hyper::Method::POST, "/api/v1/blobs/upload") => {
                        // Use Axum router for multipart handling
                        // Create a temporary router and convert request
                        let router = create_axum_router(blob_service.clone());
                        use tower::ServiceExt;
                        let mut svc = router.into_service();
                        
                        // Convert hyper::body::Incoming to axum::body::Body
                        let (parts, body) = req.into_parts();
                        let body_bytes = match http_body_util::BodyExt::collect(body).await {
                            Ok(collected) => collected.to_bytes(),
                            Err(e) => {
                                // Create error response
                                let err_msg = format!("Failed to read body: {}", e);
                                let resp = Response::builder()
                                    .status(HyperStatusCode::BAD_REQUEST)
                                    .body(hyper::body::Incoming::from(err_msg.into_bytes()))
                                    .unwrap();
                                return Ok(resp);
                            }
                        };
                        
                        // Rebuild request with axum body
                        let axum_req = axum::extract::Request::from_parts(parts, axum::body::Body::from(body_bytes.to_vec()));
                        
                        // Call axum service
                        let axum_resp = match svc.call(axum_req).await {
                            Ok(resp) => resp,
                            Err(e) => {
                                // Create error response
                                let err_msg = format!("Axum router error: {}", e);
                                let resp = Response::builder()
                                    .status(HyperStatusCode::INTERNAL_SERVER_ERROR)
                                    .body(hyper::body::Incoming::from(err_msg.into_bytes()))
                                    .unwrap();
                                return Ok(resp);
                            }
                        };
                        
                        // Convert axum response back to hyper response
                        let (parts, body) = axum_resp.into_parts();
                        let body_bytes = match http_body_util::BodyExt::collect(body).await {
                            Ok(collected) => collected.to_bytes(),
                            Err(e) => {
                                // Create error response
                                let err_msg = format!("Failed to read response body: {}", e);
                                let resp = Response::builder()
                                    .status(HyperStatusCode::INTERNAL_SERVER_ERROR)
                                    .body(hyper::body::Incoming::from(err_msg.into_bytes()))
                                    .unwrap();
                                return Ok(resp);
                            }
                        };
                        
                        return Ok(Response::from_parts(parts, hyper::body::Incoming::from(body_bytes.to_vec())));
                    }
                    (_, path) if path.starts_with("/api/v1/blobs/") && path.ends_with("/download/raw") => {
                        // Extract blob_id from path
                        let blob_id = path
                            .strip_prefix("/api/v1/blobs/")
                            .and_then(|p| p.strip_suffix("/download/raw"))
                            .unwrap_or("");

                        if !blob_id.is_empty() {
                            // Get metadata
                            let metadata = match blob_service.get_metadata(blob_id).await {
                                Ok(m) => m,
                                Err(e) => {
                                    let err_msg = format!("Blob not found: {}", e);
                                    let resp = Response::builder()
                                        .status(HyperStatusCode::NOT_FOUND)
                                        .body(hyper::body::Incoming::from(err_msg.into_bytes()))
                                        .unwrap();
                                    return Ok(resp);
                                }
                            };

                            // Download blob data
                            let data = match blob_service.download_blob(blob_id).await {
                                Ok(d) => d,
                                Err(e) => {
                                    let err_msg = format!("Failed to download blob: {}", e);
                                    let resp = Response::builder()
                                        .status(HyperStatusCode::INTERNAL_SERVER_ERROR)
                                        .body(hyper::body::Incoming::from(err_msg.into_bytes()))
                                        .unwrap();
                                    return Ok(resp);
                                }
                            };

                            // Build response
                            let content_type = if metadata.content_type.is_empty() {
                                "application/octet-stream"
                            } else {
                                &metadata.content_type
                            };
                            let content_disposition = format!("attachment; filename=\"{}\"", 
                                if metadata.name.is_empty() { "file" } else { &metadata.name });

                            let resp = match Response::builder()
                                .status(HyperStatusCode::OK)
                                .header("content-type", content_type)
                                .header("content-disposition", content_disposition)
                                .header("content-length", data.len().to_string())
                                .body(hyper::body::Incoming::from(data))
                            {
                                Ok(r) => r,
                                Err(e) => {
                                    let err_msg = format!("Failed to build response: {}", e);
                                    let resp = Response::builder()
                                        .status(HyperStatusCode::INTERNAL_SERVER_ERROR)
                                        .body(hyper::body::Incoming::from(err_msg.into_bytes()))
                                        .unwrap();
                                    return Ok(resp);
                                }
                            };
                            return Ok(resp);
                        }
                    }
                    _ => {}
                }
            }

            // Forward all other requests to inner service (gRPC server)
            inner.call(req).await
        })
    }
}

/// Create Axum router for blob HTTP routes (used internally)
/// Note: This is a helper function but we handle routing in HttpRouterService directly
#[allow(dead_code)]
fn create_axum_router(
    blob_service: Arc<plexspaces_blob::BlobService>,
) -> Router {
    let blob_service_clone = blob_service.clone();
    
    // Upload handler
    let upload_handler = move |mut multipart: Multipart| async move {
        let mut file_data: Option<Vec<u8>> = None;
        let mut file_name: Option<String> = None;
        let mut tenant_id: Option<String> = None;
        let mut namespace: Option<String> = None;
        let mut content_type: Option<String> = None;
        let mut blob_group: Option<String> = None;
        let mut kind: Option<String> = None;

        while let Some(field) = multipart.next_field().await.map_err(|e| {
            (StatusCode::BAD_REQUEST, format!("Failed to parse multipart: {}", e))
        })? {
            let field_name = field.name().unwrap_or("").to_string();
            
            match field_name.as_str() {
                "file" | "data" => {
                    // Extract metadata before consuming the field
                    if let Some(filename) = field.file_name() {
                        file_name = Some(filename.to_string());
                    }
                    if let Some(ct) = field.content_type() {
                        content_type = Some(ct.to_string());
                    }
                    // Now consume the field to get bytes
                    let data = field.bytes().await.map_err(|e| {
                        (StatusCode::BAD_REQUEST, format!("Failed to read field: {}", e))
                    })?;
                    file_data = Some(data.to_vec());
                }
                "tenant_id" => {
                    tenant_id = Some(field.text().await.map_err(|e| {
                        (StatusCode::BAD_REQUEST, format!("Failed to read tenant_id: {}", e))
                    })?);
                }
                "namespace" => {
                    namespace = Some(field.text().await.map_err(|e| {
                        (StatusCode::BAD_REQUEST, format!("Failed to read namespace: {}", e))
                    })?);
                }
                "content_type" => {
                    content_type = Some(field.text().await.map_err(|e| {
                        (StatusCode::BAD_REQUEST, format!("Failed to read content_type: {}", e))
                    })?);
                }
                "blob_group" => {
                    blob_group = Some(field.text().await.map_err(|e| {
                        (StatusCode::BAD_REQUEST, format!("Failed to read blob_group: {}", e))
                    })?);
                }
                "kind" => {
                    kind = Some(field.text().await.map_err(|e| {
                        (StatusCode::BAD_REQUEST, format!("Failed to read kind: {}", e))
                    })?);
                }
                _ => {}
            }
        }

        // Validate required fields
        let tenant_id = tenant_id.ok_or_else(|| {
            (StatusCode::BAD_REQUEST, "tenant_id is required".to_string())
        })?;
        let namespace = namespace.unwrap_or_else(|| "".to_string()); // namespace can be empty
        let file_data = file_data.ok_or_else(|| {
            (StatusCode::BAD_REQUEST, "file data is required".to_string())
        })?;
        let file_name = file_name.unwrap_or_else(|| "uploaded_file".to_string());

        // Upload blob
        let metadata = blob_service_clone
            .upload_blob(
                &tenant_id,
                &namespace,
                &file_name,
                file_data,
                content_type,
                blob_group,
                kind,
                std::collections::HashMap::new(),
                std::collections::HashMap::new(),
                None,
            )
            .await
            .map_err(|e| {
                (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to upload blob: {}", e))
            })?;

        Ok::<_, (StatusCode, String)>(Json(BlobMetadataJson::from(metadata)))
    };

    // Create Axum router
    Router::new()
        .route("/api/v1/blobs/upload", post(upload_handler))
}
