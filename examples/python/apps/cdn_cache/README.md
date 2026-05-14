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
                                │ (embedded/S3)   │
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

### Prerequisites: Blob Storage

The PlexSpaces node auto-starts an embedded S3-compatible object store (`rustfs`) when no external endpoint is configured. No manual setup is required for local development.

To use a custom S3-compatible store, set `BLOB_ENDPOINT` before starting the node:

```bash
export BLOB_ENDPOINT=http://localhost:9000
export BLOB_BACKEND=embedded   # or s3, gcp, azure
export BLOB_BUCKET=plexspaces-blobs
```

### Build and Test

```bash
./build.sh  # Build WASM actor
./test.sh   # Run tests (requires PlexSpaces node)
```

### Start Node

```bash
# Terminal 1: Start node (embedded object store auto-starts)
./scripts/server.sh

# Terminal 2: Run tests
cd examples/python/apps/cdn_cache
./test.sh 8091
```

### Blob Configuration (release.yaml)

```yaml
runtime:
  blob:
    backend: embedded   # auto-starts rustfs; use s3/gcp/azure for cloud backends
    bucket: plexspaces-blobs
    endpoint: ""        # empty = auto-start embedded store
    region: ""
    access_key_id: ""
    secret_access_key: ""
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
4. **Scalable**: Backed by S3-compatible storage (embedded by default, or AWS S3/GCP/Azure)
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
