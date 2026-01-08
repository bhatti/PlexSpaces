# WASM Test Fixtures

This directory contains pre-built WASM files used by integration tests.

## Purpose

These files are checked into git to avoid:
- Build dependencies during test execution
- Long waits for WASM compilation
- Test failures due to missing build artifacts

## Files

- `calculator_actor.wasm` - Pre-built calculator actor WASM component used by `wasm_component_integration.rs` tests

## Updating Fixtures

If the WASM files need to be updated:

1. Build the WASM file in the example directory:
   ```bash
   cd examples/simple/wasm_calculator
   ./scripts/build_python_actors.sh
   ```

2. Copy the updated file:
   ```bash
   cp examples/simple/wasm_calculator/wasm-modules/calculator_actor.wasm \
      crates/wasm-runtime/tests/fixtures/calculator_actor.wasm
   ```

3. Commit the updated fixture to git

## Note

These files should be checked into git (not in .gitignore) so tests can run without build dependencies.














