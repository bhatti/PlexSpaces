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
  -p 9001:9001 \
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
  -p 9001:9001 \
  -e PLEXSPACES_NODE_ID=node1 \
  -e PLEXSPACES_LISTEN_ADDR=0.0.0.0:9001 \
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
      - PLEXSPACES_LISTEN_ADDR=0.0.0.0:9001
      - PLEXSPACES_CLUSTER_SEED_NODES=node1:9001,node2:9001,node3:9001
    ports:
      - "9001:9001"
  
  node2:
    image: plexspaces/node:latest
    environment:
      - PLEXSPACES_NODE_ID=node2
      - PLEXSPACES_LISTEN_ADDR=0.0.0.0:9001
      - PLEXSPACES_CLUSTER_SEED_NODES=node1:9001,node2:9001,node3:9001
    ports:
      - "9002:9001"
  
  node3:
    image: plexspaces/node:latest
    environment:
      - PLEXSPACES_NODE_ID=node3
      - PLEXSPACES_LISTEN_ADDR=0.0.0.0:9001
      - PLEXSPACES_CLUSTER_SEED_NODES=node1:9001,node2:9001,node3:9001
    ports:
      - "9003:9001"
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
        - containerPort: 9001
          name: grpc
        env:
        - name: PLEXSPACES_NODE_ID
          valueFrom:
            fieldRef:
              fieldPath: metadata.name
        - name: PLEXSPACES_LISTEN_ADDR
          value: "0.0.0.0:9001"
        livenessProbe:
          grpc:
            port: 9001
          initialDelaySeconds: 30
          periodSeconds: 10
        readinessProbe:
          grpc:
            port: 9001
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

#### Build Steps

```bash
# Clone repository
git clone https://github.com/plexobject/plexspaces.git
cd plexspaces

# Install dependencies (if using buf)
buf --version || (echo "Install buf: https://buf.build/docs/installation" && exit 1)

# Generate proto files
make proto

# Build
make build

# Run tests
make test

# Install CLI
cargo install --path crates/cli
```

## Configuration

### Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `PLEXSPACES_NODE_ID` | Unique node identifier | `node1` |
| `PLEXSPACES_LISTEN_ADDR` | gRPC listen address | `0.0.0.0:9001` |
| `PLEXSPACES_CLUSTER_SEED_NODES` | Cluster seed nodes | - |
| `PLEXSPACES_JOURNALING_BACKEND` | Journaling backend | `sqlite` |
| `PLEXSPACES_TUPLESPACE_BACKEND` | TupleSpace backend | `inmemory` |
| `PLEXSPACES_CHANNEL_BACKEND` | Channel backend | `inmemory` |

### Configuration File

```yaml
# config/default.yaml
node:
  id: node1
  listen_addr: "0.0.0.0:9001"
  cluster:
    seed_nodes:
      - "node1:9001"
      - "node2:9001"
      - "node3:9001"

journaling:
  backend: sqlite
  path: /var/lib/plexspaces/journal.db

tuplespace:
  backend: redis
  url: "redis://localhost:6379"

channel:
  backend: redis
  url: "redis://localhost:6379"
```

## Backend Options

### Journaling Backends

- **SQLite**: File-based, single-node (default)
- **PostgreSQL**: Multi-node, production-ready
- **InMemory**: Testing only

### TupleSpace Backends

- **InMemory**: Single-node, testing
- **Redis**: Multi-node, production-ready
- **PostgreSQL**: Multi-node, transactional

### Channel Backends

- **InMemory**: Single-node, testing
- **Redis**: Multi-node, pub/sub
- **NATS**: Multi-node, lightweight

## Production Deployment

### AWS EKS

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
grpc_health_probe -addr=localhost:9001

# Using curl (if HTTP gateway enabled)
curl http://localhost:8080/health
```

### Kubernetes Health Probes

```yaml
livenessProbe:
  grpc:
    port: 9001
  initialDelaySeconds: 30
  periodSeconds: 10

readinessProbe:
  grpc:
    port: 9001
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
lsof -i :9001

# Kill process
kill -9 <PID>
```

### Node Won't Start

1. Check logs: `docker logs plexspaces-node`
2. Verify configuration: Check environment variables
3. Test connectivity: `telnet localhost 9001`

### Cluster Not Forming

1. Verify seed nodes are accessible
2. Check firewall rules
3. Ensure all nodes use same cluster configuration

## Next Steps

- [Getting Started](getting-started.md): Learn the basics and create your first actor
- [Concepts Guide](concepts.md): Understand core concepts
- [Architecture](architecture.md): Understand the system design
- [Examples](../examples/): Explore example applications
- [Use Cases](use-cases.md): See real-world applications
