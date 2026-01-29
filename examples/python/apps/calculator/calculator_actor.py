#!/usr/bin/env python3
"""
Calculator Actor - Simple Actor Interface

Demonstrates request-reply pattern with calculator operations.
Uses the simplified string-only WIT interface for componentize-py compatibility.
"""

import json
from wit_world import exports

# Actor state
_state = {
    "last_operation": None,
    "last_result": None,
    "history": []
}


class Actor(exports.Actor):
    """Calculator actor implementing simple-actor interface."""
    
    def init(self, config_json: str) -> str:
        """Initialize calculator with optional config."""
        global _state
        if config_json:
            try:
                config = json.loads(config_json)
                _state = config.get("state", _state)
            except Exception as e:
                return f"ERROR: Failed to parse config: {e}"
        return ""  # Success
    
    def handle(self, from_actor: str, msg_type: str, payload_json: str) -> str:
        """
        Handle calculator requests.
        
        Message types:
        - "add": Add operands
        - "subtract": Subtract operands  
        - "multiply": Multiply operands
        - "divide": Divide operands
        - "get_history": Get calculation history
        """
        global _state
        
        try:
            request = json.loads(payload_json) if payload_json else {}
            operation = request.get('operation', msg_type)
            operands = request.get('operands', [])
            
            if operation == 'add':
                result = sum(operands)
            elif operation == 'subtract':
                if len(operands) >= 2:
                    result = operands[0] - sum(operands[1:])
                else:
                    return json.dumps({'error': 'Subtract requires at least 2 operands'})
            elif operation == 'multiply':
                result = 1
                for op in operands:
                    result *= op
            elif operation == 'divide':
                if len(operands) >= 2 and operands[1] != 0:
                    result = operands[0] / operands[1]
                else:
                    return json.dumps({'error': 'Divide requires 2 operands, divisor must be non-zero'})
            elif operation == 'get_history':
                return json.dumps({'history': _state['history']})
            elif msg_type == 'call' or msg_type == 'get_state':
                return json.dumps(_state)
            else:
                return json.dumps({'error': f'Unknown operation: {operation}'})
            
            # Store result in state
            _state['last_operation'] = operation
            _state['last_result'] = result
            _state['history'].append({'operation': operation, 'operands': operands, 'result': result})
            
            return json.dumps({'result': result, 'operation': operation})
            
        except Exception as e:
            return f"ERROR: {e}"
    
    def get_state(self) -> str:
        """Get calculator state as JSON."""
        global _state
        return json.dumps(_state)
    
    def set_state(self, state_json: str) -> str:
        """Restore calculator state from JSON."""
        global _state
        try:
            _state = json.loads(state_json)
            return ""
        except Exception as e:
            return f"ERROR: Failed to restore state: {e}"
