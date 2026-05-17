#!/usr/bin/env bash
# Run all Rust examples in order. Stops on first failure unless -c is given.
#
# Usage:
#   ./test_all.sh            # run all, stop on first failure
#   ./test_all.sh -c         # continue from last failed example
#   ./test_all.sh --from name # start from specific example name
#
# Scans both apps/ and embedded/ directories for test.sh files.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
STATE_FILE="$SCRIPT_DIR/.test_state"

GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m'

CONTINUE_MODE=false
START_FROM=""
while [[ $# -gt 0 ]]; do
    case "$1" in
        -c) CONTINUE_MODE=true; shift ;;
        --from) START_FROM="$2"; shift 2 ;;
        *) echo "Unknown option: $1" >&2; exit 1 ;;
    esac
done

if $CONTINUE_MODE && [[ -f "$STATE_FILE" ]]; then
    # shellcheck disable=SC1090
    source "$STATE_FILE"
    START_FROM="${LAST_EXAMPLE:-}"
fi

# Collect all example dirs (apps/ and embedded/ sub-dirs) that have test.sh or scripts/test.sh
EXAMPLES=()
for dir in "$SCRIPT_DIR"/apps/*/; do
    name="apps/$(basename "$dir")"
    if [[ -f "$dir/test.sh" ]]; then
        EXAMPLES+=("$name")
    fi
done
for dir in "$SCRIPT_DIR"/embedded/*/; do
    name="embedded/$(basename "$dir")"
    if [[ -f "$dir/scripts/test.sh" ]] || [[ -f "$dir/test.sh" ]]; then
        EXAMPLES+=("$name")
    fi
done
IFS=$'\n' EXAMPLES=($(sort <<<"${EXAMPLES[*]}")); unset IFS

passed=0
failed=0
skipped=0
total_start=$SECONDS
skip_until=""
[[ -n "$START_FROM" ]] && skip_until="$START_FROM"

for rel_path in "${EXAMPLES[@]}"; do
    dir="$SCRIPT_DIR/$rel_path"
    name="$(basename "$dir")"
    if [[ -f "$dir/scripts/test.sh" ]]; then
        test_script="$dir/scripts/test.sh"
    else
        test_script="$dir/test.sh"
    fi
    [[ -f "$test_script" ]] || continue

    display_name="$rel_path"

    if [[ -n "$skip_until" ]]; then
        if [[ "$name" == "$skip_until" || "$rel_path" == "$skip_until" ]]; then
            skip_until=""
        else
            ((skipped++)) || true
            echo -e "${YELLOW}SKIP${NC} $display_name (resuming)"
            continue
        fi
    fi

    printf 'LAST_EXAMPLE=%s\nLAST_STATUS=RUNNING\n' "$name" > "$STATE_FILE"

    if grep -q "8094" "$test_script" 2>/dev/null; then
        args="8091 8094"
    else
        args="8091"
    fi

    start=$SECONDS
    bash "$test_script" $args >/tmp/test_all_rust_out.txt 2>&1 &
    test_pid=$!
    spinner_chars='|/-\'
    spinner_idx=0
    while kill -0 "$test_pid" 2>/dev/null; do
        elapsed_now=$((SECONDS - start))
        last_line="$(tail -1 /tmp/test_all_rust_out.txt 2>/dev/null | tr -d '\r\n' | cut -c1-60)"
        printf '\r%-50s %s %3ds  %-60s' "$display_name" "${spinner_chars:$((spinner_idx % 4)):1}" "$elapsed_now" "$last_line"
        spinner_idx=$((spinner_idx + 1))
        sleep 1
    done
    wait "$test_pid"
    test_rc=$?
    elapsed=$((SECONDS - start))
    if [[ $test_rc -eq 0 ]]; then
        printf '\r%-50s %b\n' "$display_name" "${GREEN}PASS${NC} (${elapsed}s)"
        ((passed++)) || true
        printf 'LAST_EXAMPLE=%s\nLAST_STATUS=PASSED\n' "$name" > "$STATE_FILE"
    else
        printf '\r%-50s %b\n' "$display_name" "${RED}FAIL${NC} (${elapsed}s)"
        echo "    --- last 20 lines ---"
        tail -20 /tmp/test_all_rust_out.txt | sed 's/^/    /'
        ((failed++)) || true
        printf 'LAST_EXAMPLE=%s\nLAST_STATUS=FAILED\n' "$name" > "$STATE_FILE"
        if ! $CONTINUE_MODE; then
            echo -e "\n${RED}Stopped on first failure. Use -c to continue.${NC}"
            echo -e "Total: ${GREEN}${passed} passed${NC}, ${RED}${failed} failed${NC}, ${YELLOW}${skipped} skipped${NC} — $((SECONDS - total_start))s"
            exit 1
        fi
    fi
done

total=$((SECONDS - total_start))
echo ""
echo -e "Total: ${GREEN}${passed} passed${NC}, ${RED}${failed} failed${NC}, ${YELLOW}${skipped} skipped${NC} — ${total}s"
[[ $failed -eq 0 ]]
