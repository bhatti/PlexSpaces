# CDN Cache - Blob Storage for Static Assets (Python WASM with SDK)

Demonstrates **blob storage** for serving static assets with caching capabilities.

**Real-world use cases**:
- Static asset serving (images, CSS, JS)
- User-uploaded content (profile pictures, documents)
- Media streaming (video thumbnails, audio files)
- Document management systems

## Blob Storage Pattern

```
┌─────────────┐     upload      ┌─────────────────┐
│   Client    │ ───────────────►│   CDN Cache     │
│  (upload)   │                 │    (actor)      │
└─────────────┘                 └────────┬────────┘
                                         │ blob_upload
                                         ▼
                                ┌─────────────────┐
                                │  Blob Storage   │
                                │  (S3/MinIO)     │
                                └────────┬────────┘
                                         │ blob_download
                                         ▼
┌─────────────┐     download    ┌─────────────────┐
│   Client    │ ◄───────────────│   CDN Cache     │
│  (fetch)    │   base64 data   │   (hit/miss)    │
└─────────────┘                 └─────────────────┘
```

## Blob Storage APIs Used

| API | Usage | Description |
|-----|-------|-------------|
| `blob_upload` | Store assets | Upload binary data with content type |
| `blob_download` | Retrieve assets | Download as base64-encoded string |
| `blob_delete` | Remove assets | Delete individual blobs |
| `blob_list` | Browse assets | List blobs by prefix |

## Asset Path Patterns

| Pattern | Example | Use Case |
|---------|---------|----------|
| `assets/{category}/{file}` | `assets/images/logo.png` | Static website assets |
| `thumbs/{size}/{file}` | `thumbs/128x128/photo.jpg` | Generated thumbnails |
| `users/{user_id}/{file}` | `users/u123/avatar.png` | User uploads |
| `docs/{org}/{file}` | `docs/acme/report.pdf` | Organization documents |

## Quick Start

### Prerequisites: MinIO for Blob Storage

```bash
# Start MinIO (S3-compatible object storage)
docker run -d \
  -p 9000:9000 \
  -p 9090:9090 \
  --name minio_server \
  -e MINIO_ROOT_USER=minioadmin \
  -e MINIO_ROOT_PASSWORD=minioadmin \
  -v ./data:/data \
  quay.io/minio/minio server /data --console-address :9090

# Create bucket "plexspaces-blobs" via MinIO Console:
#   1. Open http://localhost:9090 in browser
#   2. Login with minioadmin / minioadmin
#   3. Click "Buckets" → "Create Bucket"
#   4. Name: plexspaces-blobs → Click "Create Bucket"
#
# Or use AWS CLI if installed:
# AWS_ACCESS_KEY_ID=minioadmin AWS_SECRET_ACCESS_KEY=minioadmin \
#   aws --endpoint-url http://localhost:9000 s3 mb s3://plexspaces-blobs
```

### Build and Test

```bash
./build.sh  # Build WASM actor
./test.sh   # Run tests (requires PlexSpaces node)
```

### Start Node

```bash
# Terminal 1: Start node (release.yaml has MinIO config)
./scripts/server.sh

# Terminal 2: Run tests
cd examples/python/apps/cdn_cache
./test.sh 8092
```

### MinIO Configuration (release.yaml)

```yaml
runtime:
  blob:
    storage_type: s3
    bucket_name: plexspaces-blobs
    endpoint: http://localhost:9000
    region: us-east-1
    access_key_id: minioadmin
    secret_access_key: minioadmin
    force_path_style: true
```

## Operations

| Operation | Payload | Description |
|-----------|---------|-------------|
| `upload` | `{"path":"assets/logo.png","data":"<base64>","content_type":"image/png"}` | Upload asset |
| `download` | `{"path":"assets/logo.png"}` | Download asset |
| `delete` | `{"path":"assets/logo.png"}` | Delete asset |
| `list` | `{"prefix":"assets/images/"}` | List assets by prefix |
| `stats` | `{}` | Get cache statistics |
| `purge` | `{"prefix":"assets/css/"}` | Delete all assets with prefix |

## Example: Upload and Download

```python
# Upload an image (data must be base64 encoded)
import base64
data = base64.b64encode(open("logo.png", "rb").read()).decode()
upload(path="assets/images/logo.png", data=data, content_type="image/png")

# Download the image
result = download(path="assets/images/logo.png")
image_bytes = base64.b64decode(result["data"])
```

## Cache Headers

Downloads include cache-control headers for CDN integration:

```json
{
  "status": "ok",
  "path": "assets/logo.png",
  "data": "<base64>",
  "cache_control": "public, max-age=31536000"
}
```

## SDK Features Demonstrated

| Feature | How It's Used |
|---------|---------------|
| `@actor` | CDN cache service |
| `state()` | Track asset_count, total_bytes, cache_stats |
| `@handler()` | upload, download, delete, list, stats, purge |
| `host.blob_upload()` | Store binary data |
| `host.blob_download()` | Retrieve binary data |
| `host.blob_delete()` | Remove data |
| `host.blob_list()` | List by prefix |

## Why Blob Storage for CDN?

1. **Binary data**: Store images, videos, documents natively
2. **Content types**: Preserve MIME types for proper serving
3. **Prefix listing**: Organize assets hierarchically
4. **Scalable**: Backed by S3-compatible storage (MinIO)
5. **Durable**: Assets persist across actor restarts

## Files

| File | Description |
|------|-------------|
| `cdn_cache_actor.py` | CDN cache using blob storage |
| `build.sh` | Build using `plexspaces-py build` |
| `test.sh` | Integration test |

## See Also

- [PlexSpaces Python SDK](../../../../sdks/python/README.md) - SDK documentation
- [SDK Guide](../../../../docs/sdk.md) - Blob Storage API reference
- [Payment Handler Example](../payment_handler/) - KV storage example
- [Job Processing Example](../job_processing/) - TupleSpace example
