# Installation Guide

This guide covers installing and deploying PlexSpaces in various environments.

## Quick Start

### Docker (Recommended)

```bash
# Pull the latest image
docker pull plexspaces/node:latest

# Run a single node
docker run -d \
  --name plexspaces-node \
  -p 8080:8080 \
  -p 8000:8000 \
  -p 8001:8001 \
  plexspaces/node:latest
```

### Docker Compose (Multi-Node)

```bash
# Start a 3-node cluster
docker-compose up -d

# Check status
docker-compose ps

# View logs
docker-compose logs -f
```

## Installation Methods

### 1. Docker

#### Single Node

```bash
docker run -d \
  --name plexspaces-node \
  -p 8080:8080 \
  -p 8000:8000 \
  -p 8001:8001 \
  -e PLEXSPACES_NODE_ID=node1 \
  -e PLEXSPACES_LISTEN_ADDR=0.0.0.0:8000 \
  plexspaces/node:latest
```

#### Multi-Node Cluster

```yaml
# docker-compose.yml
version: '3.8'

services:
  node1:
    image: plexspaces/node:latest
    environment:
      - PLEXSPACES_NODE_ID=node1
      - PLEXSPACES_LISTEN_ADDR=0.0.0.0:8000
      - PLEXSPACES_CLUSTER_SEED_NODES=node1:8000,node2:8000,node3:8000
    ports:
      - "8000:8000"
      - "8001:8001"
  
  node2:
    image: plexspaces/node:latest
    environment:
      - PLEXSPACES_NODE_ID=node2
      - PLEXSPACES_LISTEN_ADDR=0.0.0.0:8000
      - PLEXSPACES_CLUSTER_SEED_NODES=node1:8000,node2:8000,node3:8000
    ports:
      - "8001:8000"
  
  node3:
    image: plexspaces/node:latest
    environment:
      - PLEXSPACES_NODE_ID=node3
      - PLEXSPACES_LISTEN_ADDR=0.0.0.0:8000
      - PLEXSPACES_CLUSTER_SEED_NODES=node1:8000,node2:8000,node3:8000
    ports:
      - "8002:8000"
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

# Generate proto files
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

**Default ports:**
- gRPC: `8000`
- HTTP/Dashboard: `8001`

**Verify node is running:**
```bash
# Check health
curl http://localhost:8001/api/v1/health

# View dashboard
open http://localhost:8001
```

## Security

PlexSpaces provides comprehensive security features including:

- **Node-to-Node Authentication**: Mutual TLS (mTLS) for secure inter-node communication
- **User API Authentication**: JWT-based authentication for user-facing APIs
- **Tenant Isolation**: Mandatory tenant isolation for all operations
- **Security Validation**: Automatic validation that secrets are not hardcoded in config files

**For detailed security configuration and best practices, see [Security Guide](security.md).**

## Configuration

### Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `PLEXSPACES_NODE_ID` | Unique node identifier | `node1` |
| `PLEXSPACES_LISTEN_ADDR` | gRPC listen address | `0.0.0.0:8000` |
| `PLEXSPACES_CLUSTER_SEED_NODES` | Cluster seed nodes | - |
| `PLEXSPACES_JOURNALING_BACKEND` | Journaling backend | `sqlite` (or `ddb` if `AWS_REGION` set) |
| `PLEXSPACES_TUPLESPACE_BACKEND` | TupleSpace backend | `inmemory` (or `ddb` if `AWS_REGION` set) |
| `PLEXSPACES_CHANNEL_BACKEND` | Channel backend | `inmemory` (or `sqs` if `AWS_REGION` set) |
| `PLEXSPACES_CLUSTER_NAME` | Cluster name for UDP channels | - |
| `AWS_REGION` | AWS region (enables AWS backends) | - |
| `AWS_ACCESS_KEY_ID` | AWS access key (use IAM roles in production) | - |
| `AWS_SECRET_ACCESS_KEY` | AWS secret key (use IAM roles in production) | - |
| `DYNAMODB_ENDPOINT_URL` | DynamoDB endpoint (for local testing) | - |
| `SQS_ENDPOINT_URL` | SQS endpoint (for local testing) | - |
| `S3_ENDPOINT_URL` | S3 endpoint (for local testing) | - |

### HTTP Endpoints

PlexSpaces exposes HTTP endpoints via gRPC-Gateway on the same port as gRPC (default: 8000):

**FaaS-Style Actor Invocation**:
- `GET /api/v1/actors/{tenant_id}/{namespace}/{actor_type}?param1=value1` - Read operations (ask pattern)
- `POST /api/v1/actors/{tenant_id}/{namespace}/{actor_type}` - Update operations (tell pattern)
- `GET /api/v1/actors/{namespace}/{actor_type}?param1=value1` - Read operations without tenant_id (defaults to "default")
- `POST /api/v1/actors/{namespace}/{actor_type}` - Update operations without tenant_id (defaults to "default")

**Example**:
```bash
# Get counter value (with tenant_id and namespace)
curl "http://localhost:8001/api/v1/actors/default/default/counter?action=get"

