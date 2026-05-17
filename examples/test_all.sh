#!/usr/bin/env bash
# Run all PlexSpaces examples across all language suites.
#
# For each example: runs build.sh (if present), undeploy.sh (if present, to clean
# prior state), then test.sh. Stops on first failure unless -c is given.
#
# Usage:
#   ./test_all.sh               # run all examples, stop on first failure
#   ./test_all.sh -c            # continue from last failed example
#   ./test_all.sh go            # run only go examples
#   ./test_all.sh go python     # run go then python
#   ./test_all.sh go -c         # run go suite, continue from last failed

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
STATE_FILE="$SCRIPT_DIR/.test_state"

GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m'

CONTINUE_MODE=false
LANG_FILTER=()
while [[ $# -gt 0 ]]; do
    case "$1" in
        -c) CONTINUE_MODE=true; shift ;;
        go|python|rust|typescript) LANG_FILTER+=("$1"); shift ;;
        *) echo "Unknown option: $1" >&2; exit 1 ;;
    esac
done

ALL_LANGS=(go python rust typescript)
if [[ ${#LANG_FILTER[@]} -gt 0 ]]; then
    LANGS=("${LANG_FILTER[@]}")
else
    LANGS=("${ALL_LANGS[@]}")
fi

# Load resume state
START_FROM=""
if $CONTINUE_MODE && [[ -f "$STATE_FILE" ]]; then
    source "$STATE_FILE" 2>/dev/null || true
    START_FROM="${LAST_KEY:-}"
fi

passed=0
failed=0
skipped=0
total_start=$SECONDS

run_example() {
    local lang="$1"
    local name="$2"
    local dir="$3"
    local key="${lang}/${name}"

    local test_script="$dir/test.sh"
    [[ -f "$test_script" ]] || return 0

    # Detect multi-node example
    local args="8091"
    if grep -q "8094" "$test_script" 2>/dev/null; then
        args="8091 8094"
    fi

    local safe_name="${name//\//_}"
    local log="/tmp/test_all_${lang}_${safe_name}.txt"
    local start=$SECONDS

    printf 'LAST_KEY=%s\nLAST_LANG=%s\nLAST_EXAMPLE=%s\nLAST_STATUS=RUNNING\n' \
        "$key" "$lang" "$name" > "$STATE_FILE"

    (
        cd "$dir"
        # Build
        if [[ -f "build.sh" ]]; then
            bash build.sh 2>&1 || { echo "BUILD FAILED"; exit 1; }
        fi
        # Undeploy prior state
        if [[ -f "undeploy.sh" ]]; then
            bash undeploy.sh $args >/dev/null 2>&1 || true
        fi
        # Run test
        bash test.sh $args
    ) >"$log" 2>&1 &
    local test_pid=$!

    local spinner_chars='|/-\'
    local spinner_idx=0
    while kill -0 "$test_pid" 2>/dev/null; do
        local elapsed_now=$((SECONDS - start))
        local last_line
        last_line="$(tail -1 "$log" 2>/dev/null | tr -d '\r\n' | cut -c1-55)"
        printf '\r  %-50s %s %3ds  %-55s' "${lang}/${name}" "${spinner_chars:$((spinner_idx % 4)):1}" "$elapsed_now" "$last_line"
        spinner_idx=$((spinner_idx + 1))
        sleep 1
    done
    wait "$test_pid"
    local rc=$?
    local elapsed=$((SECONDS - start))

    if [[ $rc -eq 0 ]]; then
        printf '\r  %-50s %b\n' "${lang}/${name}" "${GREEN}PASS${NC} (${elapsed}s)"
        ((passed++)) || true
        printf 'LAST_KEY=%s\nLAST_LANG=%s\nLAST_EXAMPLE=%s\nLAST_STATUS=PASSED\n' \
            "$key" "$lang" "$name" > "$STATE_FILE"
    else
        printf '\r  %-50s %b\n' "${lang}/${name}" "${RED}FAIL${NC} (${elapsed}s)"
        echo "    --- last 20 lines ---"
        tail -20 "$log" | sed 's/^/    /'
        ((failed++)) || true
        printf 'LAST_KEY=%s\nLAST_LANG=%s\nLAST_EXAMPLE=%s\nLAST_STATUS=FAILED\n' \
            "$key" "$lang" "$name" > "$STATE_FILE"
        echo -e "\n${RED}Stopped at ${key}. Use -c to resume from here.${NC}"
        echo -e "${GREEN}${passed} passed${NC}, ${RED}${failed} failed${NC}, ${YELLOW}${skipped} skipped${NC} — $((SECONDS - total_start))s"
        exit 1
    fi
}

collect_examples() {
    local lang="$1"
    local base="$SCRIPT_DIR/$lang"

    # apps/ directory (go, python, typescript, rust/apps)
    if [[ -d "$base/apps" ]]; then
        while IFS= read -r d; do
            echo "apps/$(basename "$d"):$d"
        done < <(find "$base/apps" -maxdepth 1 -mindepth 1 -type d | sort)
    fi

    # rust embedded/ directory
    if [[ -d "$base/embedded" ]]; then
        while IFS= read -r d; do
            echo "embedded/$(basename "$d"):$d"
        done < <(find "$base/embedded" -maxdepth 1 -mindepth 1 -type d | sort)
    fi
}

for lang in "${LANGS[@]}"; do
    echo ""
    echo "── $lang ──────────────────────────────────────────"

    while IFS=: read -r rel_name dir; do
        name="$rel_name"
        key="${lang}/${name}"

        [[ -f "$dir/test.sh" ]] || continue

        # Skip examples before resume point
        if [[ -n "$START_FROM" ]]; then
            if [[ "$key" == "$START_FROM" ]]; then
                START_FROM=""  # found it, start running from here
            else
                ((skipped++)) || true
                printf "  %-50s ${YELLOW}SKIP${NC}\n" "$key"
                continue
            fi
        fi

        run_example "$lang" "$name" "$dir"

    done < <(collect_examples "$lang")
done

total=$((SECONDS - total_start))
echo ""
echo -e "${GREEN}${passed} passed${NC}, ${RED}${failed} failed${NC}, ${YELLOW}${skipped} skipped${NC} — ${total}s"
[[ $failed -eq 0 ]]
