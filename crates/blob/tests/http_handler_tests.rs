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

//! Tests for HTTP handlers (upload/download)

#[cfg(feature = "server")]
mod tests {
    use plexspaces_blob::{BlobService, repository::sql::SqlBlobRepository};
    use plexspaces_proto::storage::v1::BlobConfig as ProtoBlobConfig;
    use plexspaces_core::RequestContext;
    use object_store::local::LocalFileSystem;
    use std::sync::Arc;
    use tempfile::{TempDir, NamedTempFile};
    use http_body_util::Full;
    use hyper::body::Bytes;
    use hyper::{Request, Method, Uri, StatusCode};

    async fn create_test_service() -> (Arc<BlobService>, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let local_store = Arc::new(LocalFileSystem::new_with_prefix(temp_dir.path()).unwrap());

        // Use temporary file-based database for reliability
        use sqlx::AnyPool;
        use sqlx::any::AnyPoolOptions;
        use tempfile::NamedTempFile;
        
        // Use temporary file-based database (more reliable than in-memory)
        let temp_db = NamedTempFile::new().unwrap();
        let db_path = temp_db.path().to_str().unwrap();
        let db_url = format!("sqlite:{}", db_path);
        
        let any_pool = AnyPoolOptions::new()
            .max_connections(5)
            .connect(&db_url)
            .await
            .unwrap();
        
        // Migrations are auto-applied in new()
        let repository = Arc::new(SqlBlobRepository::new(any_pool).await.unwrap());

        let config = ProtoBlobConfig {
            backend: "local".to_string(),
            bucket: "test".to_string(),
            endpoint: String::new(),
            region: String::new(),
            access_key_id: String::new(),
            secret_access_key: String::new(),
            use_ssl: false,
            prefix: "/plexspaces".to_string(),
            gcp_service_account_json: String::new(),
            azure_account_name: String::new(),
            azure_account_key: String::new(),
        };

        let service = BlobService::with_object_store(config, local_store, repository);
        (Arc::new(service), temp_dir)
    }

    #[tokio::test]
    async fn test_http_upload_handler() {
        let (service, _temp_dir) = create_test_service().await;
        let handler = plexspaces_blob::server::http::BlobHttpHandler::new(service.clone());

        // Create multipart form data
        let boundary = "----WebKitFormBoundary7MA4YWxkTrZu0gW";
        let body = format!(
            "--{}\r\n\
            Content-Disposition: form-data; name=\"file\"; filename=\"test.txt\"\r\n\
            Content-Type: text/plain\r\n\r\n\
            Hello, World!\r\n\
            --{}\r\n\
            Content-Disposition: form-data; name=\"tenant_id\"\r\n\r\n\
            tenant-1\r\n\
            --{}\r\n\
            Content-Disposition: form-data; name=\"namespace\"\r\n\r\n\
            ns-1\r\n\
            --{}--\r\n",
            boundary, boundary, boundary, boundary
        );

        let uri: Uri = "/api/v1/blobs/upload".parse().unwrap();
        let mut req = Request::builder()
            .method(Method::POST)
            .uri(uri)
            .header("content-type", format!("multipart/form-data; boundary={}", boundary))
            .body(hyper::body::Incoming::from(body.into_bytes()))
            .unwrap();

        let resp = handler.handle_request(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        
        // Verify response is JSON
        let (parts, body) = resp.into_parts();
        assert_eq!(parts.headers.get("content-type").unwrap(), "application/json");
        
        // Parse response body
        let body_bytes: Bytes = body.into();
        let metadata: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(metadata["tenant_id"], "tenant-1");
        assert_eq!(metadata["namespace"], "ns-1");
        assert_eq!(metadata["name"], "test.txt");
    }

    #[tokio::test]
    async fn test_http_upload_missing_fields() {
        let (service, _temp_dir) = create_test_service().await;
        let handler = plexspaces_blob::server::http::BlobHttpHandler::new(service.clone());

        let boundary = "----WebKitFormBoundary7MA4YWxkTrZu0gW";
        let body = format!(
            "--{}\r\n\
            Content-Disposition: form-data; name=\"file\"; filename=\"test.txt\"\r\n\r\n\
            Hello\r\n\
            --{}--\r\n",
            boundary, boundary
        );

        let uri: Uri = "/api/v1/blobs/upload".parse().unwrap();
        let req = Request::builder()
            .method(Method::POST)
            .uri(uri)
            .header("content-type", format!("multipart/form-data; boundary={}", boundary))
            .body(hyper::body::Incoming::from(body.into_bytes()))
            .unwrap();

        let resp = handler.handle_request(req).await;
        // Should fail due to missing tenant_id/namespace
        assert!(resp.is_err() || resp.unwrap().status() == StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_http_download_handler() {
        let (service, _temp_dir) = create_test_service().await;
        
        // First upload a blob
        let metadata = service
            .upload_blob(
                "tenant-1",
                "ns-1",
                "test.txt",
                b"Hello, World!".to_vec(),
                Some("text/plain".to_string()),
                None,
                None,
                std::collections::HashMap::new(),
                std::collections::HashMap::new(),
                None,
            )
            .await
            .unwrap();

        let handler = plexspaces_blob::server::http::BlobHttpHandler::new(service.clone());
        let uri: Uri = format!("/api/v1/blobs/{}/download/raw", metadata.blob_id).parse().unwrap();
        let req = Request::builder()
            .method(Method::GET)
            .uri(uri)
            .body(hyper::body::Incoming::from(vec![]))
            .unwrap();

        let resp = handler.handle_request(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        
        let (parts, body) = resp.into_parts();
        assert_eq!(parts.headers.get("content-type").unwrap(), "text/plain");
        assert!(parts.headers.get("content-disposition").is_some());
        
        let body_bytes: Bytes = body.into();
        assert_eq!(body_bytes.as_ref(), b"Hello, World!");
    }

    #[tokio::test]
    async fn test_http_download_not_found() {
        let (service, _temp_dir) = create_test_service().await;
        let handler = plexspaces_blob::server::http::BlobHttpHandler::new(service.clone());
        
        let uri: Uri = "/api/v1/blobs/nonexistent/download/raw".parse().unwrap();
        let req = Request::builder()
            .method(Method::GET)
            .uri(uri)
            .body(hyper::body::Incoming::from(vec![]))
            .unwrap();

        let resp = handler.handle_request(req).await;
        // Should fail with not found
        assert!(resp.is_err());
    }

    #[tokio::test]
    async fn test_http_unknown_route() {
        let (service, _temp_dir) = create_test_service().await;
        let handler = plexspaces_blob::server::http::BlobHttpHandler::new(service.clone());
        
        let uri: Uri = "/api/v1/unknown".parse().unwrap();
        let req = Request::builder()
            .method(Method::GET)
            .uri(uri)
            .body(hyper::body::Incoming::from(vec![]))
            .unwrap();

        let resp = handler.handle_request(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
}
