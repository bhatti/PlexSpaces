-- SPDX-License-Identifier: LGPL-2.1-or-later
-- Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
--
-- Drop locks table and indexes

DROP INDEX IF EXISTS idx_locks_holder;
DROP INDEX IF EXISTS idx_locks_expires_at;
DROP TABLE IF EXISTS locks;






