-- SPDX-License-Identifier: LGPL-2.1-or-later
-- Unified migration: Step executions (PostgreSQL)

CREATE TABLE IF NOT EXISTS step_executions (
    step_execution_id TEXT PRIMARY KEY,
    execution_id TEXT NOT NULL,
    step_id TEXT NOT NULL,
    status TEXT NOT NULL,
    input_json TEXT,
    output_json TEXT,
    error TEXT,
    attempt INTEGER NOT NULL DEFAULT 1,
    metadata_json TEXT,
    started_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    FOREIGN KEY (execution_id) REFERENCES workflow_executions(execution_id)
);
CREATE INDEX IF NOT EXISTS idx_step_executions_execution ON step_executions(execution_id);
CREATE INDEX IF NOT EXISTS idx_step_executions_status ON step_executions(status);
CREATE INDEX IF NOT EXISTS idx_step_executions_started ON step_executions(started_at DESC);
