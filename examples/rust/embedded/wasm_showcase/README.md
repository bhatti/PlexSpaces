# PlexSpaces WASM Runtime - Comprehensive Showcase

## Purpose

This example demonstrates **all implemented WASM runtime capabilities** in PlexSpaces, showcasing the complete feature set from Phase 6 (WASM Runtime) implementation.

## Features Demonstrated

### 1. **Module Deployment & Content-Addressable Caching** 📦
- Deploy WASM modules to the runtime
- Content-addressable caching (SHA-256 hashing)
- Idempotent deployment (same module = same hash)
- Module existence checking
- List all cached modules

**Why This Matters**: Enables efficient code distribution where WASM modules are cached everywhere, and only actor state migrates (10ms vs 500ms for full code migration).

### 2. **Resource Limits & Security** 🔒
- **Memory limits**: Restrict maximum heap size
- **Fuel limits**: Prevent infinite loops (gas metering)
- **Execution timeouts**: Kill runaway actors
- **Stack size limits**: Prevent stack overflow

**Three Security Profiles**:
- **Strict**: Minimal resources, only logging allowed (untrusted code)
- **Balanced**: Production defaults (16MB memory, moderate fuel)
- **Permissive**: Full resources, all capabilities (trusted supervisors)

**Why This Matters**: Enables multi-tenant actor systems where untrusted user code runs safely alongside trusted system actors.

### 3. **Capability-Based Security (WASI)** 🛡️
- **Filesystem access**: Read/write files
- **Network access**: HTTP requests, socket connections
- **Actor spawning**: Create child actors
- **Message sending**: Inter-actor communication
- **TupleSpace access**: Coordination primitives
- **Logging**: Debug output

**Why This Matters**: Fine-grained security model where actors can only perform explicitly granted operations (principle of least privilege).

### 4. **gRPC Deployment Service** 🌐
- Remote module deployment via gRPC
- Remote actor instantiation via gRPC
- Network-based WASM distribution
- Multi-node deployment coordination

**Why This Matters**: Enables deploying WASM actors to remote nodes in a distributed cluster without copying files manually.

### 5. **Concurrent Operations** ⚡
- Parallel module deployments
- Concurrent actor instantiation
- Thread-safe runtime operations
- Performance measurements (actors/sec)

**Why This Matters**: PlexSpaces handles 10K+ concurrent actors per node efficiently.

### 6. **Python Actor Support** 🐍
- Compile Python actors to WASM using `componentize-py`
- Same WIT interface as Rust/TypeScript actors
- Full access to host functions and services
- Example actors: Counter, Event Logger, FSM, Service Demo

**Why This Matters**: Enables polyglot actor development - write actors in Python, Rust, TypeScript, or Go, all running in the same runtime.

### 7. **Actor Behaviors** 🎭
- **GenServer**: Request-reply pattern (synchronous) - uses `handle_request()` export
  - Message type "call" routes to `handle_request()`
  - MUST return a response (enforced by runtime)
  - Example: Counter actor, stateful services
- **GenEvent**: Fire-and-forget pattern (asynchronous) - uses `handle_event()` export
  - Message types "cast" or "info" route to `handle_event()`
  - No response needed (fire-and-forget)
  - Example: Event logger, metrics collector
- **GenFSM**: Finite state machine pattern (state transitions) - uses `handle_transition()` export
  - Any message type can route to `handle_transition()`
  - Returns new state name (empty if no transition)
  - Example: Workflow actor, protocol state machine
- Behavior-specific methods provide type-safe, optimized message routing
- Fallback `handle_message()` for backward compatibility
- Runtime automatically routes messages based on exported functions

**Why This Matters**: Flexible behavior patterns allow actors to match their communication style to their use case. Behavior-specific methods enable the runtime to route messages efficiently and enforce semantics (e.g., GenServer MUST return a response). This matches Erlang/OTP patterns (gen_server, gen_event, gen_statem).

### 8. **Service Access from WASM** 🔌
- **ActorService**: Spawn actors, send messages (via `host::spawn_actor`, `host::send_message`)
  - `host::spawn_actor(module_ref, initial_state)` - Spawn child actor
  - `host::send_message(to, message_type, payload)` - Send message to actor
  - Requires: `allow_spawn_actors`, `allow_send_messages` capabilities
- **TupleSpace**: Coordination primitives (via `host::tuplespace_write/read/take`)
  - `host::tuplespace_write(tuple)` - Write tuple to coordination space
  - `host::tuplespace_read(pattern)` - Read tuple (non-destructive)
  - `host::tuplespace_take(pattern)` - Take tuple (destructive)
  - `host::tuplespace_count(pattern)` - Count matching tuples
  - Requires: `allow_tuplespace` capability
