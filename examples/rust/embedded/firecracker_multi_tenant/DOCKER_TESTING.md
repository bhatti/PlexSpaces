# Docker Testing Guide for Firecracker Multi-Tenant Example

## Overview

This guide explains how to test the Firecracker Multi-Tenant example using Docker, which provides a consistent environment with all prerequisites pre-installed.

## Quick Start

```bash
# Run the automated Docker test script
./scripts/test-docker.sh

# Or use validate.sh with Docker flag
./scripts/validate.sh --docker
```

## Prerequisites

### Linux
- Docker installed and running
- KVM enabled (`lsmod | grep kvm`)
- User access to `/dev/kvm` (may need: `sudo usermod -aG kvm $USER`)

### macOS
- Docker Desktop installed
- Docker Desktop configured to use **Linux containers** (not Apple Silicon containers)
- Note: Firecracker does not work natively on macOS, only in Linux containers

## Docker Setup

The Docker setup includes:
- **Firecracker binary** (v1.4.0) installed at `/usr/bin/firecracker`
- **Kernel image** downloaded to `/var/lib/firecracker/vmlinux`
- **Rootfs image** downloaded to `/var/lib/firecracker/rootfs.ext4`
- **Built example binary** ready to run

## Usage

### Build the Docker Image

```bash
cd examples/simple/firecracker_multi_tenant
docker-compose build
```

### Run Example Commands

```bash
# List running tenants
docker-compose run --rm firecracker-test firecracker_multi_tenant list

# Run workload with metrics
docker-compose run --rm firecracker-test firecracker_multi_tenant run --tenants 3 --operations 5

# Deploy a tenant
docker-compose run --rm firecracker-test firecracker_multi_tenant deploy --tenant test-tenant-1 --vcpus 1 --memory 256

# Get tenant status
docker-compose run --rm firecracker-test firecracker_multi_tenant status --tenant test-tenant-1

# Stop a tenant
docker-compose run --rm firecracker-test firecracker_multi_tenant stop --tenant test-tenant-1
```

### Run Integration Tests

To run integration tests, you need the source code available. You can either:

1. **Mount source code as volume:**
```bash
docker-compose run --rm \
  -v $(pwd)/../../..:/workspace \
  firecracker-test \
  bash -c "cd /workspace/examples/simple/firecracker_multi_tenant && cargo test --test integration_test -- --ignored"
```

2. **Or rebuild with source code included** (modify Dockerfile to copy source)

## How It Works

1. **Dockerfile** builds the example and includes Firecracker prerequisites
2. **docker-compose.yml** configures the container with:
   - Privileged mode (for KVM access)
   - `/dev/kvm` device mounted
   - `/tmp` mounted for Firecracker sockets
   - Environment variables for Firecracker paths

3. **test-docker.sh** automates the testing process

## Troubleshooting

### KVM Not Available
```
Error: /dev/kvm not found
```
**Solution:** 
- On Linux: Ensure KVM is enabled and user has access
- On macOS: Use Docker Desktop with Linux containers

### Permission Denied
```
Error: Permission denied accessing /dev/kvm
```
**Solution:**
```bash
sudo chmod 666 /dev/kvm
# Or add user to kvm group:
sudo usermod -aG kvm $USER
# Then log out and back in
```

### Firecracker Not Found in Container
The Dockerfile installs Firecracker automatically. If it's missing, rebuild:
```bash
docker-compose build --no-cache
```

## CI/CD Integration

The Docker setup can be used in CI/CD pipelines:

```yaml
# Example GitHub Actions workflow
- name: Test with Docker
  run: |
    cd examples/simple/firecracker_multi_tenant
    docker-compose build
    docker-compose run --rm firecracker-test firecracker_multi_tenant run --tenants 2 --operations 3
```

## Benefits

✅ **Consistent Environment**: Same setup everywhere  
✅ **No Host Installation**: Firecracker, kernel, rootfs all included  
✅ **CI/CD Ready**: Works in automated pipelines  
✅ **Isolated Testing**: Doesn't affect host system  
✅ **Easy Cleanup**: Just remove the container

