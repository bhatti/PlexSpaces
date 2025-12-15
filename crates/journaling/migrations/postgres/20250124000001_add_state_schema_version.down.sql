-- SPDX-License-Identifier: LGPL-2.1-or-later
-- Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
--
-- Remove state_schema_version column from checkpoints table

DROP INDEX IF EXISTS idx_checkpoint_schema_version;
ALTER TABLE checkpoints DROP COLUMN IF EXISTS state_schema_version;