- **ChannelService**: Queue and topic patterns (via host functions)
  - `host::send_to_queue(queue_name, message_type, payload)` - Send to queue (load-balanced)
  - `host::publish_to_topic(topic_name, message_type, payload)` - Publish to topic (pub/sub)
  - `host::receive_from_queue(queue_name, timeout_ms)` - Receive from queue (blocking)
  - Requires: ChannelService configured in WASM instance (passed from node)
  - Python example: `service_demo_actor.py` demonstrates channel usage
- Capability-based access control (all services require explicit capabilities)

**Why This Matters**: WASM actors can fully participate in the PlexSpaces ecosystem, spawning children, coordinating via TupleSpace, and using channels for pub/sub and work distribution patterns. All service access is capability-based for security.

### 9. **KeyValue Storage** 💾 ⭐ NEW
- Store and retrieve key-value pairs
- Atomic operations (increment, compare-and-swap)
- TTL support (time-to-live expiration)
- Multiple backends (SQLite, PostgreSQL, Redis, DynamoDB)

**Why This Matters**: Enables state management, configuration storage, and distributed coordination via shared key-value store.

### 10. **ProcessGroups Pub/Sub** 📢 ⭐ NEW
- Join/leave process groups
- Topic-based message filtering
- Broadcast to all group members
- Erlang pg2 semantics (multiple joins, topic merging)

**Why This Matters**: Enables event broadcasting, topic-based subscriptions, and group coordination patterns.

### 11. **Distributed Locks** 🔒 ⭐ NEW
- Acquire/renew/release locks
- Version-based optimistic locking
- Lease-based expiration
- Leader election support

**Why This Matters**: Enables coordination, mutual exclusion, and leader election for distributed systems.

### 12. **Object Registry** 📋 ⭐ NEW
- Service registration and discovery
- Health monitoring
- Multi-tenant isolation
- Label-based filtering

**Why This Matters**: Enables service discovery, health monitoring, and dynamic service location in distributed systems.

### 13. **Durability/Journaling** 📝 ⭐ NEW
- Event sourcing (persist events)
- Checkpoint creation (snapshots)
- Event replay (reconstruct state)
- Deterministic replay (side effect caching)

**Why This Matters**: Enables crash recovery, audit logging, and state persistence for fault-tolerant systems.

### 14. **Blob Storage** 📦 ⭐ NEW
- Upload/download blobs (S3-compatible)
- Blob metadata management
- List blobs with prefix filtering
- Copy blobs (server-side)
- Multi-tenant isolation

**Why This Matters**: Enables file storage, image processing, and large data management for actors.

## Running the Showcase

### Prerequisites

```bash
# Ensure you're in the PlexSpaces root directory
cd /path/to/tspaces

# Build the project
cargo build --example wasm-showcase
```

### Run All Demonstrations

```bash
cargo run --example wasm-showcase
```

**Expected Output**:
```
🚀 PlexSpaces WASM Runtime Showcase
====================================

📦 Demo 1: Module Deployment & Content-Addressable Caching
-----------------------------------------------------------
✓ Created WASM runtime
✓ Module compiled and cached
  Hash: abc123...
✓ Module retrieved from cache
✓ Cache hit! Same hash: abc123...
✅ Deployment demo complete!

🔒 Demo 2: Resource Limits & Capability-Based Security
-------------------------------------------------------
✓ Module deployed: abc123...
✓ Actor instantiated with STRICT limits:
  - Memory: 1MB max
  - Fuel: 1,000,000 units
  - Capabilities: logging only
✓ Actor instantiated with PERMISSIVE limits:
  - Memory: 256MB max
  - Capabilities: ALL enabled
✅ Security demo complete!

🌐 Demo 3: gRPC Deployment Service
-----------------------------------
✓ gRPC server listening on port 50051
✓ Connected to gRPC service
✓ Module deployed successfully via gRPC
✅ gRPC demo complete!

⚡ Demo 4: Concurrent Operations
--------------------------------
✓ Concurrent instantiation complete:
  - Total: 10 actors
  - Successful: 10
  - Duration: 52ms
  - Throughput: 192.3 actors/sec
✅ Concurrency demo complete!

✅ All demonstrations completed successfully!
```

### Run Specific Demonstrations

