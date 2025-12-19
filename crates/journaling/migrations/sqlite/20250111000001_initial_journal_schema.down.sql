-- SPDX-License-Identifier: LGPL-2.1-or-later
-- Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
--
-- Drop initial journal schema for SQLite

DROP INDEX IF EXISTS idx_checkpoint_actor_latest;
DROP INDEX IF EXISTS idx_journal_entry_type;
DROP INDEX IF EXISTS idx_journal_timestamp;
DROP INDEX IF EXISTS idx_journal_actor_sequence;
DROP TABLE IF EXISTS checkpoints;
DROP TABLE IF EXISTS journal_entries;





