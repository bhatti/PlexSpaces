# Calculator Actor (Python WASM)

A simple calculator demonstrating Python WASM Components in PlexSpaces.

## Overview

This example shows:
- Building Python actors with componentize-py
- Using the simple-actor WIT interface (JSON-over-string pattern)
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
cargo run -p plexspaces -- start --node-id test-node --listen-addr '0.0.0.0:8090'
```

### 3. Run Tests

```bash
./test.sh
```

## Calculator Operations

The calculator accepts JSON payloads with an `operation` field:

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
# Send operation (POST = fire-and-forget)
curl -X POST "http://localhost:8091/api/v1/actors/internal/system/calculator" \
    -H "Content-Type: application/json" \
    -d '{"operation": "add", "operands": [1, 2, 3]}'
```

## Actor Interface

Implements `plexspaces:simple-actor@0.1.0`:

```python
class Actor(exports.Actor):
    def init(self, config_json: str) -> str:
        """Initialize with optional config. Returns "" on success."""
        
    def handle(self, from_actor: str, msg_type: str, payload_json: str) -> str:
        """Handle message. Returns JSON response."""
        
    def get_state(self) -> str:
        """Returns state as JSON."""
        
    def set_state(self, state_json: str) -> str:
        """Restores state. Returns "" on success."""
```

## Files

| File | Description |
|------|-------------|
| `calculator_actor.py` | Python actor implementation |
| `build.sh` | Build script using componentize-py |
| `test.sh` | Integration test script |
| `calculator_actor.wasm` | Built WASM component (~35MB) |

## References

- [Python WASM README](../../README.md) - Full documentation
- [WIT Interface](../../../../wit/plexspaces-simple-actor/) - Interface definition
- [PlexSpaces Docs](../../../../docs/) - Framework documentation
