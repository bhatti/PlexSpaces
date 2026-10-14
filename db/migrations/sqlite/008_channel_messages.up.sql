-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Unified migration: Channel messages (SQLite)
-- Tenant isolation: (tenant_id, namespace, channel_name) mirrors kv_store schema.

CREATE TABLE IF NOT EXISTS channel_messages (
    id                  TEXT    PRIMARY KEY,
    tenant_id           TEXT    NOT NULL DEFAULT '',
    namespace           TEXT    NOT NULL DEFAULT '',
    channel_name        TEXT    NOT NULL,
    payload             BLOB    NOT NULL,
    headers_json        TEXT    NOT NULL DEFAULT '{}',
    idempotency_key     TEXT    NOT NULL DEFAULT '',
    conflict_resolution INTEGER NOT NULL DEFAULT 0,
    timestamp           INTEGER NOT NULL,
    acked               INTEGER NOT NULL DEFAULT 0,
    dead_lettered       INTEGER NOT NULL DEFAULT 0,
    delivery_count      INTEGER NOT NULL DEFAULT 0,
    send_at             INTEGER NOT NULL DEFAULT 0,
    created_at          INTEGER NOT NULL
);

-- Hot path: tenant+namespace-scoped poll of pending unacked messages eligible for delivery.
-- send_at in leading position so the relay query (send_at <= ? OR send_at = 0) uses the index.
CREATE INDEX IF NOT EXISTS idx_channel_unacked
    ON channel_messages(tenant_id, namespace, channel_name, send_at, created_at)
    WHERE acked = 0 AND dead_lettered = 0;

-- Idempotency OVERWRITE: find+delete existing queued key in O(log n), tenant-scoped
CREATE INDEX IF NOT EXISTS idx_channel_idempotency
    ON channel_messages(tenant_id, namespace, channel_name, idempotency_key)
    WHERE idempotency_key != '' AND acked = 0;

-- DLQ monitoring queries, tenant-scoped
CREATE INDEX IF NOT EXISTS idx_channel_dlq
    ON channel_messages(tenant_id, namespace, channel_name, dead_lettered)
    WHERE dead_lettered = 1;
