# N-Body WASM Example - TypeScript Actors

## Quick Start

```bash
# Option 1: Use Rust WASM (works immediately, no javy needed)
cd examples/nbody-wasm
./scripts/test.sh

# Option 2: Use TypeScript WASM (requires javy)
# First install javy:
./scripts/install_javy.sh
# OR: cargo install --git https://github.com/bytecodealliance/javy javy-cli

# Then run test (will use TypeScript if javy is available)
./scripts/test.sh
```

**Note**: The test script automatically uses Rust WASM fallback if javy is not available, so you can test immediately without installing javy.

## Overview

This example demonstrates **application-level WASM deployment** with TypeScript actors for the N-Body simulation. Unlike the native `nbody` example, this version:

- **TypeScript actors**: Body actors implemented in TypeScript
- **Application-level deployment**: Entire application deployed as a single WASM module
- **CLI deployment**: Use CLI to deploy WASM application modules
- **Framework packaging**: WASM modules packaged with the framework

## Architecture

### Application-Level Deployment

```
┌──────────────────────────────────────────────────────────┐
│  PlexSpaces Node (Framework)                              │
│                                                            │
│  ┌──────────────────────────────────────────────────┐    │
│  │  WASM Application Module                          │    │
│  │  (nbody-application.wasm)                        │    │
│  │                                                    │    │
│  │  ┌─────────────┐  ┌─────────────┐              │    │
│  │  │ Body Actor  │  │ Body Actor  │              │    │
│  │  │ (TypeScript)│  │ (TypeScript)│              │    │
│  │  └─────────────┘  └─────────────┘              │    │
│  │                                                    │    │
│  │  ┌──────────────────────────────────────────┐    │    │
│  │  │  Application Coordinator                 │    │    │
│  │  │  (TypeScript)                            │    │    │
│  │  └──────────────────────────────────────────┘    │    │
│  └──────────────────────────────────────────────────┘    │
│                                                            │
│  Framework Services:                                       │
│  - ActorService                                           │
│  - TupleSpace                                             │
│  - Metrics                                                │
└──────────────────────────────────────────────────────────┘
```

### Key Differences from Native Example

1. **Application-Level**: Entire application (all actors + coordinator) in one WASM module
2. **TypeScript**: Actors written in TypeScript, compiled to WASM
3. **Deployment**: Deploy via CLI, not compile-time linking
4. **Packaging**: WASM module can be packaged with framework or deployed dynamically

## Building TypeScript to WASM

### Prerequisites

**Important**: Install these tools **before** running the example:

```bash
# 1. Install Javy (TypeScript/JavaScript to WASM compiler)
# Download from: https://github.com/bytecodealliance/javy/releases
#
# For macOS (ARM):
#   mkdir -p ~/.local/bin
#   curl -L https://github.com/bytecodealliance/javy/releases/download/v8.0.0/javy-arm-macos-v8.0.0.gz | gunzip > ~/.local/bin/javy
#   chmod +x ~/.local/bin/javy
#
# For macOS (Intel):
#   mkdir -p ~/.local/bin
#   curl -L https://github.com/bytecodealliance/javy/releases/download/v8.0.0/javy-x86_64-macos-v8.0.0.gz | gunzip > ~/.local/bin/javy
#   chmod +x ~/.local/bin/javy
#
# Add to PATH (add to ~/.zshrc or ~/.bashrc for persistence):
#   export PATH="$PATH:$HOME/.local/bin"

# 2. Install Node.js and npm (if not already installed)
# Download from: https://nodejs.org/
# Or use package manager:
#   macOS: brew install node
#   Ubuntu: sudo apt install nodejs npm

# 3. TypeScript will be installed locally via npm (no global install needed)
```

### Build Process

```bash
cd examples/nbody-wasm/ts-actors

# Compile TypeScript to JavaScript
npm run build

# Compile JavaScript to WASM
npm run build:wasm

# Output: ../wasm-modules/nbody-body.wasm
```

## Deployment

### Using CLI

```bash
# Deploy WASM application module
plexspaces deploy \
  --application nbody-simulation \
  --version 0.1.0 \
  --wasm wasm-modules/nbody-application.wasm \
  --node localhost:9001

# Start application
plexspaces start-application \
  --application nbody-simulation \
  --node localhost:9001

# Check application status
plexspaces status \
  --application nbody-simulation \
  --node localhost:9001
```

### Using Application Service (gRPC)

The example includes a Rust binary that demonstrates deploying via gRPC:

```bash
cargo run --release --bin nbody-wasm -- \
  --deploy \
  --wasm wasm-modules/nbody-application.wasm \
  --node localhost:9001
```

## Configuration

### Application Configuration

The WASM application can be configured via `release.toml`:

```toml
[[applications]]
name = "nbody-simulation"
version = "0.1.0"
wasm_module = "wasm-modules/nbody-application.wasm"

[nbody]
body_count = 3
steps = 10
dt = 1.0
```

## TypeScript Actor Interface

The TypeScript actor must implement:

```typescript
class BodyActor {
    handleMessage(messageType: string, payload: any): any;
    getState(): Body;
    addForce(force: [number, number, number]): void;
    applyForces(dt: number): void;
}
```

## Key Features Demonstrated

1. **Application-Level WASM**: Entire application in one module
2. **TypeScript Actors**: Business logic in TypeScript
3. **CLI Deployment**: Deploy applications dynamically
4. **Framework Integration**: WASM applications use framework services
5. **Polyglot Support**: TypeScript alongside Rust

## Comparison with Native Example

| Feature | Native (`nbody`) | WASM (`nbody-wasm`) |
|---------|------------------|---------------------|
| Actor Language | Rust | TypeScript |
| Deployment | Compile-time | Runtime (CLI) |
| Module Type | Individual actors | Application-level |
| Packaging | Binary | WASM module |
| Hot Reload | No | Yes (redeploy) |

## Testing

### Simple End-to-End Test

Run the complete test (recommended):

```bash
./scripts/test.sh
```

This script will:
1. Check prerequisites (npm, javy, cargo)
2. Install TypeScript dependencies
3. Build Rust binaries (nbody-wasm, node-starter)
4. Build TypeScript to WASM
5. Start PlexSpaces node
6. Deploy WASM application
7. Verify deployment
8. Clean up

### Quick Test (After Initial Build)

If you've already built everything, use the quick test:

```bash
./scripts/quick_test.sh
```

### Manual Testing

1. **Start node** (in one terminal):
```bash
./scripts/start_node.sh
```

2. **Build WASM** (in another terminal):
```bash
./scripts/build.sh
```

3. **Deploy application**:
```bash
./scripts/deploy.sh
```

## Troubleshooting

### Javy not found
```bash
cargo install javy-cli
```

### TypeScript compilation fails
```bash
cd ts-actors
npm install
npm run build
```

### Node fails to start
- Check if port 9001 is already in use
- Check node logs: `/tmp/nbody-node.log`
- Verify environment variables: `PLEXSPACES_NODE_ID`, `PLEXSPACES_LISTEN_ADDR`

### Deployment fails
- Ensure node is running and accessible at `http://localhost:9001`
- Check WASM file exists: `wasm-modules/nbody-application.wasm`
- Verify node logs for errors

## References

- [Javy - JavaScript/TypeScript to WASM](https://github.com/bytecodealliance/javy)
- [WASM Application Deployment](../simple/polyglot_wasm_deployment/README.md)
- Native N-Body Example: [`../nbody/README.md`](../nbody/README.md)

