-- SPDX-License-Identifier: LGPL-2.1-or-later
-- Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
--
-- Initial journal schema for SQLite
--
-- Design Principles:
-- - JSON for extensibility (add fields without ALTER TABLE)
-- - Immutable entries (append-only, never UPDATE)
-- - Indexes for performance (sequence ordering, actor lookup)
-- - ULID primary keys (time-sortable)

-- Journal entries table (append-only)
CREATE TABLE IF NOT EXISTS journal_entries (
    -- Primary key (ULID for sortability)
    id TEXT PRIMARY KEY,

    -- Actor this entry belongs to (partitioning key)
    actor_id TEXT NOT NULL,

    -- Sequence number (monotonic per actor)
    sequence BIGINT NOT NULL,

    -- Timestamp (observability only, not for ordering)
    timestamp BIGINT NOT NULL,  -- UNIX timestamp (ms)

    -- Correlation ID (link related entries)
    correlation_id TEXT,

    -- Entry type (e.g., "MessageReceived", "StateChanged")
    entry_type TEXT NOT NULL,

    -- Entry payload (BLOB for SQLite)
    entry_data BLOB NOT NULL,

    -- Unique constraint: (actor_id, sequence)
    UNIQUE(actor_id, sequence)
);

-- Indexes for performance
CREATE INDEX IF NOT EXISTS idx_journal_actor_sequence
    ON journal_entries (actor_id, sequence);

CREATE INDEX IF NOT EXISTS idx_journal_timestamp
    ON journal_entries (timestamp);

CREATE INDEX IF NOT EXISTS idx_journal_entry_type
    ON journal_entries (entry_type);

-- Checkpoints table
CREATE TABLE IF NOT EXISTS checkpoints (
    -- Composite primary key (actor_id, sequence)
    actor_id TEXT NOT NULL,
    sequence BIGINT NOT NULL,

    -- Timestamp when checkpoint was created
    timestamp BIGINT NOT NULL,  -- UNIX timestamp (ms)

    -- Serialized state data (BLOB)
    state_data BLOB NOT NULL,

    -- Compression algorithm (0 = none, 1 = gzip, 2 = zstd)
    compression INTEGER NOT NULL DEFAULT 0,

    -- Metadata (JSON text)
    metadata TEXT,

    -- State schema version (for migration)
    state_schema_version INTEGER NOT NULL DEFAULT 1,

    PRIMARY KEY (actor_id, sequence)
);

-- Index for latest checkpoint lookup
CREATE INDEX IF NOT EXISTS idx_checkpoint_actor_latest
    ON checkpoints (actor_id, sequence DESC);






