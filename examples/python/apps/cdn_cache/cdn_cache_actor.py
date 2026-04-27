# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2025 PlexSpaces Contributors
"""
CDN Cache Actor - Content Delivery with Blob Storage

Demonstrates blob storage for serving static assets with caching:
- Upload assets (images, CSS, JS)
- Download with cache headers
- List assets by prefix
- Delete expired content

Real-world use cases:
- Static asset serving (images, CSS, JS)
- User-uploaded content (profile pictures, documents)
- Media streaming (video thumbnails, audio files)
- Document management systems

## Blob Storage API Used

- blob_upload: Upload binary data with content type
- blob_download: Download binary data (base64 encoded)
- blob_delete: Delete a blob
- blob_list: List blobs by prefix

## CDN Patterns

Assets: "assets/{category}/{filename}"
Thumbnails: "thumbs/{size}/{filename}"
User content: "users/{user_id}/{filename}"
"""

import json
import base64
from plexspaces import actor, state, handler, init_handler, host


@actor
class CdnCache:
    """CDN cache actor using blob storage for static assets."""
    
    # Persistent state for tracking
    asset_count: int = state(default=0)
    total_bytes: int = state(default=0)
    cache_stats: dict = state(default_factory=dict)
    
    @init_handler
    def on_init(self, config: dict) -> None:
        """Initialize CDN cache."""
        self.asset_count = 0
        self.total_bytes = 0
        self.cache_stats = {"hits": 0, "misses": 0}
        host.log("info", "CdnCache initialized")
    
    @handler("upload")
    def upload_asset(self, path: str = "", data: str = "", content_type: str = "application/octet-stream") -> dict:
        """
        Upload an asset to blob storage.
        
        Args:
            path: Asset path (e.g., "assets/images/logo.png")
            data: Base64-encoded content
            content_type: MIME type (e.g., "image/png")
        
        Returns:
            Upload status with asset URL
        """
        if not path:
            return {"error": "path required"}
        if not data:
            return {"error": "data required"}
        
        # Upload to blob storage
        result = host.blob_upload(path, data, content_type)
        
        if result and result.startswith("ERROR"):
            return {"error": result}
        
        # Track stats
        self.asset_count += 1
        try:
            # Estimate size from base64
            self.total_bytes += len(base64.b64decode(data))
        except Exception:
            pass
        
        host.log("info", f"Uploaded asset: {path} ({content_type})")
        return {
            "status": "uploaded",
            "path": path,
            "content_type": content_type,
            "url": f"/cdn/{path}"
        }
    
    @handler("download")
    def download_asset(self, path: str = "") -> dict:
        """
        Download an asset from blob storage.
        
        Args:
            path: Asset path
        
        Returns:
            Asset data (base64) and content type
        """
        if not path:
            return {"error": "path required"}
        
        # Download from blob storage
        result = host.blob_download(path)
        
        if not result:
            self.cache_stats["misses"] = self.cache_stats.get("misses", 0) + 1
            return {"error": "not_found", "path": path}
        
        if result.startswith("ERROR"):
            return {"error": result}
        
        self.cache_stats["hits"] = self.cache_stats.get("hits", 0) + 1
        
        return {
            "status": "ok",
            "path": path,
            "data": result,
            "cache_control": "public, max-age=31536000"
        }
    
    @handler("delete")
    def delete_asset(self, path: str = "") -> dict:
        """
        Delete an asset from blob storage.
        
        Args:
            path: Asset path to delete
        
        Returns:
            Deletion status
        """
        if not path:
            return {"error": "path required"}
        
        result = host.blob_delete(path)
        
        if result and result.startswith("ERROR"):
            return {"error": result}
        
        if self.asset_count > 0:
            self.asset_count -= 1
        
        host.log("info", f"Deleted asset: {path}")
        return {"status": "deleted", "path": path}
    
    @handler("list")
    def list_assets(self, prefix: str = "") -> dict:
        """
        List assets with optional prefix filter.
        
        Args:
            prefix: Path prefix (e.g., "assets/images/")
        
        Returns:
            List of asset paths
        """
        result = host.blob_list(prefix)
        
        if result.startswith("ERROR"):
            return {"error": result}
        
        try:
            assets = json.loads(result)
            return {
                "prefix": prefix,
                "assets": assets,
                "count": len(assets)
            }
        except json.JSONDecodeError:
            return {"prefix": prefix, "assets": [], "count": 0}
    
    @handler("stats")
    def get_stats(self) -> dict:
        """Get CDN cache statistics."""
        total_requests = self.cache_stats.get("hits", 0) + self.cache_stats.get("misses", 0)
        hit_rate = 0.0
        if total_requests > 0:
            hit_rate = self.cache_stats.get("hits", 0) / total_requests * 100
        
        return {
            "asset_count": self.asset_count,
            "total_bytes": self.total_bytes,
            "cache_hits": self.cache_stats.get("hits", 0),
            "cache_misses": self.cache_stats.get("misses", 0),
            "hit_rate_percent": round(hit_rate, 2)
        }
    
    @handler("purge")
    def purge_prefix(self, prefix: str = "") -> dict:
        """
        Purge all assets with a prefix (cache invalidation).
        
        Args:
            prefix: Path prefix to purge
        
        Returns:
            Number of assets purged
        """
        if not prefix:
            return {"error": "prefix required for safety"}
        
        # List assets first
        list_result = host.blob_list(prefix)
        
        if list_result.startswith("ERROR"):
            return {"error": list_result}
        
        try:
            assets = json.loads(list_result)
        except json.JSONDecodeError:
            assets = []
        
        # Delete each asset
        deleted = 0
        for asset_path in assets:
            result = host.blob_delete(asset_path)
            if not result or not result.startswith("ERROR"):
                deleted += 1
                if self.asset_count > 0:
                    self.asset_count -= 1
        
        host.log("info", f"Purged {deleted} assets with prefix: {prefix}")
        return {
            "status": "purged",
            "prefix": prefix,
            "deleted_count": deleted
        }
