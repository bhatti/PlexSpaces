# WASM Deployment Guide

## Overview

PlexSpaces supports deploying WebAssembly (WASM) applications from multiple languages (Rust, Python, TypeScript/JavaScript, Go) via HTTP multipart upload or gRPC, following industry best practices for large file uploads.

**📖 For comprehensive polyglot development guide covering all languages, WIT abstractions, and examples, see [Polyglot WASM Development Guide](polyglot.md)**

**Quick Start**: See [DEPLOY_EMPTY_NODE_GUIDE.md](../DEPLOY_EMPTY_NODE_GUIDE.md) for a complete workflow showing how to start an empty node, deploy a WASM application, and verify deployment via the dashboard.

## Architecture

### WASM Dependencies Verification

**✅ WASM actors only use WIT (WebAssembly Interface Types) APIs** - they do NOT include framework dependencies:

- ✅ **WIT Interfaces**: Actors import host functions via WIT (e.g., `host::send_message`, `host::tuplespace_write`)
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
- `name` (required): **Application name - used by ApplicationManager for storage and lookup** (use this for undeployment, not application_id)
- `version` (required): Application version (e.g., "1.0.0")
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
# Build Rust WASM first
cd examples/simple/polyglot_wasm_deployment/actors/rust
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

**TypeScript/JavaScript WASM:**
```bash
# Build TypeScript to WASM (using Javy)
cd examples/simple/polyglot_wasm_deployment/actors/typescript
npx tsc greeter.ts --target ES2020 --module commonjs
javy compile greeter.js -o greeter.wasm

# Deploy via HTTP
curl -X POST http://localhost:8001/api/v1/applications/deploy \
  -F "application_id=typescript-app" \
  -F "name=greeter" \
  -F "version=1.0.0" \
  -F "wasm_file=@greeter.wasm"
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

**Location**: `examples/simple/polyglot_wasm_deployment/actors/rust/`

**Build**:
```bash
cd examples/simple/polyglot_wasm_deployment/actors/rust
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

**Location**: `examples/simple/polyglot_wasm_deployment/actors/typescript/`

**Build**:
```bash
cd examples/simple/polyglot_wasm_deployment/actors/typescript
npx tsc greeter.ts --target ES2020 --module commonjs
javy compile greeter.js -o greeter.wasm
```

**Deploy**:
```bash
# HTTP (recommended)
curl -X POST http://localhost:8001/api/v1/applications/deploy \
  -F "application_id=typescript-app" \
  -F "name=greeter" \
  -F "version=1.0.0" \
  -F "wasm_file=@greeter.wasm"
```

### Go Actor

**Location**: `examples/simple/polyglot_wasm_deployment/actors/go/` (if exists)

**Build**:
```bash
cd examples/simple/polyglot_wasm_deployment/actors/go
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
cd examples/simple/polyglot_wasm_deployment/actors/rust
cargo build --target wasm32-wasip2 --release
# Output: target/wasm32-wasip2/release/rust_actor.wasm
```

**TypeScript:**
```bash
cd examples/simple/polyglot_wasm_deployment/actors/typescript
npx tsc greeter.ts --target ES2020 --module commonjs
javy compile greeter.js -o greeter.wasm
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
- **WASM Components vs Traditional Modules**: 
  - **Traditional WASM modules** (Rust, Go, JavaScript): ✅ **Supported** - Use these for testing
  - **WASM Components** (Python from `componentize-py`): ❌ **Not yet supported** - Will fail with component instantiation error
  - **See [WASM_COMPONENT_VS_MODULE.md](WASM_COMPONENT_VS_MODULE.md) for details**
- **ApplicationSpec is Required**: All WASM deployments must include an ApplicationSpec (auto-generated or provided). This ensures applications follow the Erlang-style application model.

**Testing WASM Deployment**:
- The integration test (`cargo test --package plexspaces-node --test http_wasm_deployment`) creates a working traditional WASM module and successfully deploys it
- Use the test script (`./scripts/test-empty-node-deployment.sh`) which automatically creates a working WASM module
- For manual testing, use Rust/Go/JavaScript WASM modules, not Python components

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

- **[Polyglot WASM Development Guide](polyglot.md)** - Comprehensive guide for polyglot development (Python, TypeScript, Rust, Go) with all WIT abstractions
- [WIT Specification](https://github.com/WebAssembly/component-model/blob/main/design/mvp/WIT.md)
- [wasm-opt Documentation](https://github.com/WebAssembly/binaryen)
- [HTTP Multipart Upload Best Practices](https://developer.mozilla.org/en-US/docs/Web/HTTP/Methods/POST)
- [Polyglot WASM Deployment Example](../examples/simple/polyglot_wasm_deployment/README.md)
- [WASM Calculator Example](../examples/simple/wasm_calculator/README.md)

