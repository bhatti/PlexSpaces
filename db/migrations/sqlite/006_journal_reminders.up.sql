-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Unified migration: Reminders (SQLite)

CREATE TABLE IF NOT EXISTS reminders (
    actor_id TEXT NOT NULL,
    reminder_name TEXT NOT NULL,
    interval_seconds INTEGER,
    interval_nanos INTEGER,
    first_fire_time_seconds INTEGER,
    first_fire_time_nanos INTEGER,
    callback_data BLOB,
    persist_across_activations INTEGER NOT NULL DEFAULT 1,
    max_occurrences INTEGER NOT NULL DEFAULT 0,
    last_fired_seconds INTEGER,
    last_fired_nanos INTEGER,
    next_fire_time_seconds INTEGER,
    next_fire_time_nanos INTEGER,
    fire_count INTEGER NOT NULL DEFAULT 0,
    is_active INTEGER NOT NULL DEFAULT 1,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    PRIMARY KEY(actor_id, reminder_name)
);
CREATE INDEX IF NOT EXISTS idx_reminders_next_fire_time ON reminders(next_fire_time_seconds, next_fire_time_nanos) WHERE is_active = 1;
CREATE INDEX IF NOT EXISTS idx_reminders_actor_id ON reminders(actor_id) WHERE is_active = 1;
