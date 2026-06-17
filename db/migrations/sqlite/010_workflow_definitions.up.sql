-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Unified migration: Workflow definitions (SQLite)

CREATE TABLE IF NOT EXISTS workflow_definitions (
    id TEXT NOT NULL,
    version TEXT NOT NULL,
    name TEXT NOT NULL,
    tenant_id TEXT NOT NULL DEFAULT '',
    namespace TEXT NOT NULL DEFAULT '',
    definition_proto BLOB NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    updated_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    PRIMARY KEY (id, version)
);
CREATE INDEX IF NOT EXISTS idx_workflow_definitions_name ON workflow_definitions(name);
CREATE INDEX IF NOT EXISTS idx_workflow_definitions_created ON workflow_definitions(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_workflow_definitions_tenant ON workflow_definitions(tenant_id, namespace);
