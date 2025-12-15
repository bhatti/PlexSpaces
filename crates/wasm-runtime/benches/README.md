# WASM Runtime Performance Benchmarks

## Overview

Comprehensive performance benchmarks for the PlexSpaces WASM runtime, validating the **< 10% overhead target** for WASM actor execution vs native actors (CLAUDE.md Phase 6 success criteria).

## Benchmark Categories

### 1. Module Operations
Measures WASM module compilation, caching, and deployment performance:
- **Module compilation**: Cold start compilation time
- **Module cache hit**: Warm start lookup time
- **Module deployment**: End-to-end deployment (transfer + compile + cache)

**Targets**:
- Compilation: < 100ms for small modules (< 1MB)
- Cache hit: < 1μs
- Deployment: < 150ms total

### 2. Actor Operations
Measures actor instantiation and lifecycle performance:
- **Actor instantiation**: Create actor from cached module
- **With state**: Instantiation with different state sizes (1KB - 100KB)

**Targets**:
- Instantiation: < 10ms from cached module
- State overhead: < 5ms for 100KB state

### 3. Throughput
Measures concurrent operation throughput:
- **Concurrent instantiation**: Multiple actors spawned in parallel
- **Bulk deployment**: Sequential module deployments

**Targets**:
- Concurrent actors: > 100 actors/sec
- Deployment throughput: > 50 modules/sec

### 4. Resource Limits
Measures resource enforcement overhead:
- **Memory limits**: Different memory limit configurations
- **Execution timeouts**: Timeout enforcement overhead

**Targets**:
- Memory limit overhead: < 100μs
- Timeout overhead: < 100μs

### 5. End-to-End
Measures complete deployment workflows:
- **Deploy + instantiate**: Full workflow latency

**Target**: < 200ms total

## Running Benchmarks

### Quick Start

```bash
# Run all benchmarks (takes ~10 minutes)
cargo bench -p plexspaces-wasm-runtime

# View HTML report
open target/criterion/report/index.html
```

### Specific Benchmark Groups

```bash
# Module operations only
cargo bench -p plexspaces-wasm-runtime -- module_operations

# Actor operations only
cargo bench -p plexspaces-wasm-runtime -- actor_instantiation

# Throughput tests only
cargo bench -p plexspaces-wasm-runtime -- concurrent

# Resource limits only
cargo bench -p plexspaces-wasm-runtime -- memory_limits

# End-to-end workflow
cargo bench -p plexspaces-wasm-runtime -- e2e_deployment
```

### Baseline Comparison

```bash
# Save current performance as baseline
cargo bench -p plexspaces-wasm-runtime -- --save-baseline main

# Make changes...

# Compare against baseline
cargo bench -p plexspaces-wasm-runtime -- --baseline main

# This shows % change for each benchmark
```

### Detailed Output

```bash
# Verbose output with statistics
cargo bench -p plexspaces-wasm-runtime -- --verbose

# Show all samples (not just summary)
cargo bench -p plexspaces-wasm-runtime -- --noplot
```

## Interpreting Results

### Criterion Output Format

```
module_compilation/compile/simple
                        time:   [45.234 ms 46.123 ms 47.012 ms]
                        thrpt:  [21.765 elem/s 22.134 elem/s 22.456 elem/s]
                        change: [-2.3% +0.5% +3.1%] (p = 0.34 > 0.05)
                        No change in performance detected.
```

**Key Metrics**:
- **time**: Mean execution time with confidence interval
  - [lower bound, estimate, upper bound]
- **thrpt**: Throughput (operations/sec)
- **change**: % change from baseline
  - Negative = faster (good!)
  - Positive = slower (bad!)
  - p-value indicates statistical significance

### Success Criteria

✅ **PASS** if:
- Module compilation < 100ms for typical modules
- Actor instantiation < 10ms
- WASM overhead < 10% vs native (measure separately)
- Concurrent throughput > 100 actors/sec
- Performance regression < ±5% between releases

❌ **FAIL** if:
- Any target exceeded by > 20%
- Performance regression > 10% from baseline
- Throughput < 50 actors/sec

### Example Analysis

```
✅ GOOD: time: [8.2ms 8.5ms 8.8ms] - Under 10ms target
✅ GOOD: change: [-5.2% -2.1% +1.0%] - Faster than baseline
✅ GOOD: thrpt: [115 elem/s] - Above 100/sec target

⚠️  WARNING: time: [9.5ms 10.2ms 10.8ms] - Near 10ms target
⚠️  WARNING: change: [+4.5% +6.2% +8.1%] - Slight regression

❌ BAD: time: [15.2ms 16.5ms 17.8ms] - Exceeds 10ms target
❌ BAD: change: [+12% +15% +18%] - Significant regression
❌ BAD: thrpt: [45 elem/s] - Below 100/sec target
```

## Performance Targets Summary

