-- SPDX-License-Identifier: LGPL-2.1-or-later
-- Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
--
-- Step executions (for history/monitoring) for PostgreSQL
--
-- ## Purpose
-- Stores individual step execution records for debugging and monitoring.
-- The actual step state is in the journal (via DurabilityFacet).
--
-- ## Design
-- - step_execution_id: ULID for sortability and uniqueness
-- - status: PENDING, RUNNING, COMPLETED, FAILED, RETRYING, CANCELLED
-- - attempt: Retry attempt number (1 = first attempt)
-- - input_json/output_json: For display/debugging (not source of truth)
-- - metadata_json: EXTENSIBLE JSON field for future fields without schema changes
--
-- ## Extensibility
-- Use metadata_json for new fields to avoid ALTER TABLE:
-- - {"backoff_duration_ms": 1000, "worker_id": "...", "metrics": {...}}
-- - Allows adding features without migration
--
-- ## Indexes
-- - idx_step_executions_execution: Find all steps for a workflow execution
-- - idx_step_executions_status: Filter by status (e.g., find all failed steps)
-- - idx_step_executions_started: List recent step executions

CREATE TABLE IF NOT EXISTS step_executions (
    step_execution_id TEXT PRIMARY KEY,
    execution_id TEXT NOT NULL,
    step_id TEXT NOT NULL,
    status TEXT NOT NULL,  -- PENDING, RUNNING, COMPLETED, FAILED, RETRYING, CANCELLED
    input_json TEXT,       -- Step input (for display/debugging)
    output_json TEXT,      -- Step output (if completed)
    error TEXT,            -- Error message (if failed)
    attempt INTEGER NOT NULL DEFAULT 1,
    metadata_json TEXT,    -- EXTENSIBLE: Additional fields without schema changes
    started_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    FOREIGN KEY (execution_id) REFERENCES workflow_executions(execution_id)
);

CREATE INDEX IF NOT EXISTS idx_step_executions_execution
    ON step_executions(execution_id);

CREATE INDEX IF NOT EXISTS idx_step_executions_status
    ON step_executions(status);

CREATE INDEX IF NOT EXISTS idx_step_executions_started
    ON step_executions(started_at DESC);








