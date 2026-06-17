-- SPDX-License-Identifier: AGPL-3.0-or-later
-- OAuth user records (SQLite)

CREATE TABLE IF NOT EXISTS users (
    user_id TEXT PRIMARY KEY,
    email TEXT NOT NULL,
    tenant_id TEXT NOT NULL,
    display_name TEXT NOT NULL DEFAULT '',
    admin INTEGER NOT NULL DEFAULT 0,
    last_login INTEGER,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    updated_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    roles_json TEXT NOT NULL DEFAULT '[]',
    groups_json TEXT NOT NULL DEFAULT '[]',
    avatar_url TEXT NOT NULL DEFAULT '',
    provider TEXT NOT NULL DEFAULT '',
    provider_sub TEXT NOT NULL DEFAULT ''
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_users_email ON users(email);
CREATE INDEX IF NOT EXISTS idx_users_tenant ON users(tenant_id);
CREATE INDEX IF NOT EXISTS idx_users_provider ON users(provider, provider_sub);
