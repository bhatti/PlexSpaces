#!/bin/bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

set -e

echo "╔════════════════════════════════════════════════════════════════╗"
echo "║  Time-Series Forecasting Example - Single Node                ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""

cd "$(dirname "$0")/.."

echo "📦 Building timeseries_forecasting..."
cargo build --release

echo ""
echo "🚀 Running timeseries_forecasting..."
echo ""

cargo run --release

echo ""
echo "✅ Example completed"

