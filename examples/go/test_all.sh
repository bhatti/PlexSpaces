#!/usr/bin/env bash
# Run all Go examples in order. Stops on first failure unless -c is given.
#
# Usage:
#   ./test_all.sh            # run all, stop on first failure
#   ./test_all.sh -c         # continue from last failed example
#   ./test_all.sh --from name # start from specific example name
#
# Each example that targets two nodes receives "8091 8094" as arguments
# (detected by grep for port 8094 in the test.sh). Single-node examples
# receive "8091".

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
STATE_FILE="$SCRIPT_DIR/.test_state"
APPS_DIR="$SCRIPT_DIR/apps"

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

mapfile -t EXAMPLES < <(find "$APPS_DIR" -maxdepth 1 -mindepth 1 -type d | sort | xargs -I{} basename {})

passed=0
failed=0
skipped=0
total_start=$SECONDS
skip_until=""
[[ -n "$START_FROM" ]] && skip_until="$START_FROM"

for name in "${EXAMPLES[@]}"; do
    test_script="$APPS_DIR/$name/test.sh"
    [[ -f "$test_script" ]] || continue

    if [[ -n "$skip_until" ]]; then
        if [[ "$name" == "$skip_until" ]]; then
            skip_until=""
        else
            ((skipped++)) || true
            echo -e "${YELLOW}SKIP${NC} $name (resuming)"
            continue
        fi
    fi

    # Save state before running
    printf 'LAST_EXAMPLE=%s\nLAST_STATUS=RUNNING\n' "$name" > "$STATE_FILE"

    # Detect multi-node example
    if grep -q "8094" "$test_script" 2>/dev/null; then
        args="8091 8094"
    else
        args="8091"
    fi

    start=$SECONDS
    bash "$test_script" $args >/tmp/test_all_go_out.txt 2>&1 &
    test_pid=$!
    spinner_chars='|/-\'
    spinner_idx=0
    while kill -0 "$test_pid" 2>/dev/null; do
        elapsed_now=$((SECONDS - start))
        last_line="$(tail -1 /tmp/test_all_go_out.txt 2>/dev/null | tr -d '\r\n' | cut -c1-60)"
        printf '\r%-45s %s %3ds  %-60s' "$name" "${spinner_chars:$((spinner_idx % 4)):1}" "$elapsed_now" "$last_line"
        spinner_idx=$((spinner_idx + 1))
        sleep 1
    done
    wait "$test_pid"
    test_rc=$?
    elapsed=$((SECONDS - start))
    if [[ $test_rc -eq 0 ]]; then
        printf '\r%-45s %b\n' "$name" "${GREEN}PASS${NC} (${elapsed}s)"
        ((passed++)) || true
        printf 'LAST_EXAMPLE=%s\nLAST_STATUS=PASSED\n' "$name" > "$STATE_FILE"
    else
        printf '\r%-45s %b\n' "$name" "${RED}FAIL${NC} (${elapsed}s)"
        echo "    --- last 20 lines ---"
        tail -20 /tmp/test_all_go_out.txt | sed 's/^/    /'
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
