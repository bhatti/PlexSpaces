-- SPDX-License-Identifier: AGPL-3.0-or-later
-- First-class tenant entities (PostgreSQL).
-- Tenants are created automatically on first OIDC login for a given slug.

CREATE TABLE IF NOT EXISTS tenants (
    tenant_id    TEXT PRIMARY KEY,
    slug         TEXT NOT NULL,
    display_name TEXT NOT NULL DEFAULT '',
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_tenants_slug ON tenants(slug);
