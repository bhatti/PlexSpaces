-- SPDX-License-Identifier: LGPL-2.1-or-later
-- Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
--
-- Locks table for SQLite
--
-- ## Purpose
-- Provides distributed lock/lease coordination using SQLite.
-- Supports optimistic locking with version checks and automatic expiration.
--
-- ## Design
-- - lock_key: Primary key (unique lock identifier)
-- - holder_id: Current lock holder (ULID)
-- - version: Optimistic locking version (incremented on each acquire)
-- - expires_at: Lock expiration timestamp (UNIX epoch seconds for SQLite)
-- - lease_duration_secs: Lease duration in seconds
-- - last_heartbeat: Last heartbeat timestamp (UNIX epoch seconds)
-- - locked: Boolean flag (1 = locked, 0 = unlocked)
-- - metadata: JSON text for extensibility (additional lock metadata)
--
-- ## Indexes
-- - idx_locks_expires_at: Find expired locks for cleanup
-- - idx_locks_holder: Find locks by holder (for cleanup on node failure)

CREATE TABLE IF NOT EXISTS locks (
    lock_key TEXT PRIMARY KEY,
    holder_id TEXT NOT NULL,
    version TEXT NOT NULL,
    expires_at INTEGER NOT NULL,
    lease_duration_secs INTEGER NOT NULL,
    last_heartbeat INTEGER NOT NULL,
    locked INTEGER NOT NULL DEFAULT 0,
    metadata TEXT
);

-- Index for finding expired locks
CREATE INDEX IF NOT EXISTS idx_locks_expires_at
    ON locks(expires_at)
    WHERE locked = 1;

-- Index for finding locks by holder
CREATE INDEX IF NOT EXISTS idx_locks_holder
    ON locks(holder_id)
    WHERE locked = 1;















