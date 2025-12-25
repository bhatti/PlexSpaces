-- SPDX-License-Identifier: LGPL-2.1-or-later
-- Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
--
-- Channel messages table for PostgreSQL
--
-- ## Purpose
-- Provides persistent channel messaging using PostgreSQL for durability.
-- Supports message recovery after crashes and multi-node persistence.
--
-- ## Design
-- - id: Primary key (ULID message identifier)
-- - channel_name: Channel identifier (for filtering messages by channel)
-- - payload: Message payload (BYTEA)
-- - timestamp: Message timestamp (TIMESTAMPTZ)
-- - acked: Boolean flag (TRUE = acknowledged, FALSE = unacked)
-- - created_at: Creation timestamp (TIMESTAMPTZ)
--
-- ## Indexes
-- - idx_channel_unacked: Find unacked messages for recovery (WHERE acked = FALSE)
-- - idx_channel_name: Find messages by channel name

CREATE TABLE IF NOT EXISTS channel_messages (
    id TEXT PRIMARY KEY,
    channel_name TEXT NOT NULL,
    payload BYTEA NOT NULL,
    timestamp TIMESTAMPTZ NOT NULL,
    acked BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL
);

-- Index for finding unacked messages (for recovery)
CREATE INDEX IF NOT EXISTS idx_channel_unacked
    ON channel_messages(channel_name, acked)
    WHERE acked = FALSE;

-- Index for finding messages by channel name
CREATE INDEX IF NOT EXISTS idx_channel_name
    ON channel_messages(channel_name);















