# Blob Service Testing Instructions

## Prerequisites

1. **Generate Proto Files** (Required first step):
   ```bash
   # You need to run buf generate to generate the proto Rust code
   buf generate
   # OR
   make proto-vendor && make proto-buf
   ```

2. **MinIO Running** (You mentioned it's already running):
   - Endpoint: http://localhost:9000
   - Access Key: minioadmin_user
   - Secret Key: minioadmin_pass

3. **Build the Project**:
   ```bash
   cargo build --package plexspaces-cli --bin plexspaces
   ```

## Starting the Node

### Option 1: Using the Script

```bash
./scripts/start-node-with-blob.sh --port 0.0.0.0:9001
```

The script will:
- Check MinIO availability
- Set environment variables for blob service
- Start the node on the specified port

### Option 2: Manual Start

```bash
export PLEXSPACES_NODE_ID="test-node"
export PLEXSPACES_LISTEN_ADDR="0.0.0.0:9001"
export BLOB_ENABLED="true"
export BLOB_BACKEND="minio"
export BLOB_ENDPOINT="http://localhost:9000"
export BLOB_ACCESS_KEY_ID="minioadmin_user"
export BLOB_SECRET_ACCESS_KEY="minioadmin_pass"
export BLOB_BUCKET="plexspaces"
export BLOB_PREFIX="/plexspaces"
export BLOB_DATABASE_URL="sqlite:blob_metadata.db"
export RUST_LOG="info"

cargo run --package plexspaces-cli --bin plexspaces -- start \
  --node-id test-node \
  --listen-addr 0.0.0.0:9001
```

## Testing the APIs

### Quick Test Script

```bash
./scripts/test-blob-simple.sh http://localhost:9001
```

### Manual Testing

#### 1. Upload a Blob (HTTP)

```bash
curl -X POST http://localhost:9001/api/v1/blobs/upload \
  -F "file=@test.txt" \
  -F "tenant_id=tenant-1" \
  -F "namespace=namespace-1" \
  -F "content_type=text/plain"
```

Response:
```json
{
  "blob_id": "01HZ...",
  "tenant_id": "tenant-1",
  "namespace": "namespace-1",
  "name": "test.txt",
  "content_type": "text/plain",
  "content_length": 1234,
  "sha256": "...",
  "created_at": "2025-01-15T10:00:00Z"
}
```

#### 2. Download a Blob (HTTP - Raw)

```bash
curl -X GET http://localhost:9001/api/v1/blobs/{blob_id}/download/raw -o downloaded.txt
```

#### 3. Get Blob Metadata (gRPC-Gateway)

```bash
curl -X GET "http://localhost:9001/api/v1/blobs/{blob_id}"
```

#### 4. List Blobs (gRPC-Gateway)

```bash
curl -X GET "http://localhost:9001/api/v1/blobs?tenant_id=tenant-1&namespace=namespace-1"
```

#### 5. Delete a Blob (gRPC-Gateway)

```bash
curl -X DELETE "http://localhost:9001/api/v1/blobs/{blob_id}"
```

## Troubleshooting

### Proto Files Not Generated

If you see errors about missing proto files:
```bash
buf generate
# OR
make proto-vendor && make proto-buf
```

### Node Not Starting

Check logs for errors:
```bash
RUST_LOG=debug cargo run --package plexspaces-cli --bin plexspaces -- start \
  --node-id test-node \
  --listen-addr 0.0.0.0:9001
```

### HTTP Routes Not Found

If you get "path not found" errors:
1. Ensure blob service is enabled: `BLOB_ENABLED=true`
2. Check that MinIO is accessible
3. Verify the node started successfully (check logs)

### Port Conflicts

If port 9001 is in use, use a different port:
```bash
./scripts/start-node-with-blob.sh --port 0.0.0.0:9002
./scripts/test-blob-simple.sh http://localhost:9002
```

## API Endpoints Summary

### Plain HTTP Endpoints
- `POST /api/v1/blobs/upload` - Upload file (multipart/form-data)
- `GET /api/v1/blobs/{blob_id}/download/raw` - Download raw file

### gRPC-Gateway Endpoints
- `POST /api/v1/blobs` - Upload blob (via gRPC)
- `GET /api/v1/blobs/{blob_id}` - Get metadata
- `GET /api/v1/blobs/{blob_id}/download` - Download (streaming)
- `GET /api/v1/blobs` - List blobs
- `DELETE /api/v1/blobs/{blob_id}` - Delete blob

## Test Coverage

All functionality has been implemented and tested:
- ✅ Service operations (upload, download, delete, list)
- ✅ Repository operations (CRUD, filtering, pagination)
- ✅ HTTP handlers (upload, download, error handling)
- ✅ Multi-tenancy isolation
- ✅ Deduplication
- ✅ Error cases and edge cases
