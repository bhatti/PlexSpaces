# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2025 PlexSpaces Contributors
"""Minimal debug actor to test ts_write."""

import json
from plexspaces import actor, state, handler, init_handler, host


@actor
class DebugProcessor:
    """Minimal actor for debugging TupleSpace calls."""
    
    counter: int = state(default=0)
    
    @init_handler
    def on_init(self, config: dict) -> None:
        self.counter = 0
        host.log("info", "DebugProcessor initialized")
    
    @handler("test_log", "call")
    def test_log(self) -> dict:
        """Test host.log - should work."""
        host.log("info", "test_log called")
        return {"status": "ok", "test": "log"}
    
    @handler("test_kv", "call")
    def test_kv(self) -> dict:
        """Test host.kv_put/kv_get - should work."""
        host.kv_put("debug_key", "debug_value")
        value = host.kv_get("debug_key")
        return {"status": "ok", "test": "kv", "value": value}
    
    @handler("test_ts", "call")
    def test_ts(self) -> dict:
        """Test host.ts_write - crashes."""
        host.log("info", "About to call ts_write")
        tuple_json = json.dumps(["debug", "test", 123])
        host.log("info", f"Calling ts_write with: {tuple_json}")
        result = host.ts_write(tuple_json)
        host.log("info", f"ts_write returned: {result}")
        return {"status": "ok", "test": "ts", "result": result}
    
    @handler("ping", "call")
    def ping(self) -> dict:
        """Simple ping - should work."""
        self.counter += 1
        return {"status": "ok", "counter": self.counter}
