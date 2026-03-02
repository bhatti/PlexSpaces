-- SPDX-License-Identifier: LGPL-2.1-or-later
-- Unified migration: Object registry (SQLite)

CREATE TABLE IF NOT EXISTS object_registrations (
    tenant_id TEXT NOT NULL,
    namespace TEXT NOT NULL,
    object_id TEXT NOT NULL,
    object_type INTEGER NOT NULL,
    object_name TEXT,
    version TEXT,
    node_id TEXT,
    grpc_address TEXT NOT NULL,
    object_category TEXT,
    health_status INTEGER NOT NULL DEFAULT 0,
    last_heartbeat BIGINT,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    registration_blob BLOB NOT NULL,
    PRIMARY KEY (tenant_id, namespace, object_id)
);
CREATE INDEX IF NOT EXISTS idx_object_registrations_type ON object_registrations(tenant_id, namespace, object_type);
CREATE INDEX IF NOT EXISTS idx_object_registrations_node ON object_registrations(tenant_id, namespace, node_id);
CREATE INDEX IF NOT EXISTS idx_object_registrations_heartbeat ON object_registrations(tenant_id, namespace, last_heartbeat);
CREATE INDEX IF NOT EXISTS idx_object_registrations_health ON object_registrations(tenant_id, namespace, health_status);
CREATE INDEX IF NOT EXISTS idx_object_registrations_category ON object_registrations(tenant_id, namespace, object_category);
CREATE INDEX IF NOT EXISTS idx_object_registrations_type_health ON object_registrations(tenant_id, namespace, object_type, health_status);
