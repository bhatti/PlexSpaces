# Bank Account - Durability Example (Python WASM)

Demonstrates **durable actors** with persistent state using a banking example.

**Real-world use case**: Banking, wallets, financial ledgers - where data must survive restarts.

## How WASM Actor Durability Works

WASM actors use the **Cloudflare Durable Objects pattern** - checkpoint-based state persistence via `get-state()` and `set-state()` WIT interface functions.

**Why this pattern for WASM?**
- Rust actors use DurabilityFacet with journaling + replay
- WASM actors can't use DurabilityFacet (requires fully initialized actors for replay)
- The checkpoint pattern is simpler, robust, and proven at scale (Cloudflare)

### The Simple Actor WIT Interface

```python
def get_state(self) -> str:
    """Called by framework to save state before shutdown/checkpoint."""
    return json.dumps({"balance": _balance, "transactions": _transactions})

def set_state(self, state_json: str) -> str:
    """Called by framework to restore state after restart."""
    data = json.loads(state_json)
    _balance = data.get("balance", 0)
    _transactions = data.get("transactions", [])
    return ""
```

### Durability Timeline

```
┌─────────────────────────────────────────────────────────────────┐
│           WASM Actor Lifecycle (Cloudflare DO Pattern)          │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  START: Actor created                                           │
│    └── init() called                                            │
│                                                                  │
│  OPERATIONS: Normal processing                                   │
│    ├── deposit($1000) → balance = 1000                         │
│    ├── withdraw($200) → balance = 800                          │
│    └── Actor maintains state internally                         │
│                                                                  │
│  CHECKPOINT: Framework calls get_state()                         │
│    └── Returns: {"balance": 800, "transactions": [...]}         │
│    └── State saved to checkpoint storage (SQLite)               │
│                                                                  │
│  CRASH/SHUTDOWN: Actor stops                                     │
│                                                                  │
│  RESTART: Actor recreated                                        │
│    ├── Framework loads latest checkpoint                        │
│    ├── Framework calls set_state(checkpoint_data)               │
│    └── Balance restored: 800 ✓                                  │
│                                                                  │
│  Note: Unlike Rust actors, WASM actors don't use journal         │
│        replay - state is restored from checkpoint only           │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

## Quick Start

```bash
./build.sh           # Build WASM actor
./test.sh            # Basic operations test (requires running node)
./test-durability.sh # Full durability test (restarts server)
```

### Start Node with Logging

To see backend initialization logs when starting the node:

```bash
# From workspace root
cd /path/to/tspaces

# Start node with INFO logging to see backend initialization
RUST_LOG=info cargo run -p plexspaces-cli -- start --node-id test-node --listen-addr 0.0.0.0:8091

# Expected output shows storage backends:
# INFO plexspaces_journaling: Journal storage initialized db_path="/tmp/plexspaces-test-node.db" backend="SQLite"
# INFO plexspaces_keyvalue: KeyValue storage initialized backend="SQLite"
# INFO plexspaces_blob: Blob storage initialized backend="local" bucket="plexspaces-blobs"
```

Then in another terminal, run the tests:

```bash
./test.sh 8092       # HTTP port is gRPC port + 1
```

## Test Scripts

### `test.sh` - Basic Operations (no restart)
Tests banking operations without restarting the server:
- Deposit/withdraw across 3 accounts
- Balance checks
- Transaction history
- Replay capability
- Error handling (insufficient funds)

**Requires**: A PlexSpaces node already running

### `test-durability.sh` - Full Durability Test (with restart)
Tests that state survives server restart:
1. Starts fresh node
2. Deploys accounts, does operations
3. Stops server (simulating crash)
4. Restarts server
5. Verifies balances were restored

**Note**: This script manages the server lifecycle itself.

## Multiple Actors via ApplicationSpec

This example deploys **3 bank accounts** via `app-config.toml`:

```toml
[supervisor]
strategy = "one_for_one"
max_restarts = 10

[[supervisor.children]]
id = "account-alice"

[[supervisor.children]]
id = "account-bob"

[[supervisor.children]]
id = "account-charlie"
```

## Operations

| Operation | Payload | Response |
|-----------|---------|----------|
| Deposit | `{"op":"deposit","amount":1000}` | `{"status":"ok","balance":1000}` |
| Withdraw | `{"op":"withdraw","amount":200}` | `{"status":"ok","balance":800}` |
| Balance | `{"op":"balance"}` | `{"account":"alice","balance":800}` |
| History | `{"op":"history","count":5}` | `{"transactions":[...]}` |
| Replay | `{"op":"replay"}` | `{"replayed":5,"rebuilt_balance":800}` |

## Durability Features Demonstrated

| Feature | How It Works |
|---------|--------------|
| **Persistent Balance** | `get_state()` saves balance before shutdown |
| **Crash Recovery** | `set_state()` restores balance on restart |
| **Transaction Log** | Every operation logged for audit/replay |
| **Replay** | Can rebuild state from transaction log |

## Files

| File | Description |
|------|-------------|
| `account_actor.py` | Bank account with get_state/set_state |
| `app-config.toml` | ApplicationSpec for 3 accounts |
| `build.sh` | Build WASM |
| `test.sh` | Basic operations test (no restart) |
| `test-durability.sh` | Full durability test (restarts server) |

## See Also

- [Durability Documentation](../../../../docs/durability.md) - Full durability guide (Rust + WASM patterns)
- [WASM Deployment Guide](../../../../docs/wasm-deployment.md) - Complete WASM deployment guide
- [Python WASM Guide](../../README.md) - Python WASM development
- [FSM Example](../fsm/) - State machine pattern
