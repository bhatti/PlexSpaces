-- SPDX-License-Identifier: LGPL-2.1-or-later
-- Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
--
-- Add event sourcing tables for SQLite
--
-- This migration adds the actor_events table and indexes for event sourcing.
-- Event sourcing is an enhancement to journaling that enables time-travel
-- debugging and state reconstruction from events.

-- Create actor_events table for event sourcing
CREATE TABLE IF NOT EXISTS actor_events (
    -- Primary key (ULID for sortability)
    id TEXT PRIMARY KEY,

    -- Actor this event belongs to (partitioning key)
    actor_id TEXT NOT NULL,

    -- Sequence number (monotonic per actor)
    sequence BIGINT NOT NULL,

    -- Event type (e.g., "counter_incremented", "user_created")
    event_type TEXT NOT NULL,

    -- Event payload (serialized, format depends on event_type)
    event_data BLOB NOT NULL,

    -- Timestamp when event occurred (Unix timestamp in milliseconds)
    timestamp BIGINT NOT NULL,

    -- Correlation ID linking to journal entry that caused this event
    caused_by TEXT,

    -- Metadata (JSON string for SQLite)
    metadata TEXT,

    -- Unique constraint: (actor_id, sequence)
    UNIQUE(actor_id, sequence)
);

-- Index for replay performance (actor_id, sequence)
-- This enables efficient O(log n) binary search for pagination
CREATE INDEX IF NOT EXISTS idx_actor_events_actor_sequence
    ON actor_events(actor_id, sequence);

-- Index for timestamp queries (for time-travel debugging)
CREATE INDEX IF NOT EXISTS idx_actor_events_timestamp
    ON actor_events(timestamp);

-- Index for causal tracking (caused_by queries)
CREATE INDEX IF NOT EXISTS idx_actor_events_caused_by
    ON actor_events(caused_by)
    WHERE caused_by IS NOT NULL;

