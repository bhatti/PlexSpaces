#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Build TypeScript job_queue_outbox: compile to JS (for verification) and optionally to WASM.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo "Building TypeScript job_queue_outbox..."

# Ensure TypeScript is installed
if [ ! -f "node_modules/.bin/tsc" ]; then
  echo "  Installing dependencies (typescript)..."
  npm install --no-audit --no-fund
fi

# Compile TypeScript to JavaScript
if [ -f "node_modules/.bin/tsc" ]; then
  ./node_modules/.bin/tsc -p .
else
  echo "  tsc not found after npm install."
  exit 1
fi

if [ ! -f "job_queue_outbox_actor.js" ]; then
  echo "  Build failed: job_queue_outbox_actor.js was not produced."
  exit 1
fi

echo "  job_queue_outbox_actor.js (for Node verification)"

# Optional: build WASM with javy
JAVY=""
if command -v javy &>/dev/null; then
  JAVY="javy"
elif [ -f "$HOME/.local/bin/javy" ]; then
  JAVY="$HOME/.local/bin/javy"
fi

if [ -n "$JAVY" ]; then
  if $JAVY build --help &>/dev/null 2>&1; then
    $JAVY build job_queue_outbox_actor.js -o job_queue_outbox_actor.wasm 2>/dev/null && echo "  job_queue_outbox_actor.wasm (javy)" || true
  else
    $JAVY compile job_queue_outbox_actor.js -o job_queue_outbox_actor.wasm 2>/dev/null && echo "  job_queue_outbox_actor.wasm (javy)" || true
  fi
fi

echo "Done. Run ./test.sh to verify."
