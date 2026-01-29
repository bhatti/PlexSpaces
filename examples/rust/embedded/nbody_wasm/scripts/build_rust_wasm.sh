#!/bin/bash
# Build Rust WASM actor as fallback when TypeScript/Javy is not available

set -e

cd "$(dirname "$0")/.."

echo "Building Rust WASM actor (fallback)..."
echo ""

# Check if wasm32 target is installed (try wasm32-wasip1 first, fallback to wasm32-unknown-unknown)
WASM_TARGET=""
if rustup target list --installed | grep -q "wasm32-wasip1"; then
    WASM_TARGET="wasm32-wasip1"
elif rustup target list --installed | grep -q "wasm32-unknown-unknown"; then
    WASM_TARGET="wasm32-unknown-unknown"
else
    echo "Installing wasm32-unknown-unknown target..."
    rustup target add wasm32-unknown-unknown
    WASM_TARGET="wasm32-unknown-unknown"
fi

echo "Using WASM target: $WASM_TARGET"

# Build WASM module
cd wasm-actors
cargo build --target $WASM_TARGET --release

# Copy to wasm-modules directory
mkdir -p ../wasm-modules
cp target/$WASM_TARGET/release/nbody_wasm_actor.wasm ../wasm-modules/nbody-application.wasm

WASM_SIZE=$(stat -f%z ../wasm-modules/nbody-application.wasm 2>/dev/null || stat -c%s ../wasm-modules/nbody-application.wasm 2>/dev/null)
echo ""
echo "✅ Rust WASM module built: wasm-modules/nbody-application.wasm ($WASM_SIZE bytes)"
echo ""
echo "Note: This is a Rust-based fallback. For TypeScript actors, install javy and use:"
echo "  ./scripts/build.sh"

