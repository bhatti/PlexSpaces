-- SPDX-License-Identifier: LGPL-2.1-or-later
-- Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
--
-- Workflow definitions (versioned templates) for PostgreSQL
--
-- ## Purpose
-- Stores workflow definition templates that can be executed multiple times.
-- Each definition has an ID and version for evolution over time.
--
-- ## Design
-- - Primary key: (id, version) for versioning support
-- - definition_proto: Serialized WorkflowDefinition protobuf message
-- - created_at/updated_at: Audit timestamps
--
-- ## Indexes
-- - idx_workflow_definitions_name: Search by name
-- - idx_workflow_definitions_created: List recent definitions

CREATE TABLE IF NOT EXISTS workflow_definitions (
    id TEXT NOT NULL,
    version TEXT NOT NULL,
    name TEXT NOT NULL,
    definition_proto BYTEA NOT NULL,  -- Serialized WorkflowDefinition
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (id, version)
);

CREATE INDEX IF NOT EXISTS idx_workflow_definitions_name
    ON workflow_definitions(name);

CREATE INDEX IF NOT EXISTS idx_workflow_definitions_created
    ON workflow_definitions(created_at DESC);







