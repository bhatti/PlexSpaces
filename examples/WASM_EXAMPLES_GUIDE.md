# WASM Examples Guide

## Lessons Learned from nbody-wasm Example

This guide documents best practices and lessons learned from implementing the `nbody-wasm` example, to be applied to future WASM examples.

## Key Patterns

### 1. Dual Build Support (TypeScript + Rust Fallback)

**Pattern**: Support both TypeScript/JavaScript WASM (via Javy) and Rust WASM as fallback.

**Why**: 
- Javy installation can be problematic (requires specific version, PATH issues)
- Rust WASM works immediately without external dependencies
- Allows testing even when Javy is not available

**Implementation**:
```bash
# Check for javy, fallback to Rust WASM
if command -v javy &> /dev/null; then
    # Build TypeScript to WASM
    javy build dist/body.js -o wasm-modules/app.wasm
else
    # Build Rust WASM fallback
    cargo build --target wasm32-unknown-unknown --release
fi
```

### 2. Application-Level Deployment

**Pattern**: Deploy entire application as single WASM module, not individual actors.

**Why**:
- Matches framework's application-level deployment model
- Simpler deployment workflow
- Better for polyglot applications

**Implementation**:
- Package all actors + coordinator in one WASM module
- Use `DeployApplicationRequest` with `wasm_module` field
- Application starts all actors internally

### 3. Javy Installation

**Pattern**: Install Javy from GitHub releases, not cargo install.

**Why**:
- `cargo install --git` can fail or be slow
- Pre-built binaries are more reliable
- Easier to add to PATH

**Implementation**:
```bash
# Download from GitHub releases
curl -L https://github.com/bytecodealliance/javy/releases/download/v8.0.0/javy-arm-macos-v8.0.0.gz | gunzip > ~/.local/bin/javy
chmod +x ~/.local/bin/javy
export PATH="$PATH:$HOME/.local/bin"
```

**Note**: Use `javy build` command, not `javy compile` (v8.0.0+ uses `build`).

### 4. TypeScript Compilation

**Pattern**: Use CommonJS module format for Javy compatibility.

**Why**:
- Javy works better with CommonJS
- ES modules can cause issues

**Implementation**:
```json
{
  "compilerOptions": {
    "target": "ES2020",
    "module": "commonjs",
    "outDir": "./dist"
  }
}
```

### 5. .gitignore for WASM Examples

**Pattern**: Exclude all build artifacts, keep source only.

**What to ignore**:
```
# WASM build outputs
wasm-modules/*.wasm
!wasm-modules/.gitkeep

# TypeScript build outputs
ts-actors/dist/
ts-actors/node_modules/

# Rust build outputs
target/
Cargo.lock

# Logs
*.log
```

### 6. Test Scripts

**Pattern**: Create simple, comprehensive test scripts.

**Structure**:
1. Check prerequisites (with helpful error messages)
2. Install dependencies
3. Build (with fallback support)
4. Start node
5. Deploy application
6. Verify deployment
7. Cleanup (with timeout to prevent stalling)

**Cleanup Pattern**:
```bash
# Graceful shutdown with timeout
kill $NODE_PID 2>/dev/null || true
for i in {1..6}; do
    if ! ps -p $NODE_PID > /dev/null 2>&1; then
        break
    fi
    sleep 0.5
done
# Force kill if still running
if ps -p $NODE_PID > /dev/null 2>&1; then
    kill -9 $NODE_PID 2>/dev/null || true
fi
```

### 7. PATH Management

**Pattern**: Always check `~/.local/bin` for javy, add to PATH in scripts.

**Why**: Users often install tools to `~/.local/bin` but forget to add to PATH.

**Implementation**:
```bash
# Check for javy in PATH or common locations
JAVY_PATH=""
if command -v javy &> /dev/null; then
    JAVY_PATH="javy"
elif [ -f "$HOME/.local/bin/javy" ]; then
    JAVY_PATH="$HOME/.local/bin/javy"
    export PATH="$PATH:$HOME/.local/bin"
fi
```

### 8. Node Starter Binary

**Pattern**: Create simple node starter binary for testing.

**Why**: 
- Easier than configuring full node setup
- Good for examples and testing
- Can be run in background for E2E tests

**Implementation**:
- Simple binary that uses `NodeBuilder`
- Configurable via environment variables
- Handles graceful shutdown

## Common Issues and Solutions

### Issue: Javy not found
**Solution**: 
- Check `~/.local/bin` and add to PATH
- Provide clear installation instructions
- Use Rust WASM fallback

### Issue: `javy compile` command not found
**Solution**: 
- Use `javy build` (v8.0.0+)
- Update all scripts and documentation

### Issue: TypeScript compilation errors
**Solution**:
- Add `@types/node` for Node.js types
- Use CommonJS module format
- Remove Node.js-specific code from WASM actors

### Issue: Cleanup stalling
**Solution**:
- Use timeout loop instead of `wait`
- Force kill after timeout
- Check process status before waiting

### Issue: WASM files in git
**Solution**:
- Add comprehensive `.gitignore`
- Remove from git: `git rm --cached *.wasm`
- Add `.gitkeep` to preserve directory structure

## Checklist for New WASM Examples

- [ ] Create TypeScript/JavaScript actor source
- [ ] Add Rust WASM fallback actor
- [ ] Create build scripts (TypeScript + Rust)
- [ ] Add `.gitignore` for build artifacts
- [ ] Create test script with cleanup
- [ ] Add node starter binary (if needed)
- [ ] Document Javy installation from GitHub releases
- [ ] Test with and without Javy
- [ ] Verify no build artifacts in git
- [ ] Update EXAMPLES_SIMPLIFICATION_PLAN.md

## References

- [Javy GitHub Releases](https://github.com/bytecodealliance/javy/releases)
- [nbody-wasm Example](../nbody-wasm/README.md)
- [Application-Level Deployment](../../docs/APPLICATION_DEPLOYMENT.md)

