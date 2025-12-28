# PlexSpaces Dashboard

Dashboard service and UI for monitoring PlexSpaces nodes, applications, actors, and workflows.

## Features

- **Home Page**: Aggregated metrics across all nodes (clusters, nodes, tenants, apps, actors by type)
- **Node Page**: Detailed metrics and data for individual nodes
- **Real-time Updates**: HTMX polling for live data
- **Search & Filter**: Filter by tenant, node, cluster with datetime support
- **Pagination**: Support for large datasets
- **Production Ready**: Complete error handling, validation, and test coverage
- **Multi-node Support**: Aggregate metrics from multiple nodes in a cluster
- **Tenant Isolation**: Role-based filtering (admin vs non-admin)
- **System Metrics**: CPU, memory, disk, network metrics via sysinfo
- **Dependency Health Monitoring**: Monitor external dependencies (PostgreSQL, Redis, Kafka, MinIO, DynamoDB, SQS) with circuit breaker state

## Architecture

- **DashboardService**: gRPC service for aggregating metrics from local and remote nodes
- **Dashboard Handlers**: HTTP handlers for serving HTML pages and static assets
- **Static Assets**: HTML templates, CSS, and JavaScript embedded in binary

## Usage

The dashboard is **enabled by default** in `plexspaces-node`. No feature flag is required.

```toml
[dependencies]
plexspaces-node = { path = "../node" }  # Dashboard enabled by default
```

To disable the dashboard, build with `--no-default-features`:

```bash
cargo build --release --bin plexspaces-node -p plexspaces-node --no-default-features
```

## Routes

- `GET /` - Home page with aggregated metrics
- `GET /node/{node_id}` - Node detail page with node-specific metrics
- `GET /static/dashboard.css` - CSS styles
- `GET /static/dashboard.js` - JavaScript for charts and interactivity
- `GET /api/v1/dashboard/*` - Dashboard API endpoints (via gRPC-Gateway)

## API Endpoints

All endpoints support pagination via `PageRequest` and return `PageResponse`:

- `GET /api/v1/dashboard/summary` - Get aggregated summary metrics
- `GET /api/v1/dashboard/nodes` - List all nodes
- `GET /api/v1/dashboard/node/{node_id}` - Get node dashboard details
- `GET /api/v1/dashboard/applications` - List applications (with filters)
- `GET /api/v1/dashboard/actors` - List actors (with filters)
- `GET /api/v1/dashboard/workflows` - List workflows (optional service)
- `GET /api/v1/dashboard/dependencies` - Get dependency health status with circuit breaker info

### Dependency Health Endpoint

The dependency health endpoint provides detailed information about external dependencies:

```bash
curl http://localhost:8001/api/v1/dashboard/dependencies | jq
```

Response includes:
- **Dependency checks**: Status of each dependency (PostgreSQL, Redis, Kafka, MinIO, DynamoDB, SQS)
- **Circuit breaker state**: Current state (Closed, Open, Half-Open) for each dependency
- **Circuit breaker metrics**: Error rates, request counts, trip counts, time in state
- **Criticality**: Whether each dependency is critical (affects readiness) or non-critical (degraded mode)
- **Health status**: Overall health (Healthy, Degraded, Unhealthy)

This information is also included in the node dashboard response under `dependency_health`.

## Dependencies

- HTMX - Dynamic content updates without page reloads
- Alpine.js - Lightweight reactivity for UI components
- uPlot - Fast time-series charts for metrics visualization

## Testing

### Prerequisites

1. **Kubernetes cluster** (local or remote)
   - Minikube, Kind, or cloud Kubernetes (EKS, GKE, AKS)
   - `kubectl` configured and connected

2. **Docker** (for building container images)
   - Docker Desktop or Docker Engine

3. **Build tools**
   - Rust toolchain (`cargo`, `rustc`)
   - `make` (for building project)

4. **Optional tools** (for testing)
   - `curl` - For API testing
   - `jq` - For JSON parsing
   - `grpcurl` - For gRPC testing (optional)

### Step 1: Build Node Binary

Build the `plexspaces-node` binary (dashboard is enabled by default):

