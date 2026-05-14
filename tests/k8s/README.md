# Kubernetes Test Configurations

**Purpose**: Test configurations for deploying PlexSpaces dashboard with 2 nodes, S3-compatible object store, and PostgreSQL.

**Note**: These are for testing purposes only. For production deployments, use the main `k8s/` directory configurations.

## Quick Start

```bash
# Deploy all components
./scripts/deploy-dashboard-k8s-test.sh

# Deploy WASM application
./scripts/deploy-wasm-app-test.sh

# Test dashboard
./scripts/test-dashboard.sh
```

## Components

1. **2 PlexSpaces Nodes** - Dashboard nodes for testing
2. **Object Store** - S3-compatible object store for blob service (default: rustfs)
3. **PostgreSQL** - Database for persistence
4. **Services** - ClusterIP services for all components

## Files

- `nodes.yaml` - 2 node deployments
- `object-store.yaml` - S3-compatible object store deployment (embedded, default: rustfs)
- `postgres.yaml` - PostgreSQL deployment
- `services.yaml` - Service definitions
- `namespace.yaml` - Test namespace

## Cleanup

```bash
kubectl delete -f tests/k8s/
```




