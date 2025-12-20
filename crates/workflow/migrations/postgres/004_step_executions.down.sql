-- SPDX-License-Identifier: LGPL-2.1-or-later
-- Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
--
-- Drop step_executions table and indexes

DROP INDEX IF EXISTS idx_step_executions_started;
DROP INDEX IF EXISTS idx_step_executions_status;
DROP INDEX IF EXISTS idx_step_executions_execution;
DROP TABLE IF EXISTS step_executions;






