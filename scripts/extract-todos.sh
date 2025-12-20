#!/bin/bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Extract TODO/FIXME/XXX/HACK comments from Rust source files

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
OUTPUT_FILE="$WORKSPACE_ROOT/PHASE1_TODOS_EXTRACTED.md"

echo "# Extracted TODO/FIXME/XXX/HACK Comments"
echo ""
echo "**Generated**: $(date -u +"%Y-%m-%d %H:%M:%S UTC")"
echo "**Source**: All \`.rs\` files in workspace"
echo ""
echo "---"
echo ""

# Extract TODOs with file path and line number
grep -rn "TODO\|FIXME\|XXX\|HACK" --include="*.rs" "$WORKSPACE_ROOT" \
    | grep -v "target/" \
    | grep -v "\.git/" \
    | sort \
    | while IFS= read -r line; do
    # Format: file:line:content
    file=$(echo "$line" | cut -d: -f1 | sed "s|^$WORKSPACE_ROOT/||")
    line_num=$(echo "$line" | cut -d: -f2)
    content=$(echo "$line" | cut -d: -f3-)
    
    # Determine type
    if echo "$content" | grep -qi "TODO"; then
        type="TODO"
    elif echo "$content" | grep -qi "FIXME"; then
        type="FIXME"
    elif echo "$content" | grep -qi "XXX"; then
        type="XXX"
    elif echo "$content" | grep -qi "HACK"; then
        type="HACK"
    else
        type="UNKNOWN"
    fi
    
    echo "### $type: \`$file:$line_num\`"
    echo ""
    echo "\`\`\`rust"
    echo "$content"
    echo "\`\`\`"
    echo ""
    echo "---"
    echo ""
done > "$OUTPUT_FILE"

echo "✅ Extracted TODOs to: $OUTPUT_FILE"
echo ""
echo "Total lines: $(wc -l < "$OUTPUT_FILE")"













































