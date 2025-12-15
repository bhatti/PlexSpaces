# Firecracker Multi-Tenant Example

## Overview

This example demonstrates **application-level isolation** using Firecracker microVMs for multi-tenant deployments. Each tenant's application runs in its own isolated VM, providing strong security boundaries with performance metrics tracking coordination vs computation.

## PlexSpaces Features Demonstrated

1. **Application-Level Isolation**: Each tenant's application runs in a separate Firecracker VM
2. **VM Lifecycle Management**: Create, boot, stop VMs via simplified CLI
3. **VM Registry**: Discover and track running VMs
4. **Performance Metrics**: Track coordination vs computation time
5. **Multi-Tenant Support**: Multiple tenants can run simultaneously

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    Multi-Tenant System                       │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  ┌──────────────┐    ┌──────────────┐    ┌──────────────┐ │
│  │   Tenant A   │    │   Tenant B   │    │   Tenant C   │ │
│  │  Application │    │  Application │    │  Application │ │
│  │              │    │              │    │              │ │
│  │  ┌────────┐  │    │  ┌────────┐  │    │  ┌────────┐  │ │
│  │  │ VM A   │  │    │  │ VM B   │  │    │  │ VM C   │  │ │
│  │  │        │  │    │  │        │  │    │  │        │  │ │
│  │  │ Actors │  │    │  │ Actors │  │    │  │ Actors │  │ │
│  │  └────────┘  │    │  └────────┘  │    │  └────────┘  │ │
│  └──────────────┘    └──────────────┘    └──────────────┘ │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

## Prerequisites

**Required for real Firecracker VMs:**
- Firecracker binary installed (`/usr/bin/firecracker` or in PATH)
- Linux kernel image (`/var/lib/firecracker/vmlinux`)
- Root filesystem image (`/var/lib/firecracker/rootfs.ext4`)
- Root privileges (for TAP devices)
- Linux kernel 4.14+ with KVM support

**Environment Variable Overrides:**
You can override default paths using environment variables:
```bash
export PLEXSPACES_FIRECRACKER_BIN=/path/to/firecracker
export PLEXSPACES_FIRECRACKER_KERNEL=/path/to/vmlinux
export PLEXSPACES_FIRECRACKER_ROOTFS=/path/to/rootfs.ext4
```

The validation script will check for these prerequisites and fail if they're not available.

## Configuration

The example uses `ConfigBootstrap` to load configuration from `release.toml` with environment variable overrides:

```bash
# Override paths via environment variables
export PLEXSPACES_FIRECRACKER_KERNEL=/path/to/vmlinux
export PLEXSPACES_FIRECRACKER_ROOTFS=/path/to/rootfs.ext4
```

See `release.toml` for all configuration options.

## Building

```bash
cd examples/simple/firecracker_multi_tenant
cargo build --release
```

## Usage

### Deploy Tenant Application

```bash
# Deploy tenant with default resources (1 vCPU, 256 MiB)
cargo run --release -- deploy --tenant tenant-a

# Deploy tenant with custom resources
cargo run --release -- deploy --tenant tenant-b --vcpus 2 --memory 512
```

### List Running Tenants

```bash
cargo run --release -- list
```

Output:
```
Listing running tenants...
Found 2 tenant(s):
  - vm-01HZ...: Running (/tmp/firecracker-vm-01HZ....sock)
  - vm-01HZ...: Running (/tmp/firecracker-vm-01HZ....sock)
```

### Get Tenant Status

```bash
cargo run --release -- status --tenant tenant-a
```

### Stop Tenant

```bash
cargo run --release -- stop --tenant tenant-a
```

### Run Example Workload with Metrics

```bash
# Run workload with 3 tenants, 10 operations each
cargo run --release -- run --tenants 3 --operations 10
```

This will:
1. Simulate deploying multiple tenants
2. Run operations for each tenant
3. Track coordination vs computation metrics
4. Print performance reports

## Performance Metrics

The example tracks two key metrics:

1. **Compute Time**: Time spent on actual computation (actor processing)
2. **Coordinate Time**: Time spent on coordination (VM management, message passing, deployment)

### Granularity Ratio

The **granularity ratio** (compute / coordinate) indicates efficiency:
- **< 10×**: Too much overhead, consider coarser granularity
- **10×-100×**: Acceptable for small problems
- **> 100×**: Excellent efficiency, parallelism beneficial

### Example Output

```
=== Firecracker Multi-Tenant Performance Report ===
Tenant: tenant-0

Timing:
  Compute Time:       500 ms
  Coordinate Time:      50 ms
  Total Time:         550 ms

Operations:
  Messages Sent:         10
  VM Operations:          1

Performance:
  Granularity Ratio:   10.00× (compute/coordinate)
    ✓  Acceptable (>10×), but could be better
  Efficiency:           90.9% (compute/total)
==================================================
```

## How It Works

### 1. Tenant Deployment

When deploying a tenant:
1. **Coordination Phase**: Create VM config, start Firecracker process, boot VM
2. **Computation Phase**: Deploy application, instantiate actors, process messages

### 2. VM Registry

The VM Registry:
- Scans `/tmp` for Firecracker socket files (`firecracker-*.sock`)
- Extracts VM ID from socket filename
- Connects to VM API to get current state
- Provides simple discovery interface

### 3. Metrics Collection

Metrics are collected using `CoordinationComputeTracker` from the PlexSpaces framework:
- `start_compute()` / `end_compute()`: Measures computation time
- `start_coordinate()` / `end_coordinate()`: Measures coordination time
- `increment_message()`: Tracks message passing
- Automatically calculates granularity ratio and efficiency
- Standardized metrics format across all examples

## Testing and Validation

### Testing with Docker (Recommended)

