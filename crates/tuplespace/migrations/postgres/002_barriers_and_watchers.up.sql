-- SPDX-License-Identifier: LGPL-2.1-or-later
-- Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
--
-- TupleSpace Barriers and Watchers for PostgreSQL
--
-- ## Purpose
-- Supports barrier synchronization and tuple watchers for event-driven coordination.
--
-- ## Barriers
-- Enables N actors to synchronize at a barrier point.
-- All actors must reach the barrier before any can proceed.
--
-- ## Watchers
-- Actors can register to be notified when tuples matching a pattern are added/removed.
--
-- ## Design
-- - barrier_id: Unique barrier identifier
-- - watcher_id: Unique watcher identifier
-- - metadata_json: EXTENSIBLE JSON field for future features
--
-- ## Extensibility
-- Use metadata_json for new fields to avoid ALTER TABLE:
-- - Barriers: {"timeout_ms": 5000, "phase": 1, "coordinator_id": "..."}
-- - Watchers: {"filter_expr": "$.name == 'foo'", "max_events": 100, "callback_url": "..."}

-- Barriers table
CREATE TABLE IF NOT EXISTS barriers (
    barrier_id TEXT PRIMARY KEY,
    space_id TEXT NOT NULL,
    expected_count INTEGER NOT NULL,   -- Total actors expected
    current_count INTEGER NOT NULL DEFAULT 0,  -- Actors arrived so far
    participants_json TEXT,             -- List of participant actor IDs (JSON array)
    metadata_json TEXT,                 -- EXTENSIBLE: timeout, phase, coordinator, etc.
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    completed_at TIMESTAMPTZ,           -- When barrier released
    expires_at TIMESTAMPTZ              -- Optional timeout
);

CREATE INDEX IF NOT EXISTS idx_barriers_space
    ON barriers(space_id);

CREATE INDEX IF NOT EXISTS idx_barriers_status
    ON barriers(completed_at)
    WHERE completed_at IS NULL;  -- Active barriers only

-- Watchers table
CREATE TABLE IF NOT EXISTS watchers (
    watcher_id TEXT PRIMARY KEY,
    space_id TEXT NOT NULL,
    actor_id TEXT NOT NULL,             -- Actor receiving notifications
    pattern_hash TEXT,                  -- Pattern to watch (NULL = watch all)
    event_types TEXT NOT NULL,          -- Comma-separated: "WRITE,TAKE,UPDATE"
    metadata_json TEXT,                 -- EXTENSIBLE: filter, max events, callback, etc.
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    last_notified_at TIMESTAMPTZ,
    notification_count INTEGER NOT NULL DEFAULT 0,
    active BOOLEAN NOT NULL DEFAULT TRUE
);

CREATE INDEX IF NOT EXISTS idx_watchers_space
    ON watchers(space_id, active)
    WHERE active = TRUE;

CREATE INDEX IF NOT EXISTS idx_watchers_pattern
    ON watchers(space_id, pattern_hash, active)
    WHERE active = TRUE;

CREATE INDEX IF NOT EXISTS idx_watchers_actor
    ON watchers(actor_id, active)
    WHERE active = TRUE;





