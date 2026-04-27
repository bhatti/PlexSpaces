-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Unified migration: Blob metadata (SQLite)

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
    expires_at INTEGER,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    updated_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
);
CREATE INDEX IF NOT EXISTS idx_blob_metadata_tenant_namespace ON blob_metadata(tenant_id, namespace);
CREATE INDEX IF NOT EXISTS idx_blob_metadata_sha256 ON blob_metadata(tenant_id, namespace, sha256);
CREATE INDEX IF NOT EXISTS idx_blob_metadata_expires_at ON blob_metadata(expires_at) WHERE expires_at IS NOT NULL;
