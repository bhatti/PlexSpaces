-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Unified migration: KeyValue store (SQLite)

CREATE TABLE IF NOT EXISTS kv_store (
    tenant_id TEXT NOT NULL,
    namespace TEXT NOT NULL,
    key TEXT NOT NULL,
    value BLOB NOT NULL,
    expires_at BIGINT,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    PRIMARY KEY (tenant_id, namespace, key)
);
CREATE INDEX IF NOT EXISTS idx_kv_store_ttl_cleanup ON kv_store(tenant_id, namespace, expires_at, key) WHERE expires_at IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_kv_store_tenant_namespace ON kv_store(tenant_id, namespace, key);