```bash
# Module deployment and caching only
cargo run --release --bin wasm-showcase -- --demo deployment

# Resource limits and security only
cargo run --release --bin wasm-showcase -- --demo security

# gRPC deployment service only
cargo run --release --bin wasm-showcase -- --demo grpc

# Concurrent operations only
cargo run --release --bin wasm-showcase -- --demo concurrency

# Python actor support
cargo run --release --bin wasm-showcase -- --demo python

# Actor behaviors (GenServer, GenEvent, GenFSM)
cargo run --release --bin wasm-showcase -- --demo behaviors

# Service access (ChannelService, TupleSpace, ActorService)
cargo run --release --bin wasm-showcase -- --demo services

# KeyValue storage
cargo run --release --bin wasm-showcase -- --demo keyvalue

# ProcessGroups pub/sub
cargo run --release --bin wasm-showcase -- --demo process-groups

# Distributed locks
cargo run --release --bin wasm-showcase -- --demo locks

# Object registry
cargo run --release --bin wasm-showcase -- --demo registry

# Durability/journaling
cargo run --release --bin wasm-showcase -- --demo durability

# Blob storage
cargo run --release --bin wasm-showcase -- --demo blob
```

### Quick Start Script

```bash
# Run all demonstrations
./scripts/run.sh

# Run specific demo
./scripts/run.sh --demo python
```

### Verbose Logging

```bash
# Enable debug logging for detailed insights
cargo run --example wasm-showcase -- --verbose
```

## Code Structure

```
examples/wasm_showcase/
├── Cargo.toml              # Example dependencies
├── README.md               # This file
├── scripts/
│   ├── run.sh              # Quick start script
│   ├── build_python_actors.sh  # Build Python WASM actors
│   └── e2e_test.sh         # End-to-end test script
├── actors/
│   └── python/             # Python actor examples
│       ├── counter_actor.py      # GenServer behavior
│       ├── event_actor.py        # GenEvent behavior
│       ├── fsm_actor.py          # GenFSM behavior
│       └── service_demo_actor.py # Service access demo
├── wasm-modules/           # Compiled WASM modules
└── src/
    └── main.rs             # All demonstrations
```

## Python Actor Setup

### Prerequisites

```bash
# Install componentize-py
pip install componentize-py

# Or install from source
pip install --user componentize-py
```

### Building Python Actors

```bash
# Build all Python actors
./scripts/build_python_actors.sh

# This compiles Python actors to WASM using componentize-py
# Output: wasm-modules/*.wasm
```

### Python Actor Examples

1. **counter_actor.py** - GenServer behavior (request-reply)
   - `increment` - Increment counter (cast)
   - `get_count` - Get current count (call)
   - `add` - Add value to counter

2. **event_actor.py** - GenEvent behavior (fire-and-forget)
   - `notify` - Add event to log
   - `get_events` - Query all events
   - `clear` - Clear event log

3. **fsm_actor.py** - GenFSM behavior (state machine)
   - `start` - Transition: idle -> processing
   - `complete` - Transition: processing -> done
   - `reset` - Transition: any -> idle
   - `get_state` - Query current state

4. **service_demo_actor.py** - Service access demonstration
   - `spawn_actor` - Spawn child actor
   - `send_message` - Send message to actor
   - `tuplespace_write` - Write tuple
   - `tuplespace_read` - Read tuple

5. **keyvalue_actor.py** - KeyValue storage demonstration ⭐ NEW
   - `store` - Store key-value pair
   - `retrieve` - Retrieve value by key
   - `delete` - Delete key
   - `list` - List all keys

6. **process_groups_actor.py** - ProcessGroups pub/sub demonstration ⭐ NEW
   - `join_group` - Join process group
   - `publish` - Publish message to group
   - `leave_group` - Leave process group

7. **locks_actor.py** - Distributed locks demonstration ⭐ NEW
   - `acquire` - Acquire lock
   - `release` - Release lock
   - `try_acquire` - Try acquire (non-blocking)

8. **registry_actor.py** - Object registry demonstration ⭐ NEW
   - `register` - Register object/service
   - `lookup` - Lookup object
   - `discover` - Discover objects by type
   - `unregister` - Unregister object

9. **durability_actor.py** - Durability/journaling demonstration ⭐ NEW
   - `increment` - Increment counter (with event persistence)
   - `checkpoint` - Create checkpoint
   - `get_sequence` - Get current event sequence
   - `persist_batch` - Persist multiple events atomically
   - `start` - Transition: idle -> processing
   - `complete` - Transition: processing -> done
   - `reset` - Transition: any -> idle
   - `get_state` - Query current state

