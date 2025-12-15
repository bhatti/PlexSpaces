#!/usr/bin/env bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Build all WASM actors for PlexSpaces examples
#
# Usage: ./scripts/build-all-wasm.sh

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

echo "🔨 Building All WASM Actors"
echo "============================"
echo ""

# Color codes
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

# Ensure wasm32-unknown-unknown target is installed
echo "📦 Checking WASM target..."
if ! rustup target list | grep -q "wasm32-unknown-unknown (installed)"; then
    echo "Installing wasm32-unknown-unknown target..."
    rustup target add wasm32-unknown-unknown
fi
echo ""

# Build calculator WASM actor
echo -e "${YELLOW}1. Building calculator WASM actor${NC}"
cd "$PROJECT_ROOT/examples/wasm-calculator/wasm-actors"
cargo build --target wasm32-unknown-unknown --release
mkdir -p ../wasm-modules
cp target/wasm32-unknown-unknown/release/calculator_wasm_actor.wasm ../wasm-modules/
echo -e "${GREEN}✓${NC} Calculator WASM built: $(ls -lh ../wasm-modules/calculator_wasm_actor.wasm | awk '{print $5}')"
echo ""

# Build heat diffusion WASM actor
echo -e "${YELLOW}2. Building heat diffusion WASM actor${NC}"
cd "$PROJECT_ROOT/examples/heat_diffusion/wasm-actors"
cargo build --target wasm32-unknown-unknown --release
mkdir -p ../wasm-modules
cp target/wasm32-unknown-unknown/release/heat_diffusion_wasm_actor.wasm ../wasm-modules/
echo -e "${GREEN}✓${NC} Heat diffusion WASM built: $(ls -lh ../wasm-modules/heat_diffusion_wasm_actor.wasm | awk '{print $5}')"
echo ""

echo "============================"
echo -e "${GREEN}✓ All WASM actors built successfully!${NC}"
echo ""
echo "WASM modules:"
ls -lh "$PROJECT_ROOT"/examples/*/wasm-modules/*.wasm
