-- SPDX-License-Identifier: LGPL-2.1-or-later
-- Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
--
-- Migration: Create blob_metadata table for blob storage service (SQLite)

CREATE TABLE IF NOT EXISTS blob_metadata (
    blob_id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    namespace TEXT NOT NULL,
    name TEXT NOT NULL,
    sha256 TEXT NOT NULL,
    content_type TEXT,
    content_length INTEGER NOT NULL,
    etag TEXT,
    blob_group TEXT,
    kind TEXT,
    metadata_json TEXT,
    tags_json TEXT,
    expires_at INTEGER,  -- UNIX timestamp
    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),  -- UNIX timestamp
    updated_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))   -- UNIX timestamp
);

-- Indexes for efficient querying
CREATE INDEX IF NOT EXISTS idx_blob_metadata_tenant_namespace
ON blob_metadata(tenant_id, namespace);

CREATE INDEX IF NOT EXISTS idx_blob_metadata_sha256
ON blob_metadata(tenant_id, namespace, sha256);

CREATE INDEX IF NOT EXISTS idx_blob_metadata_expires_at
ON blob_metadata(expires_at)
WHERE expires_at IS NOT NULL;





