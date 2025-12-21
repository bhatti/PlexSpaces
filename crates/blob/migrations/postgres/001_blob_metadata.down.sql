-- SPDX-License-Identifier: LGPL-2.1-or-later
-- Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
--
-- Drop blob_metadata table and indexes

DROP INDEX IF EXISTS idx_blob_metadata_expires_at;
DROP INDEX IF EXISTS idx_blob_metadata_sha256;
DROP INDEX IF EXISTS idx_blob_metadata_tenant_namespace;
DROP TABLE IF EXISTS blob_metadata;








