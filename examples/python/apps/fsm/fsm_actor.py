"""
FSM Actor - Finite State Machine (Order Workflow Example)

Demonstrates a state machine pattern for order processing:
  idle -> pending -> processing -> shipped -> delivered

Real-world use case: E-commerce order tracking, payment processing, ticket workflows.

## WASM/componentize-py Memory Workarounds

This code applies workarounds for Python 3.14 WASM memory bugs:
1. String literals for simple JSON (avoid json.dumps crashes)
2. Flat control flow (avoid nested try-except)
3. Simple return values (avoid complex dict crashes)

See examples/python/README.md for full documentation.
"""

import json
from wit_world import exports

# FSM state
_state = "idle"
_order_id = ""
_items = []

# Valid transitions
TRANSITIONS = {
    "idle": ["pending"],
    "pending": ["processing", "cancelled"],
    "processing": ["shipped", "cancelled"],
    "shipped": ["delivered"],
    "delivered": [],
    "cancelled": []
}


class Actor(exports.Actor):
    """Order FSM actor."""
    
    def init(self, config_json: str) -> str:
        """Initialize FSM."""
        global _state, _order_id, _items
        _state = "idle"
        _order_id = ""
        _items = []
        return ""
    
    def handle(self, from_actor: str, msg_type: str, payload_json: str) -> str:
        """Handle FSM transitions."""
        global _state, _order_id, _items
        
        # Parse payload
        payload = {}
        if payload_json:
            payload = json.loads(payload_json)
        
        op = payload.get("op", msg_type)
        
        # Get current state
        if op == "get" or op == "status":
            return '{"state":"' + _state + '","order_id":"' + _order_id + '"}'
        
        # Create order (idle -> pending)
        if op == "create":
            if _state != "idle":
                return '{"error":"must_be_idle"}'
            _order_id = payload.get("order_id", "order-1")
            _items = payload.get("items", [])
            _state = "pending"
            return '{"status":"ok","state":"pending"}'
        
        # Start processing (pending -> processing)
        if op == "process":
            if _state != "pending":
                return '{"error":"must_be_pending"}'
            _state = "processing"
            return '{"status":"ok","state":"processing"}'
        
        # Ship order (processing -> shipped)
        if op == "ship":
            if _state != "processing":
                return '{"error":"must_be_processing"}'
            _state = "shipped"
            return '{"status":"ok","state":"shipped"}'
        
        # Deliver order (shipped -> delivered)
        if op == "deliver":
            if _state != "shipped":
                return '{"error":"must_be_shipped"}'
            _state = "delivered"
            return '{"status":"ok","state":"delivered"}'
        
        # Cancel order (pending/processing -> cancelled)
        if op == "cancel":
            if _state not in ["pending", "processing"]:
                return '{"error":"cannot_cancel"}'
            _state = "cancelled"
            return '{"status":"ok","state":"cancelled"}'
        
        # Reset (any -> idle)
        if op == "reset":
            _state = "idle"
            _order_id = ""
            _items = []
            return '{"status":"ok","state":"idle"}'
        
        # Get valid transitions
        if op == "transitions":
            valid = TRANSITIONS.get(_state, [])
            return '{"state":"' + _state + '","valid":' + json.dumps(valid) + '}'
        
        return '{"error":"unknown_op"}'
    
    def get_state(self) -> str:
        """Get FSM state."""
        global _state, _order_id
        return '{"state":"' + _state + '","order_id":"' + _order_id + '"}'
    
    def set_state(self, state_json: str) -> str:
        """Restore FSM state."""
        global _state, _order_id, _items
        data = json.loads(state_json)
        _state = data.get("state", "idle")
        _order_id = data.get("order_id", "")
        _items = data.get("items", [])
        return ""
