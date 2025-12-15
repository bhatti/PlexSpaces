-- SPDX-License-Identifier: LGPL-2.1-or-later
-- Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
--
-- Initial journal schema for PostgreSQL
--
-- Design Principles:
-- - JSONB for extensibility (add fields without ALTER TABLE)
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
    timestamp TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    -- Correlation ID (link related entries)
    correlation_id TEXT,

    -- Entry type (e.g., "MessageReceived", "StateChanged")
    entry_type TEXT NOT NULL,

    -- Entry payload (JSONB for extensibility)
    entry_data JSONB NOT NULL,

    -- Unique constraint: (actor_id, sequence)
    CONSTRAINT unique_actor_sequence UNIQUE (actor_id, sequence)
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

    -- Checkpoint creation timestamp
    timestamp TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    -- Serialized actor state (compressed)
    state_data BYTEA NOT NULL,

    -- Compression type (0=None, 1=Zstd, 2=Snappy)
    compression INTEGER NOT NULL DEFAULT 0,

    -- Metadata (actor type, version, etc.)
    metadata JSONB,

    PRIMARY KEY (actor_id, sequence)
);

-- Index for latest checkpoint lookup
CREATE INDEX IF NOT EXISTS idx_checkpoint_actor_latest
    ON checkpoints (actor_id, sequence DESC);

-- Statistics view (optional, for observability)
CREATE OR REPLACE VIEW journal_stats AS
SELECT
    actor_id,
    COUNT(*) AS entry_count,
    MIN(sequence) AS min_sequence,
    MAX(sequence) AS max_sequence,
    MIN(timestamp) AS oldest_entry,
    MAX(timestamp) AS newest_entry
FROM journal_entries
GROUP BY actor_id;
