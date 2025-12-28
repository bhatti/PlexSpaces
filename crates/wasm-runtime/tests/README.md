# WASM Runtime Integration Tests

## Overview

All integration tests in this directory are designed to run **offline without network access or SSL**.

## Test Design Principles

### 1. No Network Dependencies
- ✅ All tests use in-memory mocks and services
- ✅ No external API calls
- ✅ No internet connectivity required
- ✅ Localhost-only gRPC tests (127.0.0.1)

### 2. No SSL Requirements
- ✅ Tests do not use HTTPS
- ✅ No SSL certificate validation
- ✅ Localhost HTTP only (for gRPC tests)

### 3. In-Memory Services
All tests use in-memory implementations:
- `InMemoryKVStore` - Key-value storage
- `MemoryLockManager` - Distributed locks
- `MemoryJournalStorage` - Event journaling
- `MockMessageSender` - Actor messaging
- `MockChannelService` - Queue/topic messaging
- `ProcessGroupRegistry` - Process groups (uses InMemoryKVStore)
- `ObjectRegistry` - Service discovery (uses InMemoryKVStore)

### 4. Offline-First
- Tests can run in isolated environments
- No dependency on external services
- No network configuration required
- Works in CI/CD without network access

## Test Files

### `messaging_host_functions_integration.rs`
Tests for messaging functions (link, unlink, monitor, demonitor)
- Uses `MockMessageSender` (in-memory)
- No network access required

### `new_host_functions_integration.rs`
Tests for KeyValue, ProcessGroups, Locks, Registry
- Uses `InMemoryKVStore`, `MemoryLockManager`, `MemoryJournalStorage`
- No network access required

### `durability_host_functions_integration.rs`
Tests for all durability functions
- Uses `MemoryJournalStorage` (in-memory)
- No network access required

### `wasm_component_integration.rs`
Tests for component loading and instantiation
- Loads WASM files from local filesystem
- Uses in-memory services
- Gracefully skips if WASM files not present

### `grpc_integration.rs`
Tests for gRPC deployment service
- Uses localhost (127.0.0.1) only
- No external network access
- No SSL required (HTTP only)

### `integration_tests.rs`
Tests for behavior routing and channels
- Uses `MockChannelService` (in-memory)
- No network access required

### `blob_host_functions_integration.rs`
Tests for all blob storage operations
- Uses `LocalFileSystem` with temp directory (in-memory)
- Uses in-memory SQLite for metadata
- Tests all WIT blob methods: upload, download, delete, exists, list, metadata, copy
- No network access required

## Running Tests

### Run All Tests (Offline)
```bash
# All tests work offline - no network required
cargo test --package plexspaces-wasm-runtime --test '*integration*' --no-fail-fast

# Run specific test suite
cargo test --package plexspaces-wasm-runtime --test messaging_host_functions_integration
cargo test --package plexspaces-wasm-runtime --test new_host_functions_integration
cargo test --package plexspaces-wasm-runtime --test durability_host_functions_integration
cargo test --package plexspaces-wasm-runtime --test blob_host_functions_integration
```

### Offline Mode
```bash
# Force offline mode (if dependencies are cached)
cargo test --package plexspaces-wasm-runtime --offline
```

## Test Requirements

### Required
- ✅ Rust toolchain (cargo, rustc)
- ✅ Dependencies in cargo cache (if running offline)
- ✅ Local filesystem access (for WASM file tests)

### Not Required
- ❌ Network access
- ❌ SSL certificates
- ❌ External services
- ❌ Internet connectivity
- ❌ Database connections
- ❌ Message queue services

## Troubleshooting

### Tests Fail with Network Errors
- **Cause**: Tests are trying to download dependencies
- **Solution**: Run `cargo build` first to cache dependencies, then run tests with `--offline`

### Tests Fail with SSL Errors
- **Cause**: Cargo trying to download dependencies
- **Solution**: Configure SSL per `docs/SSL_CERTIFICATE_FIX.md`, or use `--offline` if dependencies cached

### WASM File Not Found
- **Cause**: Calculator WASM example not built
- **Solution**: Tests will skip gracefully - this is expected if examples aren't built

## Best Practices

1. **Always use mocks** - Never use real external services in tests
2. **In-memory only** - All storage should be in-memory
3. **Localhost only** - If network needed, use 127.0.0.1 only
4. **Graceful skipping** - Skip tests if optional resources unavailable
5. **No assumptions** - Don't assume network, SSL, or external services




