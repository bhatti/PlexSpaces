-- SPDX-License-Identifier: LGPL-2.1-or-later
-- Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
--
-- Signals table for Signal step (Awakeable pattern) for PostgreSQL
--
-- Purpose: Store external signals that workflows can wait for
-- Used by: Signal step type for external event coordination

CREATE TABLE IF NOT EXISTS signals (
    signal_id TEXT PRIMARY KEY NOT NULL,
    execution_id TEXT NOT NULL,
    signal_name TEXT NOT NULL,
    payload TEXT NOT NULL,
    received_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (execution_id) REFERENCES workflow_executions(execution_id)
);

-- Index for efficient signal lookup by execution and name
CREATE INDEX IF NOT EXISTS idx_signals_execution_name
ON signals(execution_id, signal_name, received_at);

-- Index for cleanup queries
CREATE INDEX IF NOT EXISTS idx_signals_execution
ON signals(execution_id);





