"""
Advanced Calculator Actor - Demonstrates GenFSM behavior with state machine.
Shows calculator with different states: idle, calculating, error.
"""

# State machine states
STATES = ['idle', 'calculating', 'error']

def handle_transition(from_actor: str, message_type: str, payload: bytes) -> str:
    """
    Handle state transitions for calculator FSM.
    
    States:
    - idle: Ready to accept calculations
    - calculating: Currently processing
    - error: Error state, needs reset
    """
    import json
    
    try:
        request = json.loads(payload.decode('utf-8'))
        operation = request.get('operation', '')
        current_state = request.get('current_state', 'idle')
        
        # State transition logic
        if current_state == 'idle':
            if operation in ['add', 'subtract', 'multiply', 'divide']:
                return 'calculating'
        elif current_state == 'calculating':
            if operation == 'complete':
                return 'idle'
            elif operation == 'error':
                return 'error'
        elif current_state == 'error':
            if operation == 'reset':
                return 'idle'
        
        return ''  # No state transition
        
    except Exception as e:
        return 'error'


def handle_request(from_actor: str, message_type: str, payload: bytes) -> bytes:
    """
    Handle calculator requests in calculating state.
    """
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
            return json.dumps({'error': f'Invalid operation: {operation}'}).encode('utf-8')
        
        response = {'result': result, 'operation': operation, 'state': 'calculating'}
        return json.dumps(response).encode('utf-8')
        
    except Exception as e:
        return json.dumps({'error': str(e), 'state': 'error'}).encode('utf-8')


def snapshot_state() -> bytes:
    """Snapshot FSM state."""
    import json
    state = {'current_state': 'idle', 'last_result': 0}
    return json.dumps(state).encode('utf-8')































