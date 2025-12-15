# Firecracker Integration - MicroVM Support for PlexSpaces

**Purpose**: Provides Firecracker microVM integration for PlexSpaces actors, enabling strong isolation with < 200ms boot time (target: 125ms).

## Overview

This crate implements **Walking Skeleton Phase 7** (Week 13-14): Firecracker as the microVM isolation layer for PlexSpaces actors.

### Why Firecracker?

- **Fast boot**: < 125ms from cold start
- **Lightweight**: ~10MB memory overhead per VM
- **Secure**: Strong isolation via KVM virtualization
- **Minimal**: Purpose-built for serverless/container workloads
- **Battle-tested**: Powers AWS Lambda and Fargate

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│ PlexSpaces Actor System                                 │
│                                                         │
│  ┌───────────────────────────────────────────────────┐ │
│  │ FirecrackerWasmRuntime                            │ │
│  │  - VM pool management                             │ │
│  │  - WASM module deployment                         │ │
│  │  - Actor placement                                │ │
│  └───────────────┬─────────────────────────────────── │
│                  │                                     │
│  ┌───────────────▼───────────┬──────────────────────┐ │
│  │ FirecrackerVm             │  FirecrackerVm       │ │
│  │  - vm_id: "vm-001"        │   - vm_id: "vm-002"  │ │
│  │  - state: Running         │   - state: Running   │ │
│  │  - process: Child         │   - process: Child   │ │
│  └────────┬──────────────────┴──────┬───────────────┘ │
│           │                          │                 │
└───────────┼──────────────────────────┼─────────────────┘
            │                          │
    ┌───────▼────────┐        ┌───────▼────────┐
    │ Firecracker    │        │ Firecracker    │
    │ MicroVM        │        │ MicroVM        │
    │                │        │                │
    │ ┌────────────┐ │        │ ┌────────────┐ │
    │ │ WASM       │ │        │ │ WASM       │ │
    │ │ Runtime    │ │        │ │ Runtime    │ │
    │ │            │ │        │ │            │ │
    │ │ ┌─Actor─┐  │ │        │ │ ┌─Actor─┐  │ │
    │ │ │ State │  │ │        │ │ │ State │  │ │
    │ │ └───────┘  │ │        │ │ └───────┘  │ │
    │ └────────────┘ │        │ └────────────┘ │
    │                │        │                │
    │ Linux kernel   │        │ Linux kernel   │
    └────────────────┘        └────────────────┘
          ▲                          ▲
          │ TAP device               │ TAP device
          │ (tap-vm001)              │ (tap-vm002)
          │                          │
    ┌─────┴──────────────────────────┴─────┐
    │ Host Network Bridge                  │
    │ (br-firecracker)                     │
    │ IP: 10.0.0.1/24                      │
    └──────────────────────────────────────┘
