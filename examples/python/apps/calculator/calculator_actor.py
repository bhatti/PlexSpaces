"""
Calculator Actor - Simple Actor Interface (Python WASM with SDK)

Demonstrates request-reply pattern with calculator operations.
Uses PlexSpaces Python SDK for minimal boilerplate.

## SDK Features Used

- @actor: Marks class as PlexSpaces actor
- state(): Defines persistent state fields
- @handler(): Routes messages to methods
"""

from plexspaces import actor, state, handler, init_handler


@actor
class Calculator:
    """Calculator actor implementing basic math operations."""
    
    # Persistent state fields
    last_operation: str = state(default=None)
    last_result: float = state(default=None)
    history: list = state(default_factory=list)
    
    @init_handler
    def on_init(self, config: dict):
        """Initialize calculator with optional config."""
        if "state" in config:
            saved_state = config["state"]
            self.last_operation = saved_state.get("last_operation")
            self.last_result = saved_state.get("last_result")
            self.history = saved_state.get("history", [])
    
    @handler("add")
    def add(self, operands: list = None) -> dict:
        """Add operands."""
        if operands is None:
            operands = []
        result = sum(operands)
        self._record("add", operands, result)
        return {"result": result, "operation": "add"}
    
    @handler("subtract")
    def subtract(self, operands: list = None) -> dict:
        """Subtract operands (first - rest)."""
        if operands is None or len(operands) < 2:
            return {"error": "Subtract requires at least 2 operands"}
        result = operands[0] - sum(operands[1:])
        self._record("subtract", operands, result)
        return {"result": result, "operation": "subtract"}
    
    @handler("multiply")
    def multiply(self, operands: list = None) -> dict:
        """Multiply operands."""
        if operands is None:
            operands = []
        result = 1
        for op in operands:
            result *= op
        self._record("multiply", operands, result)
        return {"result": result, "operation": "multiply"}
    
    @handler("divide")
    def divide(self, operands: list = None) -> dict:
        """Divide first operand by second."""
        if operands is None or len(operands) < 2:
            return {"error": "Divide requires 2 operands"}
        if operands[1] == 0:
            return {"error": "Divide requires 2 operands, divisor must be non-zero"}
        result = operands[0] / operands[1]
        self._record("divide", operands, result)
        return {"result": result, "operation": "divide"}
    
    @handler("get_history")
    def get_history(self) -> dict:
        """Get calculation history."""
        return {"history": self.history}
    
    @handler("call", "get_state")
    def get_state_handler(self) -> dict:
        """Get current state."""
        return {
            "last_operation": self.last_operation,
            "last_result": self.last_result,
            "history": self.history
        }
    
    def _record(self, operation: str, operands: list, result: float):
        """Record operation in history."""
        self.last_operation = operation
        self.last_result = result
        self.history.append({
            "operation": operation,
            "operands": operands,
            "result": result
        })
