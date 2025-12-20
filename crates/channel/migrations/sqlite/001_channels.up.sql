-- SPDX-License-Identifier: LGPL-2.1-or-later
-- Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
--
-- Channel messages table for SQLite
--
-- ## Purpose
-- Provides persistent channel messaging using SQLite for durability.
-- Supports message recovery after crashes and single-node persistence.
--
-- ## Design
-- - id: Primary key (ULID message identifier)
-- - channel_name: Channel identifier (for filtering messages by channel)
-- - payload: Message payload (BLOB)
-- - timestamp: Message timestamp (UNIX epoch seconds)
-- - acked: Boolean flag (1 = acknowledged, 0 = unacked)
-- - created_at: Creation timestamp (UNIX epoch seconds)
--
-- ## Indexes
-- - idx_channel_unacked: Find unacked messages for recovery (WHERE acked = 0)
-- - idx_channel_name: Find messages by channel name

CREATE TABLE IF NOT EXISTS channel_messages (
    id TEXT PRIMARY KEY,
    channel_name TEXT NOT NULL,
    payload BLOB NOT NULL,
    timestamp INTEGER NOT NULL,
    acked INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL
);

-- Index for finding unacked messages (for recovery)
CREATE INDEX IF NOT EXISTS idx_channel_unacked
    ON channel_messages(channel_name, acked)
    WHERE acked = 0;

-- Index for finding messages by channel name
CREATE INDEX IF NOT EXISTS idx_channel_name
    ON channel_messages(channel_name);






