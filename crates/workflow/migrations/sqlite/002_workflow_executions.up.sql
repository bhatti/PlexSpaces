-- SPDX-License-Identifier: LGPL-2.1-or-later
-- Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
--
-- Workflow executions (metadata only, state in journal) for SQLite
--
-- ## Purpose
-- Stores queryable metadata for workflow executions.
-- The actual execution state is in the journal (via DurabilityFacet).
--
-- ## Design
-- - execution_id: ULID for sortability
-- - status: PENDING, RUNNING, COMPLETED, FAILED, CANCELLED, TIMED_OUT
-- - input_json/output_json: For display/debugging (not source of truth)
-- - Foreign key to workflow_definitions for referential integrity
-- - metadata_json: EXTENSIBLE JSON field for future fields without schema changes
--
-- ## Extensibility
-- Use metadata_json for new fields to avoid ALTER TABLE:
-- - {"retry_count": 3, "parent_execution_id": "...", "tags": [...]}
-- - Allows adding features without migration
--
-- ## Indexes
-- - idx_workflow_executions_status: Filter by status
-- - idx_workflow_executions_definition: Find all executions of a definition
-- - idx_workflow_executions_node: Find executions on a node
-- - idx_workflow_executions_created: List recent executions

CREATE TABLE IF NOT EXISTS workflow_executions (
    execution_id TEXT PRIMARY KEY,
    definition_id TEXT NOT NULL,
    definition_version TEXT NOT NULL,
    status TEXT NOT NULL,  -- PENDING, RUNNING, COMPLETED, FAILED, CANCELLED, TIMED_OUT
    current_step_id TEXT,
    input_json TEXT,      -- Initial input (for display/debugging)
    output_json TEXT,     -- Final output (if completed)
    error TEXT,           -- Error message (if failed)
    node_id TEXT,         -- Node executing this workflow (node_owner)
    version INTEGER NOT NULL DEFAULT 1,  -- Version for optimistic locking
    last_heartbeat INTEGER,  -- Last heartbeat timestamp (UNIX timestamp)
    metadata_json TEXT,   -- EXTENSIBLE: Additional fields without schema changes
    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),  -- UNIX timestamp
    started_at INTEGER,   -- UNIX timestamp
    completed_at INTEGER, -- UNIX timestamp
    updated_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),  -- UNIX timestamp
    FOREIGN KEY (definition_id, definition_version)
        REFERENCES workflow_definitions(id, version)
);

CREATE INDEX IF NOT EXISTS idx_workflow_executions_status
    ON workflow_executions(status);

CREATE INDEX IF NOT EXISTS idx_workflow_executions_definition
    ON workflow_executions(definition_id);

CREATE INDEX IF NOT EXISTS idx_workflow_executions_node
    ON workflow_executions(node_id);

CREATE INDEX IF NOT EXISTS idx_workflow_executions_created
    ON workflow_executions(created_at DESC);

-- Index for stale workflow detection (health monitoring)
CREATE INDEX IF NOT EXISTS idx_workflow_executions_heartbeat
    ON workflow_executions(status, last_heartbeat)
    WHERE status IN ('RUNNING', 'PENDING');

-- Index for version-based queries (optimistic locking)
CREATE INDEX IF NOT EXISTS idx_workflow_executions_version
    ON workflow_executions(execution_id, version);








