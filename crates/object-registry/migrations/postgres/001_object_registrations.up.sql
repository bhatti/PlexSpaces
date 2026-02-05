-- SPDX-License-Identifier: LGPL-2.1-or-later
-- Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
--
-- Object Registrations table for PostgreSQL
--
-- ## Purpose
-- Provides unified registration and discovery for distributed objects in PlexSpaces:
-- actors, tuplespaces, services, nodes, workflows, applications, VMs, and process groups.
-- Uses indexed columns for fast queries while preserving full registration blob.
--
-- ## Design
-- - Composite primary key: (tenant_id, namespace, object_id) for tenant isolation
-- - Indexed columns for fast discover and heartbeat queries
-- - registration_blob: Full ObjectRegistration protobuf for complete data
-- - Separate indexed fields allow efficient queries without deserializing blob
--
-- ## Indexes
-- - PRIMARY KEY: Fast lookup by (tenant_id, namespace, object_id)
-- - idx_object_registrations_type: Filter by object_type for discover
-- - idx_object_registrations_node: Find objects on a specific node
-- - idx_object_registrations_heartbeat: Find stale registrations
-- - idx_object_registrations_health: Filter by health status
-- - idx_object_registrations_category: Filter by object category

CREATE TABLE IF NOT EXISTS object_registrations (
    -- Identity (composite primary key for tenant isolation)
    tenant_id TEXT NOT NULL,
    namespace TEXT NOT NULL,
    object_id TEXT NOT NULL,
    
    -- Indexed columns for fast queries
    object_type INTEGER NOT NULL,           -- ObjectType enum value (1=Actor, 2=TupleSpace, etc.)
    object_name TEXT,                        -- Human-readable name (optional)
    version TEXT,                            -- Semantic version string
    node_id TEXT,                            -- Node hosting this object
    grpc_address TEXT NOT NULL,              -- gRPC endpoint address
    object_category TEXT,                    -- Sub-type (e.g., "GenServer", "redis", "order-service")
    health_status INTEGER NOT NULL DEFAULT 0, -- HealthStatus enum (0=Unknown, 1=Healthy, etc.)
    
    -- Timestamps
    last_heartbeat TIMESTAMPTZ,              -- Last heartbeat for health monitoring
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    
    -- Full registration data (protobuf serialized)
    registration_blob BYTEA NOT NULL,
    
    PRIMARY KEY (tenant_id, namespace, object_id)
);

-- Index for filtering by object_type (discover by type)
CREATE INDEX IF NOT EXISTS idx_object_registrations_type
    ON object_registrations(tenant_id, namespace, object_type);

-- Index for finding objects on a specific node
CREATE INDEX IF NOT EXISTS idx_object_registrations_node
    ON object_registrations(tenant_id, namespace, node_id)
    WHERE node_id IS NOT NULL;

-- Index for finding stale registrations (heartbeat monitoring)
CREATE INDEX IF NOT EXISTS idx_object_registrations_heartbeat
    ON object_registrations(tenant_id, namespace, last_heartbeat)
    WHERE last_heartbeat IS NOT NULL;

-- Index for filtering by health status
CREATE INDEX IF NOT EXISTS idx_object_registrations_health
    ON object_registrations(tenant_id, namespace, health_status);

-- Index for filtering by object category
CREATE INDEX IF NOT EXISTS idx_object_registrations_category
    ON object_registrations(tenant_id, namespace, object_category)
    WHERE object_category IS NOT NULL;

-- Composite index for common discover queries (type + health)
CREATE INDEX IF NOT EXISTS idx_object_registrations_type_health
    ON object_registrations(tenant_id, namespace, object_type, health_status);
