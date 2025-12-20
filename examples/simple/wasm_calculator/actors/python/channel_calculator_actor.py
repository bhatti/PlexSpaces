#!/usr/bin/env python3
"""
Channel Calculator Actor - Demonstrates channel service usage.
Uses queues and topics for calculator operation distribution.
"""

def handle_request(from_actor: str, message_type: str, payload: bytes) -> tuple[bytes, str | None]:
    """
    Handle calculator requests using channel service.
    
    Uses channels to:
    - Send calculation requests to queue (load-balanced)
    - Publish calculation results to topic (pub/sub)
    - Receive calculation requests from queue
    """
    import json
    
    try:
        request = json.loads(payload.decode('utf-8'))
        operation = request.get('operation', '')
        operands = request.get('operands', [])
        use_channels = request.get('use_channels', False)
        
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
        
        response = {
            'result': result,
            'operation': operation,
            'operands': operands,
            'channels_used': use_channels
        }
        
        # In a real implementation, we would use host functions:
        # host::send_to_queue("calc-queue", "result", json.dumps(response))
        # host::publish_to_topic("calc-results", "result", json.dumps(response))
        # This demonstrates the pattern
        
        return json.dumps(response).encode('utf-8'), None
        
    except Exception as e:
        return json.dumps({'error': str(e)}).encode('utf-8'), None

def handle_message(from_actor: str, message_type: str, payload: bytes) -> tuple[bytes, str | None]:
    """Fallback handler."""
    return handle_request(from_actor, message_type, payload)

def snapshot_state() -> tuple[bytes, str | None]:
    """Snapshot state."""
    import json
    state = {'channels_enabled': True}
    return json.dumps(state).encode('utf-8'), None













































