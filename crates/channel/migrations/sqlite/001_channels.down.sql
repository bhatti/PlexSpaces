-- SPDX-License-Identifier: LGPL-2.1-or-later
-- Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
--
-- Drop channel messages table and indexes

DROP INDEX IF EXISTS idx_channel_name;
DROP INDEX IF EXISTS idx_channel_unacked;
DROP TABLE IF EXISTS channel_messages;















