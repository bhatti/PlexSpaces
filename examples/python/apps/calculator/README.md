# Calculator Actor (Python WASM with SDK)

A simple calculator demonstrating Python WASM actors using the PlexSpaces SDK.

## Overview

This example shows:
- Building Python actors with PlexSpaces SDK
- Using `@actor`, `state()`, and `@handler()` decorators
- Calculator operations: add, subtract, multiply, divide
- State management and history tracking

## Quick Start

### 1. Build

```bash
./build.sh
```

### 2. Start PlexSpaces Node

```bash
cd /path/to/tspaces
cargo run -p plexspaces-cli -- start --node-id test-node --listen-addr '0.0.0.0:8090'
```

### 3. Run Tests

```bash
./test.sh
```

## SDK Implementation

```python
from plexspaces import actor, state, handler

@actor
class Calculator:
    history: list = state(default_factory=list)
    
    @handler("add")
    def add(self, operands: list = None) -> dict:
        result = sum(operands or [])
        return {"result": result, "operation": "add"}
```

**Before SDK**: 98 lines with manual WIT interface  
**After SDK**: 90 lines with decorators (cleaner, type-safe)

## Calculator Operations

```json
// Addition
{"operation": "add", "operands": [10, 20, 30]}
// Returns: {"result": 60, "operation": "add"}

// Subtraction
{"operation": "subtract", "operands": [100, 25]}
// Returns: {"result": 75, "operation": "subtract"}

// Multiplication
{"operation": "multiply", "operands": [5, 6, 7]}
// Returns: {"result": 210, "operation": "multiply"}

// Division
{"operation": "divide", "operands": [100, 4]}
// Returns: {"result": 25.0, "operation": "divide"}

// Get history
{"operation": "get_history"}
// Returns: {"history": [...]}
```

## HTTP API

```bash
# Send operation
curl -X POST "http://localhost:8092/api/v1/actors/calculator-test/calculator-test" \
    -H "Content-Type: application/json" \
    -d '{"operation": "add", "operands": [1, 2, 3]}'
```

## SDK Features Demonstrated

| Feature | How It's Used |
|---------|---------------|
| `@actor` | Marks `Calculator` as PlexSpaces actor |
| `state()` | Defines `history`, `last_result` as persistent |
| `@handler()` | Routes `add`, `subtract`, `multiply`, `divide` |
| `@init_handler` | Restores state from config |

## Files

| File | Description |
|------|-------------|
| `calculator_actor.py` | Python actor using SDK decorators |
| `build.sh` | Build using `plexspaces-py build` |
| `test.sh` | Integration test script |

## References

- [PlexSpaces Python SDK](../../../../sdks/python/README.md) - SDK documentation
- [SDK Guide](../../../../docs/sdk.md) - Complete SDK reference
- [Python WASM README](../../README.md) - Python WASM development