```bash
# Build the binary (dashboard enabled by default)
cargo build --release --bin plexspaces-node -p plexspaces-node

# Or build the entire workspace
cargo build --release

# Or use Makefile
make build

# Build Docker image (dashboard enabled by default)
docker build -t plexspaces-node:latest .

# Verify binary exists
./target/release/plexspaces-node --help
```

**Note**: 
- Dashboard is **enabled by default** - no feature flag needed
- The binary is at `target/release/plexspaces-node`
- Docker build includes dashboard by default (all features enabled)

### Step 2: Start Node Locally (Quick Test)

For quick local testing without Kubernetes:

```bash
# Build the binary
cargo build --release --bin plexspaces-node -p plexspaces-node

# Start the node
./target/release/plexspaces-node \
    --node-id "test-node" \
    --listen-address "0.0.0.0:8000"
```

The node will start with:
- **gRPC endpoint**: `http://localhost:8000`
- **Dashboard HTTP**: `http://localhost:8001`

Access dashboard at: http://localhost:8001

### Step 2b: Deploy Test Environment (Kubernetes)

For Kubernetes testing, deploy 2 PlexSpaces nodes with MinIO and PostgreSQL:

```bash
# Deploy all components (namespace, MinIO, PostgreSQL, nodes, services)
./scripts/deploy-dashboard-k8s-test.sh
```

This script will:
1. Create `plexspaces-test` namespace
2. Deploy MinIO (object storage for blob service)
3. Deploy PostgreSQL (database for persistence)
4. Deploy 2 PlexSpaces nodes (node-1 and node-2) with dashboard enabled
5. Create ClusterIP services for all components

**Expected output:**
```
🚀 Deploying PlexSpaces dashboard test environment...
📦 Creating namespace...
📦 Deploying MinIO...
📦 Deploying PostgreSQL...
⏳ Waiting for MinIO and PostgreSQL to be ready...
📦 Deploying PlexSpaces nodes...
📦 Deploying services...
⏳ Waiting for nodes to be ready...
✅ Deployment complete!

📊 Dashboard URLs:
  Node 1: http://localhost:8001 (port-forward: kubectl port-forward -n plexspaces-test svc/plexspaces-node-1 8001:8001)
  Node 2: http://localhost:8002 (port-forward: kubectl port-forward -n plexspaces-test svc/plexspaces-node-2 8002:8001)
```

### Step 3: Verify Deployment

Check that all pods are running:

```bash
# Check pod status
kubectl get pods -n plexspaces-test

# Expected output:
# NAME                                  READY   STATUS    RESTARTS   AGE
# minio-xxx                             1/1     Running   0          2m
# postgres-xxx                          1/1     Running   0          2m
# plexspaces-node-1-xxx                1/1     Running   0          1m
# plexspaces-node-2-xxx                1/1     Running   0          1m
```

Check node logs:

```bash
# Node 1 logs
kubectl logs -n plexspaces-test deployment/plexspaces-node-1 --tail=50

# Node 2 logs
kubectl logs -n plexspaces-test deployment/plexspaces-node-2 --tail=50

# Look for dashboard initialization messages:
# "Dashboard service initialized"
# "Dashboard HTTP router registered"
```

### Step 4: Port Forward to Access Dashboard

Set up port forwarding to access the dashboard from your local machine:

```bash
# Terminal 1: Port forward Node 1
kubectl port-forward -n plexspaces-test svc/plexspaces-node-1 8001:8001

# Terminal 2: Port forward Node 2 (optional, for multi-node testing)
kubectl port-forward -n plexspaces-test svc/plexspaces-node-2 8002:8001
```

### Step 5: Access Dashboard in Browser

Open your browser and navigate to:

- **Node 1 Dashboard**: http://localhost:8001/
- **Node 2 Dashboard**: http://localhost:8002/ (if port-forwarded)

You should see:
- Home page with aggregated metrics (clusters, nodes, tenants, apps, actors)
- Search and filter controls
- Nodes table with links to node detail pages
- Workflows table (empty initially)

### Step 6: Deploy Test Application

Deploy a WASM application to generate metrics for the dashboard:

```bash
# Deploy WASM calculator application (script will build if needed)
./scripts/deploy-wasm-app-test.sh http://localhost:8001 wasm-calculator
```

