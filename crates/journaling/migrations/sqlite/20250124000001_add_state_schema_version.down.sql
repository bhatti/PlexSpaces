-- SPDX-License-Identifier: LGPL-2.1-or-later
-- Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
--
-- Remove state_schema_version column from checkpoints table
-- Note: SQLite doesn't support DROP COLUMN, so this is a no-op
-- To fully remove, would need to recreate table

-- SQLite doesn't support DROP COLUMN, so we can't fully rollback
-- This migration is effectively irreversible for SQLite
