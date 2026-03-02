-- SPDX-License-Identifier: LGPL-2.1-or-later
-- Unified migration: TupleSpace tuples (SQLite)

CREATE TABLE IF NOT EXISTS tuples (
    id TEXT PRIMARY KEY,
    tuple_data TEXT NOT NULL,
    created_at TEXT NOT NULL,
    expires_at TEXT,
    renewable INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_expires_at ON tuples(expires_at) WHERE expires_at IS NOT NULL;
