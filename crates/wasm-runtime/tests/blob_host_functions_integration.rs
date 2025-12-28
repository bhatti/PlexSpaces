// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// Integration tests for WASM blob host functions
// Validates that all blob operations are available via WASM and working correctly
//
// NOTE: These tests are designed to run offline without network access or SSL.
// All tests use in-memory blob storage (LocalFileSystem with temp directory)
// and do not require external services or network connectivity.

#[cfg(feature = "component-model")]
mod tests {
    use plexspaces_wasm_runtime::component_host::BlobImpl;
    use plexspaces_wasm_runtime::component_host::plexspaces::actor::blob::Host as BlobHost;
    use plexspaces_wasm_runtime::component_host::plexspaces::actor::types::Context;
    use plexspaces_core::ActorId;
    use plexspaces_blob::{BlobService, repository::sql::SqlBlobRepository};
    use plexspaces_proto::storage::v1::BlobConfig as ProtoBlobConfig;
    use std::sync::Arc;
    use plexspaces_wasm_runtime::HostFunctions;
    use tempfile::{TempDir, NamedTempFile};
    use object_store::local::LocalFileSystem;

    async fn create_test_blob_service() -> (Arc<BlobService>, TempDir, NamedTempFile) {
        // Install sqlx::any default drivers before any database operations
        // This is required for AnyPool to work with sqlite
        sqlx::any::install_default_drivers();
        
        // Create temp directory for local filesystem
        let temp_dir = TempDir::new().unwrap();
        let local_store = Arc::new(LocalFileSystem::new_with_prefix(temp_dir.path()).unwrap());

        // Create SQLite repository - use temporary file-based database for reliability
        // This is more production-grade than in-memory and works correctly with connection pools
        use sqlx::AnyPool;
        use sqlx::any::AnyPoolOptions;
        use tempfile::NamedTempFile;
        
        // Create a temporary file for the SQLite database
        let temp_db = NamedTempFile::new().unwrap();
        let db_path = temp_db.path().to_str().unwrap();
        let db_url = format!("sqlite:{}", db_path);
        
        // Create pool with reasonable connection limit
        let any_pool = AnyPoolOptions::new()
            .max_connections(5)
            .connect(&db_url)
            .await
            .unwrap();
        
        // Auto-apply migrations using new() - migrations are automatically applied
        let repository = Arc::new(SqlBlobRepository::new(any_pool).await
            .expect("Failed to create blob repository with migrations"));

        // Create blob config
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

        // Create service with custom object store (for testing)
        let service = BlobService::with_object_store(config, local_store, repository);
        (Arc::new(service), temp_dir, temp_db)
    }

    fn create_test_host_functions_with_blob(blob_service: Arc<BlobService>) -> Arc<HostFunctions> {
        Arc::new(HostFunctions::with_all_services(
            None, // message_sender
            None, // channel_service
            None, // keyvalue_store
            None, // process_group_registry
            None, // lock_manager
            None, // object_registry
            None, // journal_storage
            Some(blob_service), // blob_service
        ))
    }

    fn create_test_blob_impl(host_functions: Arc<HostFunctions>) -> BlobImpl {
        BlobImpl {
            actor_id: ActorId::from("test-actor".to_string()),
            host_functions,
        }
    }

    // Helper to create context for tests
    fn test_context(tenant_id: &str, namespace: &str) -> Context {
        Context {
            tenant_id: tenant_id.to_string(),
            namespace: namespace.to_string(),
        }
    }

    #[tokio::test]
    async fn test_blob_upload() {
        let (_service, _temp_dir, _temp_db) = create_test_blob_service().await;
        let host_functions = create_test_host_functions_with_blob(_service);
        let mut blob_impl = create_test_blob_impl(host_functions);

        let data = b"Hello, World!".to_vec();
        let result = blob_impl
            .upload(
                test_context("test-tenant", "test-namespace"),
                "test-bucket".to_string(),
                "test-key.txt".to_string(),
                data.clone(),
                Some("text/plain".to_string()),
            )
            .await;

        assert!(result.is_ok());
        let metadata = result.unwrap();
        assert_eq!(metadata.bucket, "test-bucket");
        assert_eq!(metadata.key, "test-key.txt");
        assert_eq!(metadata.size, data.len() as u64);
        assert_eq!(metadata.content_type, Some("text/plain".to_string()));
        assert!(metadata.blob_id.len() > 0);
    }

