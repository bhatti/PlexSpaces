#!/bin/bash
# Build and deploy N-Body WASM application

set -e

cd "$(dirname "$0")/.."

echo "╔════════════════════════════════════════════════════════════════╗"
echo "║     N-Body WASM - Complete Workflow                           ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""

# Step 1: Build
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Step 1: Building TypeScript to WASM"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
./scripts/build.sh

# Step 2: Start node (if not running)
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Step 2: Starting PlexSpaces node (if not running)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Note: Make sure a PlexSpaces node is running on localhost:9001"
echo "      You can start one with: cargo run --bin plexspaces-node"
echo ""

# Step 3: Deploy
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Step 3: Deploying WASM application"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
./scripts/deploy.sh

echo ""
echo "╔════════════════════════════════════════════════════════════════╗"
echo "║                    Deployment Complete                        ║"
echo "╚════════════════════════════════════════════════════════════════╝"

