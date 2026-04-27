#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Start empty PlexSpaces node using repo scripts/server.sh (gRPC 8091, HTTP 8092).
# Run from repo root. Node runs in background; script exits when server is ready.

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../../" && pwd)"

cd "$REPO_ROOT"
if [ ! -f "scripts/server.sh" ]; then
  echo "ERROR: scripts/server.sh not found (run from repo root or bank_account directory)"
  exit 1
fi
chmod +x scripts/server.sh
exec ./scripts/server.sh
