#!/usr/bin/env python3
"""
Durable Calculator Actor - Demonstrates durability/journaling features.
Shows how calculator state is persisted and can be recovered.
"""

# Actor state
_calculator_state = {
    'last_operation': None,
    'last_result': 0.0,
    'operation_count': 0,
    'history': []
}

def init(initial_state: bytes) -> tuple[None, str | None]:
    """Initialize actor with persisted state."""
    global _calculator_state
    try:
        if initial_state:
            import json
            _calculator_state = json.loads(initial_state.decode('utf-8'))
        else:
            _calculator_state = {
                'last_operation': None,
                'last_result': 0.0,
                'operation_count': 0,
                'history': []
            }
        return None, None
    except Exception as e:
        return None, f"Init failed: {str(e)}"

def handle_request(from_actor: str, message_type: str, payload: bytes) -> tuple[bytes, str | None]:
    """
    Handle calculator requests with state persistence.
    State is automatically journaled by DurabilityFacet.
    """
    global _calculator_state
    import json
    
    try:
        request = json.loads(payload.decode('utf-8'))
        operation = request.get('operation', '')
        operands = request.get('operands', [])
        
        # Perform calculation
        if operation == 'add':
            result = sum(operands)
        elif operation == 'subtract' and len(operands) >= 2:
            result = operands[0] - sum(operands[1:])
        elif operation == 'multiply':
            result = 1
            for op in operands:
                result *= op
        elif operation == 'divide' and len(operands) >= 2 and operands[1] != 0:
            result = operands[0] / operands[1]
        else:
            return json.dumps({'error': f'Invalid operation: {operation}'}).encode('utf-8'), None
        
        # Update state (will be journaled)
        _calculator_state['last_operation'] = operation
        _calculator_state['last_result'] = result
        _calculator_state['operation_count'] += 1
        _calculator_state['history'].append({
            'operation': operation,
            'operands': operands,
            'result': result
        })
        
        # Keep history limited
        if len(_calculator_state['history']) > 10:
            _calculator_state['history'] = _calculator_state['history'][-10:]
        
        response = {
            'result': result,
            'operation': operation,
            'operation_count': _calculator_state['operation_count'],
            'durable': True  # Indicates state is persisted
        }
        return json.dumps(response).encode('utf-8'), None
        
    except Exception as e:
        return json.dumps({'error': str(e)}).encode('utf-8'), None

def snapshot_state() -> tuple[bytes, str | None]:
    """Snapshot calculator state for checkpointing."""
    global _calculator_state
    try:
        import json
        return json.dumps(_calculator_state).encode('utf-8'), None
    except Exception as e:
        return b"", f"Snapshot failed: {str(e)}"

def shutdown() -> tuple[None, str | None]:
    """Graceful shutdown - state is already persisted."""
    return None, None













































