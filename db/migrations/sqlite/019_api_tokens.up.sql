-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Long-lived API tokens for programmatic access (SQLite).
-- Plaintext token is never stored; only a SHA-256 hex digest is persisted.

CREATE TABLE IF NOT EXISTS api_tokens (
    token_id     TEXT PRIMARY KEY,
    user_id      TEXT NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
    tenant_id    TEXT NOT NULL,
    name         TEXT NOT NULL,
    prefix       TEXT NOT NULL,
    token_hash   TEXT NOT NULL,
    scopes_json  TEXT NOT NULL DEFAULT '[]',
    expires_at   INTEGER,
    created_at   INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    last_used_at INTEGER,
    revoked_at   INTEGER,
    is_admin     INTEGER NOT NULL DEFAULT 0
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_api_tokens_hash   ON api_tokens(token_hash);
CREATE INDEX        IF NOT EXISTS idx_api_tokens_user   ON api_tokens(user_id);
CREATE INDEX        IF NOT EXISTS idx_api_tokens_tenant ON api_tokens(tenant_id);
