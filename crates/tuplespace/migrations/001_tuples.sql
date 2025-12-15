-- SPDX-License-Identifier: LGPL-2.1-or-later
-- Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

-- TupleSpace Tuples
--
-- ## Purpose
-- Stores tuples for Linda-style coordination and distributed shared memory.
-- Supports pattern matching, leases (TTL), and extensibility.
--
-- ## Design
-- - tuple_id: ULID for sortability and uniqueness
-- - space_id: TupleSpace namespace identifier
-- - tuple_data: Serialized tuple (JSON or protobuf)
-- - tuple_hash: SHA-256 hash for deduplication and pattern matching
-- - metadata_json: EXTENSIBLE JSON field for future features
-- - lease_expires_at: Automatic cleanup for leased tuples
-- - taken: Boolean flag (true = tuple taken, false = available)
--
-- ## Extensibility
-- Use metadata_json for new fields to avoid ALTER TABLE:
-- - {"owner_id": "actor-123", "priority": 10, "tags": ["barrier", "sync"]}
-- - {"pattern_fields": ["name", "age"], "watchers": ["watcher-1", "watcher-2"]}
-- - Allows adding features like ownership, priority, watchers without migration
--
-- ## Pattern Matching
-- tuple_hash enables fast pattern matching:
-- - Exact match: WHERE tuple_hash = hash(pattern)
-- - Partial match: WHERE tuple_data LIKE '%pattern%' (slower, but flexible)
--
-- ## Indexes
-- - PRIMARY KEY (tuple_id): Fast individual tuple lookup
-- - idx_tuples_space: Find all tuples in a space
-- - idx_tuples_hash: Fast pattern matching
-- - idx_tuples_lease: TTL cleanup
-- - idx_tuples_available: Find available (not taken) tuples
--
-- ## Cleanup
-- Periodic cleanup of expired leases:
-- DELETE FROM tuples WHERE lease_expires_at < CURRENT_TIMESTAMP;

CREATE TABLE IF NOT EXISTS tuples (
    tuple_id TEXT PRIMARY KEY,
    space_id TEXT NOT NULL,
    tuple_data TEXT NOT NULL,    -- Serialized tuple (JSON or protobuf base64)
    tuple_hash TEXT NOT NULL,    -- SHA-256 hash for pattern matching
    metadata_json TEXT,           -- EXTENSIBLE: owner, priority, tags, watchers, etc.
    taken BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    lease_expires_at TIMESTAMP    -- NULL = no expiration, otherwise auto-delete
);

-- Index for space-scoped operations
CREATE INDEX IF NOT EXISTS idx_tuples_space
    ON tuples(space_id);

-- Index for pattern matching
CREATE INDEX IF NOT EXISTS idx_tuples_hash
    ON tuples(tuple_hash);

-- Index for lease cleanup
CREATE INDEX IF NOT EXISTS idx_tuples_lease
    ON tuples(lease_expires_at)
    WHERE lease_expires_at IS NOT NULL;

-- Index for available tuples (read/take operations)
CREATE INDEX IF NOT EXISTS idx_tuples_available
    ON tuples(space_id, taken)
    WHERE taken = FALSE;

-- Compound index for efficient pattern matching queries
CREATE INDEX IF NOT EXISTS idx_tuples_space_hash_available
    ON tuples(space_id, tuple_hash, taken);
