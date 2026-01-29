# Native References

This directory contains native code implementations from other frameworks for reference/comparison.

These are **read-only references** showing how the same functionality is implemented in:
- Temporal, Orleans, Erlang/OTP
- Ray, Cadence, Conductor
- AWS Step Functions, Azure Durable Functions
- And other frameworks

## Directory Structure

```
native_references/
├── python/      # Python native implementations
├── typescript/  # TypeScript native implementations
├── go/          # Go native implementations
```

## Purpose

These files help users understand:
1. How patterns translate from other frameworks to PlexSpaces
2. The equivalent PlexSpaces APIs for familiar patterns
3. Migration paths from existing systems

## See Also

- [rust_embedded migrating_* examples](../rust_embedded/)
- [Architecture Comparison](../../docs/architecture.md)
