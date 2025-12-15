# Simple Examples

This directory contains simple, focused examples demonstrating key PlexSpaces features.

## Examples

### 1. Timers Example (`timers_example/`)

Demonstrates **non-durable, in-memory timers** using `TimerFacet`.

**Key Features**:
- Non-durable timers (lost on deactivation)
- Periodic and one-time timers
- High-frequency operations (< 1 second intervals)

**Run**:
```bash
cd timers_example
./test.sh  # Recommended: includes validation and metrics
# or
cargo run  # Quick run
```

**See**: [timers_example/README.md](timers_example/README.md) for complete documentation

### 2. Reminders Example (`reminders_example/`)

Demonstrates **durable, persistent reminders** using `ReminderFacet` with:
- SQLite backend for persistence
- VirtualActorFacet integration for auto-activation
- Full lifecycle (deactivation/reactivation)

**Key Features**:
- Durable reminders (survive crashes)
- Auto-activation when reminders fire
- Max occurrences support
- Long-term scheduling (hours, days, months)

**Run**:
```bash
cd reminders_example
./test.sh  # Recommended: includes validation and metrics
# or
cargo run  # Quick run
```

**See**: [reminders_example/README.md](reminders_example/README.md) for complete documentation

### 3. Time-Series Forecasting Example (`timeseries_forecasting/`)

Demonstrates an **end-to-end time-series forecasting application** using PlexSpaces actors, inspired by [Ray's time-series forecasting example](https://docs.ray.io/en/latest/ray-overview/examples/e2e-timeseries/README.html).

**Key Features**:
- Distributed data preprocessing using actor-based parallelism
- Model training with actor coordination
- Model validation using offline batch inference
- Online model serving via actor messages

**Run**:
```bash
cd timeseries_forecasting
./scripts/run_single_node.sh
# or
cargo run
```

**See**: [timeseries_forecasting/README.md](timeseries_forecasting/README.md) for complete documentation

### 4. WASM Calculator Example (`wasm_calculator/`)

Demonstrates **polyglot actor support** using WebAssembly (WASM) modules.

**Key Features**:
- Actors implemented in multiple languages (Rust, Go, Python, JavaScript)
- WASM module deployment and execution
- Content-addressed module caching
- Simple calculator use case

**Run**:
```bash
cd wasm_calculator
cargo run -- --module rust-calculator
```

**See**: [wasm_calculator/README.md](wasm_calculator/README.md) for complete documentation

### 5. Firecracker Multi-Tenant Example (`firecracker_multi_tenant/`)

Demonstrates **application-level isolation** using Firecracker microVMs for multi-tenant deployments.

**Key Features**:
- Each tenant's application runs in a separate Firecracker VM
- Strong security boundaries (VM-level isolation)
- Fast boot times (< 200ms)
- Multi-tenant support

**Run**:
```bash
cd firecracker_multi_tenant
cargo run -- deploy --tenant tenant-a
```

**See**: [firecracker_multi_tenant/README.md](firecracker_multi_tenant/README.md) for complete documentation

## Running All Tests

To run all example tests with validation and metrics:

```bash
./run_all_tests.sh
```

This will:
- Run both timers and reminders examples
- Validate expected behavior
- Show comprehensive metrics
- Exit with appropriate status codes for CI/CD

## Test Scripts

Each example includes a `test.sh` script that:

1. **Builds** the example in release mode
2. **Runs** the example for a configured duration
3. **Validates** expected behavior:
   - Facet attachment
   - Actor spawning
   - Timer/reminder firing
   - Message delivery
4. **Shows metrics**:
   - Fire counts
   - Timing information
   - Setup validation
5. **Exits** with appropriate status codes (0 = success, 1 = failure)

### Example Test Output

```
╔════════════════════════════════════════════════════════════════╗
║  Timers Example Test Script                                   ║
╚════════════════════════════════════════════════════════════════╝

📦 Building timers_example...
✅ Build successful

🚀 Running timers_example for 8 seconds...

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
📊 Metrics & Validation
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

Test Results:
  • Actor spawned: ✅ (1)
  • TimerFacet attached: ✅
  • Total TimerFired messages: 15

Timer Metrics:
  • Heartbeat timer fires: 4 (expected: ≥3)
  • Cleanup timer fires: 1 (expected: ≥1)
  • Poll timer fires: 10 (expected: ≥10)

✅ All validations passed!
```

## Design Principles Demonstrated

These examples showcase key PlexSpaces design principles:

1. **Facet-Based Extensibility**: Capabilities added via opt-in facets
2. **Cohesive Design**: Timers and reminders are separate, focused facets
3. **Simplicity**: Clear distinction between durable (reminders) and non-durable (timers)
4. **Integration**: Reminders integrate with VirtualActorFacet for auto-activation

## See Also

- `docs/TIMERS_REMINDERS_DESIGN.md` - Complete design documentation
- `crates/journaling/src/timer_facet.rs` - TimerFacet implementation
- `crates/journaling/src/reminder_facet.rs` - ReminderFacet implementation

