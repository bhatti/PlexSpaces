-- SPDX-License-Identifier: LGPL-2.1-or-later
-- Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
--
-- TupleSpace Tuples for SQLite
--
-- ## Purpose
-- Stores tuples for Linda-style coordination and distributed shared memory.
-- Supports pattern matching, leases (TTL), and extensibility.
--
-- ## Design
-- - id: ULID for sortability and uniqueness
-- - tuple_data: Serialized tuple (JSON encoded)
-- - created_at: ISO 8601 timestamp (TEXT)
-- - expires_at: ISO 8601 timestamp (TEXT, NULL = no expiration)
-- - renewable: Boolean flag (0 = not renewable, 1 = renewable)
--
-- ## Indexes
-- - PRIMARY KEY (id): Fast individual tuple lookup
-- - idx_expires_at: TTL cleanup (partial index for non-null values)

CREATE TABLE IF NOT EXISTS tuples (
    id TEXT PRIMARY KEY,
    tuple_data TEXT NOT NULL,    -- JSON encoded tuple
    created_at TEXT NOT NULL,    -- ISO 8601 timestamp
    expires_at TEXT,             -- ISO 8601 timestamp, NULL = no expiration
    renewable INTEGER NOT NULL DEFAULT 0  -- 0 = not renewable, 1 = renewable
);

-- Index for TTL cleanup (partial index for non-null values)
CREATE INDEX IF NOT EXISTS idx_expires_at
    ON tuples(expires_at)
    WHERE expires_at IS NOT NULL;