The script will:
1. Check for existing WASM files
2. Build WASM from examples if needed
3. Deploy via gRPC (grpcurl) or HTTP API (gRPC-Gateway)

**Expected output:**
```
🚀 Deploying WASM application: wasm-calculator
   Node: http://localhost:8001 (gRPC: localhost:8000)
📦 Building wasm-calculator...
📦 WASM file: examples/simple/wasm_calculator/target/wasm32-wasi/release/wasm_calculator.wasm
   Size: 123456 bytes
📤 Encoding WASM file...
📤 Deploying via gRPC (grpcurl)...
✅ Deployment successful via gRPC!

🔍 Verify deployment:
   curl -s http://localhost:8001/api/v1/dashboard/applications | jq '.applications[] | select(.name == "wasm-calculator")'
```

**Alternative: Manual Deployment**

If you have a WASM module ready:

```bash
# Using grpcurl (if installed)
WASM_BASE64=$(base64 -w 0 path/to/calculator.wasm)

grpcurl -plaintext \
  -d "{
    \"application_id\": \"wasm-calculator\",
    \"name\": \"wasm-calculator\",
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

**Using Example Applications:**

```bash
# Option 1: Build wasm-calculator example
cd examples/simple/wasm_calculator
cargo build --release --target wasm32-wasi
cd ../../..

# Deploy using the built WASM
./scripts/deploy-wasm-app-test.sh \
  http://localhost:8001 \
  wasm-calculator \
  examples/simple/wasm_calculator/target/wasm32-wasi/release/wasm_calculator.wasm

# Option 2: Use wasm-showcase Python actors
cd examples/wasm_showcase
./scripts/build_python_actors.sh
cd ../..

# Deploy Python counter actor
./scripts/deploy-wasm-app-test.sh \
  http://localhost:8001 \
  python-counter \
  examples/wasm_showcase/wasm-modules/counter_actor.wasm
```

**Verify Application Deployment:**

```bash
# Check applications list
curl -s http://localhost:8001/api/v1/dashboard/applications | jq '.applications'

# Check application status
curl -s http://localhost:8001/api/v1/applications/wasm-calculator/status | jq '.'

# View in dashboard
open http://localhost:8001/  # macOS
# or
xdg-open http://localhost:8001/  # Linux
```

### Step 7: Test Dashboard APIs

**Quick Test (Automated):**

```bash
# Run the automated test script (builds node, starts it, tests endpoints)
./scripts/test-dashboard.sh
```

This script will:
- Build the node binary if needed
- Start the node on `localhost:8000` (gRPC) and `localhost:8001` (HTTP/Dashboard)
- Test all dashboard endpoints
- Display dashboard URLs

**Manual API Testing:**

**Manual API Testing:**

```bash
# Test summary API
curl -s http://localhost:8001/api/v1/dashboard/summary | jq '.'

# Test nodes API
curl -s http://localhost:8001/api/v1/dashboard/nodes | jq '.'

# Test node dashboard (replace node-1 with actual node ID)
curl -s http://localhost:8001/api/v1/dashboard/node/node-1 | jq '.'

# Test applications API
curl -s http://localhost:8001/api/v1/dashboard/applications | jq '.'

# Test actors API
curl -s http://localhost:8001/api/v1/dashboard/actors | jq '.'

# Test workflows API
curl -s http://localhost:8001/api/v1/dashboard/workflows | jq '.'

# Test dependency health API (new)
curl -s http://localhost:8001/api/v1/dashboard/dependencies | jq '.'
```

**Test with Filters:**

```bash
# Filter by tenant
curl -s "http://localhost:8001/api/v1/dashboard/applications?tenant_id=test-tenant" | jq '.'

# Filter by name pattern
curl -s "http://localhost:8001/api/v1/dashboard/applications?name_pattern=calc" | jq '.'

# Filter actors by type
curl -s "http://localhost:8001/api/v1/dashboard/actors?actor_type=calculator" | jq '.'

