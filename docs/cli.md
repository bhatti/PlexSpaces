# CLI Reference

The PlexSpaces CLI provides commands for managing nodes, actors, applications, and system resources.

## Installation

```bash
# Install from source
cargo install --path crates/cli

# Or use the binary directly
cargo run --bin plexspaces -- --help
```

## Commands

### Node Management

#### Start Node

```bash
plexspaces node start \
  --node-id node1 \
  --address 0.0.0.0:9000 \
  --config config/default.yaml
```

#### Stop Node

```bash
plexspaces node stop --node-id node1
```

#### List Nodes

```bash
plexspaces node list
```

#### Node Status

```bash
plexspaces node status --node-id node1
```

### Actor Operations

#### Spawn Actor

```bash
plexspaces actor spawn \
  --name counter \
  --behavior GenServer \
  --node-id node1
```

#### Send Message

```bash
plexspaces actor send \
  --to counter@node1 \
  --message '{"type": "increment", "amount": 5}'
```

#### List Actors

```bash
plexspaces actor list --node-id node1
```

#### Actor Status

```bash
plexspaces actor status --actor-id counter@node1
```

### Application Deployment

#### Deploy Application

```bash
plexspaces application deploy \
  --name my-app \
  --wasm app.wasm \
  --config app-config.yaml
```

#### List Applications

```bash
plexspaces application list
```

#### Application Status

```bash
plexspaces application status --name my-app
```

#### Stop Application

```bash
plexspaces application stop --name my-app
```

### Firecracker VM Management

#### Deploy to VM

```bash
plexspaces firecracker deploy \
  --tenant tenant1 \
  --vcpus 1 \
  --memory 256 \
  --wasm app.wasm
```

#### List VMs

```bash
plexspaces firecracker list
```

#### VM Status

```bash
plexspaces firecracker status --vm-id vm-001
```

#### Stop VM

```bash
plexspaces firecracker stop --vm-id vm-001
```

### Workflow Operations

#### Create Workflow

```bash
plexspaces workflow create \
  --definition workflow.yaml \
  --input input.json
```

#### List Workflows

```bash
plexspaces workflow list
```

#### Workflow Status

```bash
plexspaces workflow status --workflow-id <id>
```

#### Cancel Workflow

```bash
plexspaces workflow cancel --workflow-id <id>
```

### TupleSpace Operations

#### Write Tuple

```bash
plexspaces tuplespace write \
  --space default \
  --tuple '["order", "12345", "pending"]'
```

#### Read Tuple

```bash
plexspaces tuplespace read \
  --space default \
  --pattern '["order", "12345", ?]'
```

#### List Tuples

```bash
plexspaces tuplespace list --space default
```

### System Operations

#### Health Check

```bash
plexspaces health --node-id node1
```

#### Metrics

```bash
plexspaces metrics --node-id node1
```

#### Logs

```bash
plexspaces logs --node-id node1 --follow
```

## Configuration

### Global Flags

- `--config`: Path to configuration file
- `--node-id`: Target node ID
- `--verbose`: Enable verbose output
- `--quiet`: Suppress non-error output

### Environment Variables

- `PLEXSPACES_NODE_ID`: Default node ID
- `PLEXSPACES_CONFIG`: Default config file path
- `PLEXSPACES_LOG_LEVEL`: Log level (debug, info, warn, error)

## Examples

### Deploy and Run Actor

```bash
# Start node
plexspaces node start --node-id node1

# Spawn actor
plexspaces actor spawn --name counter --behavior GenServer

# Send message
plexspaces actor send --to counter@node1 --message '{"type": "increment"}'

# Check status
plexspaces actor status --actor-id counter@node1
```

### Deploy Application

```bash
# Build WASM
wasm-pack build --target wasm32-wasi

# Deploy
plexspaces application deploy \
  --name my-app \
  --wasm pkg/my_app.wasm

# Check status
plexspaces application status --name my-app
```

## See Also

- [Getting Started](getting-started.md): Quick start guide
- [Installation](installation.md): Installation instructions
- [Architecture](architecture.md): System design
