# Installation Guide

This guide covers installing and deploying PlexSpaces in various environments.

## Quick Start

### Docker (Recommended)

The official PlexSpaces Docker image is available at `plexobject/plexspaces`:

```bash
# Pull the latest image
docker pull plexobject/plexspaces:latest

# Run a single node (empty, ready for WASM deployments)
# gRPC and HTTP share a single port (8000)
docker run -d \
  --name plexspaces-node \
  -p 8000:8000 \
  -e PLEXSPACES_NODE_ID=node1 \
  -e PLEXSPACES_DISABLE_AUTH=1 \
  plexobject/plexspaces:latest
```

**Note**: The default Docker image starts an **empty node** with no pre-deployed applications. You can deploy WASM applications after the node starts. See [Deploying WASM Applications](#deploying-wasm-applications) below.

### Docker Compose (Production Setup)

```bash
# Start node with PostgreSQL and embedded object store
docker-compose up -d

# Check status
docker-compose ps

# View logs
docker-compose logs -f plexspaces-node
```

## Installation Methods

### 1. Docker

#### Official Docker Image

The official PlexSpaces Docker image is published to Docker Hub as `plexobject/plexspaces`:

```bash
# Pull latest version
docker pull plexobject/plexspaces:latest

# Or pull a specific version
docker pull plexobject/plexspaces:v0.1.0
```

#### Single Node (Empty, Ready for Deployments)

```bash
# Run empty node (auth disabled for testing)
# gRPC and HTTP share a single port (8000)
docker run -d \
  --name plexspaces-node \
  -p 8000:8000 \
  -e PLEXSPACES_NODE_ID=node1 \
  -e PLEXSPACES_LISTEN_ADDR=0.0.0.0:8000 \
  -e PLEXSPACES_DISABLE_AUTH=1 \
  plexobject/plexspaces:latest

# Check if node is ready
curl http://localhost:8000/api/v1/health

# View logs
docker logs -f plexspaces-node
```

**Production Configuration** (with authentication):

```bash
# Run with JWT authentication enabled
# gRPC and HTTP share a single port (8000)
docker run -d \
  --name plexspaces-node \
  -p 8000:8000 \
  -e PLEXSPACES_NODE_ID=node1 \
  -e PLEXSPACES_LISTEN_ADDR=0.0.0.0:8000 \
  -e PLEXSPACES_JWT_SECRET=your-secret-key-here \
  -v /path/to/certs:/app/certs:ro \
  -e PLEXSPACES_MTLS_CA_CERT=/app/certs/ca.crt \
  -e PLEXSPACES_MTLS_SERVER_CERT=/app/certs/server.crt \
  -e PLEXSPACES_MTLS_SERVER_KEY=/app/certs/server.key \
  plexobject/plexspaces:latest
```

#### Docker Compose (Multi-Node with Dependencies)

The `docker-compose.yml` file provides a production-ready setup with PostgreSQL and the embedded object store (rustfs):

```bash
# Start all services (auth enabled by default)
docker-compose up -d

# Check status
docker-compose ps

# View logs
docker-compose logs -f plexspaces-node

# Stop all services
docker-compose down
```

The compose file includes:
- **PlexSpaces Node**: Empty node ready for WASM deployments
- **PostgreSQL**: Shared database for scheduler, workflow, journaling, etc.
- **Embedded object store (rustfs)**: S3-compatible blob storage (auto-started by the node when no external endpoint is configured)

**Configuration**:
- Auth is **enabled by default** in `docker-compose.yml` (production-ready)
- To disable auth for testing, override at runtime:
  ```bash
  # Option 1: Override via environment variable
  PLEXSPACES_DISABLE_AUTH=1 docker-compose up
  
  # Option 2: Run one-off container with override
  docker-compose run -e PLEXSPACES_DISABLE_AUTH=1 plexspaces-node
  
  # Option 3: Modify docker-compose.yml to uncomment PLEXSPACES_DISABLE_AUTH=1
  ```
- To enable debug logs (similar to `scripts/server.sh`):
  ```bash
  RUST_LOG=warn,plexspaces_actor=debug,plexspaces_node=debug,plexspaces_services=debug,plexspaces_wasm_runtime=debug,plexspaces_actor=debug,plexspaces_application=debug,plexspaces_facet=debug,plexspaces_mailbox=debug \
  docker-compose up
  ```
- See [Security Configuration](#security-configuration) for production setup

#### Building Docker Image Locally

```bash
# Build from source
docker build -t plexobject/plexspaces:latest .

# Build with Firecracker enabled in addition to the default dashboard support
docker build --build-arg ENABLE_FIRECRACKER=1 -t plexobject/plexspaces:latest .

# Build with version tag
docker build -t plexobject/plexspaces:v0.1.0 .

# Build multiple tags at once
docker build -t plexobject/plexspaces:latest \
             -t plexobject/plexspaces:v0.1.0 \
             .

# Run locally built image
docker run -d \
  --name plexspaces-node \
  -p 8000:8000 \
  -e PLEXSPACES_DISABLE_AUTH=1 \
  plexobject/plexspaces:latest

# Or use the build script
./scripts/build-docker.sh latest
```

#### Publishing Docker Image

To publish the Docker image to Docker Hub:

```bash
# Login to Docker Hub
docker login

# Tag image (replace VERSION with actual version)
docker tag plexobject/plexspaces:latest plexobject/plexspaces:VERSION

# Push latest
docker push plexobject/plexspaces:latest

# Push versioned tag
docker push plexobject/plexspaces:VERSION
```

**Note**: Ensure you have push access to the `plexobject` Docker Hub organization.

#### Starting Server with Debug Logs (Similar to scripts/server.sh)

To start a server similar to `scripts/server.sh` with debug logs and auth disabled:

```bash
# Start with debug logs and auth disabled (for testing)
docker run -d \
  --name plexspaces-node \
  -p 8000:8000 \
  -e PLEXSPACES_NODE_ID=test-node \
  -e PLEXSPACES_LISTEN_ADDR=0.0.0.0:8000 \
  -e PLEXSPACES_DISABLE_AUTH=1 \
  -e PLEXSPACES_JWT_SECRET=test-secret \
  -e WASMTIME_BACKTRACE_DETAILS=1 \
  -e RUST_LOG=warn,plexspaces_actor=debug,plexspaces_node=debug,plexspaces_services=debug,plexspaces_wasm_runtime=debug,plexspaces_actor=debug,plexspaces_application=debug,plexspaces_facet=debug,plexspaces_mailbox=debug \
  -v $(pwd)/release.yaml:/app/config/release.yaml:ro \
  plexobject/plexspaces:latest

# View logs
docker logs -f plexspaces-node

# Check if server is ready
curl http://localhost:8000/api/v1/health
```

**With Docker Compose** (override auth and logging at runtime):

```bash
# Start with auth disabled and debug logs
PLEXSPACES_DISABLE_AUTH=1 \
RUST_LOG=warn,plexspaces_actor=debug,plexspaces_node=debug,plexspaces_services=debug,plexspaces_wasm_runtime=debug,plexspaces_actor=debug,plexspaces_application=debug,plexspaces_facet=debug,plexspaces_mailbox=debug \
docker-compose up

# Or run a one-off container with overrides
docker-compose run -e PLEXSPACES_DISABLE_AUTH=1 \
  -e RUST_LOG=warn,plexspaces_actor=debug,plexspaces_node=debug,plexspaces_services=debug,plexspaces_wasm_runtime=debug,plexspaces_actor=debug,plexspaces_application=debug,plexspaces_facet=debug,plexspaces_mailbox=debug \
  plexspaces-node
```

**Key differences from `scripts/server.sh`**:
- Uses Docker image instead of local build
- Port is 8000 instead of 8091 (configurable) (configurable)
- Release config path is `/app/config/release.yaml` (mounted or default)
- mTLS certs can be mounted as volume if needed

#### Testing WASM Deployment with Docker

1. **Start empty node** (with auth disabled for testing):
```bash
docker run -d \
  --name plexspaces-node \
  -p 8000:8000 \
  -e PLEXSPACES_NODE_ID=node1 \
  -e PLEXSPACES_DISABLE_AUTH=1 \
  plexobject/plexspaces:latest
```

2. **Wait for node to be ready**:
```bash
# Check health endpoint
curl http://localhost:8000/api/v1/health
```

3. **Deploy WASM application** (see [Deploying WASM Applications](#deploying-wasm-applications)):
```bash
# Example: Deploy calculator WASM app
curl -X POST http://localhost:8000/api/v1/applications/deploy \
  -F "application_id=calculator-app" \
  -F "name=calculator" \
  -F "version=1.0.0" \
  -F "wasm_file=@examples/simple/wasm_calculator/wasm-modules/calculator_actor.wasm"
```

4. **Verify deployment**:
```bash
# List deployed applications
curl http://localhost:8000/api/v1/dashboard/applications | jq
```

### 2. Kubernetes

#### Basic Deployment

```bash
# Deploy
kubectl apply -f k8s/deployment.yaml
kubectl apply -f k8s/service.yaml

# Check status
kubectl get pods -l app=plexspaces
kubectl get svc plexspaces
```

#### Deployment Manifest

```yaml
# k8s/deployment.yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: plexspaces
  labels:
    app: plexspaces
spec:
  replicas: 3
  selector:
    matchLabels:
      app: plexspaces
  template:
    metadata:
      labels:
        app: plexspaces
    spec:
      containers:
      - name: plexspaces-node
        image: plexspaces/node:latest
        ports:
        - containerPort: 8000
          name: grpc
        env:
        - name: PLEXSPACES_NODE_ID
          valueFrom:
            fieldRef:
              fieldPath: metadata.name
        - name: PLEXSPACES_LISTEN_ADDR
          value: "0.0.0.0:8000"
        livenessProbe:
          grpc:
            port: 8000
          initialDelaySeconds: 30
          periodSeconds: 10
        readinessProbe:
          grpc:
            port: 8000
          initialDelaySeconds: 10
          periodSeconds: 5
        resources:
          requests:
            cpu: 100m
            memory: 256Mi
          limits:
            cpu: 1000m
            memory: 1Gi
```

### 3. From Source

#### Prerequisites

- Rust 1.70+
- Protocol Buffers compiler (`buf` CLI recommended)
- Git
- **macOS only**: CA certificates for SSL (see SSL Certificate Configuration below)

#### SSL Certificate Configuration (macOS)

On macOS, cargo requires SSL certificates to download dependencies from crates.io. After installing `ca-certificates` via homebrew, configure the system:

**Option 1: Create Symlink (Recommended)**
```bash
# Install ca-certificates
brew install ca-certificates

# Create symlink (requires sudo)
sudo mkdir -p /etc/ssl
sudo ln -sf /opt/homebrew/etc/ca-certificates/cert.pem /etc/ssl/cert.pem

# Verify symlink
ls -la /etc/ssl/cert.pem
```

**Option 2: Copy Certificate File**
```bash
# Install ca-certificates
brew install ca-certificates

# Copy certificate file (requires sudo)
sudo mkdir -p /etc/ssl
sudo cp /opt/homebrew/etc/ca-certificates/cert.pem /etc/ssl/cert.pem

# Verify file exists
ls -la /etc/ssl/cert.pem
```

**Option 3: Environment Variables (No sudo required)**
```bash
# Install ca-certificates
brew install ca-certificates

# Add to ~/.zshrc or ~/.bash_profile
export SSL_CERT_FILE=/opt/homebrew/etc/ca-certificates/cert.pem
export GIT_SSL_CAINFO=/opt/homebrew/etc/ca-certificates/cert.pem

# Reload shell
source ~/.zshrc  # or source ~/.bash_profile
```

**Verify SSL Configuration:**
```bash
# Test cargo can download dependencies
cargo build --package plexspaces-wasm-runtime --lib
```

If you see SSL certificate errors, try Option 2 (copy) instead of Option 1 (symlink), as some tools may not follow symlinks properly.

#### Build Steps

```bash
# Clone repository
git clone https://github.com/plexobject/plexspaces.git
cd plexspaces

# Install dependencies (if using buf)
buf --version || (echo "Install buf: https://buf.build/docs/installation" && exit 1)

# Generate proto files (Rust + Python + TypeScript + Go; first run may need plugins)
# make proto-install-deps   # once: venv + betterproto, ts-proto, protoc-gen-go
make proto

# Build release version (recommended)
make release

# Or build CLI manually
cargo build --release --bin plexspaces -p plexspaces-cli

# Binary location
./target/release/plexspaces
```

#### Starting a Node

```bash
# Start with default settings
cargo run --release --bin plexspaces -- start

# Or use the built binary
./target/release/plexspaces start

# Or with custom node ID and address
cargo run --release --bin plexspaces -- start \
  --node-id my-node-1 \
  --listen-addr 0.0.0.0:8000

# With release config file (if supported)
cargo run --release --bin plexspaces -- start \
  --node-id my-node-1 \
  --listen-addr 0.0.0.0:8000
```

**Default port:**
- gRPC + HTTP/Dashboard (single port): `8000`

**Verify node is running:**
```bash
# Check health
curl http://localhost:8000/api/v1/health

# View dashboard
open http://localhost:8000
```

## Security

PlexSpaces provides comprehensive security features including:

- **Node-to-Node Authentication**: Mutual TLS (mTLS) for secure inter-node communication
- **User API Authentication**: JWT-based authentication for user-facing APIs
- **Tenant Isolation**: Mandatory tenant isolation for all operations
- **Security Validation**: Automatic validation that secrets are not hardcoded in config files
- **Auto-Generation**: Development certificates can be auto-generated (production should use proper key management)

### Security Configuration

#### Authentication Default Behavior

**Important**: Authentication is **enabled by default** in PlexSpaces for production security. The framework validates that required secrets are configured when auth is enabled.

**For Testing**: You can disable authentication via environment variable:
```bash
# Disable auth for testing (Docker)
docker run -e PLEXSPACES_DISABLE_AUTH=1 plexobject/plexspaces:latest

# Disable auth for testing (local)
export PLEXSPACES_DISABLE_AUTH=1
./target/release/plexspaces start
```

**For Production**: Always use proper JWT and/or mTLS configuration. Never disable authentication in production.

#### Environment Variables for Secrets

**CRITICAL**: All secrets must be provided via environment variables, never hardcoded in config files.

| Variable | Description | Required When |
|----------|-------------|---------------|
| `PLEXSPACES_JWT_SECRET` | JWT secret for HS256 signing | JWT enabled (unless using JWKS) |
| `PLEXSPACES_MTLS_CA_CERT` | Path to CA certificate file | mTLS enabled (unless auto-generating) |
| `PLEXSPACES_MTLS_SERVER_CERT` | Path to server certificate file | mTLS enabled (unless auto-generating) |
| `PLEXSPACES_MTLS_SERVER_KEY` | Path to server private key file | mTLS enabled (unless auto-generating) |
| `PLEXSPACES_MTLS_CERT_DIR` | Directory for auto-generated certificates | mTLS auto-generation (default: `/app/certs`) |
| `PLEXSPACES_DISABLE_AUTH` | Disable auth validation (testing only) | Never in production |

#### Docker Security Configuration

**Default Behavior**: Auth is **enabled by default** in both Docker image and `docker-compose.yml` for production security.

**Testing (Disable Auth at Runtime)**:
```bash
# Single container
docker run -e PLEXSPACES_DISABLE_AUTH=1 plexobject/plexspaces:latest

# Docker Compose - override at runtime
PLEXSPACES_DISABLE_AUTH=1 docker-compose up

# Or run one-off container
docker-compose run -e PLEXSPACES_DISABLE_AUTH=1 plexspaces-node
```

**Production (JWT Enabled)**:
```bash
docker run \
  -e PLEXSPACES_JWT_SECRET=your-secret-key-here \
  -v /path/to/certs:/app/certs:ro \
  -e PLEXSPACES_MTLS_CA_CERT=/app/certs/ca.crt \
  -e PLEXSPACES_MTLS_SERVER_CERT=/app/certs/server.crt \
  -e PLEXSPACES_MTLS_SERVER_KEY=/app/certs/server.key \
  plexobject/plexspaces:latest
```

**Docker Compose** (see `docker-compose.yml`):
- Auth is **enabled by default** (production-ready)
- To disable auth for testing, override at runtime: `PLEXSPACES_DISABLE_AUTH=1 docker-compose up`
- Or uncomment `PLEXSPACES_DISABLE_AUTH=1` in `docker-compose.yml` for local testing
- Configure JWT/mTLS via environment variables for production

#### JWT Configuration

```bash
# Set JWT secret (required for HS256)
export PLEXSPACES_JWT_SECRET="your-secret-key-here"

# Or use JWKS for RS256 (no secret needed)
# Configure jwks_url in SecurityConfig
```

#### mTLS Configuration

**Option 1: Provide Certificate Files**

```bash
# Set certificate paths via environment variables
export PLEXSPACES_MTLS_CA_CERT="/path/to/ca.crt"
export PLEXSPACES_MTLS_SERVER_CERT="/path/to/server.crt"
export PLEXSPACES_MTLS_SERVER_KEY="/path/to/server.key"
```

**Option 2: Auto-Generation (Development Only)**

```yaml
# In release.yaml or node config
runtime:
  security:
    mtls:
      enable_mtls: true
      auto_generate: true
      cert_dir: "/app/certs"  # Optional, defaults to /app/certs
```

Auto-generated certificates are saved to `cert_dir`:
- `ca.crt` - CA certificate
- `ca.key` - CA private key
- `server.crt` - Server certificate
- `server.key` - Server private key

**⚠️ Security Note**: Auto-generated certificates are for development/testing only. Production should use proper certificate management (cert-manager, Vault, AWS Certificate Manager, etc.).

#### Validation Behavior

- **If auth is enabled but keys are missing**: Node will fail to start with a fatal error
- **If `PLEXSPACES_DISABLE_AUTH=1`**: Validation is skipped (testing only)
- **If `disable_auth=true` in config**: Validation is skipped (testing only)

**For detailed security configuration and best practices, see [Security Guide](security.md).**

## Configuration

### Centralized Configuration Management

PlexSpaces uses a centralized configuration manager (`config_manager::initialize`) that handles all configuration with a clear priority order:

1. **Environment Variables** (highest priority) - Always override config file settings
2. **Configuration File** (release.yaml) - Default settings
3. **Built-in Defaults** - Sensible defaults for all settings

**Key Design Principle**: The `config_manager` is the **single source of truth** for all `PLEXSPACES_*` environment variables. No other component reads environment variables directly for configuration.

### Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `PLEXSPACES_NODE_ID` | Unique node identifier | `node1` |
| `PLEXSPACES_LISTEN_ADDR` | gRPC listen address | `0.0.0.0:8090` |
| `PLEXSPACES_DATABASE_URL` | Database connection string | `sqlite://${base_dir}/db/plexspaces.db` |
| `PLEXSPACES_BASE_DIR` | Base directory for all data | `~/plexspaces` |
| `PLEXSPACES_WASM_APPS_DIR` | Directory for WASM applications (auto-deploy on startup) | `${base_dir}/apps` |
| `PLEXSPACES_SAVE_WASM_APPS` | Save deployed WASM files to disk (testing only, default: disabled) | `0` or `1` |
| `PLEXSPACES_CLUSTER_SEED_NODES` | Cluster seed nodes | - |
| `PLEXSPACES_JOURNALING_BACKEND` | Journaling backend | `sqlite` (or `ddb` if `AWS_REGION` set) |
| `PLEXSPACES_TUPLESPACE_BACKEND` | TupleSpace backend | `inmemory` (or `ddb` if `AWS_REGION` set) |
| `PLEXSPACES_CHANNEL_PROVIDER` | Channel provider | `IN_MEMORY` |
| `PLEXSPACES_MAILBOX_PROVIDER` | Mailbox provider | `IN_MEMORY` |
| `PLEXSPACES_CLUSTER_NAME` | Overrides `spec.node.cluster_name` after load. Used for node registry membership labels, `ListConnectedNodes` / `from_registry` shard placement when the placement cluster field is empty, SWIM reconciliation with peers that omit cluster on ping, and UDP/multicast channel grouping. Set the **same** value on every node in a multinode deployment. If unset or empty and the release file leaves `node.cluster_name` empty, `config_manager::initialize` sets `node.cluster_name` to `default`. | - |
| `PLEXSPACES_JWT_SECRET` | JWT secret for HS256 (required if JWT enabled) | - |
| `PLEXSPACES_MTLS_CA_CERT` | Path to mTLS CA certificate | - |
| `PLEXSPACES_MTLS_SERVER_CERT` | Path to mTLS server certificate | - |
| `PLEXSPACES_MTLS_SERVER_KEY` | Path to mTLS server private key | - |
| `PLEXSPACES_MTLS_CERT_DIR` | Directory for auto-generated certificates | `/app/certs` |
| `PLEXSPACES_DISABLE_AUTH` | Disable auth validation (testing only) | - |
| `AWS_REGION` | AWS region (enables AWS backends) | - |
| `AWS_ACCESS_KEY_ID` | AWS access key (use IAM roles in production) | - |
| `AWS_SECRET_ACCESS_KEY` | AWS secret key (use IAM roles in production) | - |
| `DYNAMODB_ENDPOINT_URL` | DynamoDB endpoint (for local testing) | - |
| `SQS_ENDPOINT_URL` | SQS endpoint (for local testing) | - |
| `S3_ENDPOINT_URL` | S3 endpoint (for local testing) | - |

### Channel Providers

PlexSpaces supports multiple channel providers (renamed from "backends"):

| Provider | Enum Value | Description |
|----------|------------|-------------|
| `IN_MEMORY` | 0 | Fast, non-persistent (testing) |
| `REDIS` | 1 | Distributed, durable (Redis Streams) |
| `KAFKA` | 2 | High-throughput, durable |
| `SQLITE` | 3 | File-based, durable (single-node) |
| `NATS` | 4 | Lightweight pub/sub (multi-node) |
| `UDP` | 5 | Low-latency multicast (best-effort) |
| `SQS` | 6 | AWS-managed, auto-scaling |
| `PROCESS_GROUP` | 7 | In-cluster multicast |
| `POSTGRES` | 8 | PostgreSQL-based durable messaging |

### HTTP Endpoints

PlexSpaces exposes HTTP endpoints via gRPC-Gateway on the same port as gRPC (default: 8000):

**FaaS-Style Actor Invocation**:
- `GET /api/v1/actors/{namespace}/{actor_type}?param1=value1` - Read operations (tenant from JWT when auth is enabled)
- `POST /api/v1/actors/{namespace}/{actor_type}` - Update operations (tenant from JWT when auth is enabled)

**Example**:
```bash
# Get counter value (tenant comes from JWT when auth is enabled)
curl "http://localhost:8000/api/v1/actors/default/counter?action=get"

# Increment counter (tenant comes from JWT when auth is enabled)
curl -X POST "http://localhost:8000/api/v1/actors/default/counter" \
  -H "Content-Type: application/json" \
  -d '{"action":"increment"}'
```

See [Concepts: FaaS-Style Invocation](concepts.md#faas-style-invocation) for detailed documentation.

### Configuration File (release.yaml)

PlexSpaces uses a release configuration file (`release.yaml`) inspired by Erlang/OTP releases:

```yaml
# release.yaml
name: my-release
version: "1.0.0"
description: "My PlexSpaces release"

node:
  id: node1
  grpc_address: "0.0.0.0:8091"
  heartbeat_interval_ms: 5000
  clustering_enabled: true

runtime:
  # Base directory for all data (overridable via PLEXSPACES_BASE_DIR)
  base_dir: ""  # Defaults to ~/plexspaces
  
  # WASM applications directory (overridable via PLEXSPACES_WASM_APPS_DIR)
  wasm_apps_directory: ""  # Defaults to ${base_dir}/apps
  # Save deployed WASM files to disk (testing only, default: false)
  # Only saves when deploying via API (HTTP/gRPC), not during auto-deploy
  save_wasm_apps: false  # Overridable via PLEXSPACES_SAVE_WASM_APPS=1
  
  # Shared database configuration (overridable via PLEXSPACES_DATABASE_URL)
  db:
    connection_string: ""  # Defaults to sqlite://${base_dir}/db/plexspaces.db?mode=rwc
    pool_size: 10
    auto_migrate: true
  
  # Channel provider (0=IN_MEMORY, 1=REDIS, 2=KAFKA, etc.)
  channel_provider: 0
  
  # Mailbox provider (same enum as channel_provider)
  mailbox_provider: 0
  
  # gRPC configuration
  grpc:
    listen_addr: "0.0.0.0:8091"
    max_message_size: 104857600  # 100MB
  
  # Health check configuration
  health:
    enabled: true
    port: 8092
  
  # Security configuration
  security:
    disable_auth: false
    mtls:
      enable_mtls: false
      auto_generate: false
    jwt:
      enabled: false

# Applications to deploy on startup
applications: []
```

### Key Configuration Changes (v0.2+)

The following field names were updated for clarity:

| Old Name | New Name | Location |
|----------|----------|----------|
| `shared_database` | `db` | `runtime.db` |
| `channel_backend` | `channel_provider` | `runtime.channel_provider` |
| `mailbox_backend` | `mailbox_provider` | `runtime.mailbox_provider` |
| `wasm_apps_directory` | `wasm_apps_directory` | `runtime.wasm_apps_directory` (moved from `node`) |
| `save_wasm_apps` | `save_wasm_apps` | `runtime.save_wasm_apps` (default: false, testing only) |
| `ChannelBackend` enum | `ChannelProvider` enum | Proto files |

Release configs generated by the CLI also use the shared database under `${base_dir}/db/plexspaces.db?mode=rwc`. PlexSpaces does not place the runtime SQLite database in `/tmp`.

### AWS Configuration (Optional)

```yaml
# AWS configuration (enables AWS backends when AWS_REGION is set)
aws:
  region: "us-east-1"
  dynamodb:
    table_prefix: "plexspaces-"
  sqs:
    queue_prefix: "plexspaces-"
  s3:
    bucket: "plexspaces"

# UDP channel configuration (for cluster-wide multicast)
udp:
  multicast_address: "239.255.0.1"
  multicast_port: 9999
  cluster_name: "my-cluster"
```

## Backend Options

### Journaling Backends

- **SQLite**: File-based, single-node (default)
- **PostgreSQL**: Multi-node, production-ready
- **DynamoDB**: AWS-managed, auto-scaling, serverless (requires `AWS_REGION`)
- **InMemory**: Testing only

### TupleSpace Backends

- **InMemory**: Single-node, testing
- **Redis**: Multi-node, production-ready
- **PostgreSQL**: Multi-node, transactional
- **DynamoDB**: AWS-managed, auto-scaling, serverless (requires `AWS_REGION`)

### Channel Backends

- **InMemory**: Single-node, testing (non-durable)
- **Redis**: Multi-node, pub/sub, durable (Redis Streams)
- **Kafka**: Multi-node, high-throughput, durable
- **SQLite**: Single-node, durable, file-based persistence
- **NATS**: Multi-node, lightweight, pub/sub
- **UDP**: Multi-node, low-latency multicast pub/sub (best-effort, non-durable)
- **SQS**: AWS-managed, auto-scaling, serverless with DLQ support (requires `AWS_REGION`)

### KeyValue Backends

- **InMemory**: Single-node, testing
- **Redis**: Multi-node, production-ready
- **DynamoDB**: AWS-managed, auto-scaling, serverless (requires `AWS_REGION`)

### Blob Storage Backends

- **FileSystem**: Local file storage, single-node
- **S3**: AWS object storage with DynamoDB metadata (requires `AWS_REGION`)

**Channel Selection Guide**:
- **Development/Testing**: InMemory or SQLite
- **Production (Durable)**: Redis, Kafka, SQLite, or SQS
- **AWS Production**: SQS (auto-scaling, DLQ support, serverless)
- **Low-Latency Cluster Messaging**: UDP multicast (requires `cluster_name` configuration)
- **High-Throughput**: Kafka
- **Lightweight Pub/Sub**: NATS or Redis

**AWS Backend Selection**:
- **Full AWS Stack**: Set `AWS_REGION` environment variable - all components automatically use AWS backends
- **Hybrid**: Mix AWS and other backends by configuring individual components
- **Cost-Effective**: DynamoDB On-Demand, SQS Standard, S3 Standard

**AWS Backend Selection**:
- **Full AWS Stack**: Set `AWS_REGION` environment variable - all components automatically use AWS backends
- **Hybrid**: Mix AWS and other backends by configuring individual components
- **Cost-Effective**: DynamoDB On-Demand, SQS Standard, S3 Standard

## Production Deployment

### AWS Deployment

PlexSpaces supports full AWS deployment using DynamoDB, SQS, and S3 as backends. All tables, queues, and buckets are automatically created on first use.

#### Prerequisites

1. **AWS Account**: Active AWS account with appropriate permissions
2. **AWS CLI**: Installed and configured (`aws configure`)
3. **IAM Permissions**: The following permissions are required:
   - DynamoDB: `CreateTable`, `DescribeTable`, `PutItem`, `GetItem`, `UpdateItem`, `DeleteItem`, `Query`, `Scan`, `BatchWriteItem`
   - SQS: `CreateQueue`, `GetQueueUrl`, `SendMessage`, `ReceiveMessage`, `DeleteMessage`, `GetQueueAttributes`
   - S3: `CreateBucket`, `PutObject`, `GetObject`, `DeleteObject`, `ListBucket`

#### Configuration

Set environment variables or configure in `config/default.yaml`:

```bash
# AWS Region (required)
export AWS_REGION=us-east-1

# AWS Credentials (use IAM roles in production, not hardcoded keys)
export AWS_ACCESS_KEY_ID=your-access-key-id
export AWS_SECRET_ACCESS_KEY=your-secret-access-key

# Optional: Override endpoints for local testing
# export DYNAMODB_ENDPOINT_URL=http://localhost:8000
# export SQS_ENDPOINT_URL=http://localhost:4566
# export S3_ENDPOINT_URL=http://localhost:4566
```

Or configure in `config/default.yaml`:

```yaml
aws:
  region: "us-east-1"
  dynamodb:
    table_prefix: "plexspaces-"
    endpoint_url: ""  # Leave empty for production
  sqs:
    queue_prefix: "plexspaces-"
    endpoint_url: ""  # Leave empty for production
  s3:
    bucket: "plexspaces"
    endpoint_url: ""  # Leave empty for production
```

#### Enable AWS Backends

When `AWS_REGION` is set, PlexSpaces automatically uses AWS backends:

- **Locks**: DynamoDB
- **Scheduler**: DynamoDB
- **KeyValue**: DynamoDB
- **Channel**: SQS (with DLQ support)
- **Workflow**: DynamoDB
- **Journaling**: DynamoDB
- **Blob Storage**: DynamoDB (metadata) + S3 (object storage)
- **TupleSpace**: DynamoDB

#### Deployment Steps

1. **Configure AWS Credentials**:
   ```bash
   # Option 1: Environment variables
   export AWS_REGION=us-east-1
   export AWS_ACCESS_KEY_ID=your-key
   export AWS_SECRET_ACCESS_KEY=your-secret
   
   # Option 2: AWS CLI configuration
   aws configure
   
   # Option 3: IAM Role (recommended for EC2/ECS)
   # Attach IAM role to EC2 instance or ECS task
   ```

2. **Start PlexSpaces Node**:
   ```bash
   # With AWS backends (automatic when AWS_REGION is set)
   export AWS_REGION=us-east-1
   ./target/release/plexspaces start --node-id node1
   ```

3. **Verify AWS Resources**:
   ```bash
   # List DynamoDB tables
   aws dynamodb list-tables --region us-east-1 | grep plexspaces
   
   # List SQS queues
   aws sqs list-queues --region us-east-1 | grep plexspaces
   
   # List S3 buckets
   aws s3 ls | grep plexspaces
   ```

#### AWS EKS Deployment

```bash
# Create EKS cluster
eksctl create cluster --name plexspaces-cluster --region us-east-1

# Create IAM OIDC provider
eksctl utils associate-iam-oidc-provider --cluster plexspaces-cluster --approve

# Create IAM service account with DynamoDB, SQS, S3 permissions
eksctl create iamserviceaccount \
  --name plexspaces-sa \
  --namespace default \
  --cluster plexspaces-cluster \
  --attach-policy-arn arn:aws:iam::aws:policy/AmazonDynamoDBFullAccess \
  --attach-policy-arn arn:aws:iam::aws:policy/AmazonSQSFullAccess \
  --attach-policy-arn arn:aws:iam::aws:policy/AmazonS3FullAccess \
  --approve

# Deploy with IAM role
kubectl apply -f k8s/deployment.yaml

# Expose via LoadBalancer
kubectl expose deployment plexspaces --type=LoadBalancer
```

#### Kubernetes Deployment with AWS

```yaml
# k8s/deployment-aws.yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: plexspaces
  labels:
    app: plexspaces
spec:
  replicas: 3
  selector:
    matchLabels:
      app: plexspaces
  template:
    metadata:
      labels:
        app: plexspaces
    spec:
      serviceAccountName: plexspaces-sa  # IAM role via service account
      containers:
      - name: plexspaces-node
        image: plexspaces/node:latest
        ports:
        - containerPort: 8000
          name: grpc
        env:
        - name: AWS_REGION
          value: "us-east-1"
        - name: PLEXSPACES_NODE_ID
          valueFrom:
            fieldRef:
              fieldPath: metadata.name
        - name: PLEXSPACES_LISTEN_ADDR
          value: "0.0.0.0:8000"
        # AWS credentials via IAM role (no need to set AWS_ACCESS_KEY_ID)
        resources:
          requests:
            cpu: 100m
            memory: 256Mi
          limits:
            cpu: 1000m
            memory: 1Gi
```

#### Cost Optimization

- **DynamoDB**: Use On-Demand billing mode (auto-scaling, pay per request)
- **SQS**: Standard queues are cost-effective for most workloads
- **S3**: Use S3 Standard for active data, S3 Intelligent-Tiering for variable access patterns
- **Auto-created Resources**: All tables/queues/buckets are created with optimal settings

#### Monitoring

```bash
# CloudWatch metrics are automatically exported
# View in AWS Console:
# - DynamoDB: Tables > Metrics
# - SQS: Queues > Monitoring
# - S3: Buckets > Metrics

# Or use AWS CLI
aws cloudwatch get-metric-statistics \
  --namespace AWS/DynamoDB \
  --metric-name ConsumedReadCapacityUnits \
  --dimensions Name=TableName,Value=plexspaces-locks \
  --start-time 2025-01-01T00:00:00Z \
  --end-time 2025-01-01T23:59:59Z \
  --period 3600 \
  --statistics Sum
```

#### Local Testing with AWS Services

For local development, use Docker Compose to run DynamoDB Local and LocalStack:

```bash
# Start local AWS services
docker-compose -f docker-compose.aws-local.yml up -d

# Set local endpoints
export DYNAMODB_ENDPOINT_URL=http://localhost:8000
export SQS_ENDPOINT_URL=http://localhost:4566
export S3_ENDPOINT_URL=http://localhost:4566
export AWS_REGION=us-east-1
export AWS_ACCESS_KEY_ID=test
export AWS_SECRET_ACCESS_KEY=test

# Run tests
./scripts/test-aws-integration.sh
```

See [docker-compose.aws-local.README.md](../docker-compose.aws-local.README.md) for details.

### AWS EKS (Legacy)

```bash
# Create EKS cluster
eksctl create cluster --name plexspaces-cluster

# Deploy
kubectl apply -f k8s/deployment.yaml

# Expose via LoadBalancer
kubectl expose deployment plexspaces --type=LoadBalancer
```

### GCP GKE

```bash
# Create GKE cluster
gcloud container clusters create plexspaces-cluster

# Deploy
kubectl apply -f k8s/deployment.yaml
```

### Azure AKS

```bash
# Create AKS cluster
az aks create --name plexspaces-cluster --resource-group myResourceGroup

# Deploy
kubectl apply -f k8s/deployment.yaml
```

## Health Checks

### gRPC Health Check

```bash
# Using grpc_health_probe
grpc_health_probe -addr=localhost:8000

# Using curl (if HTTP gateway enabled)
curl http://localhost:8080/health
```

### Kubernetes Health Probes

```yaml
livenessProbe:
  grpc:
    port: 8000
  initialDelaySeconds: 30
  periodSeconds: 10

readinessProbe:
  grpc:
    port: 8000
  initialDelaySeconds: 10
  periodSeconds: 5
```

## Monitoring

### Metrics Endpoint

```bash
# Prometheus metrics
curl http://localhost:8080/metrics
```

### Logging

```bash
# View logs (Docker)
docker logs -f plexspaces-node

# View logs (Kubernetes)
kubectl logs -f -l app=plexspaces
```

## Troubleshooting

### Port Already in Use

```bash
# Find process using port
lsof -i :8000

# Kill process
kill -9 <PID>
```

### Node Won't Start

1. Check logs: `docker logs plexspaces-node`
2. Verify configuration: Check environment variables
3. Test connectivity: `telnet localhost 8000`

### Cluster Not Forming

1. Verify seed nodes are accessible
2. Check firewall rules
3. Ensure all nodes use same cluster configuration

## Deploying WASM Applications

> **📖 For comprehensive WASM deployment guide with polyglot examples, see [WASM Deployment Guide](wasm-deployment.md)**

### Quick Reference

### Method 1: HTTP Multipart Upload (Recommended for Large Files)

**Best for**: Files >5MB (Python WASM, unoptimized builds)

```bash
# gRPC and HTTP share a single port (e.g., 8000)
curl -X POST http://localhost:8000/api/v1/applications/deploy \
  -F "application_id=calculator-app" \
  -F "name=calculator" \
  -F "version=1.0.0" \
  -F "wasm_file=@examples/simple/wasm_calculator/wasm-modules/calculator_actor.wasm"
```

**Max file size**: 100MB

### Method 2: Using the CLI Tool (Small Files Only)

**Best for**: Files <5MB (Rust, optimized JavaScript/Go)

```bash
# Deploy using the CLI (gRPC, 5MB limit)
./target/release/plexspaces deploy \
  --node localhost:8000 \
  --app-id calculator-app \
  --name calculator \
  --version 1.0.0 \
  --wasm examples/simple/wasm_calculator/wasm-modules/calculator_actor.wasm

# Or using --wasm-module (alias)
./target/release/plexspaces deploy \
  --node localhost:8000 \
  --app-id calculator-app \
  --name calculator \
  --wasm-module examples/simple/wasm_calculator/wasm-modules/calculator_actor.wasm
```

**Note**: The CLI command is `deploy`, not `application deploy`. Use `--wasm` or `--wasm-module` for the WASM file path. For files >5MB, use HTTP multipart upload instead.

> **📖 See [WASM Deployment Guide](wasm-deployment.md) for complete polyglot examples (Rust, Python, TypeScript, Go)**

### Method 3: Using the Deployment Script

```bash
# Deploy using the helper script
./scripts/deploy-wasm-app-test.sh \
  http://localhost:8000 \
  calculator-app \
  examples/simple/wasm_calculator/wasm-modules/calculator_actor.wasm
```

The script will:
1. Check if WASM file exists
2. Encode WASM to base64
3. Deploy via gRPC or HTTP API
4. Verify deployment

### Method 4: Using gRPC Directly (grpcurl) - Small Files Only

```bash
# Install grpcurl if needed
# macOS: brew install grpcurl
# Linux: See https://github.com/fullstorydev/grpcurl

# Encode WASM file
WASM_BASE64=$(base64 -w 0 calculator_actor.wasm)

# Deploy via gRPC
grpcurl -plaintext \
  -d "{
    \"application_id\": \"calculator-app\",
    \"name\": \"calculator\",
    \"version\": \"1.0.0\",
    \"wasm_module\": {
      \"name\": \"calculator\",
      \"version\": \"1.0.0\",
      \"module_bytes\": \"${WASM_BASE64}\"
    }
  }" \
  localhost:8000 \
  plexspaces.application.v1.ApplicationService/DeployApplication
```

### Method 5: Using HTTP API (gRPC-Gateway) - Small Files Only

```bash
# Encode WASM file
WASM_BASE64=$(base64 -w 0 calculator_actor.wasm)

# Deploy via HTTP
curl -X POST http://localhost:8000/api/v1/applications \
  -H "Content-Type: application/json" \
  -d "{
    \"application_id\": \"calculator-app\",
    \"name\": \"calculator\",
    \"version\": \"1.0.0\",
    \"wasm_module\": {
      \"name\": \"calculator\",
      \"version\": \"1.0.0\",
      \"module_bytes\": \"${WASM_BASE64}\"
    }
  }"
```

## Deploying Python WASM Actors

### Step 1: Build Python WASM Actors

```bash
cd examples/simple/wasm_calculator

# Build all Python actors
./scripts/build_python_actors.sh

# WASM files will be in:
# wasm-modules/calculator_actor.wasm
# wasm-modules/advanced_calculator_actor.wasm
# wasm-modules/durable_calculator_actor.wasm
# wasm-modules/tuplespace_calculator_actor.wasm
# wasm-modules/channel_calculator_actor.wasm
```

**Prerequisites for Python WASM:**
- Python 3.9+
- `componentize-py` (install with `pip install componentize-py`)

### Step 2: Deploy Each Actor

```bash
# Deploy calculator actor (HTTP multipart for large Python WASM)
curl -X POST http://localhost:8000/api/v1/applications/deploy \
  -F "application_id=calculator-app" \
  -F "name=calculator" \
  -F "version=1.0.0" \
  -F "wasm_file=@examples/simple/wasm_calculator/wasm-modules/calculator_actor.wasm"

# Deploy durable calculator actor
curl -X POST http://localhost:8000/api/v1/applications/deploy \
  -F "application_id=durable-calculator-app" \
  -F "name=durable-calculator" \
  -F "version=1.0.0" \
  -F "wasm_file=@examples/simple/wasm_calculator/wasm-modules/durable_calculator_actor.wasm"

# Deploy tuplespace calculator actor
curl -X POST http://localhost:8000/api/v1/applications/deploy \
  -F "application_id=tuplespace-calculator-app" \
  -F "name=tuplespace-calculator" \
  -F "version=1.0.0" \
  -F "wasm_file=@examples/simple/wasm_calculator/wasm-modules/tuplespace_calculator_actor.wasm"
```

## WASM Applications Auto-Deploy and Persistence

PlexSpaces supports automatic deployment of WASM applications on node startup and optional persistence of deployed applications to disk.

### Auto-Deploy on Startup

When a node starts, it automatically scans the `wasm_apps_directory` and deploys all valid WASM applications found. This enables Tomcat-style auto-deployment where applications persist across node restarts.

**File Structure**:
```
{wasm_apps_directory}/
  payment-handler/
    app.wasm                     # Required: WASM module
    application-spec.toml        # Optional: ApplicationSpec config
  calculator/
    app.wasm
    application-spec.toml        # Optional: ApplicationSpec config
```

**Supported Format**:
- **Subdirectories**: `apps/app-name/app.wasm` + `apps/app-name/application-spec.toml`

**Configuration**:
- **Environment Variable**: `PLEXSPACES_WASM_APPS_DIR` (default: `${base_dir}/apps`)
- **Config File**: `runtime.wasm_apps_directory` in `release.yaml`

**Example**:
```bash
# Set custom apps directory
docker run -d \
  --name plexspaces-node \
  -p 8000:8000 \
  -e PLEXSPACES_NODE_ID=node1 \
  -e PLEXSPACES_WASM_APPS_DIR=/app/data/apps \
  -v /host/path/apps:/app/data/apps:ro \
  plexobject/plexspaces:latest
```

### Saving WASM Files on API Deployment

When deploying WASM applications via HTTP/gRPC API, you can optionally save the WASM files to disk for persistence and auto-deploy on next restart.

**Configuration**:
- **Environment Variable**: `PLEXSPACES_SAVE_WASM_APPS=1` (default: disabled)
- **Config File**: `runtime.save_wasm_apps: true` in `release.yaml`

**Important Notes**:
- ⚠️ **Only saves during API deployments** (HTTP/gRPC) - NOT during auto-deploy
- ⚠️ **Disabled by default** - only enable for testing/development
- ⚠️ **Production**: Use proper deployment pipelines, don't save arbitrary WASM files
- Files are saved atomically (temp file → atomic move) to prevent corruption
- Saved files use subdirectory format: `{wasm_apps_directory}/{app-name}/app.wasm` and `{wasm_apps_directory}/{app-name}/application-spec.toml`

**Example**:
```bash
# Enable saving WASM files on API deployment
docker run -d \
  --name plexspaces-node \
  -p 8000:8000 \
  -e PLEXSPACES_NODE_ID=node1 \
  -e PLEXSPACES_WASM_APPS_DIR=/app/data/apps \
  -e PLEXSPACES_SAVE_WASM_APPS=1 \
  -v /host/path/apps:/app/data/apps \
  plexobject/plexspaces:latest

# Deploy via API - files will be saved to /app/data/apps/payment-handler/app.wasm and application-spec.toml
curl -X POST http://localhost:8000/api/v1/applications/deploy \
  -F "application_id=payment-handler" \
  -F "name=payment-handler" \
  -F "version=1.0.0" \
  -F "wasm_file=@payment-handler.wasm"

# On next restart, payment-handler will be auto-deployed automatically from the subdirectory
```

**Workflow**:
1. Deploy WASM app via API with `PLEXSPACES_SAVE_WASM_APPS=1`
2. Files are atomically saved to `{wasm_apps_directory}/{app-name}/app.wasm` and `{wasm_apps_directory}/{app-name}/application-spec.toml`
3. On next node restart, the application is automatically detected and deployed from the subdirectory
4. Redeploying via API overwrites the old files with the new version

## Verifying Deployment

### Check Applications

```bash
# List all applications
curl http://localhost:8000/api/v1/dashboard/applications | jq

# Or check via dashboard API
curl http://localhost:8000/api/v1/dashboard/applications | jq '.applications[]'
```

### View Dashboard

```bash
# Open dashboard in browser
open http://localhost:8000

# Or view specific endpoints
curl http://localhost:8000/api/v1/dashboard/summary | jq
curl http://localhost:8000/api/v1/dashboard/applications | jq
curl http://localhost:8000/api/v1/dashboard/actors | jq
```

## Complete Deployment Example

```bash
# 1. Build PlexSpaces binaries
make release

# 2. Start node (in one terminal)
cargo run --release --bin plexspaces -- start --node-id test-node --listen-addr 0.0.0.0:8000

# 3. Build Python WASM actors (in another terminal)
cd examples/simple/wasm_calculator
./scripts/build_python_actors.sh

# 4. Deploy calculator actor (HTTP multipart for large Python WASM)
cd ../..
curl -X POST http://localhost:8000/api/v1/applications/deploy \
  -F "application_id=calculator-app" \
  -F "name=calculator" \
  -F "version=1.0.0" \
  -F "wasm_file=@examples/simple/wasm_calculator/wasm-modules/calculator_actor.wasm"

# 5. Verify deployment
curl http://localhost:8000/api/v1/dashboard/applications | jq '.applications[] | select(.name == "calculator")'

# 6. View dashboard
open http://localhost:8000
```

## Troubleshooting Deployment

### Deployment fails

```bash
# Check node is running
curl http://localhost:8000/api/v1/health

# Check WASM file exists and is valid
file examples/simple/wasm_calculator/wasm-modules/calculator_actor.wasm

# Check gRPC connection
grpcurl -plaintext localhost:8000 list
```

### WASM file too large

```bash
# Check file size
ls -lh examples/simple/wasm_calculator/wasm-modules/*.wasm

# Python WASM files are typically ~39MB (includes Python runtime)
# This is normal for componentize-py builds
```

## Docker Quick Reference

### Official Docker Image

- **Image**: `plexobject/plexspaces:latest`
- **Registry**: Docker Hub (https://hub.docker.com/r/plexobject/plexspaces)
- **Default Behavior**: Starts empty node, ready for WASM deployments
- **Authentication**: Enabled by default (override with `PLEXSPACES_DISABLE_AUTH=1` for testing)

### Common Docker Commands

```bash
# Pull latest image
docker pull plexobject/plexspaces:latest

# Run empty node (production, auth enabled)
docker run -d \
  --name plexspaces-node \
  -p 8000:8000 \
  -e PLEXSPACES_JWT_SECRET=your-secret-key \
  plexobject/plexspaces:latest

# Run empty node (testing, auth disabled)
docker run -d \
  --name plexspaces-node \
  -p 8000:8000 \
  -e PLEXSPACES_DISABLE_AUTH=1 \
  plexobject/plexspaces:latest

# Run with debug logs (similar to scripts/server.sh)
docker run -d \
  --name plexspaces-node \
  -p 8000:8000 \
  -e PLEXSPACES_DISABLE_AUTH=1 \
  -e RUST_LOG=warn,plexspaces_actor=debug,plexspaces_node=debug,plexspaces_services=debug,plexspaces_wasm_runtime=debug,plexspaces_actor=debug,plexspaces_application=debug,plexspaces_facet=debug,plexspaces_mailbox=debug \
  plexobject/plexspaces:latest

# Run with docker-compose (auth enabled by default)
docker-compose up -d

# Run docker-compose with auth disabled (testing)
PLEXSPACES_DISABLE_AUTH=1 docker-compose up

# View logs
docker logs -f plexspaces-node

# Stop node
docker stop plexspaces-node

# Remove container
docker rm plexspaces-node
```

### Docker Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `PLEXSPACES_NODE_ID` | Node identifier | `node1` |
| `PLEXSPACES_LISTEN_ADDR` | gRPC listen address | `0.0.0.0:8000` |
| `PLEXSPACES_CLUSTER_NAME` | Logical cluster (registry, placement, messaging); same on all nodes in a cluster. When unset and release `node.cluster_name` is empty, `initialize` uses `default`. | - |
| `PLEXSPACES_RELEASE_CONFIG` | Path to release config | `/app/config/release.yaml` |
| `PLEXSPACES_BASE_DIR` | Base directory for data | `/app/data` |
| `PLEXSPACES_DISABLE_AUTH` | Disable auth (testing only) | Not set (auth enabled) |
| `PLEXSPACES_JWT_SECRET` | JWT secret for authentication | Not set |
| `PLEXSPACES_MTLS_CA_CERT` | Path to mTLS CA certificate | Not set |
| `PLEXSPACES_MTLS_SERVER_CERT` | Path to mTLS server certificate | Not set |
| `PLEXSPACES_MTLS_SERVER_KEY` | Path to mTLS server private key | Not set |

### Building Docker Image

#### Build Locally

```bash
# Build latest image
docker build -t plexobject/plexspaces:latest .

# Build with Firecracker enabled in addition to the default dashboard support
docker build --build-arg ENABLE_FIRECRACKER=1 -t plexobject/plexspaces:latest .

# Build with version tag
docker build -t plexobject/plexspaces:v0.1.0 .

# Build and tag multiple versions
docker build -t plexobject/plexspaces:latest \
             -t plexobject/plexspaces:v0.1.0 \
             -t plexobject/plexspaces:v0.1 .
```

#### Build Options

```bash
# Build default image (dashboard enabled, firecracker disabled)
docker build -t plexobject/plexspaces:latest .

# Build with extra plexspaces-cli features
docker build \
  --build-arg FEATURES="some-cli-feature" \
  -t plexobject/plexspaces:latest .

# Build with Firecracker enabled
docker build \
  --build-arg ENABLE_FIRECRACKER=1 \
  -t plexobject/plexspaces:latest .

# Build without cache (force rebuild)
docker build --no-cache -t plexobject/plexspaces:latest .

# Build and see build output
docker build --progress=plain -t plexobject/plexspaces:latest .
```

**Note**: By default, the Dockerfile builds with `plexspaces-node/dashboard` enabled and Firecracker disabled. Set `ENABLE_FIRECRACKER=1` to include both `plexspaces-cli/firecracker` and `plexspaces-node/firecracker`.

#### Verify Build

```bash
# Check image was created
docker images | grep plexspaces

# Test the image locally
docker run --rm \
  -e PLEXSPACES_DISABLE_AUTH=1 \
  -p 8000:8000 \
  
  plexobject/plexspaces:latest

# Check image size
docker images plexobject/plexspaces:latest
```

### Publishing Docker Images to Docker Hub

#### Prerequisites

1. **Docker Hub Account**: Ensure you have an account at https://hub.docker.com
2. **Organization Access**: You must have push access to the `plexobject` organization
3. **Docker CLI**: Docker must be installed and configured

#### Step-by-Step Publishing Process

**Step 1: Login to Docker Hub**

```bash
# Login to Docker Hub
docker login

# Or login with username explicitly
docker login -u plexobject

# Login with password from stdin (for CI/CD)
echo $DOCKER_PASSWORD | docker login -u plexobject --password-stdin
```

**Step 2: Build the Image**

```bash
# Build the image with proper tags
docker build -t plexobject/plexspaces:latest \
             -t plexobject/plexspaces:v0.1.0 \
             .
```

**Step 3: Tag the Image (if needed)**

```bash
# Tag existing image
docker tag plexobject/plexspaces:latest plexobject/plexspaces:v0.1.0

# Tag with commit hash (for traceability)
docker tag plexobject/plexspaces:latest plexobject/plexspaces:$(git rev-parse --short HEAD)

# Tag with date
docker tag plexobject/plexspaces:latest plexobject/plexspaces:$(date +%Y%m%d)
```

**Step 4: Push to Docker Hub**

```bash
# Push latest tag
docker push plexobject/plexspaces:latest

# Push version tag
docker push plexobject/plexspaces:v0.1.0

# Push all tags at once
docker push plexobject/plexspaces:latest
docker push plexobject/plexspaces:v0.1.0
```

#### Using the Publishing Script

A complete publishing script is available at `scripts/publish-docker.sh`:

```bash
# Make executable (if not already)
chmod +x scripts/publish-docker.sh

# Publish with version (also tags as latest)
./scripts/publish-docker.sh v0.1.0

# Publish latest only
./scripts/publish-docker.sh latest

# Publish default image (dashboard enabled, firecracker disabled)
./scripts/publish-docker.sh v0.1.0

# Publish with Firecracker enabled
./scripts/publish-docker.sh v0.1.0 "" 1
```

The script will:
1. Build the Docker image
2. Verify the image was created
3. Prompt for Docker Hub login (if not already logged in)
4. Push the version tag
5. Push the latest tag (if version provided)
6. Display success message with Docker Hub URL

**Manual Publishing** (if you prefer to run commands manually):

```bash
# Build image
docker build -t plexobject/plexspaces:v0.1.0 -t plexobject/plexspaces:latest .

# Login to Docker Hub
docker login

# Push version tag
docker push plexobject/plexspaces:v0.1.0

# Push latest tag
docker push plexobject/plexspaces:latest
```

#### Versioning Best Practices

```bash
# Semantic versioning
docker build -t plexobject/plexspaces:v1.2.3 .
docker push plexobject/plexspaces:v1.2.3

# Major version tag (v1)
docker tag plexobject/plexspaces:v1.2.3 plexobject/plexspaces:v1
docker push plexobject/plexspaces:v1

# Minor version tag (v1.2)
docker tag plexobject/plexspaces:v1.2.3 plexobject/plexspaces:v1.2
docker push plexobject/plexspaces:v1.2

# Latest tag
docker tag plexobject/plexspaces:v1.2.3 plexobject/plexspaces:latest
docker push plexobject/plexspaces:latest
```

#### Publishing from CI/CD

**GitHub Actions Example**:

```yaml
name: Build and Push Docker Image

on:
  push:
    tags:
      - 'v*'

jobs:
  build-and-push:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      
      - name: Set up Docker Buildx
        uses: docker/setup-buildx-action@v2
      
      - name: Login to Docker Hub
        uses: docker/login-action@v2
        with:
          username: ${{ secrets.DOCKER_USERNAME }}
          password: ${{ secrets.DOCKER_PASSWORD }}
      
      - name: Extract version from tag
        id: tag
        run: echo "VERSION=${GITHUB_REF#refs/tags/}" >> $GITHUB_OUTPUT
      
      - name: Build and push
        uses: docker/build-push-action@v4
        with:
          context: .
          push: true
          tags: |
            plexobject/plexspaces:${{ steps.tag.outputs.VERSION }}
            plexobject/plexspaces:latest
```

#### Verify Published Image

```bash
# Pull and test published image
docker pull plexobject/plexspaces:latest

# Run published image
docker run --rm \
  -e PLEXSPACES_DISABLE_AUTH=1 \
  -p 8000:8000 \
  
  plexobject/plexspaces:latest

# Check image on Docker Hub
# Visit: https://hub.docker.com/r/plexobject/plexspaces/tags
```

#### Troubleshooting

**Permission Denied**:
```bash
# Ensure you're logged in
docker login

# Check you have access to plexobject organization
# Visit: https://hub.docker.com/orgs/plexobject/members
```

**Image Not Found**:
```bash
# Verify image exists locally
docker images | grep plexspaces

# Check image name matches Docker Hub repository
# Repository: plexobject/plexspaces
```

**Push Fails**:
```bash
# Check Docker Hub rate limits
# Free tier: 200 pulls per 6 hours
# Authenticated: 200 pulls per 6 hours
# Pro: Unlimited

# Retry push
docker push plexobject/plexspaces:latest
```

**Note**: Ensure you have push access to the `plexobject` Docker Hub organization. Contact your organization administrator if you don't have access.

## Next Steps

- [Getting Started](getting-started.md): Learn the basics and create your first actor
- [Usage Guide](usage.md): Practical usage patterns and examples
- [Security Guide](security.md): Configure security, mTLS, JWT, and tenant isolation
- [Concepts Guide](concepts.md): Understand core concepts
- [Architecture](architecture.md): Understand the system design
- [Examples](../examples/README.md): Explore example applications
- [Use Cases](use-cases.md): See real-world applications
