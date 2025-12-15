-- SPDX-License-Identifier: LGPL-2.1-or-later
-- Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
--
-- Add reminders table for PostgreSQL
--
-- This migration adds the reminders table for durable reminder persistence.
-- Reminders are Orleans-inspired persistent timers that survive actor deactivation.

-- Create reminders table
CREATE TABLE IF NOT EXISTS reminders (
    -- Primary key: actor_id + reminder_name (composite)
    actor_id VARCHAR(255) NOT NULL,
    reminder_name VARCHAR(255) NOT NULL,
    
    -- Reminder configuration
    interval_seconds BIGINT,
    interval_nanos INTEGER,
    first_fire_time_seconds BIGINT,
    first_fire_time_nanos INTEGER,
    callback_data BYTEA,
    persist_across_activations BOOLEAN NOT NULL DEFAULT TRUE,
    max_occurrences INTEGER NOT NULL DEFAULT 0, -- 0 = infinite
    
    -- Reminder state
    last_fired_seconds BIGINT,
    last_fired_nanos INTEGER,
    next_fire_time_seconds BIGINT,
    next_fire_time_nanos INTEGER,
    fire_count INTEGER NOT NULL DEFAULT 0,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    
    -- Timestamps
    created_at BIGINT NOT NULL, -- Unix timestamp (ms)
    updated_at BIGINT NOT NULL, -- Unix timestamp (ms)
    
    PRIMARY KEY(actor_id, reminder_name)
);

-- Index for querying due reminders (next_fire_time)
CREATE INDEX IF NOT EXISTS idx_reminders_next_fire_time
    ON reminders(next_fire_time_seconds, next_fire_time_nanos)
    WHERE is_active = TRUE;

-- Index for loading reminders by actor
CREATE INDEX IF NOT EXISTS idx_reminders_actor_id
    ON reminders(actor_id)
    WHERE is_active = TRUE;