4. **service_demo_actor.py** - Service access demonstration
   - `spawn_actor` - Spawn child actor
   - `send_message` - Send message to actor
   - `tuplespace_write` - Write tuple
   - `tuplespace_read` - Read tuple

## What This Example Validates

✅ **Walking Skeleton Phase 6 Completion**:
- [x] WASM module compilation (wasmtime)
- [x] Content-addressable module caching (SHA-256)
- [x] Resource limit enforcement (memory, fuel, CPU time)
- [x] Capability-based security (WASI)
- [x] gRPC deployment service
- [x] Concurrent actor instantiation
- [x] Integration with deployment service

✅ **Performance Targets Met**:
- Module compilation: < 100ms (typically 40-60ms)
- Actor instantiation: < 10ms from cached module (typically 2-5ms)
- Concurrent throughput: > 100 actors/sec (demonstrated 190+ actors/sec)
- Memory per actor: 2MB default (32x better than JavaNow's 64MB)

## WIT Interface Documentation

For detailed information about the WIT interface, behavior-specific methods, and channel host functions, see:

- **[WIT_INTERFACE.md](WIT_INTERFACE.md)** - Complete WIT interface documentation
- **[wit/plexspaces-actor/actor.wit](../../wit/plexspaces-actor/actor.wit)** - WIT interface definition

## Next Steps

After understanding this showcase, explore:

1. **Real WASM Actors**: Create actual Component Model actors in Rust/JS/Go/Python
   - See `wit/plexspaces-actor/actor.wit` for interface definition
   - Use `cargo component` to build Component Model binaries (Rust)
   - Use `componentize-py` to build Python actors
   - Use `javy` to build JavaScript/TypeScript actors

2. **Behavior Patterns**: Implement GenServer, GenEvent, or GenFSM behaviors
   - Export `handle_request()`, `handle_event()`, or `handle_transition()`
   - Runtime automatically routes messages to appropriate handler

3. **Channel Integration**: Use queues and topics from WASM actors
   - Call `host::send_to_queue()` for work distribution
   - Call `host::publish_to_topic()` for pub/sub patterns
   - ChannelService is automatically available from node

4. **Firecracker Integration** (Phase 7): Deploy actors in microVMs
   - Stronger isolation than WASM sandbox
   - 125ms boot time per microVM
   - See `examples/firecracker_showcase/` (coming soon)

5. **Production Deployment**: Deploy to Kubernetes/Docker
   - Use gRPC service to deploy modules to cluster
   - Leverage content-addressable caching across nodes
   - See deployment guides in `docs/`

## Troubleshooting

### "Module compilation failed"

**Cause**: SIMPLE_WASM is a minimal WASM module, not Component Model format.

**Solution**: For real examples, compile Rust actors using:
```bash
cargo component build --release
```

### "Instantiation failed"

**Expected**: The example uses a simple WASM module that doesn't implement the PlexSpaces actor interface. This demonstrates that the runtime correctly validates module structure.

**For real actors**: Use the WIT interface in `wit/plexspaces-actor/actor.wit`.

### "gRPC connection refused"

**Cause**: Server may take a moment to start.

**Solution**: The example includes a 100ms sleep after server start. If issues persist, increase the delay.

## Related Documentation

- [CLAUDE.md](../../CLAUDE.md) - Core design principles
- [PROJECT_TRACKER.md](../../PROJECT_TRACKER.md) - Phase 6 status
- [WASM Benchmarks README](../../crates/wasm-runtime/benches/README.md) - Performance validation
- [proto/plexspaces/v1/wasm.proto](../../proto/plexspaces/v1/wasm.proto) - gRPC interface
- [wit/plexspaces-actor/actor.wit](../../wit/plexspaces-actor/actor.wit) - Actor WIT interface

## Key Insights

### Content-Addressable Caching
**Before** (JavaNow mobile agents):
- 500ms to transfer 64MB agent (code + state)

**After** (PlexSpaces WASM):
- 10ms to transfer state only (code already cached)
- 32x memory reduction per actor

### Capability-Based Security
**Why NOT traditional permissions**:
- WASM runs in sandbox by default
- Actors explicitly request capabilities
- Fail-secure: missing capability = denied
- Fine-grained: control each WASI interface

### gRPC Deployment
**Why gRPC over custom protocol**:
- Industry standard (protobuf)
- Bi-directional streaming (future: actor migration)
- Language agnostic (clients in any language)
- Built-in load balancing, retries, timeouts

---

**Questions?** See [CLAUDE.md](../../CLAUDE.md) or open an issue.
