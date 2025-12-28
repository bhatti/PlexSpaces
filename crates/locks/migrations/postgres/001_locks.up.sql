-- SPDX-License-Identifier: LGPL-2.1-or-later
-- Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
--
-- Locks table for PostgreSQL
--
-- ## Purpose
-- Provides distributed lock/lease coordination using PostgreSQL.
-- Supports optimistic locking with version checks and automatic expiration.
--
-- ## Design
-- - lock_key: Primary key (unique lock identifier)
-- - holder_id: Current lock holder (ULID)
-- - version: Optimistic locking version (incremented on each acquire)
-- - expires_at: Lock expiration timestamp (TIMESTAMPTZ for PostgreSQL)
-- - lease_duration_secs: Lease duration in seconds
-- - last_heartbeat: Last heartbeat timestamp (for lease renewal)
-- - locked: Boolean flag (1 = locked, 0 = unlocked)
-- - metadata: JSONB for extensibility (additional lock metadata)
--
-- ## Indexes
-- - idx_locks_expires_at: Find expired locks for cleanup
-- - idx_locks_holder: Find locks by holder (for cleanup on node failure)

CREATE TABLE IF NOT EXISTS locks (
    tenant_id TEXT NOT NULL DEFAULT 'default',
    namespace TEXT NOT NULL DEFAULT 'default',
    lock_key TEXT NOT NULL,
    holder_id TEXT NOT NULL,
    version TEXT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    lease_duration_secs INTEGER NOT NULL,
    last_heartbeat TIMESTAMPTZ NOT NULL,
    locked BOOLEAN NOT NULL DEFAULT FALSE,
    metadata JSONB,
    PRIMARY KEY (tenant_id, namespace, lock_key)
);

-- Index for finding expired locks (with tenant/namespace for efficient filtering)
CREATE INDEX IF NOT EXISTS idx_locks_expires_at
    ON locks(tenant_id, namespace, expires_at)
    WHERE locked = TRUE;

-- Index for finding locks by holder (with tenant/namespace for isolation)
CREATE INDEX IF NOT EXISTS idx_locks_holder
    ON locks(tenant_id, namespace, holder_id)
    WHERE locked = TRUE;

-- Index for tenant/namespace queries
CREATE INDEX IF NOT EXISTS idx_locks_tenant_namespace
    ON locks(tenant_id, namespace, lock_key)
    WHERE locked = TRUE;






















