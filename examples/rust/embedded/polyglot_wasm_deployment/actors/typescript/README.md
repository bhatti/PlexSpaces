# TypeScript Actor Example

This directory contains a TypeScript actor example that can be deployed to Firecracker VMs.

## Building TypeScript to WASM

### Using Javy (Recommended)

Javy is a tool that compiles JavaScript/TypeScript to WebAssembly.

```bash
# Install Javy
cargo install javy-cli

# Compile TypeScript to JavaScript first
npx tsc greeter.ts --target ES2020 --module commonjs

# Compile JavaScript to WASM
javy compile greeter.js -o greeter.wasm
```

### Using AssemblyScript

AssemblyScript compiles a subset of TypeScript to WebAssembly.

```bash
# Install AssemblyScript
npm install -g assemblyscript

# Compile
asc greeter.ts --target release --outFile greeter.wasm
```

### Using componentize-py (for Python-like syntax)

If you want Python-like syntax that compiles to WASM:

```bash
# Install componentize-py
pip install componentize-py

# Compile
componentize-py -d greeter.wit -w greeter.wasm greeter.ts
```

## Usage

Once compiled to WASM, you can deploy the actor using the polyglot deployment example:

```bash
cd ../../..
cargo run --release -- empty-vm-typescript \
  --wasm actors/typescript/greeter.wasm \
  --operations 10
```

## Notes

- The example TypeScript code is provided for demonstration
- In production, you would use a proper TypeScript-to-WASM compiler
- The WASM module should implement the PlexSpaces actor interface
- See the main README for deployment instructions

