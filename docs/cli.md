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
# Start with defaults
plexspaces start

# Start with custom node ID and address
plexspaces start --node-id node1 --listen-addr 0.0.0.0:8000

# Start with release configuration file
plexspaces start \
  --node-id node1 \
  --listen-addr 0.0.0.0:8000 \
  --release-config release.yaml

# You can use any file name or path:
plexspaces start --release-config /path/to/my-config.yaml
plexspaces start --release-config ./configs/production.yaml
plexspaces start --release-config release.yaml

# Options:
#   --node-id: Node ID (default: "node-1")
#   --listen-addr: Listen address (default: "0.0.0.0:8000")
#   --release-config <FILE>: Path to release config file (YAML format)
#                           Can be relative or absolute path
#                           If not provided, default config is created automatically
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
  --to counter//counter::default@node1 \
  --message '{"type": "increment", "amount": 5}'
```

#### List Actors

```bash
plexspaces actor list --node-id node1
```

#### Actor Status

```bash
plexspaces actor status --actor-id counter//counter::default@node1
```

### Application deployment

Commands map to gRPC `ApplicationService` (see `proto/plexspaces/v1/application/application.proto`). The node listens on `--listen-addr` for both gRPC and HTTP (single port, e.g. `scripts/server.sh`: port `8091`).

#### Deploy application

```bash
plexspaces deploy \
  --node localhost:8000 \
  --app-id my-app-id \
  --name my-app \
  --version 1.0.0 \
  --wasm app.wasm \
  --config app-config.toml
```

#### List applications

Uses `ApplicationService.ListApplications` over gRPC.

```bash
plexspaces list --node localhost:8000
plexspaces list --node localhost:8000 --json
```

`--json` prints a single JSON object shaped like `ListApplicationsResponse` (for scripts and tooling).

#### Undeploy application

Gracefully stops an application (same RPC family as deploy).

```bash
plexspaces undeploy --node localhost:8000 --app-id my-app-id
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

### Security Operations

#### Generate mTLS Certificates

Generate CA and server certificates for mTLS node-to-node authentication:

```bash
# Generate certificates with defaults (saves to ./certs)
plexspaces generate-mtls

# Generate with custom output directory
plexspaces generate-mtls --output /path/to/certs

# Generate with custom common names
plexspaces generate-mtls \
  --ca-common-name "My Company CA" \
  --server-common-name "my-node.example.com" \
  --validity-days 365

# Options:
#   --output, -o: Output directory (default: ./certs)
#   --ca-common-name: CA certificate common name (default: "PlexSpaces CA")
#   --server-common-name: Server certificate common name (default: "PlexSpaces Server")
#   --validity-days: Validity in days for server certificate (default: 90)
```

This generates:
- `ca.crt` - CA certificate (valid 1 year)
- `ca.key` - CA private key
- `server.crt` - Server certificate (signed by CA, valid 90 days by default)
- `server.key` - Server private key

**Usage:**
```bash
# Set environment variables to use generated certificates
export PLEXSPACES_MTLS_CA_CERT="./certs/ca.crt"
export PLEXSPACES_MTLS_SERVER_CERT="./certs/server.crt"
export PLEXSPACES_MTLS_SERVER_KEY="./certs/server.key"

# Or configure in release.yaml (see generate-release-config)
```

#### Generate Default Release Configuration

Generate a default release configuration file that can be customized:

```bash
# Generate with defaults (saves to release.yaml)
plexspaces generate-release-config

# Generate with custom values
plexspaces generate-release-config \
  --output my-release.yaml \
  --release-name "production-cluster" \
  --release-version "2.0.0" \
  --node-id "prod-node-1" \
  --listen-addr "0.0.0.0:8000"

# Options:
#   --output, -o: Output file path (default: release.yaml)
#   --release-name: Release name (default: "plexspaces-cluster")
#   --release-version: Release version (default: "1.0.0")
#   --node-id: Node ID (default: "node-1")
#   --listen-addr: Listen address (default: "0.0.0.0:8000")
```

The generated file includes:
- Node configuration (ID, listen address, clustering)
- Runtime configuration (gRPC, health checks)
- Security configuration (JWT, mTLS with auto-generation enabled)
- Shutdown configuration

**Usage:**
```bash
# Generate default config
plexspaces generate-release-config

# Customize the generated release.yaml file
vim release.yaml

# Start node with custom config
plexspaces start --node-id node1 --release-config release.yaml
```

### System Operations

#### Health Check

```bash
plexspaces status --node localhost:8000
```

#### Start Node

```bash
# Start with defaults
plexspaces start

# Start with custom node ID and address
plexspaces start --node-id my-node --listen-addr 0.0.0.0:8000

# Start with release configuration
plexspaces start \
  --node-id my-node \
  --listen-addr 0.0.0.0:8000 \
  --release-config release.yaml

# Options:
#   --node-id: Node ID (default: "node-1")
#   --listen-addr: Listen address (default: "0.0.0.0:8000")
#   --release-config: Path to release config file (YAML)
```

## Configuration

### Global Flags

- `--node-id`: Target node ID
- `--release-config`: Path to release configuration file (YAML)
- `--verbose`: Enable verbose output (via RUST_LOG env var)
- `--quiet`: Suppress non-error output

### Environment Variables

- `PLEXSPACES_NODE_ID`: Default node ID
- `PLEXSPACES_LISTEN_ADDR`: Default listen address
- `PLEXSPACES_CLUSTER_NAME`: Overrides release `node.cluster_name` via `config_manager` (multinode: use the same value on every node for registry and `from_registry` placement)
- `PLEXSPACES_JWT_SECRET`: JWT secret for authentication (required if JWT enabled)
- `PLEXSPACES_MTLS_CA_CERT`: Path to mTLS CA certificate
- `PLEXSPACES_MTLS_SERVER_CERT`: Path to mTLS server certificate
- `PLEXSPACES_MTLS_SERVER_KEY`: Path to mTLS server private key
- `PLEXSPACES_MTLS_CERT_DIR`: Directory for auto-generated certificates (default: `/app/certs`)
- `PLEXSPACES_DISABLE_AUTH`: Disable auth validation (testing only, never in production)
- `PLEXSPACES_LOG_LEVEL` or `RUST_LOG`: Log level (debug, info, warn, error)

## Examples

### Deploy and Run Actor

```bash
# Start node
plexspaces node start --node-id node1

# Spawn actor
plexspaces actor spawn --name counter --behavior GenServer

# Send message
plexspaces actor send --to counter//counter::default@node1 --message '{"type": "increment"}'

# Check status
plexspaces actor status --actor-id counter//counter::default@node1
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
