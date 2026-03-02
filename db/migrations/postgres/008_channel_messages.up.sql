-- SPDX-License-Identifier: LGPL-2.1-or-later
-- Unified migration: Channel messages (PostgreSQL)

CREATE TABLE IF NOT EXISTS channel_messages (
    id TEXT PRIMARY KEY,
    channel_name TEXT NOT NULL,
    payload BYTEA NOT NULL,
    timestamp TIMESTAMPTZ NOT NULL,
    acked BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_channel_unacked ON channel_messages(channel_name, acked) WHERE acked = FALSE;
CREATE INDEX IF NOT EXISTS idx_channel_name ON channel_messages(channel_name);
