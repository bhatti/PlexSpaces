-- SPDX-License-Identifier: LGPL-2.1-or-later
-- Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
--
-- Drop workflow_executions table and indexes

DROP INDEX IF EXISTS idx_workflow_executions_version;
DROP INDEX IF EXISTS idx_workflow_executions_heartbeat;
DROP INDEX IF EXISTS idx_workflow_executions_created;
DROP INDEX IF EXISTS idx_workflow_executions_node;
DROP INDEX IF EXISTS idx_workflow_executions_definition;
DROP INDEX IF EXISTS idx_workflow_executions_status;
DROP TABLE IF EXISTS workflow_executions;







