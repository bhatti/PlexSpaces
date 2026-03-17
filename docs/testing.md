# Testing Guide

## Overview

PlexSpaces has comprehensive test coverage including unit tests, integration tests, and example tests. All tests are designed to run offline without requiring external services (except where explicitly noted).

## Running Tests

### Run All Tests

```bash
# Run all unit tests and integration tests (recommended)
make test

# Fast local Rust test loop (prefers cargo-nextest when installed)
make test-fast

# Fast local compile verification without full linking/test execution
make check

# This runs:
# - All unit tests (library tests)
# - All WASM integration tests (offline, no AWS/MinIO)
# - Excludes AWS/MinIO-dependent integration tests
```

### Run Unit Tests Only

```bash
# Fastest repo-level compile pass
make build-fast

# Run only library unit tests (fastest)
cargo test --lib --all-features --workspace

# Run tests for specific package
cargo test --lib -p plexspaces-wasm-runtime
```

### Build Performance Defaults

The repository uses a single shared top-level `target/` directory for workspace crates, examples, and scripts. Local development paths also enable incremental compilation by default.

- `make build`, `make test`, `make build-fast`, and `make test-fast` all reuse the shared `target/`
- `cargo-nextest` is used automatically when installed for faster test scheduling
- `sccache` is used automatically when installed for compiler artifact caching
- `CARGO_BUILD_JOBS` controls build parallelism and `CARGO_TEST_JOBS` can be set separately for test runs

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

### Testing Examples with and without Auth

Examples and HTTP/gRPC APIs should be tested both with auth disabled and with auth enabled to ensure tenant/namespace handling is correct.

- **Auth disabled** (e.g. `PLEXSPACES_DISABLE_AUTH=1`): `tenant_id` and `namespace` come from node config (`default_tenant_id`, `default_namespace`) or can be empty. No JWT required.
- **Auth enabled**: `tenant_id` is required (from JWT or request); `namespace` is optional. RequestContext validation rejects empty `tenant_id` when auth is enabled.

Run the server with auth disabled for local/testing, then run example scripts (e.g. registry, task-queue). Run again with auth enabled and valid JWT to verify API behavior.

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

### Test Guards for External Services

Integration tests that require external services use automatic guard checks that skip
tests gracefully if the service is not available. This allows `make test` to run all
tests without failures, while still supporting integration testing when services are running.

**Available Guard Functions** (`plexspaces_common::test_helpers`):
- `redis_available()` - Redis (localhost:6379)
- `nats_available()` - NATS (localhost:4222)
- `kafka_available()` - Kafka (localhost:9092)
- `postgres_available()` - PostgreSQL (localhost:5432)
- `dynamodb_local_available()` - DynamoDB Local (localhost:8000)
- `localstack_available()` / `sqs_simulator_available()` - LocalStack (localhost:4566)
- `minio_available()` - MinIO/S3 (localhost:9000)
- `firecracker_available()` - Firecracker binary + kernel + rootfs

**Usage in Tests**:
```rust
use plexspaces_common::skip_if_unavailable;
use plexspaces_common::test_helpers::redis_available;

#[tokio::test]
async fn test_with_redis() {
    skip_if_unavailable!(redis_available().await, "Redis");
    // ... test code that requires Redis
}
```

**Running with Services**:
```bash
# Start services (e.g., with docker-compose)
docker-compose up -d redis nats kafka

# Run tests - integration tests will now execute
make test
```

### Previously Excluded Tests

The following tests require external services but now skip gracefully:
- AWS/MinIO blob storage tests (require MinIO running)
- Distributed tests (require Redis/Kafka/NATS)
- Firecracker tests (require Firecracker binary + kernel + rootfs)
- Network-based tests (require external endpoints)

All tests can be run with `make test` - tests that require unavailable services
will be skipped with an informational message.

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

