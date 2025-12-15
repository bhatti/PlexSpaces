-- SPDX-License-Identifier: LGPL-2.1-or-later
-- Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
--
-- Add state_schema_version column to checkpoints table
-- This enables checkpoint format evolution and compatibility checking

ALTER TABLE checkpoints
ADD COLUMN IF NOT EXISTS state_schema_version INTEGER NOT NULL DEFAULT 1;

-- Add index for schema version queries (optional, for debugging)
CREATE INDEX IF NOT EXISTS idx_checkpoint_schema_version
    ON checkpoints (actor_id, state_schema_version);
