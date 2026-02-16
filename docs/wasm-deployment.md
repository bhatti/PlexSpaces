# WASM Deployment Guide

## Overview

PlexSpaces supports deploying WebAssembly (WASM) applications from multiple languages (Rust, Python, TypeScript/JavaScript, Go) via HTTP multipart upload or gRPC, following industry best practices for large file uploads.

**📖 For comprehensive polyglot development guide covering all languages, WIT abstractions, and examples, see [Polyglot WASM Development Guide](polyglot.md)**

**Native Rust actors** (embedded in the node, not compiled to WASM) use the [Rust SDK](sdk.md#rust-sdk): `plexspaces_impl_handlers!`, `spawn_actor` with facets, and GenServer. For WASM deployment of Rust actors, the same WIT world as Python/TypeScript applies.

**Quick Start**: See [DEPLOY_EMPTY_NODE_GUIDE.md](../DEPLOY_EMPTY_NODE_GUIDE.md) for a complete workflow showing how to start an empty node, deploy a WASM application, and verify deployment via the dashboard.

## Architecture

### WASM Dependencies Verification

**✅ WASM actors only use WIT (WebAssembly Interface Types) APIs** - they do NOT include framework dependencies:

- ✅ **WIT Interfaces**: Actors import host functions via WIT (e.g., `host::send_message`, `host::tuplespace_write`, simple-actor `host.ts_write` for TupleSpace)
- ✅ **Standard Library**: Actors can use their language's standard library (e.g., Python's `json`, Rust's `std`)
- ❌ **No Framework Code**: WASM modules do NOT include PlexSpaces framework code - the framework is provided by the runtime

**Example (Python Actor):**
```python
# ✅ Correct - only uses standard library and WIT
import json  # Standard library

def handle_request(from_actor: str, message_type: str, payload: bytes) -> bytes:
    # Uses WIT host functions (provided by runtime)
    # host.send_message(...)  # WIT import
    # host.tuplespace_write(...)  # WIT import
    
    # Uses standard library
    request = json.loads(payload.decode('utf-8'))
    return json.dumps({'result': 42}).encode('utf-8')
```

**Example (Rust Actor):**
```rust
// ✅ Correct - only uses standard library and WIT
use std::collections::HashMap;

#[export_name = "handle_request"]
pub extern "C" fn handle_request(
    from: *const u8, from_len: usize,
    msg_type: *const u8, msg_type_len: usize,
    payload: *const u8, payload_len: usize,
) -> *const u8 {
    // Uses WIT host functions (provided by runtime)
    // host::send_message(...)  // WIT import
    
    // Uses standard library
    let mut map = HashMap::new();
    map.insert("result", 42);
    // ... serialize and return
}
```

### File Size Considerations

**Python-compiled WASM files are large (30-40MB)** because:
- `componentize-py` bundles the entire Python runtime
- This is expected and normal for Python-to-WASM compilation
- The runtime is shared across all Python actors on a node

**Size Comparison by Language:**

| Language | WASM Size | Runtime Size | Use Case |
|----------|-----------|--------------|----------|
| Rust | 100KB-1MB | Minimal | Production, performance-critical |
| Go | 2-5MB | Small | Good balance |
| JavaScript/TypeScript | 500KB-2MB | Medium | Web integration |
| Python | 30-40MB | Large | Rapid prototyping, ML |

**Size Reduction Options:**
1. **Use `wasm-opt`** (recommended):
   ```bash
   wasm-opt -Oz --strip-debug calculator_actor.wasm -o calculator_actor_opt.wasm
   # Typically reduces size by 20-40%
   ```

2. **Use Rust/Go/JavaScript** instead of Python for smaller WASM files

3. **Optimize Python code**:
   - Remove unused imports
   - Use minimal dependencies
   - Consider PyPy for smaller runtime (if supported)

## Deployment Methods

### Method 1: HTTP Multipart Upload (Recommended for Large Files)

**Best Practice**: Use HTTP multipart/form-data for large file uploads (>5MB), similar to document uploads in production applications.

**Endpoint**: `POST http://localhost:8001/api/v1/applications/deploy`

**Content-Type**: `multipart/form-data`

**Body Size Limit**: 100MB (configured via `DefaultBodyLimit` middleware in Axum)

**Fields**:
- `application_id` (required): Unique application identifier (for tracking/debugging)
- `wasm_file` (required): WASM module file (multipart file upload)
- `config` (optional): ApplicationSpec TOML configuration file
- `name` (required): Application name
- `version` (required): Application version (e.g., "1.0.0")

**Resource Limits**:
- Fuel limits are configured via `WasmConfig.limits.max_fuel` (default: 10 billion units)
- For operations requiring heavy JSON serialization or complex computations, increase fuel limits
- Fuel is consumed during execution (ops, memory access, calls)
- Zero = unlimited (not recommended for untrusted code)
- `name` (required): **Application name - used by ApplicationManager for storage and lookup** (use this for undeployment, not application_id)
- `version` (required): Application version (e.g., "1.0.0")
- `behavior_kind` (optional): OTP-style behavior for logging (e.g. `GenEvent` for event-handler actors; logs show `EventHandler`)
- `wasm_file` (required): WASM file (multipart file upload, max 100MB)
- `config` (optional): Application config TOML file (if not provided, ApplicationSpec is auto-generated)

**ApplicationSpec Auto-Generation**:
If `config` is not provided, the HTTP handler automatically creates an `ApplicationSpec` from form fields:
- `name`: From `name` form field → **Used as application identifier in ApplicationManager** (important for undeployment)
- `version`: From `version` form field
- `type`: `ApplicationTypeActive` (active application with processes)
- `description`: Auto-generated as `"WASM application: {name}"`
- `dependencies`: Empty array
- `env`: Empty map (can be set via config TOML)
- `supervisor`: None (can be set via config TOML)

**ApplicationSpec Usage**:
- The ApplicationSpec is passed to `WasmApplication::new()` which implements the `Application` trait
- Used for supervisor tree initialization (if specified in config)
- Used for environment variables (if specified in config)
- Follows the Erlang-style application model where applications are the unit of deployment
- Matches the pattern used by the `wasm-calculator` example, ensuring consistent application deployment

**⚠️ Important**: The `name` field is critical - it's used by `ApplicationManager` for all operations (registration, starting, stopping, unregistering). Always use the `name` (not `application_id`) when undeploying applications.

**Response**:
```json
{
  "success": true,
  "application_id": "calculator-app",
  "status": "APPLICATION_STATUS_RUNNING",
  "error": null
}
```

**Examples**:

**Python WASM (Calculator) - 39MB:**
```bash
# CLI automatically uses HTTP for large files (>5MB)
./target/release/plexspaces deploy \
  --node localhost:8000 \
  --app-id calculator-app \
  --name calculator \
  --version 1.0.0 \
  --wasm examples/simple/wasm_calculator/wasm-modules/calculator_actor.wasm

# Or use HTTP directly
curl -X POST http://localhost:8001/api/v1/applications/deploy \
  -F "application_id=calculator-app" \
  -F "name=calculator" \
  -F "version=1.0.0" \
  -F "wasm_file=@examples/simple/wasm_calculator/wasm-modules/calculator_actor.wasm"
```

**Rust WASM - Small (<5MB):**
```bash
# Build Rust WASM (e.g. from nbody_wasm wasm-actors or your crate)
cd examples/rust/embedded/nbody_wasm/wasm-actors
cargo build --target wasm32-wasip2 --release

# CLI uses gRPC for small files
./target/release/plexspaces deploy \
  --node localhost:8000 \
  --app-id rust-app \
  --name rust-actor \
  --wasm target/wasm32-wasip2/release/rust_actor.wasm

# Or use HTTP
curl -X POST http://localhost:8001/api/v1/applications/deploy \
  -F "application_id=rust-app" \
  -F "name=rust-actor" \
  -F "version=1.0.0" \
  -F "wasm_file=@target/wasm32-wasip2/release/rust_actor.wasm"
```

**TypeScript WASM (simple-actor WIT, recommended):**

Use the [TypeScript SDK](sdk.md#typescript-sdk): extend `PlexSpacesActor`, bundle with esbuild, then build with jco:

```bash
# From examples/typescript/apps/bank_account
npm run build          # tsc + esbuild bundle
jco componentize account_actor_bundle.mjs --wit wit/plexspaces-simple-actor -o account_actor.wasm --disable all

# Deploy via HTTP
curl -X POST http://localhost:8001/api/v1/applications/deploy \
  -F "application_id=bank-test-ts" \
  -F "name=account" \
  -F "version=1.0.0" \
  -F "wasm_file=@account_actor.wasm" \
  -F "config=@app-config.toml"
```

**Why HTTP over gRPC?**
- ✅ **Industry Standard**: HTTP multipart is the standard for file uploads (S3, GitHub, Docker)
- ✅ **No Size Limits**: HTTP can handle files up to 100MB (configurable)
- ✅ **Better Tooling**: Works with `curl`, `wget`, browsers, CDNs
- ✅ **No Global Impact**: Doesn't require increasing gRPC message size limits for all APIs
- ✅ **Resumable**: Can implement chunked/resumable uploads in the future

### Method 2: CLI Tool (Automatic HTTP Fallback)

**Note**: CLI automatically detects file size and uses the appropriate method:
- **Files ≤5MB**: Uses gRPC (faster, simpler)
- **Files >5MB and ≤100MB**: Automatically uses HTTP multipart upload
- **Files >100MB**: Returns error (optimize with wasm-opt first)

**CLI Configuration**: 
- gRPC max message size: 5MB (configured to match server)
- HTTP max file size: 100MB (same as server limit)
- Automatic fallback: CLI automatically switches to HTTP for large files

**Command**:
```bash
./target/release/plexspaces deploy \
  --node localhost:8000 \
  --app-id calculator-app \
  --name calculator \
  --version 1.0.0 \
  --wasm examples/simple/wasm_calculator/wasm-modules/calculator_actor.wasm
```

**Works for**: Small WASM files (<5MB), typically Rust or optimized JavaScript/Go

**Examples**:

**Rust WASM (Small):**
```bash
# Rust WASM files are typically <1MB, so CLI uses gRPC
./target/release/plexspaces deploy \
  --node localhost:8000 \
  --app-id rust-counter \
  --name counter \
  --wasm target/wasm32-wasip2/release/counter.wasm
```

**Optimized JavaScript:**
```bash
# After wasm-opt optimization, JavaScript WASM can be <2MB
wasm-opt -Oz --strip-debug greeter.wasm -o greeter_opt.wasm
# CLI uses gRPC for small files
./target/release/plexspaces deploy \
  --node localhost:8000 \
  --app-id greeter-app \
  --name greeter \
  --wasm greeter_opt.wasm
```

**Large Python WASM (39MB):**
```bash
# CLI automatically detects large file and uses HTTP multipart
./target/release/plexspaces deploy \
  --node localhost:8000 \
  --app-id calculator-app \
  --name calculator \
  --wasm examples/simple/wasm_calculator/wasm-modules/calculator_actor.wasm
# Output: "⚠️  WASM file size (39.00MB) exceeds gRPC limit (5MB), using HTTP multipart upload"
```

### Method 3: Undeploy Application

**⚠️ Critical**: Use the application **name** (not `application_id`) for undeployment, because `ApplicationManager` stores applications by name. This is a common source of errors.

**HTTP DELETE:**
```bash
# Use application name (not application_id)
# If you deployed with name="calculator", use:
curl -X DELETE http://localhost:8001/api/v1/applications/calculator

# NOT: curl -X DELETE http://localhost:8001/api/v1/applications/calculator-app
```

**CLI:**
```bash
cargo run --release --bin plexspaces -- application undeploy \
  --node localhost:8000 \
  --name calculator
```

**Why Name vs Application ID?**
- `application_id`: Used for tracking/debugging, can be any unique identifier
- `name`: Used by `ApplicationManager` for internal storage, registration, starting, stopping, and unregistering
- The HTTP handler uses the `name` from the ApplicationSpec for all ApplicationManager operations

**Response**:
```json
{
  "success": true,
  "application_id": "calculator-app"
}
```

## Polyglot Examples

### Python Calculator Actor

**Location**: `examples/simple/wasm_calculator/`

**Build**:
```bash
cd examples/simple/wasm_calculator
./scripts/build_python_actors.sh
```

**Deploy**:
```bash
# HTTP (recommended for 39MB Python WASM)
curl -X POST http://localhost:8001/api/v1/applications/deploy \
  -F "application_id=calculator-app" \
  -F "name=calculator" \
  -F "version=1.0.0" \
  -F "wasm_file=@wasm-modules/calculator_actor.wasm"
```

**Undeploy**:
```bash
# Use application name (not application_id)
curl -X DELETE http://localhost:8001/api/v1/applications/calculator
```

### Rust Actor

**Location**: `examples/rust/embedded/nbody_wasm/wasm-actors/` (or any crate with `wasm32-wasip2` target)

**Build**:
```bash
cd examples/rust/embedded/nbody_wasm/wasm-actors
cargo build --target wasm32-wasip2 --release
```

**Deploy**:
```bash
# CLI (works for small Rust WASM)
./target/release/plexspaces deploy \
  --node localhost:8000 \
  --app-id rust-app \
  --name rust-actor \
  --wasm target/wasm32-wasip2/release/rust_actor.wasm

# Or HTTP
curl -X POST http://localhost:8001/api/v1/applications/deploy \
  -F "application_id=rust-app" \
  -F "name=rust-actor" \
  -F "version=1.0.0" \
  -F "wasm_file=@target/wasm32-wasip2/release/rust_actor.wasm"
```

### TypeScript/JavaScript Actor

**Location**: `examples/typescript/apps/bank_account/`

Uses the [TypeScript SDK](sdk.md#typescript-sdk) and the same `plexspaces-simple-actor` WIT as Python. Build with jco (not Javy) so the component imports only `plexspaces:simple-actor/host`.

**Key Features**:
- **SDK Handles WIT Types**: WIT TypeScript types are generated automatically by the SDK - clients don't need to generate or import them
- **Iterative Serializer**: SDK uses iterative JSON serialization to avoid WASM recursion issues
- **String Returns**: Actors return JSON strings (serialized by SDK) - host parses them

**Build**:
```bash
cd examples/typescript/apps/bank_account
./scripts/build.sh     # tsc → esbuild bundle → jco componentize --disable all
```

**Deploy**:
```bash
curl -X POST http://localhost:8001/api/v1/applications/deploy \
  -F "application_id=bank-test-ts" \
  -F "name=account" \
  -F "version=1.0.0" \
  -F "wasm_file=@account_actor.wasm" \
  -F "config=@app-config.toml"
```

### Go Actor

**Location**: Use tinygo with `wasip2` target; example layout TBD.

**Build**:
```bash
tinygo build -target=wasip2 -o go_actor.wasm go_actor.go
```

**Deploy**:
```bash
# HTTP (recommended)
curl -X POST http://localhost:8001/api/v1/applications/deploy \
  -F "application_id=go-app" \
  -F "name=go-actor" \
  -F "version=1.0.0" \
  -F "wasm_file=@go_actor.wasm"
```

## API Endpoints

### HTTP Multipart Upload

**Endpoint**: `POST /api/v1/applications/deploy`

**Port**: HTTP gateway runs on gRPC port + 1 (e.g., if gRPC is 8000, HTTP is 8001)

**Max File Size**: 100MB

**Example**:
```bash
curl -X POST http://localhost:8001/api/v1/applications/deploy \
  -F "application_id=calculator-app" \
  -F "name=calculator" \
  -F "version=1.0.0" \
  -F "wasm_file=@calculator_actor.wasm" \
  -F "config=@config.toml"
```

### HTTP Undeploy

**Endpoint**: `DELETE /api/v1/applications/{application_name}`

**Important**: Use the application **name** (not `application_id`) because `ApplicationManager` stores applications by name.

**Example**:
```bash
# Use application name (not application_id)
curl -X DELETE http://localhost:8001/api/v1/applications/calculator
```

### gRPC / CLI

**Endpoint**: `plexspaces.application.v1.ApplicationService/DeployApplication`

**gRPC Message Size Limit**: 5MB (configured on both server and CLI client)

**CLI Support**: 
- **Files ≤5MB**: CLI uses gRPC (fast, efficient)
- **Files >5MB and ≤100MB**: CLI automatically uses HTTP multipart (seamless fallback)
- **Files >100MB**: CLI returns error with optimization suggestion

**CLI Configuration**: 
- gRPC max message size: 5MB (matches server)
- HTTP max file size: 100MB (matches server)
- Automatic detection: CLI checks file size and chooses appropriate method

**Example** (using grpcurl):
```bash
grpcurl -plaintext \
  -d '{
    "application_id": "rust-app",
    "name": "rust-actor",
    "version": "1.0.0",
    "wasm_module": {
      "name": "rust-actor",
      "version": "1.0.0",
      "module_bytes": "'$(base64 -i rust_actor.wasm)'"
    }
  }' \
  localhost:8000 \
  plexspaces.application.v1.ApplicationService/DeployApplication
```

## Size Limits

### gRPC Message Size

- **Default**: 4MB (gRPC default)
- **PlexSpaces Setting**: 5MB (configured for flexibility)
- **Use Case**: Small WASM files, metadata-only deployments

### HTTP Multipart Upload

- **Max WASM File Size**: 100MB (enforced by server)
- **Use Case**: Large WASM files (Python, unoptimized builds)

### Recommendations

- **Files <5MB**: CLI automatically uses gRPC (faster, simpler)
- **Files 5-100MB**: CLI automatically uses HTTP multipart (seamless)
- **Files >100MB**: Optimize with `wasm-opt` first, then deploy (CLI will use HTTP)

**CLI Behavior**:
- Automatically detects file size
- Uses gRPC for files ≤5MB
- Automatically switches to HTTP for files >5MB and ≤100MB
- Returns error for files >100MB (with suggestion to optimize)

## Optimization Recommendations

### 1. Use `wasm-opt` (Binaryen)

```bash
# Install
brew install binaryen  # macOS
apt-get install binaryen  # Linux

# Optimize WASM file
wasm-opt -Oz --strip-debug calculator_actor.wasm -o calculator_actor_opt.wasm

# Check size reduction
ls -lh calculator_actor*.wasm
```

**Expected Results**:
- Python WASM: 39MB → 25-30MB (20-30% reduction)
- Rust WASM: 1MB → 500KB (50% reduction)
- JavaScript WASM: 2MB → 1MB (50% reduction)

### 2. Build Script Integration

The build script (`build_python_actors.sh`) automatically optimizes WASM files if `wasm-opt` is available:

```bash
./examples/simple/wasm_calculator/scripts/build_python_actors.sh
# Automatically runs wasm-opt if available
```

### 3. Language Selection for Production

For production deployments, consider language choice:

| Language | WASM Size | Build Time | Runtime Performance | Use Case |
|----------|-----------|------------|---------------------|----------|
| Rust | 100KB-1MB | Medium | Excellent | Production, performance-critical |
| Go | 2-5MB | Fast | Good | Good balance, fast iteration |
| JavaScript | 500KB-2MB | Fast | Good | Web integration, rapid prototyping |
| Python | 30-40MB | Medium | Moderate | ML, data processing, rapid prototyping |

## Performance, Concurrency, and Scalability

PlexSpaces uses the **WASM Component Model** (latest and recommended): WIT-based components, wasmtime with component-model support, and polyglot actors (Python, Rust, TypeScript, Go). This section summarizes performance characteristics and how to get the most out of the system for highly performant deployments.

### Component Model vs Traditional Modules

| Path | Cost per message | When to use |
|------|------------------|-------------|
| **Component model** (SimpleActor, PlexspacesActor — Python, WIT) | Per-message re-instantiation (new Store + instance per handle). Component is not recompiled; instantiation + init per message. | Polyglot (Python, etc.), WIT interfaces, latest tooling. **Recommended for most apps.** |
| **Traditional WASM modules** (non-component) | One instantiation per actor lifetime; same Store/instance reused. | Maximum throughput per actor, hot paths, Rust/Go JS without WIT. |

The runtime replaces component state after each successful `handle()` for component-model actors to avoid wasmtime’s “cannot enter component instance” trap on the second call. That keeps behavior correct and allows multiple sequential messages per actor.

### Concurrency and Scalability

- **Per-actor locking**: One lock per WASM instance (one message at a time per actor). This is the normal actor model.
- **Across actors**: Different actors use different instances and locks. **Many actors can handle messages concurrently**; there is no global lock.
- **Horizontal scaling**: Adding more actors increases parallelism. Re-instantiation is per-actor, so scaling out (more actors, more nodes) scales well.
- **Vertical scaling (messages/sec per actor)**: For component-model actors, per-message re-instantiation is the main limit. For very high single-actor throughput, use traditional WASM modules or offload hot work to them.

### Performance Tips

1. **Engine-level pooling** (default: on)  
   - **Enabled by default**: The node creates the WASM runtime with `WasmRuntime::new()`, which uses `WasmConfig::default()` where **`enable_pooling = true`**.  
   - This turns on **wasmtime’s pooling allocator** (`InstanceAllocationStrategy::Pooling`): the engine reuses memory and instance allocations instead of allocating per instantiation.  
   - You get this automatically when starting a node; no extra config is required.  
   - To turn it off (e.g. for debugging), create the runtime with `WasmRuntime::with_config(config)` and set `config.enable_pooling = false`.

2. **Instance pooling** (recommended, default: on)  
   - **Recommendation**: Yes. Instance pooling (pre-instantiated instances you checkout instead of instantiating each time) reduces spawn latency when many actors of the same module are created.  
   - **Config**: **`use_instance_pool`** in `WasmConfig` (and in proto). **On by default** (`true`). When true, the runtime may use a per-module `InstancePool` to serve instantiate requests (checkout from pool instead of full instantiation).  
   - **Current status**: Deploy-path integration is planned. Until then, only **engine-level pooling** (above) is active when creating actors via HTTP/deploy; each actor is still created via `runtime.instantiate()`. The **`InstancePool`** type exists in `plexspaces-wasm-runtime` and can be used in custom code (e.g. high-spawn-rate workers). When deploy-path integration is complete, `use_instance_pool = true` will enable checkout-from-pool for spawns.  
   - To turn instance pooling off: set `config.use_instance_pool = false` (or the proto field when using gRPC/config).

3. **Keep durability off unless needed**  
   - `durability_enabled` is **off by default** in `WasmConfig` (and in proto).  
   - When off, no checkpoint load on init or save on terminate — no extra I/O or serialization.  
   - Turn on only when actor state must survive restarts (e.g. Durable Objects–style apps).

3. **Scale horizontally**  
   - Run more actors and/or more nodes to increase throughput.  
   - Component-model cost is per-actor; spreading load across actors avoids a single-actor bottleneck.

5. **Use `wasm-opt`**  
   - Smaller modules load and instantiate faster.  
   - See [Optimization Recommendations](#optimization-recommendations) (e.g. `wasm-opt -Oz --strip-debug`).

6. **Prefer smaller, focused actors**  
   - Many small actors can outperform fewer “heavy” actors by better utilizing concurrency and pooling.

7. **Hot path: traditional WASM**  
   - For a few actors that must handle very high message rates, use traditional WASM modules (non-component) so the same Store/instance is reused and there is no per-message re-instantiation.

8. **Resource limits**  
   - Set `limits` in `WasmConfig` (e.g. `max_memory_bytes`, `max_fuel`) to avoid runaway usage; tighter limits can also improve predictability.
   - **Fuel limits**: Default is 10 billion units (~1 second CPU time). For operations requiring heavy JSON serialization or complex computations, increase `max_fuel` (e.g., `u64::MAX / 2` for very large operations). Fuel is consumed during execution (ops, memory access, calls). Zero = unlimited (not recommended for untrusted code).

### Summary

- **Performance**: Component model is correct and recommended; it pays a per-message instantiation cost. Traditional modules give the highest per-actor throughput.
- **Concurrency**: Good — per-actor serialization, no global serialization; many actors run in parallel.
- **Scalability**: Good horizontally (more actors/nodes); per-actor throughput is the main limit for component-model.
- **For high performance**: Use pooling, keep durability off by default, scale out with more actors, optimize with `wasm-opt`, and use traditional WASM on the hottest paths if needed.

## Complete Deployment Workflow

### 1. Build WASM Module

**Python:**
```bash
cd examples/simple/wasm_calculator
./scripts/build_python_actors.sh
# Output: wasm-modules/calculator_actor.wasm
```

**Rust:**
```bash
cd examples/rust/embedded/nbody_wasm/wasm-actors
cargo build --target wasm32-wasip2 --release
# Output: target/wasm32-wasip2/release/*.wasm
```

**TypeScript (simple-actor WIT):**
```bash
cd examples/typescript/apps/bank_account
./scripts/build.sh   # tsc → esbuild bundle → jco componentize --disable all
# Output: account_actor.wasm
```

### 2. Start Empty Node

```bash
# Start an empty node using CLI
cargo run --release --bin plexspaces -- start \
  --node-id test-node \
  --listen-addr 0.0.0.0:8000
```

**Note**: 
- HTTP gateway automatically starts on port 8001 (gRPC port + 1)
- Dashboard is available at `http://localhost:8001/`
- You can check dashboard stats before deployment (should show 0 applications)

**Verify Node is Running:**
```bash
# Check dashboard summary
curl http://localhost:8001/api/v1/dashboard/summary | jq '.total_applications'
# Should return: 0
```

### 3. Deploy Application

**ApplicationSpec Creation**: The HTTP handler automatically creates an `ApplicationSpec` from form fields when deploying WASM applications. This follows the Erlang-style application model where applications are the unit of deployment.

**How ApplicationSpec is Created**:
1. If `config` field is provided (TOML), it's parsed into ApplicationSpec
2. If `config` is not provided, ApplicationSpec is auto-generated from form fields:
   - `name`: From `name` form field → **Used as application identifier in ApplicationManager** (important for undeployment)
   - `version`: From `version` form field
   - `type`: `ApplicationTypeActive` (active application with processes)
   - `description`: Auto-generated as `"WASM application: {name}"`
   - `dependencies`: Empty array
   - `env`: Empty map (can be set via config TOML)
   - `supervisor`: None (can be set via config TOML)

**ApplicationSpec Usage**:
- The ApplicationSpec is passed to `WasmApplication::new()` which implements the `Application` trait
- Used for supervisor tree initialization (if specified)
- Used for environment variables (if specified)
- Follows the same pattern as the `wasm-calculator` example

**Deployment via HTTP (Recommended for Large Files >5MB):**
```bash
# Deploy with auto-generated ApplicationSpec
curl -v -X POST http://localhost:8001/api/v1/applications/deploy \
  -F "application_id=calculator-app" \
  -F "name=calculator" \
  -F "version=1.0.0" \
  -F "wasm_file=@wasm-modules/calculator_actor.wasm"

# Deploy with custom ApplicationSpec (via config TOML)
curl -v -X POST http://localhost:8001/api/v1/applications/deploy \
  -F "application_id=calculator-app" \
  -F "name=calculator" \
  -F "version=1.0.0" \
  -F "wasm_file=@wasm-modules/calculator_actor.wasm" \
  -F "config=@app-config.toml"
```

**Deployment via CLI (For Small Files <5MB):**
```bash
cargo run --release --bin plexspaces -- application deploy \
  --node localhost:8000 \
  --app-id rust-app \
  --name rust-actor \
  --version 1.0.0 \
  --wasm target/wasm32-wasip2/release/rust_actor.wasm
```

**⚠️ Critical Notes**:
- **Application Name vs Application ID**: The `name` field is used by `ApplicationManager` for storage and lookup. Use the `name` (not `application_id`) when undeploying.
- **WASM Components (Python, TypeScript)**: ✅ **Fully Supported** - Components built with `componentize-py` (Python) or `jco componentize` (TypeScript) use the `simple-actor` WIT interface
  - Uses JSON strings for all complex data (Python: avoids pyo3 lifting; TypeScript: single WIT world)
  - TypeScript: build with `jco componentize ... --disable all` so the component only imports `plexspaces:simple-actor/host` (no WASI)
  - See `examples/python/` and `examples/typescript/apps/bank_account/` for working examples
  - See `wit/plexspaces-simple-actor/` for the WIT interface
- **Traditional WASM Modules** (Rust, Go): ✅ **Supported** - Use standard actor interface
- **ApplicationSpec is Required**: All WASM deployments must include an ApplicationSpec (auto-generated or provided). This ensures applications follow the Erlang-style application model.

**Testing WASM Deployment**:
- The integration test (`cargo test --package plexspaces-node --test http_wasm_deployment`) creates a working traditional WASM module and successfully deploys it

## Auto-Deploy and Persistence

### Auto-Deploy on Startup

PlexSpaces automatically deploys WASM applications from the `wasm_apps_directory` on node startup. This enables Tomcat-style auto-deployment where applications persist across restarts.

**File Structure**:
```
{wasm_apps_directory}/
  payment-handler/
    app.wasm                     # Required: WASM module
    application-spec.toml        # Optional: ApplicationSpec config
  calculator/
    app.wasm
    application-spec.toml
```

**Configuration**:
- Environment variable: `PLEXSPACES_WASM_APPS_DIR` (default: `${base_dir}/apps`)
- Config file: `runtime.wasm_apps_directory` in `release.yaml`

**How It Works**:
1. Node scans `wasm_apps_directory` on startup
2. Finds all subdirectories containing `app.wasm` files
3. Automatically deploys each valid WASM application
4. Errors are logged but don't prevent node startup

### Saving WASM Files on API Deployment

When deploying via HTTP/gRPC API, you can optionally save WASM files to disk for persistence.

**Configuration**:
- Environment variable: `PLEXSPACES_SAVE_WASM_APPS=1` (default: disabled)
- Config file: `runtime.save_wasm_apps: true` in `release.yaml`

**Important**:
- ⚠️ **Only saves during API deployments** (HTTP/gRPC) - NOT during auto-deploy
- ⚠️ **Disabled by default** - only enable for testing/development
- ⚠️ **Production**: Use proper deployment pipelines
- Files are saved atomically to prevent corruption
- Format: `{wasm_apps_directory}/{app-name}/app.wasm` and `{wasm_apps_directory}/{app-name}/application-spec.toml`

**Example Workflow**:
```bash
# 1. Enable saving (testing only)
export PLEXSPACES_SAVE_WASM_APPS=1

# 2. Deploy via API - files are saved to apps/payment-handler/app.wasm and application-spec.toml
curl -X POST http://localhost:8001/api/v1/applications/deploy \
  -F "application_id=payment-handler" \
  -F "name=payment-handler" \
  -F "version=1.0.0" \
  -F "wasm_file=@payment-handler.wasm"

# 3. On next restart, payment-handler is auto-deployed automatically from the subdirectory
```

See [Installation Guide](installation.md#wasm-applications-auto-deploy-and-persistence) for complete details.
- Use the test script (`./scripts/test-empty-node-deployment.sh`) which automatically creates a working WASM module
- For manual testing, use Rust/Go WASM modules or TypeScript/Python components (simple-actor WIT)

### 4. Verify Deployment

**Check Dashboard Stats:**
```bash
# Check dashboard summary (should show 1 application now)
curl http://localhost:8001/api/v1/dashboard/summary | jq '.total_applications'
# Should return: 1
```

**List Applications:**
```bash
curl http://localhost:8001/api/v1/applications
```

**View Dashboard:**
```bash
# Open in browser
open http://localhost:8001/
# Or
http://localhost:8001/dashboard/node/test-node
```

**Complete Dashboard Workflow:**
1. Start empty node → Check dashboard (0 applications)
2. Deploy WASM application → Check dashboard (1 application)
3. Undeploy application → Check dashboard (0 applications)

See [DEPLOY_EMPTY_NODE_GUIDE.md](../DEPLOY_EMPTY_NODE_GUIDE.md) for the complete workflow.

### 5. Undeploy Application

**Important**: Use the application **name** (not `application_id`) for undeployment, because `ApplicationManager` stores applications by name.

**HTTP:**
```bash
# Use application name (not application_id)
curl -X DELETE http://localhost:8001/api/v1/applications/calculator
```

**CLI:**
```bash
cargo run --release --bin plexspaces -- application undeploy \
  --node localhost:8000 \
  --name calculator
```

**Verify Undeployment:**
```bash
# Check dashboard (should show 0 applications again)
curl http://localhost:8001/api/v1/dashboard/summary | jq '.total_applications'
# Should return: 0
```

## Troubleshooting

### "Message length too large" Error

**Problem**: WASM file exceeds gRPC 5MB limit

**Solution**: 
- **CLI**: Automatically handles this - if you see this error, the CLI should have automatically switched to HTTP. Check CLI version.
- **Manual**: Use HTTP multipart upload:
```bash
curl -X POST http://localhost:8001/api/v1/applications/deploy \
  -F "wasm_file=@large_file.wasm" \
  ...
```

### "Failed to parse multipart/form-data" Error

**Problem**: HTTP multipart parsing fails when uploading WASM files

**Solution**: 
- **Server Configuration**: The server is configured with a 100MB body size limit via `DefaultBodyLimit` middleware. If you see this error:
  1. Verify the file size is ≤100MB
  2. Check that the `Content-Type` header is `multipart/form-data`
  3. Ensure all required fields are present (`application_id`, `name`, `version`, `wasm_file`)
  4. Check server logs for detailed error messages

**Example with proper curl syntax**:
```bash
curl -X POST http://localhost:8001/api/v1/applications/deploy \
  -F "application_id=calculator-app" \
  -F "name=calculator" \
  -F "version=1.0.0" \
  -F "wasm_file=@examples/simple/wasm_calculator/wasm-modules/calculator_actor.wasm"
```

### "Payload too large" Error

**Problem**: WASM file exceeds HTTP 100MB limit

**Solutions**:
1. Use `wasm-opt` to reduce size
2. Consider Rust/Go for smaller files
3. Split application into multiple smaller modules (future)

### "PyObject_SetItem" Error (Python WASM Components)

**Problem**: Python WASM components crash during initialization with:
```
error while executing at wasm backtrace:
    ...
    libpython3.12.so!PyObject_SetItem
    libcomponentize_py_runtime.so!set_item::inner
```

**Root Cause**: componentize-py's pyo3 runtime tries to call `os.putenv()` during Python initialization. WASI doesn't support runtime environment variable modification, causing the crash.

**Solution**: The PlexSpaces runtime is configured to NOT inherit environment variables. Instead, it explicitly sets only the minimal env vars Python needs:

```rust
// In crates/wasm-runtime/src/instance.rs
let wasi_ctx = wasmtime_wasi::WasiCtxBuilder::new()
    .inherit_stdio()
    // Don't use inherit_env() - causes PyObject_SetItem errors
    .env("PYTHONDONTWRITEBYTECODE", "1")
    .env("PYTHONUNBUFFERED", "1")
    .env("HOME", "/")
    .env("PATH", "/")
    .build();
```

This fix is already applied in the codebase. If you're running an older version, update to the latest.

### Python 3.14 WASM Memory Bugs (Critical)

**Problem**: Python WASM actors crash with memory deallocation errors:
```
error while executing at wasm backtrace:
    0: libpython3.14.so!tuple_dealloc
    1: libpython3.14.so!_Py_Dealloc
    ...
```

**Error Types**:
| Error | Symptom | Cause |
|-------|---------|-------|
| `match_dealloc` | Crash when using pattern matching or hashlib | `hashlib.md5()` and similar functions |
| `tuple_dealloc` | Crash when returning from functions | Complex return values, `json.dumps()` |
| `func_dealloc` | Crash during function cleanup | Helper function calls |

**Root Cause**: The Python 3.14 runtime in componentize-py has memory management bugs in the WASM environment.

**Workarounds**:

```python
# ❌ CRASHES - hashlib causes match_dealloc
import hashlib
h = hashlib.md5((flag + user).encode()).hexdigest()

# ✅ WORKS - simple inline hash
h = 0
for c in (flag + user):
    h = (h + ord(c)) % 100

# ❌ MAY CRASH - json.dumps with complex nested data
return json.dumps({"status": "ok", "data": {"nested": "value"}})

# ✅ WORKS - string literal for simple responses
return '{"status":"ok"}'

# ❌ MAY CRASH - helper function call
def my_hash(s):
    return sum(ord(c) for c in s) % 100
h = my_hash(flag + user)

# ✅ WORKS - inline the logic
h = 0
for c in (flag + user):
    h = (h + ord(c)) % 100
```

**Best Practices for Stable Python WASM Actors**:
1. **Avoid hashlib entirely** - Use simple arithmetic hash functions
2. **Use string literals for simple JSON** - `'{"ok":true}'` instead of `json.dumps({"ok": True})`
3. **Inline calculations** - Don't extract logic into helper functions
4. **Flat control flow** - Avoid nested try-except blocks
5. **Simple return values** - Keep data structures flat and simple

**Reference**: See `examples/python/apps/feature_flags/` for a working example with all workarounds applied.

### WASM File Too Large

**Problem**: Python WASM files are 30-40MB

**Solutions**:
1. Use `wasm-opt` to reduce size (20-30% reduction)
2. Consider Rust/Go for smaller files
3. Use HTTP multipart (supports up to 100MB)

### Deployment Fails

**Check**:
1. Node is running: `curl http://localhost:8001/health` or `curl http://localhost:8000/health`
2. WASM file is valid: `wasm-validate calculator_actor.wasm` (if wasm-validate is installed)
3. WIT interface matches: Check `wit/plexspaces-actor/actor.wit`
4. HTTP gateway is running: Check logs for "Starting HTTP gateway server on http://..."

### HTTP Gateway Not Accessible

**Problem**: Cannot connect to HTTP endpoint

**Check**:
1. HTTP gateway runs on gRPC port + 1 (e.g., if gRPC is 8000, HTTP is 8001)
2. Check node logs for "Starting HTTP gateway server on http://..."
3. Verify firewall allows connections to HTTP port

## Python WASM Development

### Prerequisites

```bash
# Create Python 3.12+ virtual environment
python3.12 -m venv ~/venv
source ~/venv/bin/activate

# Install componentize-py
pip install componentize-py
```

### WIT Interface (Simple Actor)

Python components use the `simple-actor` WIT interface located at `wit/plexspaces-simple-actor/world.wit`. This interface uses JSON strings for all complex data to avoid componentize-py's pyo3 lifting issues:

```wit
package plexspaces:simple-actor@0.1.0;

interface actor {
    init: func(config-json: string) -> string;
    handle: func(from-actor: string, msg-type: string, payload-json: string) -> string;
    get-state: func() -> string;
    set-state: func(state-json: string) -> string;
}

interface host {
    // Messaging
    send: func(to: string, msg-type: string, payload-json: string) -> string;
    ask: func(to: string, msg-type: string, payload-json: string, timeout-ms: u64) -> string;
    // Actor Identity
    self-id: func() -> string;
    // Actor Lifecycle
    spawn: func(module-ref: string, actor-id: string, init-config-json: string) -> string;
    stop: func(actor-id: string) -> string;
    // Linking & Monitoring (Erlang/OTP patterns)
    link: func(actor-id: string) -> string;
    unlink: func(actor-id: string) -> string;
    monitor: func(actor-id: string) -> string;
    demonitor: func(monitor-ref: string) -> string;
    // Timers (Delayed Messaging)
    send-after: func(delay-ms: u64, msg-type: string, payload-json: string) -> string;
    // Logging & Time
    log: func(level: string, message: string);
    now-ms: func() -> u64;
    // Key-Value Store
    kv-get: func(key: string) -> string;
    kv-put: func(key: string, value: string) -> string;
    kv-delete: func(key: string) -> string;
    kv-list: func(prefix: string) -> string;
    // TupleSpace, Locks, Blob Storage, Process Groups
    // (see wit/plexspaces-simple-actor/world.wit for full interface)
}

world actor-world {
    import host;
    export actor;
}
```

### TupleSpace (ts_write) for WASM

WASM actors using the simple-actor WIT can write tuples via **`host.ts_write(tuple_json)`**. The runtime parses a JSON array (e.g. `["AUDIT","action","actor_id","resource","details","ts"]`) into a Tuple and calls the same TupleSpace backend as native code. Use this for event streams, audit logs, or coordination without keyvalue. The [Audit Log](../examples/python/apps/audit_log/README.md) example currently uses **host.log only** (no ts_write) to avoid WASM integration issues; API support can be added when the runtime is more stable.

**When to use ts_write**: Prefer `ts_write` for fire-and-forget event or audit streams when WASM integration is stable; it avoids reentrancy and readonly issues that can occur when WASM calls into the keyvalue backend during message handling.

### Key-Value Storage (WASM)

WASM actors (Python simple-actor) can persist data via the host **keyvalue** API. This avoids in-actor state serialization issues and provides reliable storage across the WASM boundary.

**Choosing storage**: For event streams or audit logs, prefer **`host.ts_write(tuple_json)`** (see [TupleSpace (ts_write) for WASM](#tuplespace-ts_write-for-wasm)). Key-value values must be UTF-8 strings; binary/protobuf data in the same `kv_store` table may come from the object registry—see [KeyValue crate README](../crates/keyvalue/README.md) for inspecting the store.

| Host function | Description |
|---------------|-------------|
| `kv-get(key)` | Returns value as string, or empty if not found. Errors return `"ERROR:message"`. |
| `kv-put(key, value)` | Stores string value. Returns empty on success, `"ERROR:message"` on failure. |

**Scope**: Keys are scoped per actor (namespace derived from actor ID). The node provides an in-memory keyvalue store for WASM actors by default.

**Example (Python)**:

```python
from plexspaces import actor, handler, host

@actor
class SensorStream:
    @handler("ingest")
    def ingest(self, sensor_id: str = "", value: str = "0") -> str:
        raw = host.kv_get("readings")
        data = json.loads(raw) if raw else []
        data.append({"sensor_id": sensor_id, "value": value})
        err = host.kv_put("readings", json.dumps(data))
        return "" if not err else err

    @handler("count")
    def count(self) -> str:
        raw = host.kv_get("readings")
        data = json.loads(raw) if raw else []
        return '{"reading_count":' + str(len(data)) + '}'
```

**Best practice**: Have handlers return **strings only** (e.g. JSON built by concatenation or `json.dumps` inside the handler) to avoid componentize-py traps when crossing the WASM boundary. Full keyvalue API (TTL, list-keys, etc.) will be added to the SDKs later.

### Building Python Actors

```bash
cd examples/python/apps/calculator
./build.sh
```

The build script:
1. Generates Python bindings from WIT
2. Compiles Python to WASM Component using componentize-py
3. Produces a ~35MB WASM file (includes Python runtime)

### Example Python Actor

```python
import json
from wit_world import exports

class Actor(exports.Actor):
    def init(self, config_json: str) -> str:
        """Returns "" on success, "ERROR: ..." on failure"""
        return ""
    
    def handle(self, from_actor: str, msg_type: str, payload_json: str) -> str:
        """Returns JSON response or "ERROR: ..."."""
        request = json.loads(payload_json)
        operation = request.get('operation', msg_type)
        if operation == 'add':
            result = sum(request.get('operands', []))
            return json.dumps({'result': result})
        return json.dumps({'error': 'Unknown operation'})
    
    def get_state(self) -> str:
        return json.dumps({})
    
    def set_state(self, state_json: str) -> str:
        return ""
```

See `examples/python/README.md` for complete documentation.

### TypeScript WASM Development

TypeScript actors use the same **simple-actor** WIT world as Python. Use the [TypeScript SDK](sdk.md#typescript-sdk): extend `PlexSpacesActor<TState>`, implement `getDefaultState()` and `on<Op>(payload)` handlers, then build with **jco componentize** (not Javy).

**SDK Simplification**: The SDK automatically generates WIT TypeScript types during build - client code doesn't need to run `jco types` or import generated files. The SDK abstracts all WIT details away, keeping client code simple.

**Build** (from `examples/typescript/apps/bank_account`):

1. Install deps: `npm install` (includes `@plexspaces/sdk`, `esbuild`, `jco`)
2. Build: `./scripts/build.sh` — runs tsc, esbuild bundle (actor + SDK → single ESM), then `jco componentize account_actor_bundle.mjs --wit wit/plexspaces-simple-actor -o account_actor.wasm --disable all`

**Important**: Use `--disable all` so the component only imports `plexspaces:simple-actor/host`; the PlexSpaces runtime does not provide WASI 0.2.3 that jco would otherwise add.

**SDK Simplification**: 
- WIT TypeScript types are automatically generated by the SDK during build (`npm run build` in SDK)
- Client code doesn't need to run `jco types` or import generated files
- SDK uses iterative JSON serialization to avoid WASM recursion issues
- Just extend `PlexSpacesActor` and implement handlers - SDK handles all WIT details

See [examples/typescript/apps/bank_account/README.md](../examples/typescript/apps/bank_account/README.md) and [sdks/typescript/README.md](../sdks/typescript/README.md) for full docs.

## WASM Actor State Persistence (Durability)

WASM actors support **checkpoint-based durability** via the `get-state()` and `set-state()` WIT interface functions. This follows the [Cloudflare Durable Objects](https://developers.cloudflare.com/durable-objects/) pattern.

### How It Works

1. **Actor manages state internally**: Your WASM actor maintains state in memory
2. **Framework calls `get-state()`**: On shutdown or checkpoint interval, framework gets state
3. **State is persisted**: Framework stores state snapshot in SQLite/PostgreSQL
4. **On restart, `set-state()` is called**: Framework restores state from checkpoint

### Implementing State Persistence

```python
import json

class StatefulActor:
    def __init__(self):
        self.data = {}  # Internal state
    
    def get_state(self) -> str:
        """Called by framework to checkpoint state."""
        return json.dumps(self.data)
    
    def set_state(self, state_json: str) -> str:
        """Called by framework to restore state on restart."""
        if state_json:
            self.data = json.loads(state_json)
        return ""  # Empty = success
    
    def handle(self, from_actor: str, msg_type: str, payload_json: str) -> str:
        # Update self.data based on messages
        pass
```

### Key Points

- **JSON format**: State must be JSON-serializable
- **Empty string = success**: `set-state()` returns empty string on success
- **Graceful degradation**: `set-state()` should handle empty/null input
- **Size matters**: Keep state small for fast checkpointing

### Metrics

State operations are fully instrumented with Prometheus metrics:

- `plexspaces_wasm_get_state_total`: Total checkpoint calls
- `plexspaces_wasm_set_state_total`: Total state restore calls
- `plexspaces_wasm_state_size_bytes`: Size of persisted state

See [Durability Documentation](durability.md#wasm-actor-durability-cloudflare-durable-objects-pattern) for complete details.

## Supervisor Integration for WASM Actors

WASM actors are deployed under a supervisor tree that provides automatic restart on failure. This follows the Erlang/OTP supervision model.

### Default Supervisor Configuration

When deploying a WASM application without explicit supervisor configuration, the server automatically creates a default supervisor:

```
Strategy: OneForOne (restart only the failed actor)
Max Restarts: 5
Children: The WASM actor as a permanent worker
```

### How Supervisor Integration Works

The supervisor integration for WASM actors uses a unified approach:

1. **Factory Function Pattern**: Each WASM actor has a factory function (`StartFn`) that can recreate the actor
2. **`build_wasm_actor` Helper**: Single entry point for building WASM actors with full service wiring
3. **`Supervisor.add_child()`**: Adds WASM actors to the supervisor with proper ChildSpec
4. **Automatic Restart**: When an actor crashes, the supervisor calls the factory to recreate it

### Architecture

```
WasmApplication
  └── Root Supervisor (one-for-one)
       ├── worker-1 (WASM actor) ← Factory can recreate on crash
       └── worker-2 (WASM actor) ← Factory can recreate on crash
```

### Key Components

**`create_wasm_actor_child_spec()`** - Creates a ChildSpec with factory:
```rust
// Uses ChildSpec::worker() for consistency with Rust supervisor patterns
let spec = ChildSpec::worker(child_id, actor_id, factory);
```

**`build_wasm_actor()`** - Unified helper that:
- Wires up all services (TupleSpace, ObjectRegistry, JournalStorage, etc.)
- Creates unstarted Actor with proper context
- Returns (Actor, ActorRef) for supervisor management

**Factory Function** - Captured context for restart:
- node, proto_child_spec, module_hash, runtime
- Called by supervisor when actor needs restart

### Supervision Strategies

| Strategy | Behavior | Use Case |
|----------|----------|----------|
| OneForOne | Restart only failed actor | Independent workers |
| OneForAll | Restart all actors | Tightly coupled actors |
| RestForOne | Restart failed + started after | Dependency chain |

### Example: Feature Flags with Supervisor

```bash
# Deploy feature flags service (supervisor auto-created)
./target/debug/plexspaces deploy \
  --node localhost:8090 \
  -i feature-flags-test \
  -n flags \
  -w examples/python/apps/feature_flags/feature_flags_actor.wasm
```

The deployed application automatically has:
- OneForOne supervisor
- Automatic restart on crash (up to 5 times)
- Full service access (TupleSpace, ObjectRegistry, etc.)

### Custom Supervisor Configuration

To customize supervisor settings, provide a config TOML file:

```toml
# app-config.toml
[supervisor]
strategy = "one_for_one"
max_restarts = 10
max_restart_window_seconds = 60

[[supervisor.children]]
id = "worker-1"
type = "worker"
restart = "permanent"
shutdown_timeout_seconds = 5
```

Deploy with custom config:
```bash
curl -X POST http://localhost:8001/api/v1/applications/deploy \
  -F "application_id=my-app" \
  -F "name=my-app" \
  -F "version=1.0.0" \
  -F "wasm_file=@my_actor.wasm" \
  -F "config=@app-config.toml"
```

## Best Practices

1. **Use HTTP for Large Files**: Always use HTTP multipart for files >5MB
2. **Optimize Before Deploy**: Run `wasm-opt` on all WASM files
3. **Version Control**: Tag WASM files with version numbers
4. **Content-Addressable**: Use module hash for caching (automatic)
5. **Language Selection**: Choose language based on size/performance requirements
6. **Test Locally First**: Verify WASM file works before deploying to production
7. **Monitor Deployment**: Check dashboard after deployment to verify application is running

## Integration Tests

Integration tests are available in `crates/node/tests/http_wasm_deployment.rs`:

```bash
# Run tests (requires WASM files to be built first)
cargo test --package plexspaces-node --test http_wasm_deployment

# Build WASM files first
cd examples/simple/wasm_calculator
./scripts/build_python_actors.sh
```

Tests cover:
- HTTP multipart deployment
- HTTP undeployment
- Size limit enforcement (100MB)
- Error handling

## References

- **[SDK Guide](sdk.md)** - Python and TypeScript SDKs for building WASM actors
- **[Polyglot WASM Development Guide](polyglot.md)** - Polyglot development (Python, TypeScript, Rust, Go) with WIT abstractions
- **[Python WASM Examples](../examples/python/README.md)** - Python WASM actors with componentize-py
- **[TypeScript Bank Account Example](../examples/typescript/apps/bank_account/README.md)** - TypeScript WASM with jco and simple-actor WIT
- [WIT Specification](https://github.com/WebAssembly/component-model/blob/main/design/mvp/WIT.md)
- [componentize-py](https://github.com/bytecodealliance/componentize-py) - Python to WASM Component compiler
- [jco](https://bytecodealliance.github.io/jco/) - JavaScript/TypeScript componentize (componentize-js)
- [wasm-opt Documentation](https://github.com/WebAssembly/binaryen)
- [HTTP Multipart Upload Best Practices](https://developer.mozilla.org/en-US/docs/Web/HTTP/Methods/POST)
- [WASM Calculator Example](../examples/simple/wasm_calculator/README.md)

