# Bank Account - Durability Example (Python WASM with SDK)

Demonstrates **durable actors** with persistent state using a banking example.

**Real-world use case**: Banking, wallets, financial ledgers - where data must survive restarts.

## PlexSpaces Python SDK

This example uses the [PlexSpaces Python SDK](../../../../sdks/python/README.md) for minimal boilerplate:

```python
from plexspaces import actor, state, handler, init_handler

@actor(facets=["durability"])  # Declares this actor expects durability
class BankAccount:
    # State fields are automatically persisted
    balance: int = state(default=0)
    transactions: list = state(default_factory=list)
    
    @handler("deposit")
    def deposit(self, amount: int = 0) -> dict:
        self.balance += amount
        return {"balance": self.balance}
```

**Before SDK**: 150+ lines with manual WIT interface  
**After SDK**: ~90 lines with decorators

## Durability Configuration

### WASM vs Rust Durability

| Aspect | WASM Actors | Rust Actors |
|--------|-------------|-------------|
| **Mechanism** | Checkpoint-based (get_state/set_state) | DurabilityFacet with journal |
| **Configuration** | `WasmConfig.durability_enabled` | `facets = [{ type = "durability" }]` in app-config |
| **Storage** | SQLite checkpoint store | Journal storage (SQLite/Postgres) |
| **Annotation** | `@actor(facets=["durability"])` | N/A (Rust uses app-config) |

### How to Enable WASM Durability

**Option 1: Release config (release.yaml)**
```yaml
wasm:
  durability_enabled: true
```

**Option 2: Application spec**
```toml
# app-config.toml
[wasm]
durability_enabled = true
```

**Option 3: Node config (config/default.yaml)**
```yaml
wasm:
  durability_enabled: true
```

The `facets=["durability"]` annotation in the Python code is for **documentation and validation** - it declares that this actor expects durability to be enabled. The actual durability is provided by the node configuration.

### How WASM Durability Works

WASM actors use the **Cloudflare Durable Objects pattern** - checkpoint-based state persistence.

State fields defined with `state()` are automatically:
- Serialized via `get_state()` before shutdown
- Restored via `set_state()` on restart

### Durability Timeline

```
START: Actor created
  └── @init_handler called with config

OPERATIONS: Normal processing
  ├── deposit($1000) → balance = 1000
  ├── withdraw($200) → balance = 800
  └── State tracked in state() fields

CHECKPOINT: Framework calls get_state()
  └── SDK auto-serializes all state() fields
  └── State saved to checkpoint storage (SQLite)

RESTART: Actor recreated
  ├── Framework loads latest checkpoint
  ├── SDK auto-restores state() fields
  └── Balance restored: 800
```

## Quick Start

```bash
./build.sh           # Build WASM actor
./test.sh            # Basic operations test (requires running node)
./test-durability.sh # Full durability test (restarts server)
```

### Start Node

```bash
# From workspace root
RUST_LOG=info cargo run -p plexspaces-cli -- start --node-id test-node --listen-addr 0.0.0.0:8091
```

Then in another terminal:
```bash
./test.sh 8091       # gRPC and HTTP share a single port
```

## Operations

| Operation | Payload | Response |
|-----------|---------|----------|
| Deposit | `{"op":"deposit","amount":1000}` | `{"status":"ok","balance":1000}` |
| Withdraw | `{"op":"withdraw","amount":200}` | `{"status":"ok","balance":800}` |
| Balance | `{"op":"balance"}` | `{"account":"alice","balance":800}` |
| History | `{"op":"history","count":5}` | `{"transactions":[...]}` |
| Replay | `{"op":"replay"}` | `{"replayed":5,"rebuilt_balance":800}` |

## SDK Features Demonstrated

| Feature | How It's Used |
|---------|---------------|
| `@actor(facets=["durability"])` | Marks `BankAccount` as durable PlexSpaces actor |
| `state()` | Defines `balance`, `transactions` as persistent fields |
| `@handler()` | Routes `deposit`, `withdraw`, `balance` messages |
| `@init_handler` | Initializes account from config |

### Facets Parameter

The `facets=["durability"]` parameter:
- Documents that this actor expects durability to be enabled
- Can be used by tooling to validate app-config matches actor expectations
- Does NOT automatically enable durability (that's done via node/release config)

## Files

| File | Description |
|------|-------------|
| `account_actor.py` | Bank account using SDK decorators |
| `app-config.toml` | ApplicationSpec for 3 accounts |
| `build.sh` | Build using `plexspaces-py build` |
| `test.sh` | Basic operations test |
| `test-durability.sh` | Full durability test |

## See Also

- [PlexSpaces Python SDK](../../../../sdks/python/README.md) - SDK documentation
- [SDK Guide](../../../../docs/sdk.md) - Complete SDK reference
- [Durability Documentation](../../../../docs/durability.md) - Durability patterns
- [Python WASM Guide](../../README.md) - Python WASM development
