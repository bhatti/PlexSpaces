-- SPDX-License-Identifier: LGPL-2.1-or-later
-- Unified migration: Actor events (PostgreSQL)

CREATE TABLE IF NOT EXISTS actor_events (
    id TEXT PRIMARY KEY,
    actor_id TEXT NOT NULL,
    sequence BIGINT NOT NULL,
    event_type TEXT NOT NULL,
    event_data BYTEA NOT NULL,
    timestamp TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    caused_by TEXT,
    metadata JSONB,
    CONSTRAINT unique_actor_event_sequence UNIQUE (actor_id, sequence)
);
CREATE INDEX IF NOT EXISTS idx_actor_events_actor_sequence ON actor_events(actor_id, sequence);
CREATE INDEX IF NOT EXISTS idx_actor_events_timestamp ON actor_events(timestamp);
CREATE INDEX IF NOT EXISTS idx_actor_events_caused_by ON actor_events(caused_by) WHERE caused_by IS NOT NULL;
