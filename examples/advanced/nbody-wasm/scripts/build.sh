#!/bin/bash
# Build TypeScript actors to WASM

set -e

cd "$(dirname "$0")/.."

echo "╔════════════════════════════════════════════════════════════════╗"
echo "║     N-Body WASM - Build Script                                 ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""

# Check prerequisites
echo "Checking prerequisites..."
if ! command -v npm &> /dev/null; then
    echo "❌ npm not found. Please install Node.js and npm."
    exit 1
fi

if ! command -v javy &> /dev/null; then
    echo "❌ javy not found."
    echo ""
    echo "To install Javy, run:"
    echo "  cargo install --git https://github.com/bytecodealliance/javy javy-cli"
    echo ""
    echo "Or download from: https://github.com/bytecodealliance/javy/releases"
    exit 1
fi

if ! command -v tsc &> /dev/null; then
    echo "⚠️  TypeScript compiler not found globally. Installing locally..."
    cd ts-actors
    npm install
    cd ..
fi

echo "✅ Prerequisites check complete"
echo ""

# Build using cargo run
echo "Building TypeScript to WASM..."
cargo run --release --bin nbody-wasm -- build

echo ""
echo "✅ Build complete!"
echo ""
echo "WASM module location: wasm-modules/nbody-application.wasm"

