#!/usr/bin/env bash
# Build all PlexSpaces examples across all language suites.
#
# For each example: runs build.sh if present. Fails on first build error unless
# -c is given. Uses the same resume-state mechanism as test_all.sh.
#
# Usage:
#   ./build_all.sh               # build all examples, stop on first failure
#   ./build_all.sh -c            # continue from last failed example
#   ./build_all.sh go            # build only go examples
#   ./build_all.sh go python     # build go then python
#   ./build_all.sh go -c         # build go suite, continue from last failed

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
STATE_FILE="$SCRIPT_DIR/.build_state"

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

build_example() {
    local lang="$1"
    local name="$2"
    local dir="$3"
    local key="${lang}/${name}"

    local build_script="$dir/build.sh"
    [[ -f "$build_script" ]] || return 0

    local safe_name="${name//\//_}"
    local log="/tmp/build_all_${lang}_${safe_name}.txt"
    local start=$SECONDS

    printf 'LAST_KEY=%s\nLAST_LANG=%s\nLAST_EXAMPLE=%s\nLAST_STATUS=RUNNING\n' \
        "$key" "$lang" "$name" > "$STATE_FILE"

    (
        cd "$dir"
        bash build.sh 2>&1
    ) >"$log" 2>&1 &
    local build_pid=$!

    local spinner_chars='|/-\'
    local spinner_idx=0
    while kill -0 "$build_pid" 2>/dev/null; do
        local elapsed_now=$((SECONDS - start))
        local last_line
        last_line="$(tail -1 "$log" 2>/dev/null | tr -d '\r\n' | cut -c1-55)"
        printf '\r  %-50s %s %3ds  %-55s' "${lang}/${name}" "${spinner_chars:$((spinner_idx % 4)):1}" "$elapsed_now" "$last_line"
        spinner_idx=$((spinner_idx + 1))
        sleep 1
    done
    wait "$build_pid"
    local rc=$?
    local elapsed=$((SECONDS - start))

    if [[ $rc -eq 0 ]]; then
        printf '\r  %-50s %b\n' "${lang}/${name}" "${GREEN}BUILD OK${NC} (${elapsed}s)"
        ((passed++)) || true
        printf 'LAST_KEY=%s\nLAST_LANG=%s\nLAST_EXAMPLE=%s\nLAST_STATUS=PASSED\n' \
            "$key" "$lang" "$name" > "$STATE_FILE"
    else
        printf '\r  %-50s %b\n' "${lang}/${name}" "${RED}BUILD FAIL${NC} (${elapsed}s)"
        echo "    --- last 30 lines ---"
        tail -30 "$log" | sed 's/^/    /'
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

    if [[ -d "$base/apps" ]]; then
        while IFS= read -r d; do
            echo "apps/$(basename "$d"):$d"
        done < <(find "$base/apps" -maxdepth 1 -mindepth 1 -type d | sort)
    fi

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

        [[ -f "$dir/build.sh" ]] || continue

        if [[ -n "$START_FROM" ]]; then
            if [[ "$key" == "$START_FROM" ]]; then
                START_FROM=""
            else
                ((skipped++)) || true
                printf "  %-50s ${YELLOW}SKIP${NC}\n" "$key"
                continue
            fi
        fi

        build_example "$lang" "$name" "$dir"

    done < <(collect_examples "$lang")
done

total=$((SECONDS - total_start))
echo ""
echo -e "${GREEN}${passed} passed${NC}, ${RED}${failed} failed${NC}, ${YELLOW}${skipped} skipped${NC} — ${total}s"
[[ $failed -eq 0 ]]
