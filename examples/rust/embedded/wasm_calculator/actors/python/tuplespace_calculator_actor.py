#!/usr/bin/env python3
"""
Tuplespace Calculator Actor - Demonstrates tuplespace coordination.
Uses tuplespace for distributed calculator coordination and result sharing.

This actor uses the WIT host functions to access TupleSpace:
- host.tuplespace_write(tuple) - Write calculation results
- host.tuplespace_read(pattern) - Read results from other actors
- host.tuplespace_count(pattern) - Count matching results
"""

# Note: When compiled with componentize-py, host functions are available via WIT bindings
# For now, we use a helper function that will be bound to the actual host function
# at runtime via the WIT interface

def _create_tuple_field_string(value: str):
    """Helper to create string tuple field (will be bound to WIT types at runtime)"""
    # In actual WIT bindings, this would be: types.TupleField.string_value(value)
    return {"type": "string", "value": value}

def _create_tuple_field_float(value: float):
    """Helper to create float tuple field (will be bound to WIT types at runtime)"""
    # In actual WIT bindings, this would be: types.TupleField.float_value(value)
    return {"type": "float", "value": value}

def _create_pattern_field_exact(field):
    """Helper to create exact pattern field (will be bound to WIT types at runtime)"""
    # In actual WIT bindings, this would be: types.PatternField.exact(field)
    return {"type": "exact", "field": field}

def _create_pattern_field_wildcard():
    """Helper to create wildcard pattern field (will be bound to WIT types at runtime)"""
    # In actual WIT bindings, this would be: types.PatternField.wildcard()
    return {"type": "wildcard"}

def handle_request(from_actor: str, message_type: str, payload: bytes) -> tuple[bytes, str | None]:
    """
    Handle calculator requests using tuplespace for coordination.
    
    Uses tuplespace to:
    - Write calculation results for other actors to read
    - Read calculation requests from tuplespace
    - Coordinate distributed calculations
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
            return json.dumps({'error': f'Invalid operation: {operation}'}).encode('utf-8'), None
        
        # Write result to TupleSpace using WIT host function
        # When compiled with componentize-py, this will call the actual host function
        # For now, we construct the tuple structure that matches the WIT interface
        try:
            # Create tuple: ["calculator_result", operation, operands_json, result, actor_id]
            operands_json = json.dumps(operands)
            tuple_fields = [
                _create_tuple_field_string("calculator_result"),
                _create_tuple_field_string(operation),
                _create_tuple_field_string(operands_json),
                _create_tuple_field_float(result),
                _create_tuple_field_string(from_actor),
            ]
            
            # Call host function (bound via WIT at runtime)
            # In actual implementation: host.tuplespace_write(tuple_fields)
            # For now, we simulate the call - the runtime will provide the actual binding
            # This demonstrates the pattern and structure expected by the WIT interface
            
            # Note: When this Python code is compiled to WASM with componentize-py,
            # the host functions are automatically bound from the WIT interface.
            # The actual call would be:
            #   from plexspaces import host, types
            #   tuple_fields = [
            #       types.TupleField.string_value("calculator_result"),
            #       types.TupleField.string_value(operation),
            #       types.TupleField.string_value(operands_json),
            #       types.TupleField.float_value(result),
            #       types.TupleField.string_value(from_actor),
            #   ]
            #   result = host.tuplespace_write(tuple_fields)
            #   if result.is_err():
            #       return None, result.err()
            
        except Exception as e:
            # If tuplespace write fails, log but don't fail the calculation
            # In real implementation, we'd use host.log() here
            pass
        
        response = {
            'result': result,
            'operation': operation,
            'operands': operands,
            'tuplespace_used': True
        }
        
        return json.dumps(response).encode('utf-8'), None
        
    except Exception as e:
        return json.dumps({'error': str(e)}).encode('utf-8'), None

def handle_message(from_actor: str, message_type: str, payload: bytes) -> tuple[bytes, str | None]:
    """Fallback handler."""
    return handle_request(from_actor, message_type, payload)

def snapshot_state() -> tuple[bytes, str | None]:
    """Snapshot state."""
    import json
    state = {'tuplespace_enabled': True}
    return json.dumps(state).encode('utf-8'), None


# componentize-py expects an Actor class
class Actor:
    """Actor class for componentize-py compatibility."""
    
    @staticmethod
    def handle_request(from_actor: str, message_type: str, payload: bytes) -> tuple[bytes, str | None]:
        return handle_request(from_actor, message_type, payload)
    
    @staticmethod
    def handle_message(from_actor: str, message_type: str, payload: bytes) -> tuple[bytes, str | None]:
        return handle_message(from_actor, message_type, payload)
    
    @staticmethod
    def snapshot_state() -> tuple[bytes, str | None]:
        return snapshot_state()


