#!/bin/bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Deploy WASM module to PlexSpaces framework node

set -euo pipefail

NODE_ADDR=""
MODULE_NAME=""
MODULE_VERSION=""
MODULE_FILE=""

usage() {
    cat <<EOF
Usage: $0 [OPTIONS]

Deploy a WASM module to a PlexSpaces framework node.

Options:
    --node ADDR          Node address (e.g., localhost:8000)
    --module NAME        Module name (e.g., rust-counter)
    --version VERSION    Module version (e.g., 1.0.0)
    --file PATH          Path to WASM module file
    -h, --help           Show this help message

Example:
    $0 --node localhost:8000 \\
       --module rust-counter \\
       --version 1.0.0 \\
       --file target/wasm32-wasi/release/rust_counter.wasm
EOF
    exit 1
}

while [[ $# -gt 0 ]]; do
    case $1 in
        --node)
            NODE_ADDR="$2"
            shift 2
            ;;
        --module)
            MODULE_NAME="$2"
            shift 2
            ;;
        --version)
            MODULE_VERSION="$2"
            shift 2
            ;;
        --file)
            MODULE_FILE="$2"
            shift 2
            ;;
        -h|--help)
            usage
            ;;
        *)
            echo "Unknown option: $1"
            usage
            ;;
    esac
done

if [[ -z "$NODE_ADDR" ]] || [[ -z "$MODULE_NAME" ]] || [[ -z "$MODULE_VERSION" ]] || [[ -z "$MODULE_FILE" ]]; then
    echo "Error: Missing required arguments"
    usage
fi

if [[ ! -f "$MODULE_FILE" ]]; then
    echo "Error: WASM file not found: $MODULE_FILE"
    exit 1
fi

echo "🚀 Deploying WASM module: $MODULE_NAME@$MODULE_VERSION"
echo "   Node: $NODE_ADDR"
echo "   File: $MODULE_FILE"

# Use grpcurl or plexspaces-cli to deploy
# For now, use a simple gRPC call (requires grpcurl or custom client)
if command -v grpcurl &> /dev/null; then
    # Read WASM file and encode as base64
    WASM_BYTES=$(base64 -w 0 < "$MODULE_FILE")
    
    # Create JSON request
    REQUEST_JSON=$(cat <<EOF
{
  "module": {
    "name": "$MODULE_NAME",
    "version": "$MODULE_VERSION",
    "module_bytes": "$WASM_BYTES"
  },
  "pre_warm": "NONE"
}
EOF
)
    
    # Deploy via gRPC
    grpcurl -plaintext \
        -d "$REQUEST_JSON" \
        "$NODE_ADDR" \
        plexspaces.wasm.v1.WasmRuntimeService/DeployWasmModule
else
    echo "Error: grpcurl not found. Please install grpcurl or use plexspaces-cli"
    echo "   Install: go install github.com/fullstorydev/grpcurl/cmd/grpcurl@latest"
    exit 1
fi

echo "✅ Module deployed successfully"

