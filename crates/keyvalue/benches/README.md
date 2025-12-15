# KeyValueStore Performance Benchmarks

## Purpose

Measures performance characteristics of KeyValueStore implementations to:
- Guide deployment decisions (InMemory vs SQLite vs Redis)
- Identify performance regressions during development
- Validate performance targets from CLAUDE.md
- Compare backend implementations

## Running Benchmarks

### Quick Start

```bash
# Run all benchmarks
cargo bench -p plexspaces-keyvalue

# Run specific benchmark group
cargo bench -p plexspaces-keyvalue -- single_operations
cargo bench -p plexspaces-keyvalue -- batch_operations
cargo bench -p plexspaces-keyvalue -- concurrent_access

# Test benchmarks (fast check without full measurement)
cargo bench -p plexspaces-keyvalue --bench keyvalue_benchmarks -- --test
```

### Baseline Comparison

```bash
# Save current results as baseline
cargo bench -p plexspaces-keyvalue -- --save-baseline main

# Make changes to code...

# Compare against baseline
cargo bench -p plexspaces-keyvalue -- --baseline main
```

### Advanced Options

```bash
# Run with specific sample size (default: 100)
cargo bench -p plexspaces-keyvalue -- --sample-size 50

# Filter benchmarks by name
cargo bench -p plexspaces-keyvalue -- single_put

# Increase measurement time for stable results
cargo bench -p plexspaces-keyvalue -- --measurement-time 20
```

## Benchmark Groups

### 1. Single Operations (`single_operations`)

Tests individual operations for latency measurement.

**Benchmarks:**
- `single_put` - Put operation latency (100B, 10KB values)
- `single_get` - Get operation latency (100B, 10KB values)
- `single_delete` - Delete operation latency
- `single_exists` - Exists check latency

**Performance Targets (InMemory):**
- Single put/get: < 1μs
- Single delete/exists: < 1μs

**Example Output:**
```
single_put/InMemory/100     time: [425.32 ns 432.18 ns 440.28 ns]
single_get/InMemory/100     time: [312.45 ns 318.92 ns 326.73 ns]
```

### 2. Batch Operations (`batch_operations`)

Tests throughput with sequential bulk operations.

**Benchmarks:**
- `bulk_put` - 1000 sequential puts
- `bulk_get` - 1000 sequential gets

**Performance Targets (InMemory):**
- Bulk operations: > 1M ops/sec

**Example Output:**
```
bulk_put/InMemory_1000_ops  time: [892.34 μs 905.67 μs 921.45 μs]
                            thrpt: [1.09M elem/s 1.10M elem/s 1.12M elem/s]
```

### 3. Concurrent Access (`concurrent_access`)

Tests multi-threaded workloads.

**Benchmarks:**
- `concurrent_reads` - Parallel reads with 1, 2, 4, 8 threads
- `mixed_workload` - 80% reads, 20% writes (typical production ratio)

**Performance Targets (InMemory):**
- Concurrent reads: Linear scaling with cores
- Mixed workload: > 500K ops/sec

**Example Output:**
```
concurrent_reads/InMemory/1     time: [1.23 ms 1.25 ms 1.28 ms]
concurrent_reads/InMemory/2     time: [685.34 μs 698.21 μs 713.45 μs]  (1.8x speedup)
concurrent_reads/InMemory/4     time: [395.12 μs 402.88 μs 412.67 μs]  (3.1x speedup)
concurrent_reads/InMemory/8     time: [245.89 μs 251.34 μs 258.92 μs]  (5.0x speedup)
```

### 4. Key Patterns (`key_patterns`)

Tests different access patterns.

**Benchmarks:**
- `prefix_scan` - List operations with prefix filter (10, 100, 1000 keys)

**Performance Targets (InMemory):**
- Prefix scan (1000 keys): < 100μs

**Example Output:**
```
prefix_scan/InMemory/10         time: [2.34 μs 2.38 μs 2.43 μs]
prefix_scan/InMemory/100        time: [18.92 μs 19.34 μs 19.87 μs]
prefix_scan/InMemory/1000       time: [165.43 μs 168.92 μs 173.21 μs]
```