    #[tokio::test]
    async fn test_blob_download() {
        let (service, _temp_dir, _temp_db) = create_test_blob_service().await;
        let host_functions = create_test_host_functions_with_blob(service.clone());
        let mut blob_impl = create_test_blob_impl(host_functions);

        // First upload a blob
        let data = b"Test download data".to_vec();
        let upload_result = blob_impl
            .upload(
                test_context("test-tenant", "test-namespace"),
                "test-bucket".to_string(),
                "download-test.txt".to_string(),
                data.clone(),
                Some("text/plain".to_string()),
            )
            .await
            .unwrap();

        // Now download it (pass tenant/namespace from upload)
        let download_result = blob_impl
            .download(
                test_context("test-tenant", "test-namespace"),
                upload_result.blob_id.clone(),
                "test-bucket".to_string(),
                "download-test.txt".to_string(),
            )
            .await;

        assert!(download_result.is_ok(), "Download failed: {:?}", download_result.as_ref().err());
        let downloaded_data = download_result.unwrap();
        assert_eq!(downloaded_data, data);
    }

    #[tokio::test]
    async fn test_blob_delete() {
        let (service, _temp_dir, _temp_db) = create_test_blob_service().await;
        let host_functions = create_test_host_functions_with_blob(service.clone());
        let mut blob_impl = create_test_blob_impl(host_functions);

        // Upload a blob
        let data = b"Test delete data".to_vec();
        let upload_result = blob_impl
            .upload(
                test_context("test-tenant", "test-namespace"),
                "test-bucket".to_string(),
                "delete-test.txt".to_string(),
                data,
                Some("text/plain".to_string()),
            )
            .await
            .unwrap();

        // Delete it (pass tenant/namespace from upload)
        let delete_result = blob_impl
            .delete(
                test_context("test-tenant", "test-namespace"),
                upload_result.blob_id.clone(),
                "test-bucket".to_string(),
                "delete-test.txt".to_string(),
            )
            .await;

        assert!(delete_result.is_ok(), "Delete failed: {:?}", delete_result.as_ref().err());

        // Verify it's deleted by trying to download
        let download_result = blob_impl
            .download(
                test_context("test-tenant", "test-namespace"),
                upload_result.blob_id,
                "test-bucket".to_string(),
                "delete-test.txt".to_string(),
            )
            .await;

        assert!(download_result.is_err());
    }

    #[tokio::test]
    async fn test_blob_exists() {
        let (service, _temp_dir, _temp_db) = create_test_blob_service().await;
        let host_functions = create_test_host_functions_with_blob(service.clone());
        let mut blob_impl = create_test_blob_impl(host_functions);

        // Upload a blob
        let data = b"Test exists data".to_vec();
        let upload_result = blob_impl
            .upload(
                test_context("test-tenant", "test-namespace"),
                "test-bucket".to_string(),
                "exists-test.txt".to_string(),
                data,
                Some("text/plain".to_string()),
            )
            .await
            .unwrap();

        // Check it exists (pass tenant/namespace from upload)
        let exists_result = blob_impl
            .exists(
                test_context("test-tenant", "test-namespace"),
                upload_result.blob_id.clone(),
                "test-bucket".to_string(),
                "exists-test.txt".to_string(),
            )
            .await;

        assert!(exists_result.is_ok(), "Exists check failed: {:?}", exists_result.as_ref().err());
        assert_eq!(exists_result.unwrap(), true, "Blob should exist after upload");

        // Check non-existent blob
        let not_exists_result = blob_impl
            .exists(
                test_context("test-tenant", "test-namespace"),
                "non-existent-id".to_string(),
                "test-bucket".to_string(),
                "non-existent.txt".to_string(),
            )
            .await;

        assert!(not_exists_result.is_ok());
        assert_eq!(not_exists_result.unwrap(), false);
    }

    #[tokio::test]
    async fn test_blob_list_blobs() {
        let (service, _temp_dir, _temp_db) = create_test_blob_service().await;
        let host_functions = create_test_host_functions_with_blob(service.clone());
        let mut blob_impl = create_test_blob_impl(host_functions);

        // Upload multiple blobs with different prefixes
        let data1 = b"File 1".to_vec();
        blob_impl
            .upload(
                test_context("test-tenant", "test-namespace"),
                "test-bucket".to_string(),
                "prefix1/file1.txt".to_string(),
                data1,
                Some("text/plain".to_string()),
            )
            .await
            .unwrap();

        let data2 = b"File 2".to_vec();
        blob_impl
            .upload(
                test_context("test-tenant", "test-namespace"),
                "test-bucket".to_string(),
                "prefix1/file2.txt".to_string(),
                data2,
                Some("text/plain".to_string()),
            )
            .await
            .unwrap();

        let data3 = b"File 3".to_vec();
        blob_impl
            .upload(
                test_context("test-tenant", "test-namespace"),
                "test-bucket".to_string(),
                "prefix2/file3.txt".to_string(),
                data3,
                Some("text/plain".to_string()),
            )
            .await
            .unwrap();

        // List with prefix1
        let list_result = blob_impl
            .list_blobs(
                test_context("test-tenant", "test-namespace"),
                "test-bucket".to_string(),
                "prefix1/".to_string(),
                10,
            )
            .await;

        assert!(list_result.is_ok());
        let blobs = list_result.unwrap();
        assert_eq!(blobs.len(), 2);
        assert!(blobs.iter().any(|b| b.key == "prefix1/file1.txt"));
        assert!(blobs.iter().any(|b| b.key == "prefix1/file2.txt"));
    }