| Operation | Target | Rationale |
|-----------|--------|-----------|
| Module compilation | < 100ms | Reasonable cold start for 1MB module |
| Module cache hit | < 1μs | In-memory HashMap lookup |
| Actor instantiation | < 10ms | Acceptable warm start latency |
| State serialization | < 5ms | 100KB state transfer overhead |
| Concurrent throughput | > 100/sec | Support 1K actors in 10s |
| Memory limit overhead | < 100μs | Negligible runtime cost |
| Timeout overhead | < 100μs | Negligible runtime cost |
| E2E deployment | < 200ms | Deploy + instantiate total |

## Benchmarking Best Practices

### 1. Environment Consistency

```bash
# Disable CPU frequency scaling (Linux)
echo performance | sudo tee /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor

# Close background applications
# Run on dedicated benchmark machine if possible
```

### 2. Multiple Runs

```bash
# Run benchmarks 3 times, compare results
cargo bench -p plexspaces-wasm-runtime --save-baseline run1
cargo bench -p plexspaces-wasm-runtime --save-baseline run2
cargo bench -p plexspaces-wasm-runtime --save-baseline run3

# Results should be within ±5% variance
```

### 3. Warm-Up Period

Criterion automatically includes a warm-up period (default 3s) before measurements.
This ensures:
- JIT compilation completed
- Caches populated
- CPU frequency scaled up

### 4. Sample Size

Default: 100 samples per benchmark
Increase for more accuracy:

```rust
// In wasm_benchmarks.rs
config = Criterion::default()
    .sample_size(1000)  // 10x more samples
```

## Comparing WASM vs Native

To measure the **< 10% overhead target**, you need to compare WASM actors against native actors:

### Step 1: Benchmark Native Actor

Create `benches/native_benchmarks.rs`:

```rust
// Benchmark native actor instantiation
fn bench_native_actor_instantiation(c: &mut Criterion) {
    // ... implement native actor benchmark
}
```

### Step 2: Run Both Benchmarks

```bash
# Run WASM benchmarks
cargo bench -p plexspaces-wasm-runtime -- actor_instantiation

# Run native benchmarks
cargo bench -p plexspaces-actor -- actor_instantiation
```

### Step 3: Calculate Overhead

```
WASM overhead = (WASM_time - Native_time) / Native_time * 100%

Example:
  Native: 9.0ms
  WASM:   9.8ms
  Overhead: (9.8 - 9.0) / 9.0 * 100% = 8.9% ✅ (< 10% target)
```

## Continuous Integration

Add to CI pipeline:

```yaml
# .github/workflows/benchmarks.yml
- name: Run benchmarks
  run: cargo bench -p plexspaces-wasm-runtime -- --save-baseline ci

- name: Compare against main
  run: cargo bench -p plexspaces-wasm-runtime -- --baseline main

- name: Check regression
  run: |
    # Fail if any benchmark regresses by > 10%
    cargo bench -p plexspaces-wasm-runtime -- --baseline main | \
      grep -E "change:.*\+[0-9]{2}\." && exit 1 || exit 0
```

## Troubleshooting

### Benchmarks Too Slow

```bash
# Reduce sample size and measurement time
cargo bench -p plexspaces-wasm-runtime -- --sample-size 10 --measurement-time 5
```

### High Variance

```
    change: [+2.3% +15.7% +31.2%] (p = 0.05)
    Change within noise threshold.
```

**Causes**:
- Background processes (close them)
- CPU throttling (disable)
- Thermal throttling (ensure cooling)
- Swapping (ensure enough RAM)

**Solutions**:
- Run on dedicated machine
- Increase warm-up time
- Increase sample size
- Take multiple runs and average

### Out of Memory

For large-scale concurrent tests:

```bash
# Increase system limits (Linux)
ulimit -n 10000  # File descriptors
ulimit -v unlimited  # Virtual memory
```

## Benchmark Results Archive

Store benchmark results for historical comparison:

```bash
# Archive results
mkdir -p benchmark-history
cp -r target/criterion benchmark-history/$(date +%Y-%m-%d)

# Compare historical results
cargo bench -p plexspaces-wasm-runtime -- \
  --baseline benchmark-history/2025-11-01
```

## Related Documentation

- [CLAUDE.md](../../../CLAUDE.md) - Phase 6 success criteria
- [PROJECT_TRACKER.md](../../../PROJECT_TRACKER.md) - Implementation status
- [Criterion User Guide](https://bheisler.github.io/criterion.rs/book/)
- [WASM Performance Best Practices](https://bytecodealliance.org/articles/performance)

## Questions?

- **Why 10% overhead target?** Industry standard for acceptable virtualization overhead
- **Why these specific benchmarks?** Based on real-world PlexSpaces usage patterns
- **How often to run?** On every PR + nightly on main branch
- **What if targets missed?** Analyze bottlenecks, optimize, or revise targets with justification

---

**Last Updated**: 2025-11-20
**Benchmark Version**: 1.0.0
**Target Achievement**: 🎯 < 10% WASM overhead
