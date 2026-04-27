#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Delegates to integration test (WASM deploy + node).

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec bash "$SCRIPT_DIR/../test.sh"
