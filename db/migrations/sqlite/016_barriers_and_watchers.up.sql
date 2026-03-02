-- SPDX-License-Identifier: LGPL-2.1-or-later
-- Unified migration: TupleSpace barriers and watchers (SQLite)

CREATE TABLE IF NOT EXISTS barriers (
    barrier_id TEXT PRIMARY KEY,
    space_id TEXT NOT NULL,
    expected_count INTEGER NOT NULL,
    current_count INTEGER NOT NULL DEFAULT 0,
    participants_json TEXT,
    metadata_json TEXT,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    completed_at INTEGER,
    expires_at INTEGER
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
    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    last_notified_at INTEGER,
    notification_count INTEGER NOT NULL DEFAULT 0,
    active INTEGER NOT NULL DEFAULT 1
);
CREATE INDEX IF NOT EXISTS idx_watchers_space ON watchers(space_id, active) WHERE active = 1;
CREATE INDEX IF NOT EXISTS idx_watchers_pattern ON watchers(space_id, pattern_hash, active) WHERE active = 1;
CREATE INDEX IF NOT EXISTS idx_watchers_actor ON watchers(actor_id, active) WHERE active = 1;
