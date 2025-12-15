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
```

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

## MinIO Integration Tests

Integration tests require MinIO running at:
- Endpoint: `http://localhost:9000`
- Console: `http://localhost:9001`
- Access Key: `minioadmin_user`
- Secret Key: `minioadmin_pass`

Tests will print a warning and skip if MinIO is unavailable.
