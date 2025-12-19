-- SPDX-License-Identifier: LGPL-2.1-or-later
-- Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
--
-- Drop barriers and watchers tables and indexes

DROP INDEX IF EXISTS idx_watchers_actor;
DROP INDEX IF EXISTS idx_watchers_pattern;
DROP INDEX IF EXISTS idx_watchers_space;
DROP INDEX IF EXISTS idx_barriers_status;
DROP INDEX IF EXISTS idx_barriers_space;
DROP TABLE IF EXISTS watchers;
DROP TABLE IF EXISTS barriers;





