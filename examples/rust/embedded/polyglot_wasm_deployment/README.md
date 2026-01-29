# Polyglot WASM Deployment Example

**Purpose**: Demonstrates deploying polyglot actors (Rust, TypeScript, Go, Python) to Firecracker VMs, starting with an empty PlexSpaces node and then deploying applications dynamically.

**Architecture**: Start empty node → Deploy polyglot applications to Firecracker VMs → Track coordination vs compute metrics.

---

## Overview

This example shows how to:
1. Start an empty PlexSpaces node (framework only, ready for deployments)
2. Deploy polyglot applications (Rust, TypeScript, Go, Python) to Firecracker VMs
3. Track coordination vs compute metrics for each deployment
4. Demonstrate real Firecracker VM lifecycle management

**Use Case**: Similar to AWS Lambda, wasmCloud, Fermyon Cloud - deploy functions/actors dynamically to isolated Firecracker VMs without rebuilding containers.

---

## Architecture

```
┌─────────────────────────────────────────┐
│  Empty PlexSpaces Node                  │
│  (Framework only, ready for deployment) │
│                                         │
│  ┌──────────────────────────────────┐  │
│  │  ApplicationService (gRPC)       │  │
│  │  - Deploy applications           │  │
│  │  - Manage Firecracker VMs        │  │
│  └──────────────────────────────────┘  │
└─────────────────────────────────────────┘
         ▲
         │ Deploy polyglot applications
         │
    ┌────┴────┐
    │  CLI    │
    │  Tool   │
    └─────────┘
         │
         ▼
┌─────────────────────────────────────────┐
│  Firecracker VM (per application)       │
│  ┌──────────────────────────────────┐  │
│  │  WASM Runtime                    │  │
│  │  - Rust actor                    │  │
│  │  - TypeScript actor              │  │
│  │  - Go actor                      │  │
│  │  - Python actor                  │  │
│  └──────────────────────────────────┘  │
└─────────────────────────────────────────┘
```

---

## Prerequisites

### Local Testing (Optional)
- Rust toolchain
- Firecracker binary (optional - example works in simulation mode)

### Docker Testing (Recommended)
- Docker and Docker Compose
- KVM support (for real Firecracker VMs)
  - On Linux: Native KVM
  - On macOS: Docker Desktop with Linux containers (emulated, may have limitations)

---

## Quick Start

### Using Docker (Recommended)

The easiest way to test this example is using Docker, which includes all prerequisites:

```bash
cd examples/simple/polyglot_wasm_deployment
./scripts/validate.sh --docker
```

This will:
1. Build the Docker image with Firecracker, kernel, and rootfs
2. Verify Firecracker works in the container
3. Run the example workload
4. Test real Firecracker VM deployment

### Local Testing

```bash
# Build the example
cargo build --release

# Start an empty node (ready for deployments)
cargo run --release -- start-node --node-id my-node

# In another terminal, deploy a polyglot application
cargo run --release -- deploy \
  --name my-rust-app \
  --language rust \
  --wasm /path/to/app.wasm \
  --vcpus 1 \
  --memory 256

# List deployed applications
cargo run --release -- list

# Run example workload (simulates multiple languages)
cargo run --release -- run --operations 10
```

---

## Commands

### `start-node`
Start an empty PlexSpaces node (framework only, ready for deployments).

```bash
cargo run --release -- start-node --node-id my-node
```

### `deploy`
Deploy a polyglot application to a Firecracker VM.

```bash
cargo run --release -- deploy \
  --name my-app \
  --language rust \
  --wasm /path/to/app.wasm \
  --vcpus 1 \
  --memory 256
```

**Supported languages**: `rust`, `typescript`, `go`, `python`

### `list`
List all deployed applications and their VMs.

```bash
cargo run --release -- list
```

### `run`
Run example workload simulating multiple polyglot applications.

```bash
cargo run --release -- run --operations 10
```

### `stop`
Stop a deployed application.

```bash
cargo run --release -- stop --name my-app
```

### `empty-vm-typescript`
Complete workflow: Start empty VM → Deploy TypeScript app → Run workload.

```bash
cargo run --release -- empty-vm-typescript \
  --vm-id my-vm \
  --wasm /path/to/typescript.wasm \
  --vcpus 1 \
  --memory 256 \
  --operations 10
```

---

## Configuration

The example uses `ConfigBootstrap` for Erlang-style configuration management.

### `release.toml`

```toml
[application]
name = "polyglot-wasm-deployment"
version = "0.1.0"

[firecracker]
binary_path = "/usr/bin/firecracker"
kernel_path = "/var/lib/firecracker/vmlinux"
rootfs_path = "/var/lib/firecracker/rootfs.ext4"

[firecracker.default_vm]
vcpus = 1
memory_mib = 256
```

### Environment Variable Overrides

All configuration can be overridden via environment variables:

```bash
export PLEXSPACES_FIRECRACKER_BIN=/usr/local/bin/firecracker
export PLEXSPACES_FIRECRACKER_KERNEL=/custom/path/vmlinux
export PLEXSPACES_FIRECRACKER_ROOTFS=/custom/path/rootfs.ext4
export FIRECRACKER_NO_SECCOMP=1  # Required in Docker/emulated environments
```

---

## Metrics

The example uses `CoordinationComputeTracker` to track:
- **Coordination time**: VM deployment, WASM module loading, message passing
- **Compute time**: Actual actor processing
- **Granularity ratio**: compute_time / coordinate_time (should be >= 10×)
- **Efficiency**: compute_time / total_time

