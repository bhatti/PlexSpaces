#!/usr/bin/env bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Full E2E test: start node (scripts/server.sh), build TS→WASM, deploy, HTTP tests. No Python.
# See scripts/ (start_server.sh, build.sh, deploy.sh, test_e2e.sh) – pattern from nbody_wasm.

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

chmod +x scripts/test_e2e.sh 2>/dev/null || true
exec ./scripts/test_e2e.sh
