-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Unified migration: Journal entries + checkpoints (SQLite)

CREATE TABLE IF NOT EXISTS journal_entries (
    id TEXT PRIMARY KEY,
    actor_id TEXT NOT NULL,
    sequence BIGINT NOT NULL,
    timestamp BIGINT NOT NULL,
    correlation_id TEXT,
    entry_type TEXT NOT NULL,
    entry_data BLOB NOT NULL,
    UNIQUE(actor_id, sequence)
);
CREATE INDEX IF NOT EXISTS idx_journal_actor_sequence ON journal_entries (actor_id, sequence);
CREATE INDEX IF NOT EXISTS idx_journal_timestamp ON journal_entries (timestamp);
CREATE INDEX IF NOT EXISTS idx_journal_entry_type ON journal_entries (entry_type);

CREATE TABLE IF NOT EXISTS checkpoints (
    actor_id TEXT NOT NULL,
    sequence BIGINT NOT NULL,
    timestamp BIGINT NOT NULL,
    state_data BLOB NOT NULL,
    compression INTEGER NOT NULL DEFAULT 0,
    metadata TEXT,
    state_schema_version INTEGER NOT NULL DEFAULT 1,
    PRIMARY KEY (actor_id, sequence)
);
CREATE INDEX IF NOT EXISTS idx_checkpoint_actor_latest ON checkpoints (actor_id, sequence DESC);
