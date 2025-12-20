-- SPDX-License-Identifier: LGPL-2.1-or-later
-- Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
--
-- Workflow execution labels (for filtering and search) for SQLite
--
-- ## Purpose
-- Enables filtering workflow executions by custom labels/tags.
-- Example: Find all executions with label "environment=production"
--
-- ## Design
-- - Composite primary key: (execution_id, label_key)
-- - Index on (label_key, label_value) for efficient filtering
--
-- ## Usage
-- - Filter by label: WHERE execution_id IN (
--     SELECT execution_id FROM workflow_execution_labels
--     WHERE label_key = 'environment' AND label_value = 'production'
--   )

CREATE TABLE IF NOT EXISTS workflow_execution_labels (
    execution_id TEXT NOT NULL,
    label_key TEXT NOT NULL,
    label_value TEXT NOT NULL,
    PRIMARY KEY (execution_id, label_key),
    FOREIGN KEY (execution_id) REFERENCES workflow_executions(execution_id)
);

CREATE INDEX IF NOT EXISTS idx_workflow_execution_labels_key_value
    ON workflow_execution_labels(label_key, label_value);






