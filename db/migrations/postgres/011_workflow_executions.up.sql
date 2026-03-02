-- SPDX-License-Identifier: LGPL-2.1-or-later
-- Unified migration: Workflow executions (PostgreSQL)

CREATE TABLE IF NOT EXISTS workflow_executions (
    execution_id TEXT PRIMARY KEY,
    definition_id TEXT NOT NULL,
    definition_version TEXT NOT NULL,
    status TEXT NOT NULL,
    current_step_id TEXT,
    input_json TEXT,
    output_json TEXT,
    error TEXT,
    node_id TEXT,
    version INTEGER NOT NULL DEFAULT 1,
    last_heartbeat TIMESTAMPTZ,
    metadata_json TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    started_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (definition_id, definition_version) REFERENCES workflow_definitions(id, version)
);
CREATE INDEX IF NOT EXISTS idx_workflow_executions_status ON workflow_executions(status);
CREATE INDEX IF NOT EXISTS idx_workflow_executions_definition ON workflow_executions(definition_id);
CREATE INDEX IF NOT EXISTS idx_workflow_executions_node ON workflow_executions(node_id);
CREATE INDEX IF NOT EXISTS idx_workflow_executions_created ON workflow_executions(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_workflow_executions_heartbeat ON workflow_executions(status, last_heartbeat) WHERE status IN ('RUNNING', 'PENDING');
CREATE INDEX IF NOT EXISTS idx_workflow_executions_version ON workflow_executions(execution_id, version);