### 5. TTL Operations (`ttl_operations`)

Tests time-to-live functionality.

**Benchmarks:**
- `put_with_ttl` - Put with expiration time

**Performance Targets (InMemory):**
- Put with TTL: < 2μs (minimal overhead vs regular put)

## Understanding Results

### Criterion Output

```
single_put/InMemory/100     time: [425.32 ns 432.18 ns 440.28 ns]
                            change: [-2.5% +0.8% +4.2%]
```

- **First value** (425.32 ns): Lower bound of confidence interval
- **Second value** (432.18 ns): **Point estimate** (median time)
- **Third value** (440.28 ns): Upper bound of confidence interval
- **Change**: Performance change vs baseline (if set)

### Throughput Values

```
bulk_put/InMemory_1000_ops  thrpt: [1.09M elem/s 1.10M elem/s 1.12M elem/s]
```

- **elem/s**: Elements (operations) per second
- **M**: Million (1,000,000)
- Higher is better for throughput

### Statistical Significance

Criterion uses:
- **95% confidence intervals** for estimates
- **Statistical outlier detection** (marks outliers with ⚠️)
- **Noise threshold**: < 5% variation is considered noise

## Benchmark Configuration

Default configuration (can be modified in `benches/keyvalue_benchmarks.rs`):

```rust
struct BenchConfig {
    operation_count: 1000,       // Number of ops for throughput tests
    small_value_size: 100,       // 100 bytes
    medium_value_size: 10240,    // 10 KB
    large_value_size: 1048576,   // 1 MB (currently unused)
}
```

Criterion configuration:
- **Sample size**: 100 iterations per benchmark
- **Measurement time**: 5 seconds per benchmark (10s for bulk operations)
- **Warm-up time**: 3 seconds
- **Significance level**: 5% (95% confidence)

## Performance Targets Summary

Based on PlexSpaces performance goals (CLAUDE.md):

| Backend | Operation | Target | Notes |
|---------|-----------|--------|-------|
| InMemory | Single get/put | < 1μs | Fastest, no I/O |
| InMemory | Batch operations | > 1M ops/sec | Sequential throughput |
| InMemory | Concurrent reads | Linear scaling | Scales with CPU cores |
| SQLite | Single get/put | < 100μs | File I/O overhead |
| SQLite | Batch operations | > 10K ops/sec | WAL mode helps |
| Redis | Single get/put | < 1ms | Network latency |

## Interpreting Results for Deployment

### InMemory Backend
- **Best for**: Single-process, development, testing
- **Expect**: Sub-microsecond latency, > 1M ops/sec
- **Watch for**: Memory usage growth

### SQLite Backend
- **Best for**: Multi-process, persistent coordination
- **Expect**: ~50-100μs latency, 10-50K ops/sec
- **Watch for**: Write contention (single writer)

### Redis Backend (future)
- **Best for**: Distributed clusters, high availability
- **Expect**: ~1ms latency (network), 100K+ ops/sec
- **Watch for**: Network latency, Redis resource usage

## Troubleshooting

### Benchmarks run too slowly

```bash
# Reduce sample size
cargo bench -p plexspaces-keyvalue -- --sample-size 10

# Reduce measurement time
cargo bench -p plexspaces-keyvalue -- --measurement-time 5
```

### High variance in results

- Close other applications (reduces CPU noise)
- Run on dedicated machine (no shared VMs)
- Disable CPU frequency scaling
- Use baseline comparison for relative measurements

### "Cannot start a runtime from within a runtime" error

This indicates `rt.block_on()` is being called from within an async context.
Fixed in current benchmarks by pre-populating data outside of `to_async()`.

## Contributing

When adding new benchmarks:
1. Follow naming convention: `bench_<category>_<operation>`
2. Add to appropriate criterion group
3. Document performance targets
4. Test with `--test` flag before committing
5. Update this README with new benchmark documentation

## References

- [Criterion.rs Documentation](https://bheisler.github.io/criterion.rs/book/)
- [PlexSpaces Performance Goals](../../CLAUDE.md#performance-goals)
- [KeyValueStore Implementation](../src/lib.rs)
