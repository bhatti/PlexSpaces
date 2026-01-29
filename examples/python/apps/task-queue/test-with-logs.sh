#!/bin/bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Test script that shows both test output and server logs
# This script helps verify that server-side logs are appearing

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
HTTP_PORT="${1:-8094}"

echo "╔════════════════════════════════════════════════════════════════╗"
echo "║   Task Queue Test with Server Log Verification                ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""
echo "This script runs the test and shows server logs."
echo ""
echo "Prerequisites:"
echo "  1. Node must be running with: RUST_LOG=info cargo run -p plexspaces-cli -- start --node-id test-node --listen-addr 0.0.0.0:8093"
echo "  2. Or: RUST_LOG=plexspaces_facet=info cargo run -p plexspaces-cli -- start --node-id test-node --listen-addr 0.0.0.0:8093"
echo ""
echo "Press Enter to continue or Ctrl+C to cancel..."
read

# Run the test
"$SCRIPT_DIR/test.sh" "$HTTP_PORT"

echo ""
echo "╔════════════════════════════════════════════════════════════════╗"
echo "║   Expected Server Logs (check your node terminal)             ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""
echo "You should see logs like:"
echo ""
echo "  INFO 🔒 LockFacet: Lock acquired lock_key=\"job:job-1\" holder_id=\"worker-1\" version=\"01J...\" lease_secs=300"
echo "  WARN ⚔️  LockFacet: Try acquire failed - lock already held by another worker lock_key=\"job:job-1\" holder_id=\"worker-2\" current_holder=\"worker-1\""
echo "  INFO 🔄 LockFacet: Lock renewed (heartbeat) lock_key=\"job:job-1\" holder_id=\"worker-1\" version=\"01J...\" lease_secs=300"
echo "  INFO 🔓 LockFacet: Lock released lock_key=\"job:job-1\" holder_id=\"worker-1\""
echo ""
echo "If you don't see these logs, check:"
echo "  1. Node is running with RUST_LOG=info or RUST_LOG=plexspaces_facet=info"
echo "  2. Logs are not being filtered out"
echo "  3. Node terminal is visible"
echo ""