    #[tokio::test]
    async fn test_blob_metadata() {
        let (service, _temp_dir, _temp_db) = create_test_blob_service().await;
        let host_functions = create_test_host_functions_with_blob(service.clone());
        let mut blob_impl = create_test_blob_impl(host_functions);

        // Upload a blob
        let data = b"Test metadata data".to_vec();
        let upload_result = blob_impl
            .upload(
                test_context("test-tenant", "test-namespace"),
                "test-bucket".to_string(),
                "metadata-test.txt".to_string(),
                data.clone(),
                Some("text/plain".to_string()),
            )
            .await
            .unwrap();

        // Get metadata (pass tenant/namespace from upload)
        let metadata_result = blob_impl
            .metadata(
                test_context("test-tenant", "test-namespace"),
                upload_result.blob_id.clone(),
                "test-bucket".to_string(),
                "metadata-test.txt".to_string(),
            )
            .await;

        assert!(metadata_result.is_ok(), "Get metadata failed: {:?}", metadata_result.as_ref().err());
        let metadata = metadata_result.unwrap();
        assert_eq!(metadata.blob_id, upload_result.blob_id);
        assert_eq!(metadata.bucket, "test-bucket");
        assert_eq!(metadata.key, "metadata-test.txt");
        assert_eq!(metadata.size, data.len() as u64);
        assert_eq!(metadata.content_type, Some("text/plain".to_string()));
    }

    #[tokio::test]
    async fn test_blob_copy() {
        let (service, _temp_dir, _temp_db) = create_test_blob_service().await;
        let host_functions = create_test_host_functions_with_blob(service.clone());
        let mut blob_impl = create_test_blob_impl(host_functions);

        // Upload source blob
        let data = b"Test copy data".to_vec();
        let upload_result = blob_impl
            .upload(
                test_context("test-tenant", "test-namespace"),
                "test-bucket".to_string(),
                "source.txt".to_string(),
                data.clone(),
                Some("text/plain".to_string()),
            )
            .await
            .unwrap();

        // Copy to destination (pass tenant/namespace from upload)
        let copy_result = blob_impl
            .copy(
                test_context("test-tenant", "test-namespace"),
                upload_result.blob_id.clone(),
                "test-bucket".to_string(),
                "source.txt".to_string(),
                "test-bucket".to_string(),
                "dest.txt".to_string(),
            )
            .await;

        assert!(copy_result.is_ok(), "Copy failed: {:?}", copy_result.as_ref().err());
        let dest_metadata = copy_result.unwrap();
        assert_eq!(dest_metadata.bucket, "test-bucket");
        assert_eq!(dest_metadata.key, "dest.txt");
        assert_eq!(dest_metadata.size, data.len() as u64);

        // Verify destination exists and has same content
        let download_result = blob_impl
            .download(
                test_context("test-tenant", "test-namespace"),
                dest_metadata.blob_id,
                "test-bucket".to_string(),
                "dest.txt".to_string(),
            )
            .await;

        assert!(download_result.is_ok());
        assert_eq!(download_result.unwrap(), data);
    }

    #[tokio::test]
    async fn test_blob_upload_without_service() {
        // Test error handling when blob service is not configured
        let host_functions = Arc::new(HostFunctions::with_all_services(
            None, None, None, None, None, None, None, None, // No blob_service
        ));
        let mut blob_impl = create_test_blob_impl(host_functions);

        let data = b"Test data".to_vec();
        let result = blob_impl
            .upload(
                test_context("test-tenant", "test-namespace"),
                "test-bucket".to_string(),
                "test-key.txt".to_string(),
                data,
                Some("text/plain".to_string()),
            )
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_blob_download_nonexistent() {
        let (service, _temp_dir, _temp_db) = create_test_blob_service().await;
        let host_functions = create_test_host_functions_with_blob(service);
        let mut blob_impl = create_test_blob_impl(host_functions);

        // Try to download non-existent blob
        let result = blob_impl
            .download(
                test_context("test-tenant", "test-namespace"),
                "non-existent-id".to_string(),
                "test-bucket".to_string(),
                "non-existent.txt".to_string(),
            )
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_blob_list_empty() {
        let (service, _temp_dir, _temp_db) = create_test_blob_service().await;
        let host_functions = create_test_host_functions_with_blob(service);
        let mut blob_impl = create_test_blob_impl(host_functions);

        // List with no blobs
        let list_result = blob_impl
            .list_blobs(
                test_context("test-tenant", "test-namespace"),
                "test-bucket".to_string(),
                "nonexistent/".to_string(),
                10,
            )
            .await;

        assert!(list_result.is_ok());
        let blobs = list_result.unwrap();
        assert_eq!(blobs.len(), 0);
    }
}