```

## Key Components

### Configuration (`src/config.rs`)

Comprehensive VM configuration structures:
- **VmConfig**: Top-level VM specification (vm_id, vcpu_count, mem_size_mib, kernel, rootfs, network)
- **DriveConfig**: Disk drive specification
- **NetworkInterfaceConfig**: Network interface specification
- **MachineConfig**, **BootSource**, **Drive**, **NetworkInterface**: Firecracker API request types

### API Client (`src/api_client.rs`)

HTTP client for Firecracker REST API over Unix socket:
- `create_machine_config()`: Set vCPU and memory
- `set_boot_source()`: Configure kernel and boot args
- `add_drive()`: Attach rootfs and data drives
- `add_network_interface()`: Attach TAP devices
- `start_instance()`: Boot VM
- `pause_instance()`: Pause VM execution
- `resume_instance()`: Resume VM
- `shutdown_instance()`: Stop VM
- `get_instance_info()`: Query VM state

### VM Lifecycle (`src/vm.rs`)

High-level VM operations wrapping API client:
- `FirecrackerVm::create()`: Create VM with configuration
- `boot()`: Start VM
- `pause()`: Pause VM execution
- `resume()`: Resume VM
- `stop()`: Stop VM gracefully
- `get_state()`: Query current VM state

### Network Setup (`src/network.rs`)

TAP device creation and networking:
- `create_tap_device()`: Create TAP device for VM networking
- `delete_tap_device()`: Clean up TAP device
- `generate_tap_name()`: Generate unique TAP device name

### Health Monitoring (`src/health.rs`)

VM health checks and monitoring:
- **Hybrid Approach**: Event-driven + periodic polling
- **Health Criteria**: Process running, API responsive, state valid
- **Check Frequency**: 5 seconds (configurable)
- **Failure Detection**: Process crash, API timeout, state = Failed

### VM Supervision (`src/supervisor.rs`)

Automatic VM restart on failure:
- **Restart Policies**: Always, WithBackoff, MaxRetries, Never
- **Exponential Backoff**: Initial 1s, max 60s, factor 2.0, ±20% jitter
- **Failure Detection**: Process crash, health check failure (3 consecutive), API unresponsive, VM state = Failed

### VM Registry (`src/vm_registry.rs`)

Centralized VM discovery and tracking:
- **VM Discovery**: List all running VMs
- **VM Status**: Get status of specific VM
- **VM Grouping**: Group VMs by application/tenant (future)

### Application Deployment (`src/application_deployment.rs`)

Deploy applications to Firecracker VMs:
- `ApplicationDeployment::new()`: Create deployment configuration
- `with_vm_config()`: Configure VM settings
- `deploy()`: Create and boot VM with application

## Design Decisions

### VM Health Checks

**Decision**: Hybrid Approach (Event-driven + Periodic Polling)
- Event-driven catches immediate failures (process crash)
- Periodic polling catches "zombie" VMs (process running but API unresponsive)
- Provides redundancy for critical infrastructure

**Health Criteria**: All three must pass
1. Process Running: Firecracker process must be alive
2. API Responsive: `get_instance_info()` must succeed
3. State Valid: VM state should be Running (not Failed/Paused unexpectedly)

**Check Frequency**: 5 seconds (configurable)

### VM Supervision

**Decision**: Separate `VmSupervisor` (explicit supervision)
- Explicit control over supervision policy
- Can supervise multiple VMs
- Can be disabled per-VM
- Follows supervisor pattern from Erlang/OTP

**Restart Policy**: Configurable
```rust
pub enum RestartPolicy {
    Always,                    // Always restart (infinite)
    WithBackoff {              // Exponential backoff
        initial_delay: Duration,
        max_delay: Duration,
        factor: f64,
    },
    MaxRetries {               // Max retries then stop
        max_retries: u32,
        backoff: BackoffConfig,
    },
    Never,                     // Don't restart
}
```

### Multi-VM Coordination

**Decision**: Centralized VmRegistry (already exists)
- VmRegistry already implemented
- Simple and sufficient for current needs
- Can extend to distributed later if needed

## Implementation Status

### ✅ Completed

- ✅ Error types (`src/error.rs`)
- ✅ Configuration types (`src/config.rs`)
- ✅ Firecracker API client (`src/api_client.rs`)
- ✅ VM lifecycle management (`src/vm.rs`)
- ✅ Network setup (`src/network.rs`)
- ✅ Health monitoring (`src/health.rs`)
- ✅ VM supervision (`src/supervisor.rs`)
- ✅ VM registry (`src/vm_registry.rs`)
- ✅ Application deployment (`src/application_deployment.rs`)
- ✅ Integration tests with real Firecracker
- ✅ Docker support with seccomp fixes

### ⚠️ Known Limitations

- **macOS ARM64**: Firecracker requires native x86_64 Linux with KVM. On macOS, even with Docker Desktop, KVM emulation may not support Firecracker fully.
- **Seccomp**: In Docker/emulated environments, seccomp restrictions may cause Firecracker to fail. Use `--no-seccomp` flag or `FIRECRACKER_NO_SECCOMP=1` environment variable.

## Performance Targets

| Metric | Target | Notes |
|--------|--------|-------|
| Boot time | < 200ms (target: 125ms) | From cold start to running |
| Memory overhead | < 10MB per VM | Minimal footprint |
| Network latency | < 1ms for VM-to-VM | TAP device overhead |
| Actor instantiation | < 50ms in running VM | WASM module loading |

## Testing

### Unit Tests

```bash
cargo test -p plexspaces-firecracker
```

### Integration Tests (Real Firecracker)

```bash
# Requires Firecracker binary, kernel, and rootfs
cargo test -p plexspaces-firecracker --test integration -- --ignored
```

### Docker Tests

```bash
# Test with Docker (includes Firecracker setup)
cd examples/simple/firecracker_multi_tenant
./scripts/test-docker.sh
```

## Configuration

### Environment Variables

- `PLEXSPACES_FIRECRACKER_BIN`: Path to Firecracker binary (default: `/usr/bin/firecracker`)
- `PLEXSPACES_FIRECRACKER_KERNEL`: Path to kernel image (default: `/var/lib/firecracker/vmlinux`)
- `PLEXSPACES_FIRECRACKER_ROOTFS`: Path to rootfs image (default: `/var/lib/firecracker/rootfs.ext4`)
- `FIRECRACKER_NO_SECCOMP`: Set to `1` to disable seccomp (for Docker/emulated environments)

### Configuration File (`release.toml`)

```toml
[firecracker]
binary_path = "/usr/bin/firecracker"
kernel_path = "/var/lib/firecracker/vmlinux"
rootfs_path = "/var/lib/firecracker/rootfs.ext4"

