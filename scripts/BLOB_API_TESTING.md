# Blob Service API Testing Guide

This guide explains how to test the blob service APIs manually.

## Prerequisites

1. **MinIO Server** - S3-compatible object storage for testing
2. **Node Binary** - Built PlexSpaces node with blob service enabled
3. **curl** - For HTTP API testing
4. **jq** - For JSON parsing (optional but recommended)

## Quick Start

### 1. Start MinIO

```bash
./scripts/start-minio.sh
```

This will start MinIO in a Docker container with:
- API endpoint: `http://localhost:9000`
- Console: `http://localhost:9001`
- Access Key: `minioadmin_user`
- Secret Key: `minioadmin_pass`

### 2. Start Node with Blob Service

```bash
./scripts/start-node-with-blob.sh
```

Or with custom options:

```bash
./scripts/start-node-with-blob.sh \
  --minio-url http://localhost:9000 \
  --minio-key minioadmin_user \
  --minio-secret minioadmin_pass \
  --bucket plexspaces \
  --port 0.0.0.0:9000
```

The node will start with blob service enabled and listen on port 9000.

### 3. Run Automated Tests

```bash
./scripts/test-blob-api.sh
```

Or test against a different node:

```bash
./scripts/test-blob-api.sh http://localhost:9000
```

## Manual API Testing

### Upload a Blob (HTTP)

```bash
curl -X POST http://localhost:9000/api/v1/blobs/upload \
  -F "file=@/path/to/file.txt" \
  -F "tenant_id=tenant-1" \
  -F "namespace=namespace-1" \
  -F "content_type=text/plain" \
  -F "blob_group=test-group" \
  -F "kind=test-kind"
```

Response:
```json
{
  "blob_id": "01HZ...",
  "tenant_id": "tenant-1",
  "namespace": "namespace-1",
  "name": "file.txt",
  "content_type": "text/plain",
  "content_length": 1234,
  "sha256": "...",
  "created_at": "2025-01-15T10:00:00Z"
}
```

### Download a Blob (HTTP - Raw)

```bash
curl -X GET http://localhost:9000/api/v1/blobs/{blob_id}/download/raw \
  -o downloaded_file.txt
```

### Get Blob Metadata (gRPC-Gateway)

```bash
curl -X GET http://localhost:9000/api/v1/blobs/{blob_id}
```

Response:
```json
{
  "metadata": {
    "blob_id": "01HZ...",
    "tenant_id": "tenant-1",
    "namespace": "namespace-1",
    "name": "file.txt",
    "content_type": "text/plain",
    "content_length": 1234,
    "sha256": "...",
    "created_at": "2025-01-15T10:00:00Z"
  }
}
```

### List Blobs (gRPC-Gateway)

```bash
curl -X GET "http://localhost:9000/api/v1/blobs?tenant_id=tenant-1&namespace=namespace-1"
```

With filters:

```bash
curl -X GET "http://localhost:9000/api/v1/blobs?tenant_id=tenant-1&namespace=namespace-1&name_prefix=test&blob_group=test-group"
```

Response:
```json
{
  "blobs": [
    {
      "blob_id": "01HZ...",
      "name": "file1.txt",
      "content_type": "text/plain",
      "content_length": 1234
    }
  ],
  "page": {
    "total_size": 1,
    "next_page_token": ""
  }
}
```

### Delete a Blob (gRPC-Gateway)

```bash
curl -X DELETE http://localhost:9000/api/v1/blobs/{blob_id}
```

## API Endpoints Summary

### Plain HTTP Endpoints (for file operations)

| Method | Endpoint | Description |
|--------|----------|-------------|
| POST | `/api/v1/blobs/upload` | Upload a file (multipart/form-data) |
| GET | `/api/v1/blobs/{blob_id}/download/raw` | Download raw file data |

### gRPC-Gateway Endpoints (for metadata operations)

| Method | Endpoint | Description |
|--------|----------|-------------|
| POST | `/api/v1/blobs` | Upload blob (via gRPC) |
| GET | `/api/v1/blobs/{blob_id}` | Get blob metadata |
| GET | `/api/v1/blobs/{blob_id}/download` | Download blob (streaming) |
| GET | `/api/v1/blobs` | List blobs with filters |
| DELETE | `/api/v1/blobs/{blob_id}` | Delete blob |

## Environment Variables

The node can be configured via environment variables:

```bash
export BLOB_ENABLED=true
export BLOB_BACKEND=minio
export BLOB_ENDPOINT=http://localhost:9000
export BLOB_ACCESS_KEY_ID=minioadmin_user
export BLOB_SECRET_ACCESS_KEY=minioadmin_pass
export BLOB_BUCKET=plexspaces
export BLOB_PREFIX=/plexspaces
export BLOB_DATABASE_URL=sqlite:blob_metadata.db
```

## Troubleshooting

### MinIO Not Available

If MinIO is not running, the node will log a warning and disable blob service. Start MinIO first:

```bash
./scripts/start-minio.sh
```

### Port Already in Use

If port 9000 is already in use, change the node port:

```bash
./scripts/start-node-with-blob.sh --port 0.0.0.0:9001
```

Then update the test script:

```bash
./scripts/test-blob-api.sh http://localhost:9001
```

### Database Errors

If you see database errors, check the `BLOB_DATABASE_URL` environment variable. For SQLite, ensure the directory exists and is writable.

## Testing with gRPC Clients

You can also test using gRPC clients. The node supports both:
- Native gRPC protocol (port 9000)
- gRPC-Gateway (HTTP on port 9000)

Example using `grpcurl`:

```bash
# List services
grpcurl -plaintext localhost:9000 list

# Upload blob
grpcurl -plaintext -d '{
  "tenant_id": "tenant-1",
  "namespace": "namespace-1",
  "name": "test.txt",
  "data": "SGVsbG8gV29ybGQ="
}' localhost:9000 plexspaces.storage.v1.BlobService/UploadBlob
```
