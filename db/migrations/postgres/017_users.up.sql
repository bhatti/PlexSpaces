-- SPDX-License-Identifier: AGPL-3.0-or-later
-- OAuth user records (PostgreSQL)

CREATE TABLE IF NOT EXISTS users (
    user_id TEXT PRIMARY KEY,
    email TEXT NOT NULL,
    tenant_id TEXT NOT NULL,
    display_name TEXT NOT NULL DEFAULT '',
    admin BOOLEAN NOT NULL DEFAULT FALSE,
    last_login TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    roles_json JSONB NOT NULL DEFAULT '[]'::jsonb,
    groups_json JSONB NOT NULL DEFAULT '[]'::jsonb,
    avatar_url TEXT NOT NULL DEFAULT '',
    provider TEXT NOT NULL DEFAULT '',
    provider_sub TEXT NOT NULL DEFAULT ''
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_users_email ON users(email);
CREATE INDEX IF NOT EXISTS idx_users_tenant ON users(tenant_id);
CREATE INDEX IF NOT EXISTS idx_users_provider ON users(provider, provider_sub);