# Pagination
curl -s "http://localhost:8001/api/v1/dashboard/actors?page_size=10&page_token=0" | jq '.'
```

### Step 8: Verify Dashboard Functionality

**Home Page Tests:**

1. **Metrics Widgets**: Verify counts for clusters, nodes, tenants, apps, actors
2. **Search Box**: Test filtering by tenant-id, node-id, cluster-id
3. **Date Filter**: Test datetime range filtering (default: last 24 hours)
4. **Nodes Table**: Verify node list with IDs, clusters, capabilities, links
5. **Workflows Table**: Verify workflows list (may be empty if no workflows deployed)

**Node Detail Page Tests:**

1. **Node Metrics**: Verify node-specific metrics (CPU, memory, messages, actors)
2. **System Metrics**: Verify system-level metrics (CPU, memory, disk, network)
3. **Summary Metrics**: Verify tenant/app/actor counts for this node
4. **Applications Table**: Test search by application name/ID, namespace
5. **Actors Table**: Test search by actor ID, group, type, namespace, tenant
6. **Status Filter**: Test filtering by running/terminated actors
7. **Durable Actors**: Verify journal size and checkpoint info (if applicable)
8. **Pagination**: Test pagination controls for large datasets

**Browser Console Tests:**

Open browser developer console (F12) and verify:
- No JavaScript errors
- HTMX requests completing successfully
- Charts rendering (if uPlot is configured)
- Real-time updates working (if polling enabled)

### Step 9: Test Multi-Node Aggregation

If both nodes are running:

```bash
# Port forward both nodes
# Terminal 1:
kubectl port-forward -n plexspaces-test svc/plexspaces-node-1 8001:8001

# Terminal 2:
kubectl port-forward -n plexspaces-test svc/plexspaces-node-2 8002:8001
```

1. Deploy applications to both nodes
2. Access Node 1 dashboard: http://localhost:8001/
3. Verify aggregated metrics show data from both nodes
4. Click on node links to navigate to individual node pages

### Step 10: Cleanup

Remove test environment:

```bash
# Delete all test resources
kubectl delete -f tests/k8s/

# Or delete namespace (removes everything)
kubectl delete namespace plexspaces-test

