# Testing Guide

## Overview

PlexSpaces has comprehensive test coverage including unit tests, integration tests, and example tests. All tests are designed to run offline without requiring external services (except where explicitly noted).

## Running Tests

### Run All Tests

```bash
# Run all unit tests and integration tests (recommended)
make test

# This runs:
# - All unit tests (library tests)
# - All WASM integration tests (offline, no AWS/MinIO)
# - Excludes AWS/MinIO-dependent integration tests
```

### Run Unit Tests Only

```bash
# Run only library unit tests (fastest)
cargo test --lib --all-features --workspace

# Run tests for specific package
cargo test --lib -p plexspaces-wasm-runtime
```

### Run Integration Tests

#### WASM Integration Tests (Offline)

All WASM integration tests run offline using in-memory services:

```bash
# Run all WASM integration tests
cargo test --package plexspaces-wasm-runtime --test '*integration*' --no-fail-fast

# Run specific integration test suite
cargo test --package plexspaces-wasm-runtime --test blob_host_functions_integration
cargo test --package plexspaces-wasm-runtime --test new_host_functions_integration
cargo test --package plexspaces-wasm-runtime --test durability_host_functions_integration
cargo test --package plexspaces-wasm-runtime --test messaging_host_functions_integration
cargo test --package plexspaces-wasm-runtime --test wasm_component_integration
cargo test --package plexspaces-wasm-runtime --test integration_tests
cargo test --package plexspaces-wasm-runtime --test grpc_integration
```

#### Other Integration Tests (May Require Services)

```bash
# Run integration tests that may require external services
make test-integration

# Note: Some integration tests require:
# - MinIO (for blob storage tests)
# - Redis (for distributed tests)
# - Kafka (for messaging tests)
# These are excluded from `make test` by default
```

### Run Example Tests

```bash
# Run all example tests
make test-examples

# Run WASM example tests
make test-wasm
```

### Run Tests with Output

```bash
# Show test output (useful for debugging)
cargo test --package plexspaces-wasm-runtime --test blob_host_functions_integration -- --nocapture

# Run specific test
cargo test --package plexspaces-wasm-runtime --test blob_host_functions_integration test_blob_upload -- --nocapture
```

## Test Organization

### Unit Tests

Unit tests are in `src/` directories with `#[cfg(test)]` modules:
- Fast execution
- No external dependencies
- Test individual functions and modules

### Integration Tests

Integration tests are in `tests/` directories:
- Test complete workflows
- Use in-memory services when possible
- May require external services (clearly documented)

### WASM Integration Tests

Located in `crates/wasm-runtime/tests/`:

1. **`blob_host_functions_integration.rs`** - Blob storage operations
   - Tests all 7 WIT blob methods: upload, download, delete, exists, list, metadata, copy
   - Uses LocalFileSystem (offline)

2. **`new_host_functions_integration.rs`** - KeyValue, ProcessGroups, Locks, Registry
   - Uses InMemoryKVStore, MemoryLockManager (offline)

3. **`durability_host_functions_integration.rs`** - Journaling/durability
   - Uses MemoryJournalStorage (offline)

4. **`messaging_host_functions_integration.rs`** - Messaging operations
   - Uses MockMessageSender (offline)

5. **`wasm_component_integration.rs`** - Component model
   - Tests component loading and instantiation

6. **`integration_tests.rs`** - Behavior routing and channels
   - Uses MockChannelService (offline)

7. **`grpc_integration.rs`** - gRPC deployment service
   - Uses localhost only (offline)

## Test Design Principles

### Offline-First

All WASM integration tests are designed to run offline:
- ✅ No network access required
- ✅ No SSL certificates required
- ✅ No external services required
- ✅ Uses in-memory services (LocalFileSystem, SQLite in-memory, etc.)

### In-Memory Services

Tests use in-memory implementations:
- **Blob Storage**: `LocalFileSystem` with temp directories
- **KeyValue**: `InMemoryKVStore`
- **Locks**: `MemoryLockManager`
- **Journaling**: `MemoryJournalStorage`
- **Messaging**: `MockMessageSender`
- **Channels**: `MockChannelService`

### Excluded Tests

The following tests are excluded from `make test` (require external services):
- AWS/MinIO blob storage tests (require MinIO running)
- Distributed tests (require Redis/Kafka)
- Network-based tests (require external endpoints)

These can be run separately with `make test-integration` if services are available.

## Test Coverage

### WASM Host Functions

All WASM host functions are tested:

- ✅ **Blob**: upload, download, delete, exists, list, metadata, copy
- ✅ **KeyValue**: get, put, delete, exists, list-keys, increment, compare-and-swap
- ✅ **ProcessGroups**: create_group, join_group, leave_group, get_members, publish_to_group
- ✅ **Locks**: acquire, renew, release, try_acquire, get_lock
- ✅ **Registry**: register, unregister, lookup, discover, heartbeat
- ✅ **Durability**: persist, persist_batch, checkpoint, get_sequence, is_replaying, read_journal, compact
- ✅ **Messaging**: link, unlink, monitor, demonitor
- ✅ **Channels**: send_to_queue, receive_from_queue, publish_to_topic
- ✅ **TupleSpace**: write, read, take, watch

## Troubleshooting

### Tests Fail with SSL Errors

**Cause**: Cargo trying to download dependencies

**Solution**: 
```bash
# Configure SSL certificates (see docs/SSL_CERTIFICATE_FIX.md)
# Or use offline mode if dependencies are cached
cargo test --offline
```

### Tests Fail with "Service not configured"

**Cause**: Test trying to use service that wasn't set up

**Solution**: Check test setup - all integration tests include proper service initialization

### Tests Try to Download Dependencies

**Cause**: Dependencies not cached

**Solution**: 
```bash
# Build first to cache dependencies
cargo build
# Then run tests
cargo test
```

### Integration Tests Require External Services

**Cause**: Some integration tests need MinIO, Redis, or Kafka

**Solution**: 
- Use `make test` which excludes these tests
- Or start required services and run `make test-integration`

## Best Practices

1. **Run `make test` before committing** - Ensures all offline tests pass
2. **Use `--nocapture` for debugging** - See test output when debugging failures
3. **Run specific test suites** - Faster feedback during development
4. **Check test coverage** - Use `make test-coverage` to verify coverage requirements

## Related Documentation

- `crates/wasm-runtime/tests/README.md` - WASM test details
- `docs/SSL_CERTIFICATE_FIX.md` - SSL certificate configuration
- `Makefile` - Test targets and commands


