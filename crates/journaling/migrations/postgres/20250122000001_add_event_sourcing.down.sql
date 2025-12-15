-- SPDX-License-Identifier: LGPL-2.1-or-later
-- Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
--
-- Rollback event sourcing tables for PostgreSQL

DROP INDEX IF EXISTS idx_actor_events_caused_by;
DROP INDEX IF EXISTS idx_actor_events_timestamp;
DROP INDEX IF EXISTS idx_actor_events_actor_sequence;
DROP TABLE IF EXISTS actor_events;