Example output:
```
=== Polyglot WASM Deployment Performance Report ===
Actor: actor-123 (rust)

Timing:
  Compute Time:        300 ms
  Coordinate Time:       30 ms
  Total Time:          330 ms

Operations:
  Messages:               10

Performance:
  Granularity Ratio:   10.00× (compute/coordinate)
    ✓  Acceptable (>10×), but could be better
  Efficiency:           90.9% (compute/total)
```

---

## Testing and Validation

### Local Validation

```bash
./scripts/validate.sh
```

This runs:
- Prerequisites check
- Build verification
- Example workload test
- Actor listing test
- Empty VM + TypeScript workflow test

### Docker Validation (Recommended)

```bash
./scripts/validate.sh --docker
```

This runs the same tests in Docker with real Firecracker support.

### Docker Testing Details

The Docker setup includes:
- Firecracker binary (v1.4.0)
- Linux kernel image
- Root filesystem image
- All prerequisites pre-configured

**Note**: On macOS (ARM64), Firecracker runs in an emulated x86_64 container. While the setup is correct, VM creation may fail due to emulation limitations (seccomp/prctl syscalls). This is expected and documented in the test output.

For full Firecracker testing, use a native Linux x86_64 system.

---

## Framework Abstractions Used

This example demonstrates the use of framework abstractions:

- **`ConfigBootstrap`**: Erlang-style configuration loading from `release.toml` with environment variable overrides
- **`CoordinationComputeTracker`**: Standardized metrics tracking for coordination vs compute time
- **`NodeBuilder`**: Fluent API for creating PlexSpaces nodes
- **`ApplicationDeployment`**: Builder-style API for deploying applications to Firecracker VMs
- **`VmConfig`**: Type-safe VM configuration
- **`VmRegistry`**: VM discovery and management

---

## Example Workflow

### 1. Start Empty Node

```bash
cargo run --release -- start-node --node-id polyglot-node-1
```

Output:
```
Starting empty PlexSpaces node: polyglot-node-1
✓ Empty node started successfully
  Node ID: polyglot-node-1
  Node is ready to accept deployments via ApplicationService
```

### 2. Deploy Polyglot Applications

```bash
# Deploy Rust application
cargo run --release -- deploy \
  --name rust-counter \
  --language rust \
  --wasm target/wasm32-wasi/release/counter.wasm \
  --vcpus 1 \
  --memory 256

# Deploy TypeScript application
cargo run --release -- deploy \
  --name ts-greeter \
  --language typescript \
  --wasm target/greeter.wasm \
  --vcpus 1 \
  --memory 256
```

### 3. List Deployed Applications

```bash
cargo run --release -- list
```

Output:
```
Listing deployed actors...
Found 2 VM(s) with actors:
  - vm-abc123: Running (/tmp/firecracker-vm-abc123.sock)
  - vm-def456: Running (/tmp/firecracker-vm-def456.sock)
Discovery time: 2 ms
```

### 4. Run Example Workload

```bash
cargo run --release -- run --operations 10
```

This simulates deploying and running actors in multiple languages, showing coordination vs compute metrics for each.

---

## Docker Testing

### Quick Test

```bash
./scripts/test-docker.sh
```

### Manual Docker Commands

```bash
# Build image
docker compose build

# Run workload
docker compose run --rm polyglot-test polyglot_wasm_deployment run --operations 5

# List applications
docker compose run --rm polyglot-test polyglot_wasm_deployment list

# Deploy application
docker compose run --rm polyglot-test polyglot_wasm_deployment deploy \
  --name my-app \
  --language rust \
  --wasm /tmp/test.wasm \
  --vcpus 1 \
  --memory 256
```

---

## Implementation Details

### Real Firecracker Deployment

The example uses `ApplicationDeployment::deploy().await?` to:
1. Create a Firecracker VM with the specified configuration
2. Boot the VM (kernel + rootfs)
3. Deploy the WASM application to the VM
4. Return a `FirecrackerVm` handle for management

This is the same pattern used in `firecracker_multi_tenant` example.

### Metrics Tracking

All operations use `CoordinationComputeTracker`:
- VM deployment: Coordination time
- WASM loading: Coordination time
- Actor processing: Compute time
- Message passing: Coordination time

The tracker automatically calculates:
- Granularity ratio (compute/coordinate)
- Efficiency (compute/total)
- Performance warnings if ratio < 10×

---

## Troubleshooting

### Firecracker Not Found

**Local**: Install Firecracker or use Docker testing.

**Docker**: The Docker image includes Firecracker automatically. If you see errors, check:
- Docker is running
- Image built successfully: `docker images | grep polyglot`

### KVM Not Available

**Linux**: 
```bash
lsmod | grep kvm
sudo chmod 666 /dev/kvm
```

**macOS**: Docker Desktop provides KVM in Linux containers. If VM creation fails, this is expected due to emulation limitations.

### Seccomp Errors

If you see `prctl syscall failed` errors, ensure:
- `FIRECRACKER_NO_SECCOMP=1` is set in environment
- Docker container has `seccomp=unconfined` in security_opt

The Docker setup includes this automatically.

---

## Related Examples

- **`firecracker_multi_tenant`**: Similar pattern for multi-tenant isolation
- **`wasm_calculator`**: Python WASM deployment with tuplespace coordination
- **`wasm_showcase`**: Comprehensive WASM capabilities demonstration

---

## License

LGPL-2.1-or-later
