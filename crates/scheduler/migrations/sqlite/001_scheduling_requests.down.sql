-- SPDX-License-Identifier: LGPL-2.1-or-later
-- Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

DROP INDEX IF EXISTS idx_scheduling_requests_actor;
DROP INDEX IF EXISTS idx_scheduling_requests_node;
DROP INDEX IF EXISTS idx_scheduling_requests_created;
DROP INDEX IF EXISTS idx_scheduling_requests_status;
DROP TABLE IF EXISTS scheduling_requests;

