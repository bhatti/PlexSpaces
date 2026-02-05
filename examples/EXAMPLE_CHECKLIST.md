# PlexSpaces Example Verification Checklist

**Date**: 2025-01-23  
**Status**: 🚧 **Migration In Progress**

This checklist tracks the verification status of all PlexSpaces examples after the reorganization.

---

## rust_embedded Examples

All examples are run with: `cargo run --bin <name> -- [options]`

### Simple Examples

| Example | Compiles | Runs | Output | Metrics | --size | --test | APIs OK |
|---------|----------|------|--------|---------|--------|--------|---------|
| actor_groups_sharding | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ |
| supervision_tree | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ |
| durable_actor | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ |
| timers | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ |
| reminders | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ |
| process_groups_pubsub | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ |
| firecracker_multi_tenant | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ |
| timeseries_forecasting | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ | 📋 Reviewed (fix deferred) |
| webhook_handler | ✅ | ✅ | ✅ | ⏳ | ⏳ | ✅ | ✅ |
| chat_room | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ |

### Intermediate Examples

| Example | Compiles | Runs | Output | Metrics | --size | --test | APIs OK |
|---------|----------|------|--------|---------|--------|--------|---------|
| heat_diffusion | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ |
| matrix_multiply | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ |
| matrix_vector_mpi | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ |

### Advanced Examples

| Example | Compiles | Runs | Output | Metrics | --size | --test | APIs OK |
|---------|----------|------|--------|---------|--------|--------|---------|
| byzantine | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ |
| nbody | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ |
| nbody_wasm | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ |

### Domain Examples

| Example | Compiles | Runs | Output | Metrics | --size | --test | APIs OK |
|---------|----------|------|--------|---------|--------|--------|---------|
| genomics_pipeline | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ |
| genomic_workflow_pipeline | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ |
| finance_risk | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ |
| order_processing | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ |
| entity_recognition | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ |

### New Examples

| Example | Compiles | Runs | Output | Metrics | --size | --test | APIs OK |
|---------|----------|------|--------|---------|--------|--------|---------|
| market_feed_pubsub | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ |
| tuplespace_coordination | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ |
| wasm_showcase | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ |
| wasm_calculator | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ |

### Migration Examples (from comparison/)

| Example | Compiles | Runs | Output | Metrics | --size | --test | APIs OK |
|---------|----------|------|--------|---------|--------|--------|---------|
| migrating_temporal | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ |
| migrating_orleans | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ |
| migrating_erlang_otp | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ |
| migrating_ray | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ | ⏳ |
| ... (29 total) | | | | | | | |

---

## python_apps Actors

Actors to deploy to empty node.

| Actor | Loads | Runs | Output | APIs OK |
|-------|-------|------|--------|---------|
| calculator_actor.py | ⏳ | ⏳ | ⏳ | ⏳ |
| counter_actor.py | ⏳ | ⏳ | ⏳ | ⏳ |
| blob_actor.py | ⏳ | ⏳ | ⏳ | ⏳ |
| durability_actor.py | ⏳ | ⏳ | ⏳ | ⏳ |
| event_actor.py | ⏳ | ⏳ | ⏳ | ⏳ |
| fsm_actor.py | ⏳ | ⏳ | ⏳ | ⏳ |
| keyvalue_actor.py | ⏳ | ⏳ | ⏳ | ⏳ |
| locks_actor.py | ⏳ | ⏳ | ⏳ | ⏳ |
| process_groups_actor.py | ⏳ | ⏳ | ⏳ | ⏳ |
| registry_actor.py | ⏳ | ⏳ | ⏳ | ⏳ |
| market_feed_subscriber.py | ⏳ | ⏳ | ⏳ | ⏳ |

---

## typescript_apps Actors

| Actor | Loads | Runs | Output | APIs OK |
|-------|-------|------|--------|---------|
| greeter (greeter.ts) | ⏳ | ⏳ | ⏳ | ⏳ |
| bank_account (account_actor.ts) | ⏳ | ⏳ | ⏳ | ⏳ |
| body.ts | ⏳ | ⏳ | ⏳ | ⏳ |
| market_feed_subscriber.ts | ⏳ | ⏳ | ⏳ | ⏳ |

---

## go_apps Actors

| Actor | Loads | Runs | Output | APIs OK |
|-------|-------|------|--------|---------|
| counter.go | ⏳ | ⏳ | ⏳ | ⏳ |
| order_workflow.go | ⏳ | ⏳ | ⏳ | ⏳ |
| calculator.go | ⏳ | ⏳ | ⏳ | ⏳ |
| market_feed_subscriber.go | ⏳ | ⏳ | ⏳ | ⏳ |

---

## Legend

- ⏳ Pending
- ✅ Pass
- ❌ Fail
- 🚧 In Progress

---

## Verification Commands

```bash
# Build all examples
cd examples/rust_embedded
cargo build

# Run specific example
cargo run --bin actor_groups_sharding

# Run with options
cargo run --bin actor_groups_sharding -- --size large --json

# Quick test mode
cargo run --bin actor_groups_sharding -- --test

# Verbose output
cargo run --bin actor_groups_sharding -- -v
```

---

## Next Steps

1. [ ] Verify each example compiles
2. [ ] Verify each example runs without errors
3. [ ] Check output is meaningful and readable
4. [ ] Verify metrics are reported correctly
5. [ ] Test --size (small/medium/large) flag
6. [ ] Test --test flag for quick validation
7. [ ] Review APIs for deprecated usage
8. [ ] **TODO**: Update remaining examples that use `InMemoryKVStore` to `SqliteKVStore::new(":memory:")` with keyvalue `sql-backend`: `wasm_showcase` (see PROJECT_TRACKER.md). Done: `chat_room`, `feature_flags`, `mpi_collectives`.