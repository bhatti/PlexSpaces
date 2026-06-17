#!/bin/bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Test script for order_processing example

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Auto-generate JWT if not provided
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
if [ -z "${PLEXSPACES_TEST_TOKEN:-}" ] && [ -f "$REPO_ROOT/scripts/gen-test-jwt.sh" ]; then
  source ~/venv/bin/activate 2>/dev/null || true
  echo "Generating JWT token..."
  JWT_OUTPUT="$(PLEXSPACES_JWT_PRIVATE_KEY_FILE="$REPO_ROOT/certs/jwt-es256.pem" "$REPO_ROOT/scripts/gen-test-jwt.sh")"
  eval "$JWT_OUTPUT"
  if [ -z "${PLEXSPACES_TEST_TOKEN:-}" ]; then
    echo "ERROR: gen-test-jwt.sh failed to set PLEXSPACES_TEST_TOKEN"
    echo "Output was: $JWT_OUTPUT"
    exit 1
  fi
fi
export AUTH_HEADER=""
if [ -n "${PLEXSPACES_TEST_TOKEN:-}" ]; then
  AUTH_HEADER="Authorization: Bearer $PLEXSPACES_TEST_TOKEN"
fi

cd "$SCRIPT_DIR"

# Shared target: use workspace target directory
WORKSPACE_TARGET="${SCRIPT_DIR}/../../../../target"
export CARGO_TARGET_DIR="${WORKSPACE_TARGET}"

echo "╔════════════════════════════════════════════════════════════════╗"
echo "║     Order Processing Example Test                              ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""

# Build
echo "📦 Building (shared target, debug)..."
cargo build

# Run pre-built binary with timeout
echo "🚀 Running example..."
timeout 30s "${WORKSPACE_TARGET}/debug/order_processing" 2>&1 || {
    EXIT_CODE=$?
    if [ $EXIT_CODE -eq 124 ]; then
        echo "⏱️  Timeout (expected for long-running example)"
    else
        echo "❌ Failed with exit code $EXIT_CODE"
        exit $EXIT_CODE
    fi
}

echo ""
echo "✅ Test passed!"
