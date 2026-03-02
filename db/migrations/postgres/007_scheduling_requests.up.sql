-- SPDX-License-Identifier: LGPL-2.1-or-later
-- Unified migration: Scheduling requests (PostgreSQL)

CREATE TABLE IF NOT EXISTS scheduling_requests (
    request_id TEXT PRIMARY KEY,
    status TEXT NOT NULL,
    requirements_json JSONB NOT NULL,
    namespace TEXT,
    tenant_id TEXT,
    selected_node_id TEXT,
    actor_id TEXT,
    error_message TEXT,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    scheduled_at TIMESTAMP,
    completed_at TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    metadata_json JSONB
);
CREATE INDEX IF NOT EXISTS idx_scheduling_requests_status ON scheduling_requests(status);
CREATE INDEX IF NOT EXISTS idx_scheduling_requests_created ON scheduling_requests(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_scheduling_requests_node ON scheduling_requests(selected_node_id) WHERE selected_node_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_scheduling_requests_actor ON scheduling_requests(actor_id) WHERE actor_id IS NOT NULL;
