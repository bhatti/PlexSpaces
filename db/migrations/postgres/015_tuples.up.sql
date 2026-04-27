-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Unified migration: TupleSpace tuples (PostgreSQL)

CREATE TABLE IF NOT EXISTS tuples (
    id TEXT PRIMARY KEY,
    tuple_data TEXT NOT NULL,
    created_at TEXT NOT NULL,
    expires_at TEXT,
    renewable BOOLEAN NOT NULL DEFAULT FALSE
);
CREATE INDEX IF NOT EXISTS idx_expires_at ON tuples(expires_at) WHERE expires_at IS NOT NULL;
