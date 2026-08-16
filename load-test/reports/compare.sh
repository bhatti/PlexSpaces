#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Compare two load test runs: flag latency regressions and improvements.
#
# USAGE:
#   bash load-test/reports/compare.sh <run-A> <run-B> [--threshold N]
#   Run args can be full paths or bare run IDs (e.g. 20260731-143845).
#
# Default threshold: 10% (flag as regression if metric got >10% worse).

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
DATA_DIR="$REPO_ROOT/load-test-data"

_resolve_dir() {
  local d="$1"
  [ -d "$d" ] && echo "$d" && return
  [ -d "$DATA_DIR/$d" ] && echo "$DATA_DIR/$d" && return
  echo "ERROR: not a directory: $d" >&2; exit 1
}

DIR_A=$(_resolve_dir "${1:?Usage: compare.sh <run-A> <run-B> [--threshold N]}")
DIR_B=$(_resolve_dir "${2:?Usage: compare.sh <run-A> <run-B> [--threshold N]}")
THRESHOLD=10

shift 2
while [ $# -gt 0 ]; do
  case "$1" in
    --threshold) THRESHOLD="$2"; shift 2 ;;
    *) echo "Unknown option: $1"; exit 1 ;;
  esac
done

RUN_A="$(basename "${DIR_A%/}")"
RUN_B="$(basename "${DIR_B%/}")"

LANG_ORDER="rust-embedded rust-wasm go typescript python"

# ── Strip unit from k6 value string → ms number ──────────────────────────────
_to_ms() {
  local v="$1"
  case "$v" in
    *µs) echo "$v" | sed 's/µs//' | awk '{printf "%.3f", $1/1000}' ;;
    *ms) echo "$v" | sed 's/ms//' ;;
    *s)  echo "$v" | sed 's/s//'  | awk '{printf "%.0f", $1*1000}' ;;
    *)   echo "${v:-0}" ;;
  esac
}

