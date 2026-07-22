#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Minimal test to reproduce WASM recursion issue quickly
# Run: ./test-minimal.sh [port]

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../../" && pwd)"
PORT="${1:-8001}"
BASE_URL="http://localhost:${PORT}"

cd "$SCRIPT_DIR"

echo "=== Minimal WASM Recursion Test ==="
echo ""

# Step 1: Build WASM
echo "Step 1: Building WASM component..."
if ! ./build.sh > /dev/null 2>&1; then
  echo "  ❌ Build failed"
  exit 1
fi
echo "  ✓ Build succeeded"

# Step 2: Deploy
echo ""
echo "Step 2: Deploying WASM component..."
WASM_FILE="$SCRIPT_DIR/batch_predictor_actor.wasm"
if [ ! -f "$WASM_FILE" ]; then
  echo "  ❌ WASM file not found: $WASM_FILE"
  exit 1
fi

# Try application-spec.toml first, fallback to app-config.toml
CONFIG_FILE="${SCRIPT_DIR}/application-spec.toml"
if [ ! -f "$CONFIG_FILE" ]; then
  CONFIG_FILE="${SCRIPT_DIR}/app-config.toml"
fi

APP_ID="orleans-batch-predictor"
APP_NAME="orleans-batch-predictor"

trap 'rm -f "${APP_ZIP:-}"' EXIT
APP_ZIP="$(mktemp /tmp/app_XXXXXX.zip)"
if [ -f "$CONFIG_FILE" ]; then
  zip -j "$APP_ZIP" "${WASM_FILE}" "${CONFIG_FILE}" >/dev/null
else
  zip -j "$APP_ZIP" "${WASM_FILE}" >/dev/null
fi
DEPLOY_RESPONSE=$(curl -s -X POST \
  -F "application_id=${APP_ID}" \
  -F "name=${APP_NAME}" \
  -F "version=1.0.0" \
  -F "app_file=@${APP_ZIP}" \
  "${BASE_URL}/api/v1/applications/deploy" 2>&1)

if echo "$DEPLOY_RESPONSE" | grep -qi "error\|failed" || [ -z "$DEPLOY_RESPONSE" ]; then
  echo "  ❌ Deployment failed: $DEPLOY_RESPONSE"
  exit 1
fi
echo "  ✓ Deployed"

# Step 3: Test with minimal payload
echo ""
echo "Step 3: Testing with minimal payload (should NOT reproduce recursion)..."
ACTOR_ID="batch-predictor-model-1"

PAYLOAD='{"op":"predict_batch","shard_path":"test","data":[{"id":"1","features":[1,2]}]}'

RESPONSE=$(curl -s -w "\n%{http_code}" -X POST \
  -H "Content-Type: application/json" \
  -d "$PAYLOAD" \
  "${BASE_URL}/api/v1/actors/${APP_NAME}/${ACTOR_ID}" 2>&1)

HTTP_CODE=$(echo "$RESPONSE" | tail -n1)
BODY=$(echo "$RESPONSE" | head -n-1)

if [ "$HTTP_CODE" != "200" ]; then
  echo "  ❌ Request failed (HTTP $HTTP_CODE)"
  echo "  Response: $BODY"
  echo ""
  echo "  Check server logs for WASM backtrace:"
  echo "  tail -50 $REPO_ROOT/plexspaces-node.log | grep -A 10 'wasm backtrace'"
  exit 1
fi

echo "  ✓ Request succeeded"
echo "  Response: ${BODY:0:200}..."

# Step 4: Check for recursion in logs
echo ""
echo "Step 4: Checking for recursion pattern in logs..."
if tail -100 "$REPO_ROOT/plexspaces-node.log" | grep -q "6188.*6189.*6192"; then
  echo "  ⚠️  Recursion pattern detected (6188→6189→6192)"
  echo "  This indicates WASM recursion issue"
  exit 1
else
  echo "  ✓ No recursion pattern detected"
fi

echo ""
echo "✅ Test completed"