# Verify cleanup
kubectl get pods -n plexspaces-test
# Should return: No resources found
```

## Troubleshooting

### Dashboard Not Accessible

**Issue**: Cannot access dashboard at http://localhost:8001

**Solutions**:
1. **Verify port-forward is running**:
   ```bash
   kubectl get pods -n plexspaces-test
   kubectl port-forward -n plexspaces-test svc/plexspaces-node-1 8001:8001
   ```

2. **Check node logs for errors**:
   ```bash
   kubectl logs -n plexspaces-test deployment/plexspaces-node-1 --tail=100 | grep -i dashboard
   ```
   Look for: "Dashboard service initialized" or "Dashboard HTTP router registered"

3. **Verify dashboard is enabled (enabled by default)**:
   ```bash
   # Check binary was built
   ls -lh target/release/plexspaces-node
   
   # Or rebuild
   cargo build --release --bin plexspaces-node -p plexspaces-node
   
   # Docker build includes dashboard by default
   docker build -t plexspaces-node:latest .
   ```

4. **Check firewall/network settings**:
   ```bash
   # Test port-forward connectivity
   curl -v http://localhost:8001/
   
   # Check if port is listening
   lsof -i :8001  # macOS/Linux
   ```

5. **Verify HTTP port is exposed**:
   ```bash
   # Check service configuration
   kubectl get svc -n plexspaces-test plexspaces-node-1 -o yaml | grep -A 5 ports
   ```

### No Metrics Showing

**Issue**: Dashboard shows zero metrics

**Solutions**:
1. **Deploy test applications to generate metrics**:
   ```bash
   ./scripts/deploy-wasm-app-test.sh http://localhost:8001 wasm-calculator
   ```

2. **Check that actors are registered**:
   ```bash
   kubectl logs -n plexspaces-test deployment/plexspaces-node-1 | grep -i "actor\|application"
   ```
   Look for: "Application deployed", "Actor registered", "Actor spawned"

3. **Verify ApplicationManager is working**:
   ```bash
   # List applications via API
   curl -s http://localhost:8001/api/v1/dashboard/applications | jq '.applications | length'
   
   # Should return > 0 if applications are deployed
   ```

4. **Check ActorRegistry for registered actors**:
   ```bash
   # List actors via API
   curl -s http://localhost:8001/api/v1/dashboard/actors | jq '.actors | length'
   
   # Check specific actor
   curl -s http://localhost:8001/api/v1/dashboard/actors | jq '.actors[0]'
   ```

5. **Wait for metrics to populate**:
   - Metrics may take a few seconds to appear after deployment
   - Refresh the dashboard page
   - Check browser console for HTMX polling errors

### API Errors

**Issue**: API endpoints return errors

**Solutions**:
1. **Check gRPC-Gateway is configured correctly**:
   ```bash
   # Test gRPC-Gateway endpoint
   curl -v http://localhost:8001/api/v1/dashboard/summary
   
   # Should return JSON, not HTML
   ```

2. **Verify proto files are generated**:
   ```bash
   make proto
   # Or
   cargo build -p plexspaces-proto
   ```

3. **Check node logs for gRPC errors**:
   ```bash
   kubectl logs -n plexspaces-test deployment/plexspaces-node-1 | grep -i "error\|grpc\|dashboard"
   ```

4. **Verify request format matches proto definitions**:
   ```bash
   # Check proto definition
   cat proto/plexspaces/v1/dashboard/dashboard.proto | grep -A 10 "GetSummaryRequest"
   
   # Test with minimal request
   curl -s -X POST http://localhost:8001/api/v1/dashboard/summary \
     -H "Content-Type: application/json" \
     -d '{}' | jq '.'
   ```

5. **Check HTTP vs gRPC ports**:
   - Dashboard HTTP: port 8001
   - gRPC: port 8000
   - Ensure you're using the correct port for the protocol

### Pagination Not Working

**Issue**: Pagination controls not functioning

**Solutions**:
1. Verify `PageRequest` and `PageResponse` are properly serialized
2. Check browser console for JavaScript errors
3. Verify HTMX is loading correctly
4. Test pagination via direct API calls

### Remote Node Queries Failing

**Issue**: Cannot query remote nodes

**Solutions**:
1. **Verify ObjectRegistry has node registrations**:
   ```bash
   # Check if nodes can discover each other
   kubectl logs -n plexspaces-test deployment/plexspaces-node-1 | grep -i "node\|registry\|discovery"
   ```

2. **Check network connectivity between nodes**:
   ```bash
   # Test connectivity from node-1 to node-2
   kubectl exec -n plexspaces-test deployment/plexspaces-node-1 -- \
     curl -s http://plexspaces-node-2.plexspaces-test:8001/api/v1/dashboard/summary
   ```

3. **Verify gRPC client configuration**:
   - Check cluster seed nodes are configured correctly
   - Verify node IDs match in configuration

4. **Check node discovery is working**:
   ```bash
   # Both nodes should see each other
   kubectl logs -n plexspaces-test deployment/plexspaces-node-1 | grep "node-2"
   kubectl logs -n plexspaces-test deployment/plexspaces-node-2 | grep "node-1"
   ```

5. **Note**: Remote node aggregation is a future enhancement. Currently, dashboard primarily shows local node metrics.

## Development

### Running Tests

```bash
# Run unit tests
cd crates/dashboard
cargo test

# Run with output
cargo test -- --nocapture

# Run specific test
cargo test test_get_summary
```

### Building for Development

```bash
# Build the binary (dashboard enabled by default)
cargo build --bin plexspaces-node -p plexspaces-node

# Build with debug symbols
cargo build --bin plexspaces-node -p plexspaces-node --profile dev

# Run node locally
cargo run --bin plexspaces-node -p plexspaces-node

