# Persistence Benchmarks

Performance benchmarks for PlexSpaces persistence infrastructure using [Criterion.rs](https://github.com/bheisler/criterion.rs).

## Purpose

Track performance metrics for checkpoint, journal, and retention operations to:
- Detect performance regressions
- Measure impact of optimizations
- Establish baseline performance characteristics
- Guide production deployment sizing

## Running Benchmarks

### Full Benchmark Suite

```bash
# Run all benchmarks (takes ~5-10 minutes)
cargo bench -p plexspaces-persistence

# View HTML report
open target/criterion/report/index.html
```

### Specific Benchmark Groups

```bash
# Run only snapshot benchmarks
cargo bench -p plexspaces-persistence -- snapshot

# Run only journal benchmarks
cargo bench -p plexspaces-persistence -- journal

# Run only retention benchmarks
cargo bench -p plexspaces-persistence -- retention

# Run only side effect benchmarks
cargo bench -p plexspaces-persistence -- side_effect
```

### Quick Test (Validation Only)

```bash
# Test that benchmarks compile and run (doesn't measure performance)
cargo bench -p plexspaces-persistence -- --test
```

## Benchmark Groups

### 1. Snapshot Creation (`snapshot_create`)

**Measures**: Latency of creating snapshots with different state sizes

**Test Cases**:
- 1 KB state
- 10 KB state
- 100 KB state
- 1 MB state

**Metrics**: P50, P95, P99 latencies, throughput (bytes/sec)

### 2. Snapshot Loading (`snapshot_load`)

**Measures**: Latency of loading snapshots from storage

**Test Cases**: Same sizes as snapshot creation

**Metrics**: P50, P95, P99 latencies, throughput (bytes/sec)

### 3. Journal Writes (`journal_write`)

**Measures**: Throughput of journal entry writes

**Test Cases**:
- 1 entry batch
- 10 entries batch
- 100 entries batch
- 1000 entries batch

**Metrics**: Entries/second, P50/P95/P99 latencies

### 4. Side Effect Recording (`side_effects`)

**Measures**: ExecutionContext side effect recording performance

**Test Cases**:
- 1 side effect
- 10 side effects
- 50 side effects
- 100 side effects

**Metrics**: Side effects/second, P50/P95/P99 latencies

### 5. Side Effect Replay (`side_effect_replay`)

**Measures**: ExecutionContext replay performance

**Test Cases**:
- 10 side effects replay
- 50 side effects replay
- 100 side effects replay

**Metrics**: Replay time, validation overhead

### 6. Retention Enforcement (`retention`)

**Measures**: Snapshot retention policy enforcement performance

**Test Cases**:
- 10 snapshots (retention=5)
- 50 snapshots (retention=5)
- 100 snapshots (retention=5)

**Metrics**: Deletion time, throughput (snapshots/sec)

### 7. Promise Recording (`promises`)

**Measures**: Promise creation and resolution performance

**Test Cases**:
- 10 promises
- 50 promises
- 100 promises

**Metrics**: Promises/second, P50/P95/P99 latencies

### 8. Side Effect Types (`side_effect_types`)

**Measures**: Performance of different side effect types

**Test Cases**:
- ExternalCall (1KB request/response)
- TimerScheduled
- Sleep
- RandomGenerated (32 bytes)
- TimeAccessed

**Metrics**: Individual latencies per type

### 9. Journal Truncation (`truncation`)

**Measures**: Journal entry truncation performance

**Test Cases**:
- 100 entries
- 500 entries
- 1000 entries

**Metrics**: Truncation time, throughput (entries/sec)

## Expected Performance Baselines

### Snapshot Operations
- **Creation**: < 1 ms for 1KB, < 10 ms for 1MB
- **Loading**: < 1 ms for 1KB, < 5 ms for 1MB

### Journal Operations
- **Writes**: > 10,000 entries/sec
- **Truncation**: > 1,000 entries/sec

### ExecutionContext
- **Side Effect Recording**: < 100 µs per effect
- **Replay**: < 50 µs per effect (from cache)

### Retention
- **Enforcement**: > 100 snapshots/sec deletion

## Interpreting Results

### Criterion Output

Criterion produces three types of output:

1. **Console Output**: Real-time progress and summary statistics
2. **HTML Reports**: Detailed visualizations in `target/criterion/report/`
3. **Data Files**: Raw data in `target/criterion/<benchmark>/`

### Key Metrics

- **Time**: Median time (P50) is primary metric
- **Throughput**: Operations per second
- **Variance**: Lower variance = more consistent performance
- **Change**: Percentage change from previous run (regression detection)

### Performance Regression Thresholds

Criterion detects regressions automatically:
- **Noise threshold**: 5% (changes < 5% considered noise)
- **Regression**: > 5% slower than baseline
- **Improvement**: > 5% faster than baseline

## CI/CD Integration

### GitHub Actions

```yaml
- name: Run Benchmarks
  run: cargo bench -p plexspaces-persistence --no-fail-fast

- name: Upload Benchmark Results
  uses: actions/upload-artifact@v2
  with:
    name: criterion-results
    path: target/criterion/
```

### Baseline Comparison

```bash
# Save baseline
cargo bench -p plexspaces-persistence -- --save-baseline main

# Compare against baseline
git checkout feature-branch
cargo bench -p plexspaces-persistence -- --baseline main
```

## Troubleshooting

### Benchmarks Too Slow

Reduce sample size (less accurate):
```bash
cargo bench -p plexspaces-persistence -- --sample-size 10
```

### Insufficient Permissions

Some benchmarks may require specific permissions for high-resolution timers:
```bash
sudo cargo bench -p plexspaces-persistence
```

### Noisy Results

Reduce system noise:
1. Close other applications
2. Disable CPU frequency scaling
3. Run benchmarks multiple times
4. Use `--warm-up-time` to increase warmup period

## Future Enhancements

Planned benchmark additions:
- [ ] Compression benchmarks (zstd, snappy, none)
- [ ] Multi-threaded concurrent writes
- [ ] Large-scale retention (10K+ snapshots)
- [ ] Memory usage profiling
- [ ] Disk I/O benchmarks (when persistent storage added)

## References

- [Criterion.rs Documentation](https://bheisler.github.io/criterion.rs/book/)
- [Performance Testing Best Practices](https://github.com/bheisler/criterion.rs#tips-for-writing-benchmarks)
- [PlexSpaces Durability Phase 3 Plan](../../../docs/DURABILITY_PHASE3_PLAN.md)
