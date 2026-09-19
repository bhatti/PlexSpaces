# CLI - Command-Line Interface for PlexSpaces

**Purpose**: CLI tool for PlexSpaces framework management, providing commands for node management, actor operations, application deployment, and system administration.

## Overview

The PlexSpaces CLI provides a unified command-line interface for managing PlexSpaces nodes, actors, applications, and system resources.

## Commands

### Node Management

```bash
# Start a node
plexspaces node start --node-id node1 --address 0.0.0.0:9000

# Stop a node
plexspaces node stop --node-id node1

# List nodes
plexspaces node list
```

### Actor Operations

```bash
# Spawn an actor
plexspaces actor spawn --name counter --behavior GenServer

# Send message to actor
plexspaces actor send --to counter//counter::default@node1 --message "increment"

# List actors
plexspaces actor list --node-id node1
```

### Application Deployment

```bash
# Deploy application
plexspaces application deploy --name my-app --wasm app.wasm

# List applications (gRPC ListApplications; JSON for scripts)
plexspaces list --node localhost:8000 --json

# Stop application
plexspaces application stop --name my-app
```

### Firecracker VM Management

```bash
# Deploy to Firecracker VM
plexspaces firecracker deploy --tenant tenant1 --vcpus 1 --memory 256

# List VMs
plexspaces firecracker list

# Stop VM
plexspaces firecracker stop --vm-id vm-001
```

## Dependencies

This crate depends on:
- `plexspaces-proto`: Protocol buffer definitions
- `plexspaces-firecracker`: Firecracker VM management
- `tonic`: gRPC client
- `clap`: Command-line argument parsing
- `tokio`: Async runtime

## Dependents

This crate is a standalone binary used by:
- Developers: For managing PlexSpaces deployments
- Operators: For system administration
- CI/CD: For automated deployments

## Runtime Tuning Environment Variables

Set these before starting the node to override Tokio thread pool defaults:

| Variable | Default | Description |
|----------|---------|-------------|
| `PLEXSPACES_WORKER_THREADS` | `num_cpus` | Number of Tokio async worker threads |
| `PLEXSPACES_MAX_BLOCKING_THREADS` | `512` | `spawn_blocking` thread pool size |

```bash
PLEXSPACES_WORKER_THREADS=8 PLEXSPACES_MAX_BLOCKING_THREADS=64 plexspaces start
```

Unset or non-numeric values are silently ignored (Tokio defaults apply).

## References

- Implementation: `crates/cli/src/`
- Binary: `plexspaces` (installed via `cargo install`)
- [Architecture — Performance Tuning](../../docs/architecture.md#performance-tuning)
