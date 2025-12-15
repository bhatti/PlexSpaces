# Order Processing - Test Scripts

## Quick Start

```bash
# 1. Validate your setup
./quick-validate.sh

# 2. Start 3-node cluster with Docker
./test-docker-compose.sh up

# 3. Watch logs
./test-docker-compose.sh logs

# 4. Stop cluster
./test-docker-compose.sh down
```

## Scripts Overview

### `quick-validate.sh` ✅
**Purpose**: Fast pre-deployment validation (no compilation)

**What it checks**:
- Test scripts exist and are executable
- Docker Compose configuration is valid
- All 3 nodes (node1, node2, node3) are defined
- Service configurations are correct
- ActorService integration is implemented
- Graceful shutdown support exists

**Usage**:
```bash
./quick-validate.sh
# Output: ✓ All checks passed! (16/16)
```

**Exit Codes**:
- `0` - All checks passed
- `1` - Some checks failed

---

### `test-docker-compose.sh` 🐳
**Purpose**: Wrapper for Docker Compose commands with helpful output

**Commands**:

```bash
# Build and start 3-node cluster
./test-docker-compose.sh up

# Follow logs from all nodes (Ctrl+C to exit)
./test-docker-compose.sh logs

# Show container status and health checks
./test-docker-compose.sh status

# Stop cluster (keep data)
./test-docker-compose.sh down

# Stop cluster and remove volumes (clean slate)
./test-docker-compose.sh clean

# Show usage help
./test-docker-compose.sh help
```

**Features**:
- Colorized output for easy reading
- Health checks for all 3 nodes
- Port information display (9001, 9002, 9003)
- Helpful next-step suggestions

**What it does**:
- **up**: Builds Docker images and starts containers in detached mode
- **logs**: Follows logs from all containers (node1, node2, node3)
- **status**: Shows running containers and health status
- **down**: Stops and removes containers
- **clean**: Stops containers and removes volumes

**Cluster Configuration**:
```
Node 1 (localhost:9001): OrderProcessor + PaymentService
Node 2 (localhost:9002): InventoryService
Node 3 (localhost:9003): ShippingService
```

---

### `test-local-nodes.sh` 💻
**Purpose**: Instructions for testing multi-node deployment locally (3 terminals)

**What it does**:
1. Builds `order-coordinator` binary
2. Provides commands to run 3 nodes in separate terminals
3. Explains expected behavior for each node
4. Shows verification checklist

**Usage**:
```bash
# Read instructions
./test-local-nodes.sh

# Then manually open 3 terminals and run the commands shown
```

**Local Node Setup**:

**Terminal 1 - Node 1 (Coordinator)**:
```bash
NODE_ID=node1 NODE_ADDRESS=localhost:9001 \
SPAWN_SERVICES=orders,payment \
cargo run --bin order-coordinator
```
- Runs OrderProcessor + PaymentService
- Executes full demo workflow
- Discovers and communicates with nodes 2 & 3

**Terminal 2 - Node 2 (Inventory)**:
```bash
NODE_ID=node2 NODE_ADDRESS=localhost:9002 \
SPAWN_SERVICES=inventory \
cargo run --bin order-coordinator
```
- Runs InventoryService only
- Waits for requests from Node 1
- Press Ctrl+C to exit

**Terminal 3 - Node 3 (Shipping)**:
```bash
NODE_ID=node3 NODE_ADDRESS=localhost:9003 \
SPAWN_SERVICES=shipping \
cargo run --bin order-coordinator
```
- Runs ShippingService only
- Waits for requests from Node 1
- Press Ctrl+C to exit

**Expected Output**:
- ✓ All 3 nodes start and register with ActorRegistry
- ✓ Node 1 processes full order workflow
- ✓ Node 2 receives inventory reservation requests
- ✓ Node 3 receives shipping creation requests
- ✓ Order completes successfully across all nodes

---

### `validate-multinode.sh` (deprecated) ⚠️
**Note**: This is a more comprehensive validation script that includes compilation tests. Use `quick-validate.sh` instead for faster validation.

---

## Testing Workflow

### Recommended Workflow for Development

```bash
# 1. Make changes to code
vim src/main.rs

# 2. Quick validation (no build)
./scripts/quick-validate.sh

# 3. Build to verify compilation
cargo build --bin order-coordinator

# 4. Test locally (fastest)
# Open 3 terminals and follow instructions from:
./scripts/test-local-nodes.sh

# 5. Test with Docker (production-like)
./scripts/test-docker-compose.sh up
./scripts/test-docker-compose.sh logs
./scripts/test-docker-compose.sh down
```

### Recommended Workflow for Testing

```bash
# Docker is recommended for consistent environment
./scripts/quick-validate.sh
./scripts/test-docker-compose.sh up
./scripts/test-docker-compose.sh logs

# Expected in logs:
# - Node 1: "Order processing complete!"
# - Node 2: "Waiting for requests..."
# - Node 3: "Waiting for requests..."

./scripts/test-docker-compose.sh down
```

---

## Troubleshooting

### "docker-compose command not found"
**Solution**: Install Docker Compose or use `docker compose` (v2)
```bash
# Check if you have Docker Compose v2
docker compose version

# If yes, update scripts to use "docker compose" instead of "docker-compose"
```

### "Port already in use"
**Solution**: Stop existing containers
```bash
./test-docker-compose.sh down
# or
docker ps  # Find containers using ports 9001-9003
docker stop <container_id>
```

### "Build failed" in Docker
**Solution**: Check Docker logs
```bash
./test-docker-compose.sh down
./test-docker-compose.sh up  # Rebuild with full output
```

### Local nodes can't communicate
**Solution**: Check firewall and ports
```bash
# Ensure ports 9001-9003 are open
netstat -an | grep LISTEN | grep '900[123]'

# Check if ActorRegistry is working
# Look for "Registered actor:" messages in logs
```

---

## Environment Variables

All scripts respect these environment variables:

- `COMPOSE_FILE` - Path to docker-compose.yml (default: `./docker-compose.yml`)
- `PROJECT_NAME` - Docker Compose project name (default: `plexspaces-order-processing`)

**Node-specific variables** (for manual testing):
- `NODE_ID` - Node identifier (e.g., "node1", "node2", "node3")
- `NODE_ADDRESS` - gRPC address (e.g., "localhost:9001")
- `SPAWN_SERVICES` - Comma-separated services (e.g., "orders,payment")

---

## See Also

- `../MULTINODE_IMPLEMENTATION_SUMMARY.md` - Complete implementation documentation
- `../PHASE3_ASK_IMPLEMENTATION.md` - Ask pattern implementation details
- `../docker-compose.yml` - Cluster configuration
- `../Dockerfile` - Container build configuration
