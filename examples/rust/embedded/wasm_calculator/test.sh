#!/bin/bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Test script for wasm_calculator example
# This is a wrapper that calls the comprehensive test script

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# Call the comprehensive test script
exec "$SCRIPT_DIR/scripts/test.sh"






















































