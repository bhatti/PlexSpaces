-- SPDX-License-Identifier: LGPL-2.1-or-later
-- Unified migration: Journal entries + checkpoints (PostgreSQL)

CREATE TABLE IF NOT EXISTS journal_entries (
    id TEXT PRIMARY KEY,
    actor_id TEXT NOT NULL,
    sequence BIGINT NOT NULL,
    timestamp TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    correlation_id TEXT,
    entry_type TEXT NOT NULL,
    entry_data JSONB NOT NULL,
    CONSTRAINT unique_actor_sequence UNIQUE (actor_id, sequence)
);
CREATE INDEX IF NOT EXISTS idx_journal_actor_sequence ON journal_entries (actor_id, sequence);
CREATE INDEX IF NOT EXISTS idx_journal_timestamp ON journal_entries (timestamp);
CREATE INDEX IF NOT EXISTS idx_journal_entry_type ON journal_entries (entry_type);

CREATE TABLE IF NOT EXISTS checkpoints (
    actor_id TEXT NOT NULL,
    sequence BIGINT NOT NULL,
    timestamp TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    state_data BYTEA NOT NULL,
    compression INTEGER NOT NULL DEFAULT 0,
    metadata JSONB,
    state_schema_version INTEGER NOT NULL DEFAULT 1,
    PRIMARY KEY (actor_id, sequence)
);
CREATE INDEX IF NOT EXISTS idx_checkpoint_actor_latest ON checkpoints (actor_id, sequence DESC);
