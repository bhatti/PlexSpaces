-- SPDX-License-Identifier: LGPL-2.1-or-later
-- Unified migration: Reminders (PostgreSQL)

CREATE TABLE IF NOT EXISTS reminders (
    actor_id VARCHAR(255) NOT NULL,
    reminder_name VARCHAR(255) NOT NULL,
    interval_seconds BIGINT,
    interval_nanos INTEGER,
    first_fire_time_seconds BIGINT,
    first_fire_time_nanos INTEGER,
    callback_data BYTEA,
    persist_across_activations BOOLEAN NOT NULL DEFAULT TRUE,
    max_occurrences INTEGER NOT NULL DEFAULT 0,
    last_fired_seconds BIGINT,
    last_fired_nanos INTEGER,
    next_fire_time_seconds BIGINT,
    next_fire_time_nanos INTEGER,
    fire_count INTEGER NOT NULL DEFAULT 0,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    PRIMARY KEY(actor_id, reminder_name)
);
CREATE INDEX IF NOT EXISTS idx_reminders_next_fire_time ON reminders(next_fire_time_seconds, next_fire_time_nanos) WHERE is_active = TRUE;
CREATE INDEX IF NOT EXISTS idx_reminders_actor_id ON reminders(actor_id) WHERE is_active = TRUE;