# Get counter value (without tenant_id, defaults to "default")
curl "http://localhost:8001/api/v1/actors/default/counter?action=get"

# Increment counter (with tenant_id and namespace)
curl -X POST "http://localhost:8001/api/v1/actors/default/default/counter" \
  -H "Content-Type: application/json" \
  -d '{"action":"increment"}'

# Increment counter (without tenant_id)
curl -X POST "http://localhost:8001/api/v1/actors/default/counter" \
  -H "Content-Type: application/json" \
  -d '{"action":"increment"}'
```

See [Concepts: FaaS-Style Invocation](concepts.md#faas-style-invocation) for detailed documentation.

### Configuration File

```yaml
# config/default.yaml
node:
  id: node1
  listen_addr: "0.0.0.0:8000"
  cluster:
    seed_nodes:
      - "node1:8000"
      - "node2:8000"
      - "node3:8000"

journaling:
  backend: sqlite  # or "ddb" for DynamoDB (requires AWS_REGION)
  path: /var/lib/plexspaces/journal.db

tuplespace:
  backend: redis  # or "ddb" for DynamoDB (requires AWS_REGION)
  url: "redis://localhost:6379"

channel:
  backend: redis  # or "sqs" for SQS (requires AWS_REGION)
  url: "redis://localhost:6379"

# AWS configuration (optional - enables AWS backends when AWS_REGION is set)
aws:
  region: "us-east-1"  # Set this to enable AWS backends
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
  cluster_name: "my-cluster"  # Nodes with same cluster_name can communicate
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
# HTTP gateway runs on gRPC port + 1 (e.g., 8001 if gRPC is 8000)
curl -X POST http://localhost:8001/api/v1/applications/deploy \
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
  http://localhost:8001 \
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
curl -X POST http://localhost:8001/api/v1/applications \
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
curl -X POST http://localhost:8001/api/v1/applications/deploy \
  -F "application_id=calculator-app" \
  -F "name=calculator" \
  -F "version=1.0.0" \
  -F "wasm_file=@examples/simple/wasm_calculator/wasm-modules/calculator_actor.wasm"

# Deploy durable calculator actor
curl -X POST http://localhost:8001/api/v1/applications/deploy \
  -F "application_id=durable-calculator-app" \
  -F "name=durable-calculator" \
  -F "version=1.0.0" \
  -F "wasm_file=@examples/simple/wasm_calculator/wasm-modules/durable_calculator_actor.wasm"

# Deploy tuplespace calculator actor
curl -X POST http://localhost:8001/api/v1/applications/deploy \
  -F "application_id=tuplespace-calculator-app" \
  -F "name=tuplespace-calculator" \
  -F "version=1.0.0" \
  -F "wasm_file=@examples/simple/wasm_calculator/wasm-modules/tuplespace_calculator_actor.wasm"
```

## Verifying Deployment

### Check Applications

```bash
# List all applications
curl http://localhost:8001/api/v1/dashboard/applications | jq

# Or check via dashboard API
curl http://localhost:8001/api/v1/dashboard/applications | jq '.applications[]'
```

### View Dashboard

```bash
# Open dashboard in browser
open http://localhost:8001

# Or view specific endpoints
curl http://localhost:8001/api/v1/dashboard/summary | jq
curl http://localhost:8001/api/v1/dashboard/applications | jq
curl http://localhost:8001/api/v1/dashboard/actors | jq
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
curl -X POST http://localhost:8001/api/v1/applications/deploy \
  -F "application_id=calculator-app" \
  -F "name=calculator" \
  -F "version=1.0.0" \
  -F "wasm_file=@examples/simple/wasm_calculator/wasm-modules/calculator_actor.wasm"

# 5. Verify deployment
curl http://localhost:8001/api/v1/dashboard/applications | jq '.applications[] | select(.name == "calculator")'

# 6. View dashboard
open http://localhost:8001
```

## Troubleshooting Deployment

### Deployment fails

```bash
# Check node is running
curl http://localhost:8001/api/v1/health

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

## Next Steps

- [Getting Started](getting-started.md): Learn the basics and create your first actor
- [Usage Guide](usage.md): Practical usage patterns and examples
- [Security Guide](security.md): Configure security, mTLS, JWT, and tenant isolation
- [Concepts Guide](concepts.md): Understand core concepts
- [Architecture](architecture.md): Understand the system design
- [Examples](../examples/): Explore example applications
- [Use Cases](use-cases.md): See real-world applications
