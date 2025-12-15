# Proto - Protocol Buffer Definitions

**Purpose**: Generated protobuf definitions for PlexSpaces. This crate contains all protocol buffer generated code.

## Overview

This crate is the **source of truth** for all data structures and gRPC service definitions in PlexSpaces. All types are defined in `.proto` files first, then generated to Rust code.

## Module Structure

- `common::v1`: Common types (Empty, Error, etc.)
- `actor::v1`: Actor runtime types
- `behaviors::v1`: Behavior types (GenServer, GenEvent, etc.)
- `facets::v1`: Facet types
- `tuplespace::v1`: TupleSpace types
- `processgroups::v1`: Process group types
- `node::v1`: Node types
- `supervision::v1`: Supervision types
- `grpc::v1`: gRPC middleware types
- `metrics::v1`: Metrics types
- `system::v1`: System types
- `application::v1`: Application types
- `journaling::v1`: Journaling types
- `workflow::v1`: Workflow types
- `channel::v1`: Channel types
- `mailbox::v1`: Mailbox types
- `object_registry::v1`: Object registry types
- `scheduler::v1`: Scheduler types
- `locks::prv`: Lock types (private)
- `circuitbreaker::prv`: Circuit breaker types (private)
- `pool::v1`: Pool types

## Proto-First Design

All data structures are defined in `.proto` files:
1. Define in `.proto` file
2. Generate Rust code with `buf generate`
3. Use generated types in Rust code

## Generation

```bash
# Generate Rust code from proto files
buf generate

# Generated files are in crates/proto/src/generated/
```

## Dependencies

This crate depends on:
- `prost`: Protocol buffer runtime
- `prost-types`: Protocol buffer standard types

## Dependents

This crate is used by:
- **ALL crates**: Every crate depends on proto for type definitions

## References

- Proto files: `proto/plexspaces/v1/`
- Generated code: `crates/proto/src/generated/`
- Buf configuration: `buf.yaml`

