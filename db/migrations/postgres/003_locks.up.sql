-- Unified migration: Locks (PostgreSQL)
CREATE TABLE IF NOT EXISTS locks (
    tenant_id TEXT NOT NULL DEFAULT 'default',
    namespace TEXT NOT NULL DEFAULT 'default',
    lock_key TEXT NOT NULL,
    holder_id TEXT NOT NULL,
    version TEXT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    lease_duration_secs INTEGER NOT NULL,
    last_heartbeat TIMESTAMPTZ NOT NULL,
    locked BOOLEAN NOT NULL DEFAULT FALSE,
    metadata JSONB,
    PRIMARY KEY (tenant_id, namespace, lock_key)
);
CREATE INDEX IF NOT EXISTS idx_locks_expires_at ON locks(tenant_id, namespace, expires_at) WHERE locked = TRUE;
CREATE INDEX IF NOT EXISTS idx_locks_holder ON locks(tenant_id, namespace, holder_id) WHERE locked = TRUE;
CREATE INDEX IF NOT EXISTS idx_locks_tenant_namespace ON locks(tenant_id, namespace, lock_key) WHERE locked = TRUE;
