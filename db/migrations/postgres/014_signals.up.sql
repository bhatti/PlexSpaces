-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Unified migration: Signals (PostgreSQL)

CREATE TABLE IF NOT EXISTS signals (
    signal_id TEXT PRIMARY KEY NOT NULL,
    execution_id TEXT NOT NULL,
    signal_name TEXT NOT NULL,
    payload TEXT NOT NULL,
    received_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (execution_id) REFERENCES workflow_executions(execution_id)
);
CREATE INDEX IF NOT EXISTS idx_signals_execution_name ON signals(execution_id, signal_name, received_at);
CREATE INDEX IF NOT EXISTS idx_signals_execution ON signals(execution_id);
