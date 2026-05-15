#!/bin/bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Check for duplicate dependencies across crates
# Helps identify opportunities to consolidate dependencies
# Compatible with bash 3.x (macOS default)

set -e

echo "🔍 Checking for duplicate dependencies across crates..."
echo ""

# Colors for output
RED='\033[0;31m'
YELLOW='\033[1;33m'
GREEN='\033[0;32m'
NC='\033[0m' # No Color

# Temporary file for storing dependency counts
TMP_FILE=$(mktemp /tmp/dep-check.XXXXXX)
trap "rm -f $TMP_FILE" EXIT

# Find all Cargo.toml files in crates
CARGO_FILES=$(find crates -name "Cargo.toml" -type f | sort)
TOTAL_CRATES=$(echo "$CARGO_FILES" | wc -l | tr -d ' ')

# Extract all dependencies and count occurrences
while IFS= read -r cargo_file; do
    crate_name=$(basename $(dirname "$cargo_file"))
    
    # Extract dependencies from [dependencies] and [dev-dependencies] sections only
    # Skip workspace dependencies (plexspaces-*) and metadata fields
    in_deps_section=0
    in_dev_deps_section=0
    
    while IFS= read -r line; do
        # Check if we're entering a dependencies section
        if echo "$line" | grep -qE "^\s*\[dependencies\]"; then
            in_deps_section=1
            in_dev_deps_section=0
            continue
        fi
        if echo "$line" | grep -qE "^\s*\[dev-dependencies\]"; then
            in_deps_section=0
            in_dev_deps_section=1
            continue
        fi
        # Check if we're leaving a dependencies section (entering another section)
        if echo "$line" | grep -qE "^\s*\["; then
            in_deps_section=0
            in_dev_deps_section=0
            continue
        fi
        
        # Only process lines within dependencies sections
        if [ "$in_deps_section" -eq 1 ] || [ "$in_dev_deps_section" -eq 1 ]; then
            # Extract dependency name (skip comments and empty lines)
            dep=$(echo "$line" | grep -E "^\s*[a-zA-Z0-9_-]+\s*=" | sed 's/=.*//' | sed 's/^[[:space:]]*//' | sed 's/[[:space:]]*$//')
            if [ -n "$dep" ] && [[ ! "$dep" =~ ^# ]] && [[ ! "$dep" =~ ^plexspaces- ]]; then
                # Filter out common metadata fields that might appear in dependencies sections
                if [[ ! "$dep" =~ ^(name|version|edition|authors|license|repository|keywords|categories|description|path|default|optional|features|workspace)$ ]]; then
                    echo "$dep|$crate_name" >> "$TMP_FILE"
                fi
            fi
        fi
    done < "$cargo_file"
done <<< "$CARGO_FILES"

# Count occurrences and group by dependency
echo "Dependencies used in 3+ crates (potential candidates for workspace.dependencies):"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

DUPLICATES=0

# Sort and count
sort "$TMP_FILE" | cut -d'|' -f1 | uniq -c | sort -rn | while read count dep; do
    if [ "$count" -ge 3 ]; then
        # Get list of crates using this dependency
        crates=$(grep "^$dep|" "$TMP_FILE" | cut -d'|' -f2 | sort -u | tr '\n' ',' | sed 's/,$//' | sed 's/,/, /g')
        percentage=$(awk "BEGIN {printf \"%.0f\", ($count / $TOTAL_CRATES) * 100}")
        
        if [ "$count" -ge 10 ]; then
            echo -e "${RED}⚠️  $dep${NC} - Used in $count crates ($percentage% of workspace)"
        elif [ "$count" -ge 5 ]; then
            echo -e "${YELLOW}   $dep${NC} - Used in $count crates ($percentage% of workspace)"
        else
            echo -e "${GREEN}   $dep${NC} - Used in $count crates ($percentage% of workspace)"
        fi
        echo "      Crates: $crates"
        echo ""
        DUPLICATES=$((DUPLICATES + 1))
    fi
done

if [ $DUPLICATES -eq 0 ]; then
    echo -e "${GREEN}✓ No duplicate dependencies found (all used in < 3 crates)${NC}"
else
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo ""
    echo "Summary:"
    echo "  • Found $DUPLICATES dependencies used in 3+ crates"
    echo "  • Consider moving frequently-used dependencies to workspace.dependencies"
    echo "  • This reduces duplication and ensures version consistency"
    echo ""
    echo "To add to workspace.dependencies, add entries like:"
    echo "  [workspace.dependencies]"
    echo "  tokio = { version = \"1.35\", features = [\"full\"] }"
    echo ""
    echo "Then in crate Cargo.toml, use:"
    echo "  tokio = { workspace = true }"
fi

