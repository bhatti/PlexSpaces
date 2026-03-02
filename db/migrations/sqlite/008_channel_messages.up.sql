-- SPDX-License-Identifier: LGPL-2.1-or-later
-- Unified migration: Channel messages (SQLite)

CREATE TABLE IF NOT EXISTS channel_messages (
    id TEXT PRIMARY KEY,
    channel_name TEXT NOT NULL,
    payload BLOB NOT NULL,
    timestamp INTEGER NOT NULL,
    acked INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_channel_unacked ON channel_messages(channel_name, acked) WHERE acked = 0;
CREATE INDEX IF NOT EXISTS idx_channel_name ON channel_messages(channel_name);
