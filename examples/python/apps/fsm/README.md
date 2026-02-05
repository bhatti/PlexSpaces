# FSM Actor - Order Workflow State Machine (Python WASM with SDK)

A finite state machine demonstrating order processing workflow using PlexSpaces SDK.

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

## SDK Implementation

```python
from plexspaces import fsm_actor, state, handler

@fsm_actor  # GenStateMachine behavior for state machines
class OrderFSM:
    current_state: str = state(default="idle")
    order_id: str = state(default="")
    
    @handler("create")
    def create_order(self, order_id: str = "order-1") -> dict:
        if self.current_state != "idle":
            return {"error": "must_be_idle"}
        self.order_id = order_id
        self.current_state = "pending"
        return {"status": "ok", "state": "pending"}
```

**Before SDK**: 128 lines with manual WIT interface  
**After SDK**: 95 lines with decorators (cleaner transitions)

### Why @fsm_actor?

| Decorator | Behavior Type | Use Case |
|-----------|--------------|----------|
| `@actor` | GenServer | Request-reply actors |
| `@fsm_actor` | GenStateMachine | State machine workflows |

`@fsm_actor` sets `behavior_type = GenStateMachine`, indicating this actor follows the state machine pattern with well-defined transitions.

## Operations

| Operation | Transition | Response |
|-----------|------------|----------|
| create | idle → pending | `{"status":"ok","state":"pending"}` |
| process | pending → processing | `{"status":"ok","state":"processing"}` |
| ship | processing → shipped | `{"status":"ok","state":"shipped"}` |
| deliver | shipped → delivered | `{"status":"ok","state":"delivered"}` |
| cancel | pending/processing → cancelled | `{"status":"ok","state":"cancelled"}` |
| reset | any → idle | `{"status":"ok","state":"idle"}` |
| status | - | `{"state":"pending","order_id":"ORD-001"}` |
| transitions | - | `{"state":"pending","valid":["processing","cancelled"]}` |

## State Transitions

| From State | Valid Transitions |
|------------|-------------------|
| idle | pending |
| pending | processing, cancelled |
| processing | shipped, cancelled |
| shipped | delivered |
| delivered | (terminal) |
| cancelled | (terminal) |

## SDK Features Demonstrated

| Feature | How It's Used |
|---------|---------------|
| `@fsm_actor` | Marks `OrderFSM` as GenStateMachine actor |
| `state()` | Defines `current_state`, `order_id`, `items` |
| `@handler()` | Routes `create`, `process`, `ship`, `deliver` |

## Files

| File | Description |
|------|-------------|
| `fsm_actor.py` | FSM using SDK decorators |
| `build.sh` | Build using `plexspaces-py build` |
| `test.sh` | Integration test |

## See Also

- [PlexSpaces Python SDK](../../../../sdks/python/README.md) - SDK documentation
- [SDK Guide](../../../../docs/sdk.md) - Complete SDK reference
- [Feature Flags Example](../feature_flags/) - Similar pattern
