# Building Polyglot Applications with WebAssembly: How PlexSpaces Turns Four Languages into One Distributed Runtime

*By Shahzad Bhatti*

---

I spent three decades watching the same pattern repeat itself. Teams pick a language. They build services. They hit a wall — the Python team needs Rust's performance, the Go team needs Python's ML libraries, and the TypeScript team needs both. So they stitch together microservices with REST calls, gRPC bridges, and message queues, adding latency and complexity at every seam.

WebAssembly changes this equation entirely. It compiles code from any language into a universal binary format, runs it in a sandboxed environment, and delivers near-native performance — all without containers, VMs, or language-specific runtimes cluttering your production stack.

When I built [PlexSpaces](https://github.com/bhatti/PlexSpaces), I designed its polyglot layer on top of WebAssembly and the WASI Component Model. Today, you write actors in **Python, Rust, Go, or TypeScript**, compile them to WASM, and deploy them to the same runtime. The framework handles persistence, fault tolerance, supervision, and scaling — regardless of which language produced the binary.

This post walks you through the core concepts of WebAssembly, shows how PlexSpaces leverages the Component Model for polyglot development, and then gets hands-on: we build, test, and deploy a real application in each of the four supported languages.

---

## Why WebAssembly Matters Beyond the Browser

Most developers first encountered WebAssembly as a browser technology — a way to run C++ games or Rust crypto libraries inside Chrome. But the server-side story has matured dramatically.

### The Component Model: WebAssembly's Missing Piece

Early WebAssembly only understood numbers. You passed integers and floats across the boundary, and that was it. The **WebAssembly Component Model** fixes this limitation by defining rich, typed interfaces that components use to communicate. Think of it as an IDL (Interface Definition Language) for WASM — but one that works across every language that compiles to WebAssembly.

The key building blocks:

- **WIT (WebAssembly Interface Types)**: A language for defining typed function signatures across components. A function defined in WIT can accept strings, records, lists, variants, and enums — not just raw bytes. WIT bridges the type systems of Rust, Python, Go, and TypeScript into a single, shared contract.

- **Components**: Self-contained WASM modules that declare their imports (what they need from the host) and exports (what they provide). A Rust component and a Python component that implement the same WIT interface become interchangeable at the binary level.

- **WASI (WebAssembly System Interface)**: The standardized API that gives WASM modules access to system resources — file I/O, networking, clocks, and random number generation — without breaking the sandbox. WASI Preview 2 shipped in 2024 with HTTP, filesystem, and socket support. WASI 0.3, released in February 2026, added native async support for concurrent I/O.

### What This Means in Practice

You compile a Python actor and a Rust actor to WASM. Both implement the same WIT interface. The runtime loads them identically, calls the same exported functions, and provides the same host capabilities — messaging, key-value storage, tuple spaces, distributed locks. The Python actor handles ML inference; the Rust actor handles high-throughput event processing. They communicate through PlexSpaces message passing without knowing or caring which language sits on the other side.

This is not "Write Once, Run Anywhere" in the old Java sense. This is "Write in Whatever Language Fits, Run Together on the Same Runtime."

---

## How PlexSpaces Makes It Work

PlexSpaces is a unified distributed actor framework that combines patterns from Erlang/OTP, Orleans, Temporal, and modern serverless architectures into a single abstraction. I described the five foundational pillars in my [earlier post](https://shahbhat.medium.com/building-plexspaces-decades-of-distributed-systems-distilled-into-one-framework-a63132040dd8): TupleSpace coordination, Erlang/OTP supervision, durable execution, WASM runtime, and Firecracker isolation. Here I focus on the WASM layer and how it enables polyglot development.

### Architecture at a Glance

```
┌─────────────────────────────────────────────────────────┐
│                    PlexSpaces Node                       │
├─────────────────────────────────────────────────────────┤
│                                                         │
│  ┌───────────────────────────────────────────────────┐  │
│  │  WASM Runtime (wasmtime)                          │  │
│  │  • Loads and executes WASM modules                │  │
│  │  • Provides WIT host functions                    │  │
│  │  • Enforces resource limits and sandboxing        │  │
│  └───────────────────────────────────────────────────┘  │
│                          │                              │
│                          ▼                              │
│  ┌───────────────────────────────────────────────────┐  │
│  │  Polyglot WASM Actors                             │  │
│  │                                                   │  │
│  │  ┌──────────┐ ┌──────────┐ ┌────────┐ ┌───────┐  │  │
│  │  │ Python   │ │TypeScript│ │  Rust  │ │  Go   │  │  │
│  │  │ Actor    │ │  Actor   │ │ Actor  │ │ Actor │  │  │
│  │  └──────────┘ └──────────┘ └────────┘ └───────┘  │  │
│  │        │            │           │          │      │  │
│  │        └────────────┼───────────┼──────────┘      │  │
│  │                     ▼                             │  │
│  │  ┌───────────────────────────────────────────┐    │  │
│  │  │  WIT Host Functions (Unified Interface)   │    │  │
│  │  │  messaging · tuplespace · keyvalue        │    │  │
│  │  │  blob · channels · durability · locks     │    │  │
│  │  │  process-groups · registry · workflow      │    │  │
│  │  └───────────────────────────────────────────┘    │  │
│  └───────────────────────────────────────────────────┘  │
│                          │                              │
│                          ▼                              │
│  ┌───────────────────────────────────────────────────┐  │
│  │  Framework Services                               │  │
│  │  ActorService · TupleSpace · KeyValue · Blob      │  │
│  │  Channels · Locks · Registry · Workflow            │  │
│  └───────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────┘
```

Every actor — regardless of source language — targets the same **WIT world** (`plexspaces-simple-actor`). The runtime provides ten categories of host functions through WIT interfaces:

| WIT Interface | Purpose | Key Operations |
|---------------|---------|----------------|
| **Messaging** | Actor-to-actor communication | `send_message`, `ask_message`, `spawn_actor` |
| **TupleSpace** | Linda-style distributed coordination | `write`, `read`, `take`, `read_all` |
| **KeyValue** | Distributed key-value storage | `get`, `put`, `delete`, `compare_and_swap` |
| **Blob** | Large object storage | `upload`, `download`, `list_blobs` |
| **Channels** | Queues and pub/sub | `send_to_queue`, `publish_to_topic` |
| **Durability** | State persistence and recovery | `persist`, `checkpoint`, `is_replaying` |
| **Locks** | Distributed locking | `acquire`, `release`, `try_acquire` |
| **ProcessGroups** | Actor group management | `create_group`, `join_group`, `publish_to_group` |
| **Registry** | Service discovery | `register`, `lookup`, `discover` |
| **Workflow** | Durable orchestration | `start_workflow`, `schedule_activity`, `await_workflow` |

The critical insight: these host functions form the same contract for every language. A Python actor calls `host.keyvalue_put(ctx, key, value)` through the same WIT binding that a Rust actor calls. The framework multiplexes all of them onto the same backend services — the same distributed key-value store, the same tuple space, the same journal.

### Language Toolchains

Each language uses a different compiler to produce WASM, but the output targets the same runtime:

| Language | Compiler | WASM Size | Performance | Best For |
|----------|----------|-----------|-------------|----------|
| **Rust** | `cargo` (wasm32-wasip2) | 100KB–1MB | Excellent | Production, performance-critical paths |
| **Go** | `tinygo` | 2–5MB | Good | Balanced performance, fast iteration |
| **TypeScript** | `jco componentize` | 500KB–2MB | Good | Web integration, rapid development |
| **Python** | `componentize-py` | 30–40MB | Moderate | ML inference, data processing, prototyping |

Now let's build something real in each language.

---

## Python: A Calculator Actor with the SDK

Python shines for rapid prototyping and data-heavy workloads. The PlexSpaces Python SDK uses decorators (`@actor`, `@handler`, `state()`) that eliminate boilerplate and let you focus on business logic.

### The Actor Code

```python
# calculator_actor.py
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
            saved = config["state"]
            self.last_operation = saved.get("last_operation")
            self.last_result = saved.get("last_result")
            self.history = saved.get("history", [])

    @handler("add")
    def add(self, operands: list = None) -> dict:
        """Add operands together."""
        result = sum(operands or [])
        self._record("add", operands, result)
        return {"result": result, "operation": "add"}

    @handler("subtract")
    def subtract(self, operands: list = None) -> dict:
        """Subtract: first operand minus rest."""
        if not operands or len(operands) < 2:
            return {"error": "Subtract requires at least 2 operands"}
        result = operands[0] - sum(operands[1:])
        self._record("subtract", operands, result)
        return {"result": result, "operation": "subtract"}

    @handler("multiply")
    def multiply(self, operands: list = None) -> dict:
        """Multiply all operands."""
        result = 1
        for op in (operands or []):
            result *= op
        self._record("multiply", operands, result)
        return {"result": result, "operation": "multiply"}

    @handler("divide")
    def divide(self, operands: list = None) -> dict:
        """Divide first operand by second."""
        if not operands or len(operands) < 2:
            return {"error": "Divide requires 2 operands"}
        if operands[1] == 0:
            return {"error": "Division by zero"}
        result = operands[0] / operands[1]
        self._record("divide", operands, result)
        return {"result": result, "operation": "divide"}

    @handler("get_history")
    def get_history(self) -> dict:
        """Return calculation history."""
        return {"history": self.history}

    @handler("call", "get_state")
    def get_state_handler(self) -> dict:
        """Snapshot current state."""
        return {
            "last_operation": self.last_operation,
            "last_result": self.last_result,
            "history": self.history,
        }

    def _record(self, operation, operands, result):
        self.last_operation = operation
        self.last_result = result
        self.history.append({
            "operation": operation,
            "operands": operands,
            "result": result,
        })
```

Notice how the `@actor` decorator marks the class, `state()` declares persistent fields that survive crashes, and each `@handler("operation")` routes incoming messages to the right method. The SDK handles WIT serialization, state checkpointing, and all the plumbing underneath.

### Build and Deploy

```bash
# Install the Python-to-WASM compiler
pip install componentize-py

# Compile to a WASM component
componentize-py calculator_actor.py \
    -o calculator_actor.wasm \
    --wit wit/plexspaces-actor/actor.wit

# Start a PlexSpaces node
cargo run --release --bin plexspaces -- start \
    --node-id calc-node \
    --listen-addr 0.0.0.0:8000

# Deploy the WASM module
curl -X POST http://localhost:8001/api/v1/applications/deploy \
    -F "application_id=calculator-app" \
    -F "name=calculator" \
    -F "version=1.0.0" \
    -F "wasm_file=@calculator_actor.wasm"
```

### Send a Request

```bash
curl -X POST http://localhost:8001/api/v1/actors/calculator-app/ask \
    -H "Content-Type: application/json" \
    -d '{"message_type": "add", "payload": {"operands": [10, 20, 30]}}'

# Response: {"result": 60, "operation": "add"}
```

The actor processes the request, updates its persistent state, and returns the result. If the node crashes and restarts, the framework replays the journal and restores the calculator's state — including its full history — exactly as it was.

---

## TypeScript: A Bank Account with Durable State

TypeScript brings type safety and rapid development. The PlexSpaces TypeScript SDK uses an inheritance-based pattern: extend `PlexSpacesActor`, implement `on<Operation>()` handlers, and the SDK wires everything to WIT.

### The Actor Code

```typescript
// account_actor.ts
import { PlexSpacesActor } from "@plexspaces/sdk";

interface Transaction {
  type: string;
  amount: number;
  balance_after: number;
}

interface BankAccountState {
  account_id: string;
  balance: number;
  transactions: Transaction[];
}

export class BankAccountActor extends PlexSpacesActor<BankAccountState> {
  getDefaultState(): BankAccountState {
    return { account_id: "", balance: 0, transactions: [] };
  }

  protected override onInit(config: Record<string, unknown>): void {
    this.state.account_id = String(config.account_id ?? "");
    this.state.balance = 0;
    this.state.transactions = [];
  }

  onBalance(): Record<string, unknown> {
    return { account: this.state.account_id, balance: this.state.balance };
  }

  onDeposit(payload: Record<string, unknown>): Record<string, unknown> {
    const amount = Number(payload.amount ?? 0);
    if (amount <= 0) return { error: "invalid_amount" };
    this.state.balance += amount;
    this.state.transactions.push({
      type: "deposit",
      amount,
      balance_after: this.state.balance,
    });
    return { status: "ok", balance: this.state.balance };
  }

  onWithdraw(payload: Record<string, unknown>): Record<string, unknown> {
    const amount = Number(payload.amount ?? 0);
    if (amount <= 0) return { error: "invalid_amount" };
    if (amount > this.state.balance) {
      return { error: "insufficient_funds", balance: this.state.balance };
    }
    this.state.balance -= amount;
    this.state.transactions.push({
      type: "withdraw",
      amount,
      balance_after: this.state.balance,
    });
    return { status: "ok", balance: this.state.balance };
  }

  onHistory(payload: Record<string, unknown>): Record<string, unknown> {
    const count = Math.min(
      Number(payload.count ?? 5),
      this.state.transactions.length
    );
    return { transactions: this.state.transactions.slice(-count) };
  }

  onReplay(): Record<string, unknown> {
    let rebuilt = 0;
    for (const tx of this.state.transactions) {
      if (tx.type === "deposit") rebuilt += tx.amount;
      else if (tx.type === "withdraw") rebuilt -= tx.amount;
    }
    return {
      replayed: this.state.transactions.length,
      rebuilt_balance: rebuilt,
      current_balance: this.state.balance,
    };
  }
}

// WIT actor export
const instance = new BankAccountActor();
export const actor = {
  init: (c: string) => instance.init(c),
  handle: (from: string, msg: string, payload: string) =>
    instance.handle(from, msg, payload),
  getState: () => instance.getState(),
  setState: (s: string) => instance.setState(s),
};
```

The `BankAccountActor` manages deposits, withdrawals, and transaction history with full durability. The `onReplay()` handler rebuilds the balance from the transaction log — demonstrating event-sourcing patterns that the framework makes trivial.

### Supervision Configuration

PlexSpaces deploys actors under Erlang-style supervision trees. Define the topology in `app-config.toml`:

```toml
# app-config.toml
[supervisor]
strategy = "one_for_one"
max_restarts = 10
max_restart_window_seconds = 60

[[supervisor.children]]
id = "account-alice"
type = "worker"
restart = "permanent"
shutdown_timeout_seconds = 5

[[supervisor.children]]
id = "account-bob"
type = "worker"
restart = "permanent"
shutdown_timeout_seconds = 5

[[supervisor.children]]
id = "account-charlie"
type = "worker"
restart = "permanent"
shutdown_timeout_seconds = 5
```

This configuration deploys three bank account actors under a supervisor that restarts any failed actor individually (one-for-one strategy), tolerating up to 10 restarts within 60 seconds before escalating.

### Build and Deploy

```bash
# Install dependencies
npm install

# Compile TypeScript to JavaScript
tsc -p .

# Bundle actor + SDK into one ESM file
node build-bundle.mjs

# Build WASM component using jco
jco componentize actor_bundle.mjs \
    --wit wit/plexspaces-simple-actor \
    -o account_actor.wasm \
    --disable all

# Deploy to PlexSpaces
curl -X POST http://localhost:8001/api/v1/applications/deploy \
    -F "application_id=bank-app" \
    -F "name=bank-account" \
    -F "version=1.0.0" \
    -F "wasm_file=@account_actor.wasm"
```

### Interact with the Accounts

```bash
# Deposit into Alice's account
curl -X POST http://localhost:8001/api/v1/actors/account-alice/ask \
    -H "Content-Type: application/json" \
    -d '{"message_type": "deposit", "payload": {"amount": 1000}}'
# Response: {"status": "ok", "balance": 1000}

# Withdraw from Alice's account
curl -X POST http://localhost:8001/api/v1/actors/account-alice/ask \
    -H "Content-Type: application/json" \
    -d '{"message_type": "withdraw", "payload": {"amount": 250}}'
# Response: {"status": "ok", "balance": 750}

# Check transaction history
curl -X POST http://localhost:8001/api/v1/actors/account-alice/ask \
    -H "Content-Type: application/json" \
    -d '{"message_type": "history", "payload": {"count": 10}}'
```

---

## Go: An Erlang/OTP-Style Rate Limiter

Go delivers a pragmatic balance between performance and developer productivity. The PlexSpaces Go SDK uses an interface-based pattern: implement the `Actor` interface, embed `BaseActor` for automatic state serialization, and register your actor for WASM export.

### The Actor Code

This example implements a sliding-window rate limiter — the kind you find inside API gateways like NGINX, Kong, or Envoy. Each client gets an independent window with configurable limits:

```go
// rate_limiter.go
package main

import (
    "encoding/json"
    "fmt"
    "github.com/plexobject/plexspaces/sdks/go/plexspaces"
)

// SlidingWindowLimiter implements per-client sliding window rate limiting.
type SlidingWindowLimiter struct {
    plexspaces.BaseActor

    WindowSizeMs uint64                    `json:"window_size_ms"`
    MaxRequests  int                       `json:"max_requests"`
    Clients      map[string]*ClientWindow  `json:"clients"`
    TotalChecks  int                       `json:"total_checks"`
    TotalAllowed int                       `json:"total_allowed"`
    TotalDenied  int                       `json:"total_denied"`
}

type ClientWindow struct {
    Timestamps []uint64 `json:"timestamps"`
    Allowed    int      `json:"allowed"`
    Denied     int      `json:"denied"`
}

var host = plexspaces.NewHost()

func NewSlidingWindowLimiter() *SlidingWindowLimiter {
    a := &SlidingWindowLimiter{
        WindowSizeMs: 60000,
        MaxRequests:  100,
        Clients:      make(map[string]*ClientWindow),
    }
    a.SetSelf(a) // enables automatic JSON state serialization
    return a
}

func (s *SlidingWindowLimiter) Init(configJSON string) string {
    var config struct {
        ActorID string         `json:"actor_id"`
        Args    map[string]any `json:"args"`
    }
    json.Unmarshal([]byte(configJSON), &config)

    if args := config.Args; args != nil {
        if v, ok := args["window_size_ms"]; ok {
            s.WindowSizeMs = uint64(v.(float64))
        }
        if v, ok := args["max_requests"]; ok {
            s.MaxRequests = int(v.(float64))
        }
    }

    host.Info(fmt.Sprintf("RateLimiter: window=%dms, max=%d req/window",
        s.WindowSizeMs, s.MaxRequests))
    return ""
}

func (s *SlidingWindowLimiter) Handle(from, msgType, payloadJSON string) string {
    switch msgType {
    case "check_rate":
        return s.checkRate(payloadJSON)
    case "stats":
        return s.getStats()
    default:
        data, _ := json.Marshal(map[string]any{"error": "unknown: " + msgType})
        return string(data)
    }
}

func (s *SlidingWindowLimiter) checkRate(payloadJSON string) string {
    var req struct {
        ClientID string `json:"client_id"`
    }
    json.Unmarshal([]byte(payloadJSON), &req)

    window, exists := s.Clients[req.ClientID]
    if !exists {
        window = &ClientWindow{Timestamps: make([]uint64, 0)}
        s.Clients[req.ClientID] = window
    }

    now := host.NowMs()
    cutoff := now - s.WindowSizeMs

    // Slide the window: remove expired timestamps
    var active []uint64
    for _, ts := range window.Timestamps {
        if ts > cutoff {
            active = append(active, ts)
        }
    }
    window.Timestamps = active

    // Check the limit
    allowed := len(window.Timestamps) < s.MaxRequests
    if allowed {
        window.Timestamps = append(window.Timestamps, now)
        window.Allowed++
        s.TotalAllowed++
    } else {
        window.Denied++
        s.TotalDenied++
    }
    s.TotalChecks++

    remaining := s.MaxRequests - len(window.Timestamps)
    if remaining < 0 {
        remaining = 0
    }

    data, _ := json.Marshal(map[string]any{
        "allowed":   allowed,
        "remaining": remaining,
        "limit":     s.MaxRequests,
        "client_id": req.ClientID,
    })
    return string(data)
}
```

The comparison to Erlang/OTP maps directly:

| Erlang/OTP | PlexSpaces Go |
|---|---|
| `gen_server:start_link/3` | Supervisor in `app-config.toml` |
| `handle_call/3` | `Handle(from, msgType, payload)` |
| `#state{}` record | Go struct with JSON tags |
| `gen_server:call(Pid, Msg)` | `host.Ask(actorID, msgType, data)` |

### Build and Deploy

```bash
# Compile to WASM using TinyGo
tinygo build -target=wasi -o rate_limiter.wasm .

# Deploy
curl -X POST http://localhost:8001/api/v1/applications/deploy \
    -F "application_id=rate-limiter-app" \
    -F "name=rate-limiter" \
    -F "version=1.0.0" \
    -F "wasm_file=@rate_limiter.wasm"
```

### Configuration

```toml
# app-config.toml
[supervisor]
strategy = "one_for_one"
max_restarts = 10
max_restart_window_seconds = 60

[[supervisor.children]]
id = "rate-limiter"
type = "worker"
restart = "permanent"
shutdown_timeout_seconds = 10

[supervisor.children.args]
window_size_ms = "60000"
max_requests = "100"
```

### Test Rate Limiting

```bash
# Check if a client request is allowed
curl -X POST http://localhost:8001/api/v1/actors/rate-limiter/ask \
    -H "Content-Type: application/json" \
    -d '{"message_type": "check_rate", "payload": {"client_id": "api-client-1"}}'
# Response: {"allowed": true, "remaining": 99, "limit": 100, "client_id": "api-client-1"}

# After 100 requests within the window:
# Response: {"allowed": false, "remaining": 0, "limit": 100, "client_id": "api-client-1"}
```

---

## Rust: A Calculator with Maximum Performance

Rust produces the smallest, fastest WASM binaries. When you need every microsecond — high-frequency trading, real-time event processing, computational pipelines — Rust actors deliver near-native performance with binary sizes under 1MB.

### The Actor Code

This calculator uses `#![no_std]` to eliminate the standard library entirely, producing a tiny, self-contained WASM module:

```rust
// lib.rs
#![no_std]
extern crate alloc;

use alloc::vec::Vec;
use core::slice;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum Operation {
    Add,
    Subtract,
    Multiply,
    Divide,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalculatorState {
    calculation_count: u64,
    last_result: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalculationRequest {
    operation: Operation,
    operands: Vec<f64>,
}

static mut STATE: CalculatorState = CalculatorState {
    calculation_count: 0,
    last_result: None,
};

/// Initialize actor with optional persisted state
#[no_mangle]
pub extern "C" fn init(state_ptr: *const u8, state_len: usize) -> i32 {
    unsafe {
        if state_len == 0 {
            STATE = CalculatorState { calculation_count: 0, last_result: None };
            return 0;
        }
        let state_bytes = slice::from_raw_parts(state_ptr, state_len);
        match serde_json::from_slice::<CalculatorState>(state_bytes) {
            Ok(state) => { STATE = state; 0 }
            Err(_) => -1,
        }
    }
}

/// Handle incoming calculation requests
#[no_mangle]
pub extern "C" fn handle_message(
    _from_ptr: *const u8, _from_len: usize,
    type_ptr: *const u8, type_len: usize,
    payload_ptr: *const u8, payload_len: usize,
) -> *const u8 {
    unsafe {
        let msg_type = core::str::from_utf8(
            slice::from_raw_parts(type_ptr, type_len)
        ).unwrap_or("");

        match msg_type {
            "calculate" => {
                let payload = slice::from_raw_parts(payload_ptr, payload_len);
                let request: CalculationRequest = match serde_json::from_slice(payload) {
                    Ok(req) => req,
                    Err(_) => return core::ptr::null(),
                };
                if let Ok(result) = execute(&request) {
                    STATE.calculation_count += 1;
                    STATE.last_result = Some(result);
                }
                core::ptr::null()
            }
            _ => core::ptr::null(),
        }
    }
}

fn execute(req: &CalculationRequest) -> Result<f64, &'static str> {
    let (a, b) = (req.operands[0], req.operands[1]);
    match req.operation {
        Operation::Add      => Ok(a + b),
        Operation::Subtract => Ok(a - b),
        Operation::Multiply => Ok(a * b),
        Operation::Divide   => {
            if b == 0.0 { Err("Division by zero") } else { Ok(a / b) }
        }
    }
}
```

### Cargo Configuration

```toml
# Cargo.toml
[package]
name = "calculator-wasm-actor"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]  # Required for WASM

[dependencies]
serde = { version = "1.0", features = ["derive", "alloc"], default-features = false }
serde_json = { version = "1.0", features = ["alloc"], default-features = false }
serde-json-core = "0.5"
wee_alloc = "0.4"

[profile.release]
opt-level = "z"     # Optimize for size
lto = true          # Link-time optimization
codegen-units = 1   # Better optimization
strip = true        # Strip debug symbols
```

### Build and Deploy

```bash
# Add the WASM target
rustup target add wasm32-wasip2

# Build with release optimizations
cargo build --target wasm32-wasip2 --release

# Optimize the binary further
wasm-opt -Oz --strip-debug \
    target/wasm32-wasip2/release/calculator_wasm_actor.wasm \
    -o calculator_actor.wasm

# Deploy
curl -X POST http://localhost:8001/api/v1/applications/deploy \
    -F "application_id=rust-calc" \
    -F "name=calculator" \
    -F "version=1.0.0" \
    -F "wasm_file=@calculator_actor.wasm"
```

The resulting binary? Under 200KB. Compare that to a Python actor at 30–40MB or even a TypeScript actor at 1–2MB. When you deploy hundreds of actors per node, those size differences translate directly into memory savings and faster cold starts.

---

## Mixing Languages in One Application

Here is where PlexSpaces truly differentiates itself. You don't pick one language for your entire system. You pick the right language for each actor:

- **Python** handles the ML inference pipeline — it has the libraries, the ecosystem, the data scientist familiarity
- **Rust** processes the high-throughput event stream — it has the performance, the memory safety, the tiny binaries
- **TypeScript** serves the API layer — it has the web ecosystem, the rapid iteration, the frontend team's expertise
- **Go** runs the operational infrastructure — rate limiting, health checks, coordination — it has the pragmatism and the concurrency model

All four actors deploy to the same PlexSpaces node. They communicate through the framework's message passing. They share the same tuple space for coordination. They persist state through the same durability layer. The runtime treats them identically because they all compile to the same WASM Component Model target.

### A Polyglot Deployment

```bash
# Deploy Python ML actor
curl -X POST http://localhost:8001/api/v1/applications/deploy \
    -F "application_id=ml-pipeline" \
    -F "name=ml-inference" \
    -F "wasm_file=@ml_actor.wasm"

# Deploy Rust event processor
curl -X POST http://localhost:8001/api/v1/applications/deploy \
    -F "application_id=event-processor" \
    -F "name=event-stream" \
    -F "wasm_file=@event_actor.wasm"

# Deploy TypeScript API gateway
curl -X POST http://localhost:8001/api/v1/applications/deploy \
    -F "application_id=api-gateway" \
    -F "name=api-layer" \
    -F "wasm_file=@api_actor.wasm"

# Deploy Go rate limiter
curl -X POST http://localhost:8001/api/v1/applications/deploy \
    -F "application_id=rate-limiter" \
    -F "name=rate-limiter" \
    -F "wasm_file=@rate_limiter.wasm"
```

Each actor runs in its own WASM sandbox with enforced resource limits. A buggy Python actor cannot crash a Rust actor. A memory leak in TypeScript cannot affect Go. The isolation comes free — it is a fundamental property of WebAssembly, not something the framework adds on top.

---

## Testing Your WASM Actors

PlexSpaces provides comprehensive testing at multiple levels — all designed to run offline without external services.

### Unit Tests

Each language tests actors natively before compilation:

```bash
# Python
python -m pytest tests/

# TypeScript
npm test

# Go
go test ./...

# Rust
cargo test --lib
```

### WASM Integration Tests

After compilation, run integration tests against the real WASM runtime:

```bash
# Run all WASM integration tests (offline, in-memory services)
cargo test --package plexspaces-wasm-runtime --test '*integration*' --no-fail-fast

# Test specific capabilities
cargo test --package plexspaces-wasm-runtime --test messaging_host_functions_integration
cargo test --package plexspaces-wasm-runtime --test durability_host_functions_integration
cargo test --package plexspaces-wasm-runtime --test blob_host_functions_integration
```

These integration tests use in-memory backends — `InMemoryKVStore`, `MemoryLockManager`, `MemoryJournalStorage`, `MockChannelService` — so they run fast and require no infrastructure.

### End-to-End Tests

Deploy to a local node and verify the full stack:

```bash
# Start a test node
cargo run --release --bin plexspaces -- start \
    --node-id test-node \
    --listen-addr 0.0.0.0:8000

# Deploy and test
curl -X POST http://localhost:8001/api/v1/applications/deploy \
    -F "application_id=test-app" \
    -F "wasm_file=@actor.wasm"

# Send test messages and verify responses
curl -X POST http://localhost:8001/api/v1/actors/test-app/ask \
    -d '{"message_type": "add", "payload": {"operands": [1, 2]}}'
```

---

## The Tooling Landscape

### Setting Up Your Environment

Install the compilers for whichever languages you plan to use:

```bash
# Rust (produces the smallest, fastest WASM)
rustup target add wasm32-wasip2

# Go (good balance of size and speed)
# macOS: brew install tinygo
# Linux: see https://tinygo.org/getting-started/install/

# TypeScript (rapid development, web ecosystem)
npm install -g @bytecodealliance/jco

# Python (ML, data processing, prototyping)
pip install componentize-py

# Optional: WASM binary optimizer
cargo install wasm-opt
```

### Optimizing WASM Binaries

Size matters when you deploy hundreds of actors:

```bash
# Strip debug symbols and optimize for size
wasm-opt -Oz --strip-debug actor.wasm -o actor_opt.wasm

# Check the result
ls -lh actor_opt.wasm
```

### Deployment via CLI or HTTP

```bash
# CLI deployment (files < 5MB)
./target/release/plexspaces deploy \
    --node localhost:8000 \
    --app-id my-app \
    --name my-actor \
    --version 1.0.0 \
    --wasm actor.wasm

# HTTP multipart upload (any size, recommended for production)
curl -X POST http://localhost:8001/api/v1/applications/deploy \
    -F "application_id=my-app" \
    -F "name=my-actor" \
    -F "version=1.0.0" \
    -F "wasm_file=@actor.wasm"
```

---

## What WebAssembly Gives You (That Containers Don't)

Let me address the elephant in the room: "Why not just use Docker?"

Containers solve many problems well. But WebAssembly solves some problems *better*:

**Startup time.** A WASM module instantiates in microseconds. A container takes seconds. When you auto-scale actors in response to load spikes, microsecond cold starts mean your users never notice.

**Memory footprint.** A Rust WASM actor uses ~200KB. The equivalent Docker container starts at 50MB minimum (Alpine base image alone). On a single node, you run thousands of WASM actors where you might run dozens of containers.

**Security isolation.** WASM sandboxing is capability-based. A module cannot access the filesystem, network, or memory outside its sandbox unless the host explicitly grants each capability through WASI. Containers share a kernel and rely on namespace isolation — a fundamentally larger attack surface.

**True polyglot.** With containers, each language gets its own image, runtime, dependency tree, and deployment pipeline. With WASM, all languages produce the same artifact type, run on the same runtime, and share the same deployment pipeline.

**Composability.** The Component Model lets you link WASM modules from different languages into a single process. No network calls. No serialization overhead. Direct function invocation across language boundaries. Try that with Docker.

PlexSpaces actually supports *both*: WASM sandboxing for lightweight actors and Firecracker microVMs for workloads that need full hardware-level isolation. You pick the isolation model per workload, and the framework handles the rest.

---

## Where WebAssembly Is Heading

The ecosystem moves fast. Here are the milestones that matter:

- **Wasm 3.0** became the W3C standard in September 2025, standardizing nine production features including WasmGC, exception handling, and tail calls
- **WASI 0.3** shipped in February 2026 with native async support, enabling concurrent I/O without blocking
- **WASI 1.0** is on track for late 2026 or early 2027, providing the stability guarantees that enterprise adopters need
- **Wasmtime** leads the runtime ecosystem with full Component Model and WASI 0.2 support
- **Wasmer 6.0** achieved ~95% of native speed on benchmarks
- **Docker** now runs WASM components alongside containers in Docker Desktop and Docker Engine

The gaps are real — threading support remains limited, and language parity outside Rust is still a work in progress. But adoption grew 28% year-over-year, and the Component Model is no longer a research project. It is becoming the infrastructure layer that WebAssembly's "run anything, anywhere, safely" promise always required.

---

## Get Started

PlexSpaces is open source. Clone the repository and start building:

```bash
git clone https://github.com/bhatti/PlexSpaces.git
cd PlexSpaces

# Build the framework
cargo build --release

# Explore the examples
ls examples/python/apps/   # calculator, bank_account, chat_room, nbody, ...
ls examples/typescript/apps/ # bank_account, migrating_cloudflare_workers, ...
ls examples/go/apps/        # migrating_erlang_otp, migrating_cloudflare_workers, ...
ls examples/rust/apps/      # calculator, nbody, session_manager, ...
```

Each example includes its own `app-config.toml`, build scripts, and test instructions. Pick a language, pick a pattern, and deploy your first polyglot actor in minutes.

The distributed systems problems I spent thirty years solving — fault tolerance, state management, multi-language support, coordination, scaling — PlexSpaces distills them into one framework. WebAssembly makes the polyglot piece real. The Component Model makes it composable. And the WASI standard makes it portable.

Pick the right language for each job. Compile it to WASM. Deploy it to PlexSpaces. Let the framework handle the rest.

---

*PlexSpaces is available at [github.com/bhatti/PlexSpaces](https://github.com/bhatti/PlexSpaces). Star the repo, try the examples, and join the conversation.*
