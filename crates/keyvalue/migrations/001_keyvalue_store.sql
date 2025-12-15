-- SPDX-License-Identifier: LGPL-2.1-or-later
-- Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

-- Key-Value Store
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
-- DELETE FROM keyvalue_store WHERE expires_at < CURRENT_TIMESTAMP;

CREATE TABLE IF NOT EXISTS keyvalue_store (
    key TEXT PRIMARY KEY,
    value_blob BLOB NOT NULL,
    content_type TEXT,           -- e.g., "application/json", "application/protobuf"
    metadata_json TEXT,           -- EXTENSIBLE: namespace, tags, checksum, compression, etc.
    version INTEGER NOT NULL DEFAULT 1,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    expires_at TIMESTAMP          -- NULL = no expiration
);

-- Index for TTL cleanup
CREATE INDEX IF NOT EXISTS idx_keyvalue_expires
    ON keyvalue_store(expires_at)
    WHERE expires_at IS NOT NULL;

-- Index for version-based optimistic locking
CREATE INDEX IF NOT EXISTS idx_keyvalue_version
    ON keyvalue_store(version);
