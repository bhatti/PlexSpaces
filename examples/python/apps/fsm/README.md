# FSM Actor - Order Workflow State Machine (Python WASM)

A finite state machine demonstrating order processing workflow with state transitions.

**Real-world use case**: E-commerce order tracking, payment processing, ticket workflows.

## State Diagram

```
┌───────┐  create   ┌─────────┐  process  ┌────────────┐
│ idle  │ ────────► │ pending │ ────────► │ processing │
└───────┘           └────┬────┘           └─────┬──────┘
    ▲                    │                      │
    │                    │ cancel               │ ship
    │                    ▼                      ▼
    │              ┌───────────┐          ┌─────────┐
    │              │ cancelled │          │ shipped │
    │              └───────────┘          └────┬────┘
    │                    ▲                     │
    │ reset              │ cancel              │ deliver
    │                    │                     ▼
    └────────────────────┴───────────── ┌───────────┐
                                        │ delivered │
                                        └───────────┘
```

## Quick Start

```bash
./build.sh  # Build WASM actor
./test.sh   # Run tests (requires PlexSpaces node)
```

## Operations

### Create Order (idle → pending)
```json
{"op": "create", "order_id": "ORD-001", "items": ["widget", "gadget"]}
```

### Process Order (pending → processing)
```json
{"op": "process"}
```

### Ship Order (processing → shipped)
```json
{"op": "ship"}
```

### Deliver Order (shipped → delivered)
```json
{"op": "deliver"}
```

### Cancel Order (pending/processing → cancelled)
```json
{"op": "cancel"}
```

### Get Current State
```json
{"op": "status"}
```
Response:
```json
{"state": "processing", "order_id": "ORD-001"}
```

### Reset (any → idle)
```json
{"op": "reset"}
```

### Get Valid Transitions
```json
{"op": "transitions"}
```
Response:
```json
{"state": "pending", "valid": ["processing", "cancelled"]}
```

## State Transitions

| From State | Valid Transitions |
|------------|-------------------|
| idle | pending |
| pending | processing, cancelled |
| processing | shipped, cancelled |
| shipped | delivered |
| delivered | (none - terminal) |
| cancelled | (none - terminal) |

## Example Workflow

```bash
# 1. Create order
curl -X POST "http://localhost:8091/api/v1/actors/internal/system/fsm" \
  -H "Content-Type: application/json" \
  -d '{"op":"create","order_id":"ORD-001"}'

# 2. Process it
curl -X POST ... -d '{"op":"process"}'

# 3. Ship it
curl -X POST ... -d '{"op":"ship"}'

# 4. Mark delivered
curl -X POST ... -d '{"op":"deliver"}'
```

## Files

| File | Description |
|------|-------------|
| `fsm_actor.py` | FSM implementation |
| `build.sh` | Build with componentize-py |
| `test.sh` | Integration test |

## Known Issues: componentize-py Memory Bugs

This actor applies workarounds for Python 3.14 WASM memory bugs:
- Uses string literals for simple JSON responses
- Avoids nested try-except blocks
- Keeps return values simple

See [Python WASM Guide](../../README.md) for full documentation.

## See Also

- [Python WASM Guide](../../README.md)
- [Feature Flags Example](../feature_flags/) - Similar pattern with rollouts
- [Receipt Storage Example](../receipt_storage/) - CRUD pattern