# Or run the release binary
./target/release/plexspaces-node --node-id "dev-node" --listen-address "0.0.0.0:8000"
```

### Adding New Metrics

1. Update proto definitions in `proto/plexspaces/v1/dashboard/dashboard.proto`
2. Regenerate proto code: `make proto`
3. Update `DashboardServiceImpl` to collect new metrics
4. Update frontend HTML to display new metrics
5. Add tests for new functionality

## Production Deployment

For production deployments:

1. **Build the node binary** (dashboard enabled by default):
   ```bash
   # Build the binary
   cargo build --release --bin plexspaces-node -p plexspaces-node
   
   # Or build Docker image (dashboard enabled by default)
   docker build -t plexspaces-node:latest .
   ```

2. **Configure authentication** (dashboard respects tenant filtering):
   - Set up auth middleware to populate `x-tenant-id` and `x-user-role` headers
   - Dashboard automatically filters data based on tenant and admin status

3. **Set up ingress** for external access (instead of port-forward):
   ```yaml
   apiVersion: networking.k8s.io/v1
   kind: Ingress
   metadata:
     name: plexspaces-dashboard
   spec:
     rules:
     - host: dashboard.example.com
       http:
         paths:
         - path: /
           pathType: Prefix
           backend:
             service:
               name: plexspaces-node-1
               port:
                 number: 8001
   ```

4. **Configure TLS** for secure connections:
   - Use cert-manager for automatic certificate management
   - Or configure TLS in ingress with your certificates

5. **Set resource limits** in Kubernetes deployments:
   ```yaml
   resources:
     requests:
       cpu: 500m
       memory: 512Mi
     limits:
       cpu: 2000m
       memory: 2Gi
   ```

6. **Enable monitoring** for dashboard service metrics:
   - Dashboard uses `metrics` crate for observability
   - Integrate with Prometheus/Grafana for metrics collection

7. **Configure caching** for aggregated metrics (if needed):
   - Add caching layer for frequently accessed metrics
   - Use Redis or in-memory cache for dashboard data

8. **Multi-node aggregation**:
   - Configure ObjectRegistry for node discovery
   - Set up cluster seed nodes for automatic discovery
   - Dashboard will aggregate metrics from all discovered nodes

## Quick Reference

### Common Commands

```bash
# Build the binary (dashboard enabled by default)
cargo build --release --bin plexspaces-node -p plexspaces-node

# Quick test (local, automated)
./scripts/test-dashboard.sh

# Or start node manually
./target/release/plexspaces-node --node-id "test-node" --listen-address "0.0.0.0:8000"

# Deploy test environment (Kubernetes)
./scripts/deploy-dashboard-k8s-test.sh

# Port forward to access dashboard (Kubernetes)
kubectl port-forward -n plexspaces-test svc/plexspaces-node-1 8001:8001

# Deploy test application
./scripts/deploy-wasm-app-test.sh http://localhost:8001 wasm-calculator

# Test dashboard APIs (manual)
curl http://localhost:8001/api/v1/dashboard/summary | jq

# Check logs (local)
tail -f /tmp/plexspaces-node.log

# Check logs (Kubernetes)
kubectl logs -n plexspaces-test deployment/plexspaces-node-1 --tail=50 -f

# Cleanup (Kubernetes)
kubectl delete -f tests/k8s/
```

### API Endpoints Quick Reference

```bash
# Summary (aggregated metrics)
curl http://localhost:8001/api/v1/dashboard/summary

# Nodes list
curl http://localhost:8001/api/v1/dashboard/nodes

# Node details
curl http://localhost:8001/api/v1/dashboard/node/node-1

# Applications (with filters)
curl "http://localhost:8001/api/v1/dashboard/applications?name_pattern=calc"

# Actors (with filters)
curl "http://localhost:8001/api/v1/dashboard/actors?actor_type=calculator&status=running"

# Workflows
curl http://localhost:8001/api/v1/dashboard/workflows

# Dependency Health (with circuit breaker info)
curl http://localhost:8001/api/v1/dashboard/dependencies
```

## References

- [PlexSpaces Architecture](../../docs/architecture.md)
- [CLAUDE.md Development Guide](../../CLAUDE.md)
- [Dashboard Proto Definitions](../../proto/plexspaces/v1/dashboard/dashboard.proto)
- [Dashboard Testing Guide](../../DASHBOARD_TESTING.md) - Quick start guide for local testing
- [Kubernetes Test Configs](../../tests/k8s/README.md)
- [WASM Examples](../../examples/wasm_showcase/README.md)
- [Dependency Monitoring Implementation](../../DEPENDENCY_MONITORING_IMPLEMENTATION.md) - Dependency health monitoring details




