#!/usr/bin/env bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Test script for Session Manager app (debug build, run with timeout).
# Delegates to scripts/test.sh.

set -e
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"
exec bash scripts/test.sh
