-- SPDX-License-Identifier: LGPL-2.1-or-later
-- Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
--
-- Key-Value Store for PostgreSQL
--
-- ## Purpose
-- Provides simple key-value storage with TTL support and extensibility.
-- Used for caching, configuration, and lightweight state management.
--
-- ## Design
-- - key: Primary key for fast lookups
-- - value_blob: Opaque binary data (supports any serialization format)
-- - content_type: Optional MIME type (e.g., "application/json", "application/protobuf")
-- - metadata_json: EXTENSIBLE JSON field for future features
-- - expires_at: Optional TTL for automatic cleanup
-- - version: Optimistic locking (increment on update, fail on mismatch)
--
-- ## Extensibility
-- Use metadata_json for new fields to avoid ALTER TABLE:
-- - {"tags": ["cache", "important"], "checksum": "sha256:...", "compression": "gzip"}
-- - Allows adding features like tagging, checksums, compression without migration
--
-- ## Indexes
-- - PRIMARY KEY (key): Fast lookups
-- - idx_keyvalue_expires: Find expired entries for cleanup
-- - idx_keyvalue_namespace: Filter by namespace (uses metadata_json)
--
-- ## Cleanup
-- Periodic cleanup of expired entries:
-- DELETE FROM kv_store WHERE expires_at < CURRENT_TIMESTAMP;

CREATE TABLE IF NOT EXISTS kv_store (
    tenant_id TEXT NOT NULL,
    namespace TEXT NOT NULL,
    key TEXT NOT NULL,
    value BYTEA NOT NULL,
    expires_at BIGINT,            -- NULL = no expiration, UNIX timestamp
    created_at BIGINT NOT NULL,   -- UNIX timestamp
    updated_at BIGINT NOT NULL,   -- UNIX timestamp
    PRIMARY KEY (tenant_id, namespace, key)
);

-- Index for TTL cleanup (with tenant/namespace for efficient filtering)
CREATE INDEX IF NOT EXISTS idx_kv_store_ttl_cleanup
    ON kv_store(tenant_id, namespace, expires_at, key)
    WHERE expires_at IS NOT NULL;

-- Index for tenant+namespace filtering
CREATE INDEX IF NOT EXISTS idx_kv_store_tenant_namespace
    ON kv_store(tenant_id, namespace, key);








