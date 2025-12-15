# Framework Comparison Template

Use this template to create new framework comparisons.

## Directory Structure

```
comparison/<framework>/
├── README.md              # Comparison documentation
├── Cargo.toml            # Rust dependencies
├── src/
│   └── main.rs           # PlexSpaces implementation
├── native/               # Native framework code (sample)
│   └── example.<ext>     # Native implementation
├── scripts/
│   ├── run_native.sh     # Run native example (if applicable)
│   ├── run_plexspaces.sh # Run PlexSpaces example
│   └── benchmark.sh      # Run benchmarks
├── tests/
│   └── comparison_test.rs # Unit/integration tests
└── metrics/
    └── benchmark_results.json # Benchmark results
```

## README.md Structure

1. **Title and Overview** - Framework name and use case
2. **Native Framework Implementation** - Sample code
3. **PlexSpaces Implementation** - Rust code
4. **Key Differences** - Comparison table
5. **Running the Comparison** - Instructions
6. **Performance Comparison** - Benchmarks
7. **Feature Comparison** - Feature matrix
8. **Code Comparison** - Side-by-side code
9. **When to Use Each** - Decision guide
10. **References** - Links to documentation

## Example README Sections

### Use Case
Describe the specific use case being compared.

### Native Framework Implementation
Show sample code (doesn't need to compile, just demonstrate patterns).

### PlexSpaces Implementation
Fully functional Rust code.

### Key Differences Table
| Feature | Native Framework | PlexSpaces |
|---------|------------------|------------|
| Language | ... | Rust |
| ... | ... | ... |

### Performance Comparison
Include benchmarks with metrics.

### Feature Comparison
Feature matrix showing what each framework supports.

## Implementation Checklist

- [ ] Create directory structure
- [ ] Write README.md with comparison
- [ ] Add native framework sample code
- [ ] Implement PlexSpaces version
- [ ] Add Cargo.toml with dependencies
- [ ] Create scripts for running/benchmarking
- [ ] Add unit/integration tests
- [ ] Run benchmarks and document results
- [ ] Update main comparison README.md

## Testing

Each comparison should include:
- Unit tests for core functionality
- Integration tests for multi-node scenarios
- Benchmark tests for performance comparison

## Benchmarks

Track:
- Latency (P50, P95, P99)
- Throughput (ops/sec)
- Resource usage (CPU, memory)
- Cold start times
- Scalability metrics
