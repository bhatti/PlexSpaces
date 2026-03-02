-- SPDX-License-Identifier: LGPL-2.1-or-later
-- Unified migration: Signals (SQLite)

CREATE TABLE IF NOT EXISTS signals (
    signal_id TEXT PRIMARY KEY NOT NULL,
    execution_id TEXT NOT NULL,
    signal_name TEXT NOT NULL,
    payload TEXT NOT NULL,
    received_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    FOREIGN KEY (execution_id) REFERENCES workflow_executions(execution_id)
);
CREATE INDEX IF NOT EXISTS idx_signals_execution_name ON signals(execution_id, signal_name, received_at);
CREATE INDEX IF NOT EXISTS idx_signals_execution ON signals(execution_id);
