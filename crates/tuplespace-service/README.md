# TupleSpace Service - gRPC Service Wrapper

**Purpose**: Implements the gRPC TuplePlexSpaceService that delegates to a TupleSpaceProvider. This allows any TupleSpace implementation (InMemory, Redis, SQLite, etc.) to be exposed as a gRPC service for remote access.

## Architecture

```text
┌─────────────────────────────────────┐
│   gRPC Client                       │
│   (Remote Actor/Service)            │
└────────┬────────────────────────────┘
         │ gRPC/HTTP2
     ┌───▼──────────────────────────┐
     │  TuplePlexSpaceServiceImpl    │
     │  (This crate)                 │
     └────────┬──────────────────────┘
              │ Delegates to
         ┌────▼─────────────────┐
         │  TupleSpaceProvider  │ (trait)
         └────────┬─────────────┘
                  │
     ┌────────────┼─────────────┐
     │            │             │
  ┌──▼───┐   ┌───▼────┐   ┌───▼────┐
  │Memory│   │  Redis │   │ SQLite │
  └──────┘   └────────┘   └────────┘
```

## Usage Examples

### Expose TupleSpace as gRPC Service

```rust
use plexspaces_tuplespace::TupleSpace;
use plexspaces_tuplespace_service::TuplePlexSpaceServiceImpl;
use std::sync::Arc;

// Create a TupleSpace provider (InMemory)
let provider = Arc::new(TupleSpace::with_tenant_namespace("acme-corp", "production"));

// Wrap it in a gRPC service
let service = TuplePlexSpaceServiceImpl::new(provider);

// Serve via tonic
// Server::builder()
//     .add_service(TuplePlexSpaceServiceServer::new(service))
//     .serve(addr).await.unwrap();
```

## Design Principles

- **Provider Delegation**: Service doesn't implement logic, just translates gRPC ↔ Rust
- **Multi-Tenancy**: Tenant/namespace from provider ensures isolation
- **Proto-First**: All types from protobuf definitions
- **Stateless Service**: All state in provider, service is just a facade

## Dependencies

This crate depends on:
- `plexspaces_proto`: Protocol buffer definitions
- `plexspaces_tuplespace`: TupleSpace provider trait
- `tonic`: gRPC framework

## Dependents

This crate is used by:
- `plexspaces_node`: Node exposes TupleSpace via gRPC

## References

- Implementation: `crates/tuplespace-service/src/`
- Tests: `crates/tuplespace-service/tests/`
- Proto definitions: `proto/plexspaces/v1/tuplespace.proto`

