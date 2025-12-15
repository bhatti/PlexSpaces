# PlexSpaces Blob Storage Service

## Overview

S3-compatible blob storage service with metadata management for PlexSpaces. Supports multiple backends: S3, MinIO, GCP Cloud Storage, Azure Blob Storage.

## Proto-First Design

This crate follows **proto-first design** as per CLAUDE.md:

1. **Proto definitions**: `proto/plexspaces/v1/storage/blob.proto`
2. **Generated types**: After running `buf generate`, types are available in `plexspaces_proto::storage::v1::*`
3. **Implementation**: Uses generated proto types directly

## Architecture

- **Blob Storage**: Actual binary data stored in S3-compatible backend (via `object_store` crate)
- **Metadata Storage**: BlobMetadata stored in SQL (SQLite/PostgreSQL) for querying
- **Multi-tenancy**: Isolation via tenant_id and namespace
- **Path Structure**: `/plexspaces/{tenant_id}/{namespace}/{blob_id}`

## Setup

1. **Generate proto code** (required first):
   ```bash
   buf generate
   ```

2. **Build**:
   ```bash
   cargo build -p plexspaces-blob
   ```

## Usage

```rust
use plexspaces_blob::{BlobService, BlobConfig, BlobRepository};
use plexspaces_proto::storage::v1::{BlobMetadata, BlobConfig};

// Create config from proto
let config = BlobConfig {
    backend: "minio".to_string(),
    bucket: "plexspaces".to_string(),
    endpoint: Some("http://localhost:9000".to_string()),
    // ... other fields
};

// Create repository (SQL-based)
let repository = Arc::new(SqlBlobRepository::new(pool));

// Create service
let blob_service = BlobService::new(config, repository).await?;

// Upload a blob
let metadata = blob_service.upload_blob(
    "tenant-1",
    "namespace-1",
    "my-file.txt",
    b"file contents".to_vec(),
    None, // content_type
    None, // blob_group
    None, // kind
    std::collections::HashMap::new(), // metadata
    std::collections::HashMap::new(), // tags
    None, // expires_after
).await?;

// Download a blob
let data = blob_service.download_blob(&metadata.blob_id).await?;

// Generate presigned URL for direct client access
use chrono::Duration;
let presigned_url = blob_service
    .generate_presigned_url(&metadata.blob_id, "GET", Duration::hours(1))
    .await?;
// Client can now use this URL to download directly from storage
```

## Presigned URLs

Presigned URLs allow clients to access blobs directly from the storage backend (S3/MinIO) without going through the PlexSpaces server. This is useful for:

- **Direct Downloads**: Clients can download large files directly from storage
- **Direct Uploads**: Clients can upload files directly to storage
- **CDN Integration**: URLs can be used with CDNs for better performance
- **Temporary Access**: URLs expire after a specified duration (1 second to 7 days)

### Features

- ✅ **GET Operations**: Generate presigned URLs for downloading blobs
- ✅ **PUT Operations**: Generate presigned URLs for uploading/updating blobs
- ✅ **Configurable Expiration**: Set expiration from 1 second to 7 days (AWS S3 limit)
- ✅ **MinIO Support**: Works with MinIO and custom S3-compatible endpoints
- ✅ **AWS S3 Support**: Works with AWS S3

### Usage

```rust
use chrono::Duration;

// Generate presigned URL for GET (download)
let download_url = blob_service
    .generate_presigned_url(&blob_id, "GET", Duration::hours(1))
    .await?;

// Generate presigned URL for PUT (upload)
let upload_url = blob_service
    .generate_presigned_url(&blob_id, "PUT", Duration::minutes(30))
    .await?;
```

### API Endpoint

Via gRPC-Gateway (HTTP):
```bash
POST /api/v1/blobs/{blob_id}/presigned-url
Content-Type: application/json

{
  "blob_id": "01HZ...",
  "operation": "GET",  # or "PUT"
  "expires_after": {
    "seconds": 3600
  }
}
```

Response:
```json
{
  "url": "http://localhost:9001/plexspaces/...?X-Amz-Algorithm=...",
  "expires_at": "2025-01-15T11:00:00Z"
}
```

### Requirements

- The `presigned-urls` feature must be enabled (enabled by default in `plexspaces-node`)
- Access key ID and secret access key must be configured
- For MinIO: endpoint must be configured

## Configuration

Configuration can be provided via:
1. **Proto config** (from node config)
2. **Environment variables**:
   - `BLOB_BACKEND` (default: "minio")
   - `BLOB_BUCKET` (default: "plexspaces")
   - `BLOB_ENDPOINT` (required for MinIO)
   - `BLOB_ACCESS_KEY_ID` or `AWS_ACCESS_KEY_ID`
   - `BLOB_SECRET_ACCESS_KEY` or `AWS_SECRET_ACCESS_KEY`
   - `BLOB_PREFIX` (default: "/plexspaces")

## Testing

### Integration Tests

Integration tests require MinIO running at:
- Endpoint: `http://localhost:9001` (or `9000` as fallback)
- Console: `http://localhost:9002`
- Access Key: `minioadmin_user`
- Secret Key: `minioadmin_pass`

Run integration tests:
```bash
cargo test --package plexspaces-blob \
    --features "sql-backend,presigned-urls" \
    --test integration_tests
```

Or use the test script:
```bash
./scripts/test-blob-integration.sh
```

Tests will print a warning and skip if MinIO is unavailable.

### API Testing

See `scripts/BLOB_TESTING_GUIDE.md` for comprehensive testing instructions including:
- HTTP API testing
- gRPC-Gateway API testing
- Presigned URL testing
- End-to-end workflows
