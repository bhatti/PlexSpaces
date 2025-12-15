-- SPDX-License-Identifier: LGPL-2.1-or-later
-- Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
--
-- Rollback: Remove reminders table

DROP INDEX IF EXISTS idx_reminders_next_fire_time;
DROP INDEX IF EXISTS idx_reminders_actor_id;
DROP TABLE IF EXISTS reminders;

