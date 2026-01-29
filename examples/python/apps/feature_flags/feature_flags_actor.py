"""
Feature Flags Service - Python WASM Actor

A feature flag management service for controlling feature rollouts.
Real-world use case: A/B testing, gradual rollouts, kill switches.

## WASM/componentize-py Memory Workarounds

The Python 3.14 runtime in componentize-py has memory management bugs that cause
crashes. This code uses specific patterns to avoid them:

1. **Avoid hashlib** - hashlib.md5() causes `match_dealloc` crash
   - Use simple inline hash instead of hashlib functions

2. **Use string literals for simple JSON** - json.dumps() can cause `tuple_dealloc` crash
   - Return '{"status":"ok"}' instead of json.dumps({"status": "ok"})

3. **Inline calculations** - Calling helper functions can cause `func_dealloc` crash
   - Inline hash calculations instead of calling a separate function

4. **Flat control flow** - Nested try-except can cause deallocation issues
   - Use simple if statements instead of nested try-except blocks

5. **Avoid complex return values** - Complex dicts/tuples can crash on deallocation
   - Keep returned data structures simple and flat

These issues are tracked in the componentize-py project and may be fixed in future versions.
"""

import json
from wit_world import exports

# Feature flags storage - simple dict
# Note: Global state works fine in componentize-py
_flags = {}


class Actor(exports.Actor):
    """Feature flags actor."""
    
    def init(self, config_json: str) -> str:
        """Initialize with optional preset flags."""
        global _flags
        _flags = {}
        return ""
    
    def handle(self, from_actor: str, msg_type: str, payload_json: str) -> str:
        """Handle feature flag operations."""
        global _flags
        
        # Parse payload
        payload = {}
        if payload_json:
            payload = json.loads(payload_json)
        
        op = payload.get("op", "")
        flag_name = payload.get("flag", "")
        
        # Create flag
        if op == "create":
            if not flag_name:
                return '{"error":"flag_required"}'
            if flag_name in _flags:
                return '{"error":"flag_exists"}'
            _flags[flag_name] = {"enabled": False, "rollout": 100}
            return '{"status":"ok"}'
        
        # Enable flag
        if op == "enable":
            if flag_name not in _flags:
                return '{"error":"flag_not_found"}'
            _flags[flag_name]["enabled"] = True
            return '{"status":"ok"}'
        
        # Disable flag
        if op == "disable":
            if flag_name not in _flags:
                return '{"error":"flag_not_found"}'
            _flags[flag_name]["enabled"] = False
            return '{"status":"ok"}'
        
        # Set rollout percentage
        if op == "rollout":
            if flag_name not in _flags:
                return '{"error":"flag_not_found"}'
            pct = int(payload.get("pct", 100))
            _flags[flag_name]["rollout"] = pct
            return '{"status":"ok"}'
        
        # Check if flag is enabled for user
        if op == "check":
            if not flag_name:
                return '{"error":"flag_required"}'
            if flag_name not in _flags:
                return '{"enabled":false,"reason":"not_found"}'
            f = _flags[flag_name]
            if not f["enabled"]:
                return '{"enabled":false,"reason":"disabled"}'
            # For rollout < 100, use simple deterministic hash
            rollout = f["rollout"]
            if rollout >= 100:
                return '{"enabled":true,"reason":"full"}'
            user = payload.get("user", "")
            # Simple hash: sum of char codes mod 100
            h = 0
            for c in (flag_name + user):
                h = (h + ord(c)) % 100
            enabled = h < rollout
            return '{"enabled":' + ('true' if enabled else 'false') + '}'
        
        # List all flags
        if op == "list":
            count = len(_flags)
            return '{"status":"ok","count":' + str(count) + '}'
        
        # Delete flag
        if op == "delete":
            if flag_name not in _flags:
                return '{"error":"flag_not_found"}'
            del _flags[flag_name]
            return '{"status":"ok"}'
        
        return '{"error":"unknown_op"}'
    
    def get_state(self) -> str:
        """Return state for durability."""
        global _flags
        return json.dumps({"flags": _flags})
    
    def set_state(self, state_json: str) -> str:
        """Restore state."""
        global _flags
        _flags = json.loads(state_json).get("flags", {})
        return ""
