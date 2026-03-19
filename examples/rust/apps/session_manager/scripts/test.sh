#!/usr/bin/env bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Delegates to integration test (WASM deploy + node).

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec bash "$SCRIPT_DIR/../test.sh"