[firecracker.vm_defaults]
vcpu_count = 1
mem_size_mib = 256
```

## Usage Examples

### Create and Boot VM

```rust
use plexspaces_firecracker::{FirecrackerVm, VmConfig};

let vm_config = VmConfig {
    vm_id: "vm-001".to_string(),
    vcpu_count: 1,
    mem_size_mib: 256,
    kernel_image_path: "/var/lib/firecracker/vmlinux".to_string(),
    rootfs: DriveConfig {
        drive_id: "rootfs".to_string(),
        path_on_host: "/var/lib/firecracker/rootfs.ext4".to_string(),
        is_root_device: true,
        is_read_only: false,
    },
    ..Default::default()
};

let mut vm = FirecrackerVm::create(vm_config).await?;
vm.boot().await?;
```

### Deploy Application to VM

```rust
use plexspaces_firecracker::{ApplicationDeployment, VmConfig};

let vm_config = VmConfig { /* ... */ };
let deployment = ApplicationDeployment::new("my-app")
    .with_vm_config(vm_config);

let mut vm = deployment.deploy().await?;
// VM is now running with application deployed
```

### Health Monitoring

```rust
use plexspaces_firecracker::VmHealthMonitor;

let monitor = VmHealthMonitor::new(vm.clone(), Duration::from_secs(5));
monitor.start_monitoring().await?;

// Monitor will check health every 5 seconds
// Triggers restart if health check fails 3 times consecutively
```

### VM Supervision

```rust
use plexspaces_firecracker::{VmSupervisor, RestartPolicy};

let mut supervisor = VmSupervisor::new();
supervisor.supervise(vm, RestartPolicy::WithBackoff {
    initial_delay: Duration::from_secs(1),
    max_delay: Duration::from_secs(60),
    factor: 2.0,
}).await?;
```

## Prerequisites

- **Linux**: Kernel 4.14+ with KVM support
- **Firecracker**: Binary installed (download from [releases](https://github.com/firecracker-microvm/firecracker/releases))
- **Kernel Image**: Linux kernel for VMs (download from [Firecracker quickstart](https://s3.amazonaws.com/spec.ccfc.min/img/quickstart_guide/x86_64/kernels/vmlinux.bin))
- **Rootfs Image**: Root filesystem for VMs (download from [Firecracker quickstart](https://s3.amazonaws.com/spec.ccfc.min/img/quickstart_guide/x86_64/rootfs/bionic.rootfs.ext4))
- **Permissions**: Root or `CAP_NET_ADMIN` for TAP devices

## References

- [Firecracker API Documentation](https://github.com/firecracker-microvm/firecracker/blob/main/src/api_server/swagger/firecracker.yaml)
- [CLAUDE.md Phase 7](../../CLAUDE.md#phase-7-firecracker-microvm-isolation-week-13-14)
- [Walking Skeleton Approach](../../CLAUDE.md#walking-skeleton-approach---revised-remoting-first)
- [WASM Runtime Integration](../wasm-runtime/README.md)

