-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Unified migration: TupleSpace barriers and watchers (PostgreSQL)

CREATE TABLE IF NOT EXISTS barriers (
    barrier_id TEXT PRIMARY KEY,
    space_id TEXT NOT NULL,
    expected_count INTEGER NOT NULL,
    current_count INTEGER NOT NULL DEFAULT 0,
    participants_json TEXT,
    metadata_json TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    completed_at TIMESTAMPTZ,
    expires_at TIMESTAMPTZ
);
CREATE INDEX IF NOT EXISTS idx_barriers_space ON barriers(space_id);
CREATE INDEX IF NOT EXISTS idx_barriers_status ON barriers(completed_at) WHERE completed_at IS NULL;

CREATE TABLE IF NOT EXISTS watchers (
    watcher_id TEXT PRIMARY KEY,
    space_id TEXT NOT NULL,
    actor_id TEXT NOT NULL,
    pattern_hash TEXT,
    event_types TEXT NOT NULL,
    metadata_json TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    last_notified_at TIMESTAMPTZ,
    notification_count INTEGER NOT NULL DEFAULT 0,
    active BOOLEAN NOT NULL DEFAULT TRUE
);
CREATE INDEX IF NOT EXISTS idx_watchers_space ON watchers(space_id, active) WHERE active = TRUE;
CREATE INDEX IF NOT EXISTS idx_watchers_pattern ON watchers(space_id, pattern_hash, active) WHERE active = TRUE;
CREATE INDEX IF NOT EXISTS idx_watchers_actor ON watchers(actor_id, active) WHERE active = TRUE;
