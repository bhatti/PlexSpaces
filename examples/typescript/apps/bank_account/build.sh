#!/usr/bin/env bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Build TypeScript bank_account: compile to JS (for verification) and optionally to WASM (javy).
# Runtime currently expects WIT components (Python componentize-py); Javy WASM is not yet loadable.
# Verification: run ./test.sh (uses compiled JS in Node, no server).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo "Building TypeScript bank_account..."

# Ensure TypeScript is installed (npm install from package.json)
if [ ! -f "node_modules/.bin/tsc" ]; then
  echo "  Installing dependencies (typescript)..."
  npm install --no-audit --no-fund
fi

# Compile TypeScript to JavaScript (required for verify.mjs and optional for javy)
if [ -f "node_modules/.bin/tsc" ]; then
  ./node_modules/.bin/tsc -p .
else
  echo "  tsc not found after npm install."
  exit 1
fi

if [ ! -f "account_actor.js" ]; then
  echo "  Build failed: account_actor.js was not produced."
  exit 1
fi

echo "  account_actor.js (for Node verification)"

# Optional: build WASM with javy (not loadable by node yet; same API as Python when supported)
JAVY=""
if command -v javy &>/dev/null; then
  JAVY="javy"
elif [ -f "$HOME/.local/bin/javy" ]; then
  JAVY="$HOME/.local/bin/javy"
fi

if [ -n "$JAVY" ]; then
  if $JAVY build --help &>/dev/null 2>&1; then
    $JAVY build account_actor.js -o account_actor.wasm 2>/dev/null && echo "  account_actor.wasm (javy; node deploy uses Python component)" || true
  else
    $JAVY compile account_actor.js -o account_actor.wasm 2>/dev/null && echo "  account_actor.wasm (javy)" || true
  fi
fi

echo "Done. Run ./test.sh to verify."
