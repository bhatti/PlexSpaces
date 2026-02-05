-- SPDX-License-Identifier: LGPL-2.1-or-later
-- Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
--
-- Drop object_registrations table and indexes

DROP INDEX IF EXISTS idx_object_registrations_type_health;
DROP INDEX IF EXISTS idx_object_registrations_category;
DROP INDEX IF EXISTS idx_object_registrations_health;
DROP INDEX IF EXISTS idx_object_registrations_heartbeat;
DROP INDEX IF EXISTS idx_object_registrations_node;
DROP INDEX IF EXISTS idx_object_registrations_type;
DROP TABLE IF EXISTS object_registrations;