The easiest way to test with real Firecracker is using Docker, which includes all prerequisites:

```bash
# Run the Docker test script
./scripts/test-docker.sh
```

This script will:
1. Build a Docker image with Firecracker, kernel, and rootfs pre-installed
2. Run the example workload in Docker
3. Test VM registry
4. Run integration tests

**Requirements:**
- Docker installed and running
- `/dev/kvm` access (Linux with KVM support)
- **Note:** On macOS, Firecracker requires Linux containers. Docker Desktop must be configured to use Linux containers (not Apple Silicon containers). Firecracker does not work on macOS natively.

**Manual Docker usage:**
```bash
# Build the image
docker-compose build

# Run example commands
docker-compose run --rm firecracker-test firecracker_multi_tenant list
docker-compose run --rm firecracker-test firecracker_multi_tenant run --tenants 2 --operations 5
docker-compose run --rm firecracker-test firecracker_multi_tenant deploy --tenant test-tenant-1

# Run integration tests
docker-compose run --rm firecracker-test bash -c "cd /app && cargo test --test integration_test -- --ignored"
```

**Important Notes:**
- On macOS: Firecracker requires Linux containers. Docker Desktop must use Linux containers (not Apple Silicon containers). Firecracker does not work natively on macOS.
- On Linux: Ensure KVM is enabled (`lsmod | grep kvm`) and your user has access to `/dev/kvm` (may need to add user to `kvm` group: `sudo usermod -aG kvm $USER`)
- The Docker container runs in privileged mode to access KVM. For production, consider using specific capabilities instead.

### Run Integration Tests with Real Firecracker (Host)

The example includes integration tests that use real Firecracker VMs:

```bash
# Run integration tests (requires Firecracker setup)
cargo test --test integration_test -- --ignored

# Run specific test
cargo test --test integration_test test_deploy_tenant_with_real_firecracker -- --ignored
```

These tests verify:
- Real VM creation and booting
- Application deployment to VMs
- Multi-tenant isolation
- VM lifecycle management

### Validate Example

```bash
# Run validation script (requires real Firecracker)
./scripts/validate.sh
```

The validation script will:
1. Check prerequisites (Firecracker, kernel, rootfs) - **fails if not available**
2. Build the example
3. Run example workload with metrics
4. Validate metrics output (granularity ratios, efficiency)
5. Run integration tests with real Firecracker
6. Test VM registry

### Setting Up Firecracker for Testing

To test with real Firecracker, you need:

1. **Install Firecracker binary:**
   ```bash
   wget https://github.com/firecracker-microvm/firecracker/releases/download/v1.4.0/firecracker-v1.4.0-x86_64.tgz
   tar -xzf firecracker-v1.4.0-x86_64.tgz
   sudo mv release-v1.4.0-x86_64/firecracker-v1.4.0-x86_64 /usr/bin/firecracker
   sudo chmod +x /usr/bin/firecracker
   ```

2. **Download kernel image:**
   ```bash
   sudo mkdir -p /var/lib/firecracker
   wget https://s3.amazonaws.com/spec.ccfc.min/img/quickstart_guide/x86_64/kernels/vmlinux.bin
   sudo mv vmlinux.bin /var/lib/firecracker/vmlinux
   sudo chmod 644 /var/lib/firecracker/vmlinux
   ```

3. **Download rootfs image:**
   ```bash
   wget https://s3.amazonaws.com/spec.ccfc.min/img/quickstart_guide/x86_64/rootfs/bionic.rootfs.ext4
   sudo mv bionic.rootfs.ext4 /var/lib/firecracker/rootfs.ext4
   sudo chmod 644 /var/lib/firecracker/rootfs.ext4
   ```

4. **Verify setup:**
   ```bash
   firecracker --version
   ls -lh /var/lib/firecracker/
   ```

**Note:** You can override paths using environment variables:
```bash
export PLEXSPACES_FIRECRACKER_BIN=/path/to/firecracker
export PLEXSPACES_FIRECRACKER_KERNEL=/path/to/vmlinux
export PLEXSPACES_FIRECRACKER_ROOTFS=/path/to/rootfs.ext4
```

## Use Cases

- **SaaS Applications**: Isolate customer workloads
- **Untrusted Code**: Run third-party code safely
- **Compliance**: Meet regulatory requirements for data isolation
- **Security**: Prevent cross-tenant data access

## Comparison to Container Isolation

| Feature | Containers | Firecracker VMs |
|---------|-----------|-----------------|
| Isolation | Process-level | VM-level (stronger) |
| Boot Time | ~1-5 seconds | < 200ms |
| Overhead | Low | Very low |
| Security | Good | Excellent |

## Key PlexSpaces Framework Features

This example showcases:
- **ConfigBootstrap**: Erlang-style configuration loading from `release.toml` with environment variable overrides
- **CoordinationComputeTracker**: Standardized metrics tracking for coordination vs computation
- **Application Deployment**: Deploy entire applications to Firecracker VMs
- **VM Lifecycle Management**: Create, boot, stop VMs via simplified API
- **VM Registry**: Discover and track running VMs
- **Network Isolation**: TAP devices for VM networking
- **Multi-Tenancy**: Isolated execution environments
- **Performance Metrics**: Coordination vs computation tracking using framework abstractions

## Next Steps

- Add resource limits per tenant
- Add network policies
- Add automatic scaling
- Add health checks and supervision
- Integrate with gRPC DeployApplication API

## References

- [PlexSpaces Firecracker Integration](../../../../crates/firecracker)
- [Firecracker Documentation](https://github.com/firecracker-microvm/firecracker)
- [Application Deployment Builder](../../../../crates/firecracker/src/application_deployment.rs)
- [VM Registry](../../../../crates/firecracker/src/vm_registry.rs)
