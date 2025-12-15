-- SPDX-License-Identifier: LGPL-2.1-or-later
-- Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

-- Scheduling requests (metadata and status tracking)
--
-- ## Purpose
-- Stores queryable metadata for scheduling requests.
-- Enables efficient queries by status (especially PENDING for recovery).
--
-- ## Design
-- - request_id: ULID for sortability
-- - status: PENDING, SCHEDULED, FAILED
-- - requirements_json: Serialized ActorResourceRequirements (JSONB for PostgreSQL)
-- - metadata_json: EXTENSIBLE JSONB field for future fields without schema changes
--
-- ## Extensibility
-- Use metadata_json for new fields to avoid ALTER TABLE:
-- - {"retry_count": 3, "priority": "high", "tags": [...]}
-- - Allows adding features without migration
--
-- ## Indexes
-- - idx_scheduling_requests_status: Filter by status (for recovery)
-- - idx_scheduling_requests_created: List recent requests
-- - idx_scheduling_requests_node: Find requests by node
-- - idx_scheduling_requests_actor: Find requests by actor

CREATE TABLE IF NOT EXISTS scheduling_requests (
    request_id TEXT PRIMARY KEY,              -- ULID (time-sortable)
    status TEXT NOT NULL,                     -- PENDING, SCHEDULED, FAILED
    requirements_json JSONB NOT NULL,         -- Serialized ActorResourceRequirements (JSONB)
    namespace TEXT,
    tenant_id TEXT,
    
    -- Result (if scheduled)
    selected_node_id TEXT,
    actor_id TEXT,                            -- Created actor ID
    
    -- Error (if failed)
    error_message TEXT,
    
    -- Timestamps
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    scheduled_at TIMESTAMP,
    completed_at TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    
    -- Extensibility: Additional fields without schema changes
    metadata_json JSONB
);

-- Indexes for efficient queries
CREATE INDEX IF NOT EXISTS idx_scheduling_requests_status 
    ON scheduling_requests(status);

CREATE INDEX IF NOT EXISTS idx_scheduling_requests_created 
    ON scheduling_requests(created_at DESC);

CREATE INDEX IF NOT EXISTS idx_scheduling_requests_node 
    ON scheduling_requests(selected_node_id) 
    WHERE selected_node_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_scheduling_requests_actor 
    ON scheduling_requests(actor_id) 
    WHERE actor_id IS NOT NULL;

