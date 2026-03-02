-- SPDX-License-Identifier: LGPL-2.1-or-later
-- Unified migration: Locks (SQLite)

CREATE TABLE IF NOT EXISTS locks (
    tenant_id TEXT NOT NULL DEFAULT 'default',
    namespace TEXT NOT NULL DEFAULT 'default',
    lock_key TEXT NOT NULL,
    holder_id TEXT NOT NULL,
    version TEXT NOT NULL,
    expires_at INTEGER NOT NULL,
    lease_duration_secs INTEGER NOT NULL,
    last_heartbeat INTEGER NOT NULL,
    locked INTEGER NOT NULL DEFAULT 0,
    metadata TEXT,
    PRIMARY KEY (tenant_id, namespace, lock_key)
);
CREATE INDEX IF NOT EXISTS idx_locks_expires_at ON locks(tenant_id, namespace, expires_at) WHERE locked = 1;
CREATE INDEX IF NOT EXISTS idx_locks_holder ON locks(tenant_id, namespace, holder_id) WHERE locked = 1;
CREATE INDEX IF NOT EXISTS idx_locks_tenant_namespace ON locks(tenant_id, namespace, lock_key) WHERE locked = 1;