# Parse lang-*/*.log files in a run dir → TSV: lang scenario p50 p95 p99 rps errors
parse_dir() {
  local dir="$1"
  for lang_dir in "$dir"/lang-*/; do
    [ -d "$lang_dir" ] || continue
    local lang; lang="$(basename "$lang_dir" | sed 's/^lang-//')"
    for f in "$lang_dir"/*.log; do
      [ -f "$f" ] || continue
      local scenario; scenario="$(basename "$f" .log)"

      local kv_line
      kv_line=$(grep -m1 'perf_kv_latency_ms\.\.\.\.' "$f" 2>/dev/null || true)
      [ -z "$kv_line" ] && continue

      local p50 p95 p99 rps errors
      p50=$(_to_ms "$(echo "$kv_line" | grep -o 'med=[^ ]*' | sed 's/med=//')")
      p95=$(_to_ms "$(echo "$kv_line" | grep -o 'p(95)=[^ ]*' | sed 's/p(95)=//')")
      p99=$(_to_ms "$(echo "$kv_line" | grep -o 'p(99)=[^ ]*' | sed 's/p(99)=//')")
      rps=$(grep -m1 'http_reqs\.\.\.' "$f" 2>/dev/null | awk '{print $2}' || echo "0")
      errors=$(grep 'perf_error_rate\.\.\.\.' "$f" 2>/dev/null | tail -1 | \
        grep -o '[0-9.]*%' | head -1 | tr -d '%' || echo "0")

      printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
        "$lang" "$scenario" "${p50:-0}" "${p95:-0}" "${p99:-0}" "${rps:-0}" "${errors:-0}"
    done
  done
}

ROWS_A=$(mktemp); ROWS_B=$(mktemp)
trap 'rm -f "$ROWS_A" "$ROWS_B"' EXIT

parse_dir "$DIR_A" | sort > "$ROWS_A"
parse_dir "$DIR_B" | sort > "$ROWS_B"

if [ ! -s "$ROWS_A" ] || [ ! -s "$ROWS_B" ]; then
  echo "ERROR: one or both run dirs have no parseable scenario logs in lang-*/*.log"
  echo "  A ($RUN_A): $(wc -l < "$ROWS_A") scenarios"
  echo "  B ($RUN_B): $(wc -l < "$ROWS_B") scenarios"
  exit 1
fi

regressions=0; improvements=0; unchanged=0

echo ""
echo "╔═══════════════════════════════════════════════════════════════════════════════════════════════╗"
printf "║  Load Test Comparison                                                                         ║\n"
printf "║  A (before): %-79s║\n" "$RUN_A"
printf "║  B (after):  %-79s║\n" "$RUN_B"
printf "║  Threshold:  %d%%   (⚠ regression / ✓ improvement)%-38s║\n" "$THRESHOLD" ""
echo "╠═══════════════════════════════════════════════════════════════════════════════════════════════╣"
printf "║  %-18s %-30s %10s %10s %9s %9s  %-18s║\n" \
  "Language" "Scenario" "A p95ms" "B p95ms" "A RPS" "B RPS" "Status"
echo "╠═══════════════════════════════════════════════════════════════════════════════════════════════╣"

while IFS=$'\t' read -r lang scenario p50_a p95_a p99_a rps_a errors_a; do
  match=$(grep "^${lang}	${scenario}	" "$ROWS_B" || true)
  [ -z "$match" ] && continue

  IFS=$'\t' read -r _l _s p50_b p95_b p99_b rps_b errors_b <<< "$match"

  status=$(awk -v pa="$p95_a" -v pb="$p95_b" -v ra="$rps_a" -v rb="$rps_b" \
               -v thr="$THRESHOLD" 'BEGIN{
    result="ok"; label="unchanged"
    if(pa+0>0&&pb+0>0){
      pct=(pb-pa)/pa*100
      if(pct>thr) {result="regression"; label=sprintf("⚠  REGRESSION p95 +%.0f%%",pct)}
      if(pct<-thr){result="improved";   label=sprintf("✓  improved p95 %.0f%%",pct)}
    }
    if(result=="ok"&&ra+0>0&&rb+0>0){
      pct=(ra-rb)/ra*100
      if(pct>thr) {result="regression"; label=sprintf("⚠  REGRESSION rps -%.0f%%",pct)}
      if(pct<-thr){result="improved";   label=sprintf("✓  improved rps +%.0f%%",-pct)}
    }
    print result "\t" label
  }')
  verdict="${status##*	}"; category="${status%%	*}"

  [ "$category" = "regression" ] && regressions=$((regressions+1))
  [ "$category" = "improved"   ] && improvements=$((improvements+1))
  [ "$category" = "ok"         ] && unchanged=$((unchanged+1))

  err_flag="  "
  awk -v e="${errors_b:-0}" 'BEGIN{if(e+0>0) exit 0; exit 1}' 2>/dev/null && err_flag="⚠ "

  printf "║  %-18s %-30s %10s %10s %9s %9s  %s%-16s║\n" \
    "$lang" "$scenario" "${p95_a}ms" "${p95_b}ms" "$rps_a" "$rps_b" "$err_flag" "$verdict"
done < "$ROWS_A"

echo "╠═══════════════════════════════════════════════════════════════════════════════════════════════╣"
printf "║  Summary: %d regressions  %d improvements  %d unchanged%-44s║\n" \
  "$regressions" "$improvements" "$unchanged" ""
echo "╚═══════════════════════════════════════════════════════════════════════════════════════════════╝"

# ── Per-language RSS delta ────────────────────────────────────────────────────
echo ""
echo "Resource Usage Delta  (A → B)"
echo "───────────────────────────────────────────────────────────────────────"
printf "  %-18s  %-12s %-12s %-12s %-12s\n" "Language" "A RSS max" "B RSS max" "A CPU pk" "B CPU pk"
printf "  %-18s  %-12s %-12s %-12s %-12s\n" "--------" "---------" "---------" "--------" "--------"

for lang in $LANG_ORDER; do
  csv_a="$DIR_A/lang-${lang}/metrics.csv"
  csv_b="$DIR_B/lang-${lang}/metrics.csv"
  [ -f "$csv_a" ] || [ -f "$csv_b" ] || continue

  _rss_cpu() {
    local csv="$1"
    [ -f "$csv" ] || { echo "–	–"; return; }
    awk -F',' 'NR>1{
      r=$4+$6; if(r>rss_max)rss_max=r
      c=$5+$7; if(c>cpu_max)cpu_max=c
    } END{printf "%.0fMB\t%.1f%%",rss_max,cpu_max}' "$csv"
  }
  IFS=$'\t' read -r rss_a cpu_a <<< "$(_rss_cpu "$csv_a")"
  IFS=$'\t' read -r rss_b cpu_b <<< "$(_rss_cpu "$csv_b")"
  printf "  %-18s  %-12s %-12s %-12s %-12s\n" "$lang" "$rss_a" "$rss_b" "$cpu_a" "$cpu_b"
done

echo ""
if [ "$regressions" -gt 0 ]; then
  echo "  ⚠  $regressions regression(s) detected — review before merging."
  exit 1
else
  echo "  ✓  No regressions at ${THRESHOLD}% threshold."
  exit 0
fi
