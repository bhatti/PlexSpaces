#!/bin/bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Deploy WASM application to test nodes for dashboard testing

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
NODE_URL="${1:-http://localhost:8000}"
APP_NAME="${2:-wasm-calculator}"
WASM_FILE="${3:-}"

# gRPC and HTTP share a single port — extract host:port directly
NODE_GRPC=$(echo "$NODE_URL" | sed 's|http://||' | sed 's|https://||')

echo "🚀 Deploying WASM application: ${APP_NAME}"
echo "   Node: ${NODE_URL} (gRPC + HTTP single port)"

# If WASM file not provided, try to find or build one
if [ -z "$WASM_FILE" ]; then
    # Try to find existing WASM file
    if [ -f "examples/simple/wasm_calculator/target/wasm32-wasi/release/wasm_calculator.wasm" ]; then
        WASM_FILE="examples/simple/wasm_calculator/target/wasm32-wasi/release/wasm_calculator.wasm"
        echo "📦 Using existing WASM file: ${WASM_FILE}"
    elif [ -f "examples/wasm_showcase/actors/python/counter_actor.py" ]; then
        echo "📦 Building WASM from Python actor..."
        cd examples/wasm_showcase
        ./scripts/build_python_actors.sh || {
            echo "⚠️  Could not build Python actors, trying wasm-calculator..."
            cd "$SCRIPT_DIR/.."
            cd examples/simple/wasm_calculator
            cargo build --release --target wasm32-wasi || {
                echo "❌ Could not build WASM. Please provide WASM file path."
                echo "Usage: $0 <node_url> <app_name> <wasm_file_path>"
                exit 1
            }
            cd "$SCRIPT_DIR/.."
            WASM_FILE="examples/simple/wasm_calculator/target/wasm32-wasi/release/wasm_calculator.wasm"
        }
        cd "$SCRIPT_DIR/.."
    else
        echo "📦 Building wasm-calculator..."
        cd examples/simple/wasm_calculator
        cargo build --release --target wasm32-wasi || {
            echo "❌ Could not build WASM. Please provide WASM file path."
            echo "Usage: $0 <node_url> <app_name> <wasm_file_path>"
            exit 1
        }
        cd "$SCRIPT_DIR/.."
        WASM_FILE="examples/simple/wasm_calculator/target/wasm32-wasi/release/wasm_calculator.wasm"
    fi
fi

if [ ! -f "$WASM_FILE" ]; then
    echo "❌ WASM file not found: ${WASM_FILE}"
    exit 1
fi

echo "📦 WASM file: ${WASM_FILE}"
WASM_SIZE=$(stat -f%z "$WASM_FILE" 2>/dev/null || stat -c%s "$WASM_FILE" 2>/dev/null)
echo "   Size: ${WASM_SIZE} bytes"

# Encode WASM file to base64
echo "📤 Encoding WASM file..."
WASM_BASE64=$(base64 -w 0 < "$WASM_FILE" 2>/dev/null || base64 < "$WASM_FILE" | tr -d '\n')

# Check if grpcurl is available
if command -v grpcurl &> /dev/null; then
    echo "📤 Deploying via gRPC (grpcurl)..."
    
    # Create JSON request
    REQUEST_JSON=$(cat <<EOF
{
  "application_id": "${APP_NAME}",
  "name": "${APP_NAME}",
  "version": "1.0.0",
  "wasm_module": {
    "name": "${APP_NAME}",
    "version": "1.0.0",
    "module_bytes": "${WASM_BASE64}",
    "module_hash": "",
    "wit_interface": ""
  }
}
EOF
)
    
    # Deploy via gRPC
    RESPONSE=$(grpcurl -plaintext \
        -d "$REQUEST_JSON" \
        "${NODE_GRPC}" \
        plexspaces.application.v1.ApplicationService/DeployApplication 2>&1) || {
        echo "❌ Deployment failed via gRPC"
        echo "Response: $RESPONSE"
        echo ""
        echo "💡 Alternative: Use HTTP API if gRPC-Gateway is enabled"
        exit 1
    }
    
    echo "$RESPONSE" | jq '.' 2>/dev/null || echo "$RESPONSE"
    echo "✅ Deployment successful via gRPC!"
    
elif command -v curl &> /dev/null; then
    echo "📤 Deploying via HTTP API (gRPC-Gateway)..."
    
    # Try HTTP API (requires gRPC-Gateway)
    RESPONSE=$(curl -s -X POST "${NODE_URL}/api/v1/applications/deploy" \
        -H "Content-Type: application/json" \
        -d "{
            \"application_id\": \"${APP_NAME}\",
            \"name\": \"${APP_NAME}\",
            \"version\": \"1.0.0\",
            \"wasm_module\": {
                \"name\": \"${APP_NAME}\",
                \"version\": \"1.0.0\",
                \"module_bytes\": \"${WASM_BASE64}\",
                \"module_hash\": \"\",
                \"wit_interface\": \"\"
            }
        }" 2>&1) || {
        echo "❌ Deployment failed via HTTP"
        echo "Response: $RESPONSE"
        echo ""
        echo "💡 Install grpcurl for gRPC deployment:"
        echo "   brew install grpcurl  # macOS"
        echo "   or download from: https://github.com/fullstorydev/grpcurl"
        exit 1
    }
    
    echo "$RESPONSE" | jq '.' 2>/dev/null || echo "$RESPONSE"
    
    # Check if deployment was successful
    if echo "$RESPONSE" | grep -q "success\|application_id"; then
        echo "✅ Deployment successful via HTTP!"
    else
        echo "⚠️  Deployment response unclear. Check node logs."
    fi
else
    echo "❌ Neither grpcurl nor curl found. Cannot deploy."
    echo "   Install one of:"
    echo "     - grpcurl: brew install grpcurl"
    echo "     - curl: Usually pre-installed"
    exit 1
fi

echo ""
echo "✅ Deployment complete!"
echo ""
echo "🔍 Verify deployment:"
echo "   curl -s ${NODE_URL}/api/v1/dashboard/applications | jq '.applications[] | select(.name == \"${APP_NAME}\")'"
echo ""
echo "📊 View in dashboard:"
echo "   ${NODE_URL}/"
echo "   ${NODE_URL}/node/node-1"
echo ""
echo "📋 Check application status:"
echo "   kubectl logs -n plexspaces-test deployment/plexspaces-node-1 | grep -i '${APP_NAME}'"




