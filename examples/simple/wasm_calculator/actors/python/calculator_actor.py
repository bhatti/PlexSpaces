"""
Calculator Actor - GenServer Behavior
Demonstrates request-reply pattern with calculator operations.
"""

def handle_request(from_actor: str, message_type: str, payload: bytes) -> bytes:
    """
    Handle calculator requests (GenServer behavior - request-reply).
    
    Message types:
    - "add": Add operands
    - "subtract": Subtract operands
    - "multiply": Multiply operands
    - "divide": Divide operands
    """
    import json
    
    try:
        # Parse request
        request = json.loads(payload.decode('utf-8'))
        operation = request.get('operation', '')
        operands = request.get('operands', [])
        
        # Perform calculation
        if operation == 'add':
            result = sum(operands)
        elif operation == 'subtract':
            if len(operands) >= 2:
                result = operands[0] - sum(operands[1:])
            else:
                return json.dumps({'error': 'Subtract requires at least 2 operands'}).encode('utf-8')
        elif operation == 'multiply':
            result = 1
            for op in operands:
                result *= op
        elif operation == 'divide':
            if len(operands) >= 2 and operands[1] != 0:
                result = operands[0] / operands[1]
            else:
                return json.dumps({'error': 'Divide requires 2 operands, divisor must be non-zero'}).encode('utf-8')
        else:
            return json.dumps({'error': f'Unknown operation: {operation}'}).encode('utf-8')
        
        # Return result
        response = {'result': result, 'operation': operation}
        return json.dumps(response).encode('utf-8')
        
    except Exception as e:
        return json.dumps({'error': str(e)}).encode('utf-8')


def handle_event(from_actor: str, message_type: str, payload: bytes) -> None:
    """
    Handle calculator events (GenEvent behavior - fire-and-forget).
    Logs calculation events without returning a response.
    """
    import json
    
    try:
        request = json.loads(payload.decode('utf-8'))
        operation = request.get('operation', '')
        operands = request.get('operands', [])
        
        # Log the event (in real implementation, this would use host::log)
        print(f"Calculator event: {operation} with operands {operands}")
        
    except Exception as e:
        print(f"Error processing calculator event: {e}")


def handle_transition(from_actor: str, message_type: str, payload: bytes) -> str:
    """
    Handle calculator state transitions (GenFSM behavior).
    Manages calculator state (idle, calculating, error).
    """
    import json
    
    try:
        request = json.loads(payload.decode('utf-8'))
        operation = request.get('operation', '')
        
        # Simple state machine: idle -> calculating -> idle
        # Or: idle -> error -> idle
        if operation == 'start':
            return 'calculating'
        elif operation == 'complete':
            return 'idle'
        elif operation == 'error':
            return 'error'
        elif operation == 'reset':
            return 'idle'
        else:
            return ''  # No state transition
            
    except Exception as e:
        return 'error'


def snapshot_state() -> bytes:
    """Snapshot calculator state for persistence."""
    import json
    # In a real implementation, this would return the current state
    state = {'last_operation': None, 'result': 0}
    return json.dumps(state).encode('utf-8')































