#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Analyze a load test run: per-language deep-dive, then cross-language comparison.
#
# USAGE:
#   bash load-test/reports/analyze.sh [<run-dir-or-id>]
#   bash load-test/reports/analyze.sh          # uses latest run in load-test-data/
#
# REQUIRES: awk, sed, python3

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
DATA_DIR="$REPO_ROOT/load-test-data"

# ── Find run dir ──────────────────────────────────────────────────────────────
RUN_DIR="${1:-}"
if [ -z "$RUN_DIR" ]; then
  RUN_DIR=$(ls -1d "$DATA_DIR"/20* 2>/dev/null | sort | tail -1 || true)
  [ -z "$RUN_DIR" ] && { echo "No run dirs found in $DATA_DIR. Run the load test first."; exit 1; }
  echo "(Using latest run: $(basename "$RUN_DIR"))"
fi
[ -d "$RUN_DIR" ] || RUN_DIR="$DATA_DIR/$RUN_DIR"
[ -d "$RUN_DIR" ] || { echo "ERROR: not a directory: $RUN_DIR"; exit 1; }
RUN_ID="$(basename "$RUN_DIR")"

LANG_ORDER="rust-embedded rust-wasm go typescript python"

# ── Helpers ───────────────────────────────────────────────────────────────────

# Convert a k6 duration value to ms, displaying as µs when < 1ms.
# Input: "0s", "1ms", "355µs", "2.5ms", bare number (treated as ms)
# Output: human-readable string like "355µs" or "2.5ms"
_fmt_duration() {
  local v="$1"
  # Strip µ (U+00B5, UTF-8: 0xC2 0xB5) with sed before passing to awk — locale-safe.
  local norm; norm=$(printf '%s' "$v" | sed 's/\xc2\xb5s$/us/')
  echo "$norm" | awk '
  {
    s = $0
    if (s ~ /us$/) {
      gsub(/us$/, "", s); us = s + 0
      if (us < 1000) { printf "%dµs", int(us); next }
      ms = us / 1000.0
    } else if (s ~ /ms$/) {
      gsub(/ms$/, "", s); ms = s + 0
    } else if (s ~ /s$/) {
      gsub(/s$/, "", s); ms = s * 1000
    } else {
      ms = s + 0
    }
    # k6 reports integer ms; 0 means <1ms not truly zero — show <1ms so it is not misleading.
    if (ms == 0) { print "<1ms"; next }
    if (ms < 1) { printf "%dµs", int(ms * 1000); next }
    # Show one decimal for sub-10ms values so 1.0ms vs 1.5ms are distinguishable.
    if (ms < 10) { printf "%.2fms", ms; next }
    printf "%.1fms", ms
  }'
}

# Convert a k6 duration value to a plain ms float for numeric comparison/storage.
_to_ms_num() {
  local v="$1"
  local norm; norm=$(printf '%s' "$v" | sed 's/\xc2\xb5s$/us/')
  echo "$norm" | awk '
  {
    s = $0
    if (s ~ /us$/) { gsub(/us$/, "", s); printf "%.4f", (s+0)/1000; next }
    if (s ~ /ms$/) { gsub(/ms$/, "", s); printf "%.4f", s+0; next }
    if (s ~ /s$/)  { gsub(/s$/, "", s);  printf "%.4f", (s+0)*1000; next }
    printf "%.4f", s+0
  }'
}

# ── Parse k6 log: extract key metrics ─────────────────────────────────────────
# Returns tab-separated: p50_ms p95_ms p99_ms rps errors test_type max_vus duration_s start_ts end_ts elapsed_s
# Supports two formats:
#   1. k6 native stats table (kv_http, spawn/lifecycle) — "perf_kv_latency_ms......"
#   2. handleSummary box format (echo_http) — "║  p50=1.0ms  p95=..."
parse_k6_log() {
  local f="$1"
  [ -f "$f" ] || return

  local fname; fname="$(basename "$f" .log)"
  local test_type
  case "$fname" in
    echo_http*virtual*) test_type="echo/virtual" ;;
    echo_http*)         test_type="echo/regular" ;;
    kv_http*)           test_type="kv/http" ;;
    lifecycle*virtual*) test_type="spawn/virtual" ;;
    lifecycle*regular*) test_type="spawn/regular" ;;
    lifecycle*)         test_type="spawn/lifecycle" ;;
    ws_connections*|ws_capacity*) test_type="ws/capacity" ;;
    actor_capacity*virtual*) test_type="actor/virtual" ;;
    actor_capacity*)    test_type="actor/capacity" ;;
    *)                  test_type="unknown" ;;
  esac

  local p50_raw p95_raw p99_raw rps errors max_vus dur_s
  local start_ts end_ts elapsed_s
  start_ts=$(grep -m1 '^  Start: ' "$f" 2>/dev/null | sed 's/^  Start: //' || echo "")
  end_ts=$(grep -m1 '^  End: ' "$f" 2>/dev/null | sed 's/^  End: //' | sed 's/  (.*//' || echo "")
  elapsed_s=$(grep -m1 '^  End: ' "$f" 2>/dev/null | grep -oE '\([0-9]+s\)' | tr -d '()s' || echo "")

  # ── ws/capacity box: p50=max_conns  p95=upgrade_p95  p99=reg_p95  rps=0 ──
  if [ "$test_type" = "ws/capacity" ]; then
    p50_raw=$(grep -m1 'max_concurrent_thin_nodes:' "$f" 2>/dev/null | grep -oE '[0-9]+' | head -1 || echo "0")
    # upgrade p95 is first p95= in the box (WebSocket upgrade section)
    p95_raw=$(grep 'p95=' "$f" 2>/dev/null | head -1 | sed 's/.*p95=\([^ ]*\).*/\1/' | tr -d '║ ' || echo "0ms")
    # registration handshake p95 is the second p95= in the box
    local reg_line; reg_line=$(grep 'p95=' "$f" 2>/dev/null | sed -n '2p' || true)
    local p99_ws; p99_ws=$(echo "$reg_line" | sed 's/.*p95=\([^ ]*\).*/\1/' | tr -d '║ ')
    [ -z "$p99_ws" ] && p99_ws=""
    errors=$(grep -m1 'upgrade_error_rate:' "$f" 2>/dev/null | grep -oE '[0-9]+\.[0-9]+' | head -1 || echo "0")
    dur_s=$(grep -m1 'Total run:\|║.*duration:' "$f" 2>/dev/null | grep -oE '[0-9]+s' | head -1 || echo "?")
    max_vus=$(grep -m1 'max_concurrent_thin_nodes:' "$f" 2>/dev/null | grep -oE '[0-9]+' | head -1 || echo "0")
    rps="0"
    local p50_ws p95_ws
    p50_ws="$p50_raw"                       # store as plain integer, not ms
    p95_ws=$(_to_ms_num "${p95_raw:-0ms}")
    local p99_final; [ -n "$p99_ws" ] && p99_final=$(_to_ms_num "$p99_ws") || p99_final="NA"
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
      "${p50_ws:-0}" "${p95_ws:-0}" "${p99_final:-NA}" "${rps:-0}" "${errors:-0}" \
      "$test_type" "${max_vus:-0}" "${dur_s:-?}" \
      "${start_ts:-}" "${end_ts:-}" "${elapsed_s:-}"
    return
  fi

  # ── actor/capacity and actor/virtual box: p50=peak_actors  p95=peak_rss_MB  rps=spawn_rps ──
  if [ "$test_type" = "actor/capacity" ] || [ "$test_type" = "actor/virtual" ]; then
    local peak_actors peak_rss spawn_rps_raw overhead_kb
    peak_actors=$(grep -m1 'max_live_actors\|peak_live_actors\|live_at_peak' "$f" 2>/dev/null | grep -oE '[0-9]+' | head -1 || echo "0")
    peak_rss=$(grep -m1 'peak_rss:' "$f" 2>/dev/null | grep -oE '[0-9]+' | head -1 || echo "0")
    spawn_rps_raw=$(grep -m1 'spawn_rps:' "$f" 2>/dev/null | grep -oE '[0-9.]+' | head -1 || echo "0")
    overhead_kb=$(grep -m1 'overhead_per_actor:' "$f" 2>/dev/null | grep -oE '[0-9]+\.[0-9]+' | head -1 || echo "")
    errors=$(grep -m1 'error_rate:' "$f" 2>/dev/null | grep -oE '[0-9]+\.[0-9]+' | head -1 || echo "0")
    dur_s=$(grep -m1 'duration:' "$f" 2>/dev/null | grep -oE '[0-9]+s' | head -1 || echo "?")
    max_vus="1"
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
      "${peak_actors:-0}" "${peak_rss:-0}" "${overhead_kb:-NA}" "${spawn_rps_raw:-0}" "${errors:-0}" \
      "$test_type" "${max_vus:-0}" "${dur_s:-?}" \
      "${start_ts:-}" "${end_ts:-}" "${elapsed_s:-}"
    return
  fi

  # ── Format 1: k6 native stats table (kv_http, lifecycle) ─────────────────
  local metric_line
  metric_line=$(grep -m1 'perf_kv_latency_ms\.\.\.\.' "$f" 2>/dev/null || true)
  if [ -z "$metric_line" ]; then
    metric_line=$(grep -m1 'perf_echo_latency_ms\.\.\.\.' "$f" 2>/dev/null || true)
  fi
  if [ -z "$metric_line" ]; then
    metric_line=$(grep -m1 'perf_spawn_ms\.\.\.\.' "$f" 2>/dev/null || true)
  fi

  if [ -n "$metric_line" ]; then
    # Native stats table format
    p50_raw=$(echo "$metric_line" | grep -o 'med=[^ ]*' | sed 's/med=//')
    p95_raw=$(echo "$metric_line" | grep -o 'p(95)=[^ ]*' | sed 's/p(95)=//')
    p99_raw=$(echo "$metric_line" | grep -o 'p(99)=[^ ]*' | sed 's/p(99)=//')
    rps=$(grep -m1 'http_reqs\.\.\.' "$f" 2>/dev/null | awk '{gsub("/s","",$3); print $3}' || echo "0")
    errors=$(grep 'perf_error_rate\.\.\.\.' "$f" 2>/dev/null | tail -1 | \
      grep -o '[0-9.]*%' | head -1 | tr -d '%' || echo "0")
    max_vus=$(grep -m1 'vus_max\.' "$f" 2>/dev/null | awk '{print $2}' || echo "0")
    [ -z "$max_vus" ] && max_vus="0"
    local progress_line
    progress_line=$(grep -E '\[.*100%.*\]' "$f" 2>/dev/null | tail -1 || true)
    # Duration: matches "30.0s/30s" → take after "/" ; or "30s" → take as-is
    dur_s=$(echo "$progress_line" | grep -oE '[0-9.]+[smh](/[0-9.]+[smh])?' | tail -1 | \
      sed 's|.*/||' || echo "")
    [ -z "$dur_s" ] && dur_s="?"
    local vu_from_line
    vu_from_line=$(echo "$progress_line" | grep -oE '[0-9]+ VUs' | grep -oE '^[0-9]+' || echo "")
    [ -n "$vu_from_line" ] && max_vus="$vu_from_line"
  else
    # ── Format 2: handleSummary box format (echo_http) ──────────────────────
    # Look for the latency box line: ║    p50=1.0ms    p95=1.0ms    p99=–
    local latency_line
    latency_line=$(grep -m1 '║.*p50=' "$f" 2>/dev/null || true)
    [ -z "$latency_line" ] && return

    p50_raw=$(echo "$latency_line" | sed 's/.*p50=\([^ ]*\).*/\1/' | tr -d '║ ')
    p95_raw=$(echo "$latency_line" | sed 's/.*p95=\([^ ]*\).*/\1/' | tr -d '║ ')
    local p99_raw_raw
    p99_raw_raw=$(echo "$latency_line" | sed 's/.*p99=\([^ ]*\).*/\1/' | tr -d '║ ')
    # p99=– means not collected
    if [ "$p99_raw_raw" = "–" ] || [ -z "$p99_raw_raw" ]; then
      p99_raw=""
    else
      p99_raw="$p99_raw_raw"
    fi

    # RPS from the VUs/duration header box line
    local header_line
    header_line=$(grep -m1 '║.*VUs:.*RPS:' "$f" 2>/dev/null || true)
    rps=$(echo "$header_line" | sed 's/.*RPS: \([0-9.]*\).*/\1/' | tr -d '║ ')
    max_vus=$(echo "$header_line" | sed 's/.*VUs: \([0-9]*\).*/\1/' | tr -d '║ ')
    dur_s=$(echo "$header_line" | sed 's/.*duration: \([^ │║]*\).*/\1/' | tr -d '║ ')
    [ -z "$dur_s" ] && dur_s="?"
    [ -z "$max_vus" ] && max_vus="0"
    [ -z "$rps" ] && rps="0"

    # Error rate from box
    local err_line
    err_line=$(grep -m1 '║.*actor_error_rate:' "$f" 2>/dev/null || true)
    errors=$(echo "$err_line" | sed 's/.*actor_error_rate: \([0-9.]*\).*/\1/' | tr -d '║ ')
    [ -z "$errors" ] && errors="0"
  fi

  local p50 p95 p99
  p50=$(_to_ms_num "${p50_raw:-0}")
  p95=$(_to_ms_num "${p95_raw:-0}")
  if [ -n "${p99_raw:-}" ]; then
    p99=$(_to_ms_num "$p99_raw")
  else
    p99="NA"
  fi

  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
    "${p50:-0}" "${p95:-0}" "${p99:-NA}" "${rps:-0}" "${errors:-0}" \
    "$test_type" "${max_vus:-0}" "${dur_s:-?}" \
    "${start_ts:-}" "${end_ts:-}" "${elapsed_s:-}"
}

# ── Prometheus per-test metrics from snapshots ────────────────────────────────
# Args: snap_dir scenario_label
# Finds the per-test snapshot by label (new naming) or falls back to all snaps.
print_prometheus_metrics() {
  local snap_dir="$1"
  local label="$2"
  [ -d "$snap_dir" ] || return
  ls "$snap_dir"/snap_*.json >/dev/null 2>&1 || return

  python3 - "$snap_dir" "$label" <<'PYEOF' 2>/dev/null || true
import sys, os, json, glob

snap_dir = sys.argv[1]
label    = sys.argv[2] if len(sys.argv) > 2 else ""

snaps = sorted(glob.glob(os.path.join(snap_dir, "snap_*.json")))

# Primary: label-based match (new naming: snap_<label>_<timestamp>.json)
window_snaps = []
if label:
    for f in snaps:
        if os.path.basename(f).startswith(f"snap_{label}_"):
            try:
                with open(f) as fh: s = json.load(fh)
                window_snaps.append(s)
            except: pass

# Fallback: all snapshots (old naming or label not found)
if not window_snaps:
    for f in snaps:
        try:
            with open(f) as fh: s = json.load(fh)
            window_snaps.append(s)
        except: continue

if not window_snaps:
    sys.exit(0)

# Collect actors and Prometheus metrics from window
actor_counts = []
routing_p50_vals = []
actor_proc_p50_vals = []
routing_p95_vals = []
actor_proc_p95_vals = []

for s in window_snaps:
    # Actor counts
    ac = s.get('main_actors', 0) or 0
    ec = s.get('embedded_actors', 0) or 0
    actor_counts.append((ec, ac))

    for key in ('main_metrics_table', 'embedded_metrics_table'):
        mt = s.get(key)
        if not mt: continue
        rows = mt if isinstance(mt, list) else mt.get('metrics', [])
        for row in (rows or []):
            n = row.get('name', '')
            labels = row.get('labels', {})
            try: v = float(row.get('value') or 0)
            except: continue
            q = labels.get('quantile', '')
            if 'message_routing_duration_seconds' in n:
                if q == '0.5': routing_p50_vals.append(v * 1000)
                if q == '0.95': routing_p95_vals.append(v * 1000)
            elif 'actor_message_processing_duration_seconds' in n:
                if q == '0.5': actor_proc_p50_vals.append(v * 1000)
                if q == '0.95': actor_proc_p95_vals.append(v * 1000)

# Print actors
if actor_counts:
    emb_vals = [e for e,m in actor_counts if e > 0]
    main_vals = [m for e,m in actor_counts if m > 0]
    emb_str = f"emb={max(emb_vals)}" if emb_vals else "emb=0"
    main_str = f"main={max(main_vals)}" if main_vals else "main=0"
    print(f"│  │    system:  actors(peak): {emb_str}  {main_str}")

# Print Prometheus latency
if routing_p50_vals or actor_proc_p50_vals:
    parts = []
    if routing_p50_vals:
        rp50 = sum(routing_p50_vals)/len(routing_p50_vals)
        rp95 = sum(routing_p95_vals)/len(routing_p95_vals) if routing_p95_vals else 0
        def fms(v):
            if v < 1: return f"{int(v*1000)}µs"
            return f"{v:.2f}ms"
        parts.append(f"routing p50={fms(rp50)} p95={fms(rp95)}")
    if actor_proc_p50_vals:
        ap50 = sum(actor_proc_p50_vals)/len(actor_proc_p50_vals)
        ap95 = sum(actor_proc_p95_vals)/len(actor_proc_p95_vals) if actor_proc_p95_vals else 0
        parts.append(f"actor_proc p50={fms(ap50)} p95={fms(ap95)}")
    if parts:
        print(f"│  │    prom:    {' | '.join(parts)}")
PYEOF
}

# ── Collect summary rows for comparison table ─────────────────────────────────
ROWS=$(mktemp); trap 'rm -f "$ROWS"' EXIT

echo ""
echo "╔══════════════════════════════════════════════════════════════════════════════╗"
printf "║  Load Test Analysis — Run: %-51s║\n" "$RUN_ID"
echo "╚══════════════════════════════════════════════════════════════════════════════╝"

# ── Check for pre-built index (fast path) ────────────────────────────────────
INDEX_FILE="$RUN_DIR/run-index.json"

# ── Per-language sections ─────────────────────────────────────────────────────
for lang in $LANG_ORDER; do
  lang_dir="$RUN_DIR/lang-${lang}"
  [ -d "$lang_dir" ] || continue

  echo ""
  echo "┌─ $lang ─────────────────────────────────────────────────────────────────────"

  # k6 scenario logs — use pre-built index if available, else scan files
  # Print one scenario block with clean indented layout
  _print_scenario() {
    local scenario="$1" test_type="$2" max_vus="$3" dur_s="$4"
    local p50_f="$5" p95_f="$6" p99_f="$7" rps="$8" errors="$9"
    local ts_info="${10:-}" actors_line="${11:-}" prom_line="${12:-}" extra_line="${13:-}"
    local err_flag=""
    awk -v e="${errors:-0}" 'BEGIN{if(e+0>0){exit 0}else{exit 1}}' && err_flag="  ⚠ ERRORS"
    echo "│"
    printf "│  ┌─ %-44s [%s]  VUs=%-4s  dur=%s\n" "$scenario" "$test_type" "$max_vus" "$dur_s"
    [ -n "$ts_info" ]     && printf "│  │    time:    %s\n" "$ts_info"
    printf "│  │    k6:     p50=%-10s p95=%-10s p99=%-6s  RPS=%-12s Errors=%s%%%s\n" \
      "$p50_f" "$p95_f" "$p99_f" "$rps" "$errors" "$err_flag"
    [ -n "$actors_line" ] && printf "│  │    system:  %s\n" "$actors_line"
    [ -n "$extra_line" ]  && printf "│  │    system:  %s\n" "$extra_line"
    [ -n "$prom_line" ]   && printf "│  │    prom:    %s\n" "$prom_line"
    echo "│  └"
  }

  found_log=0
  if [ -f "$INDEX_FILE" ]; then
    # Fast path: read from index
    while IFS=$'\t' read -r scenario test_type max_vus dur_s p50 p95 p99 rps errors \
        emb_actors main_actors routing_p50 routing_p95 actor_p50 actor_p95 actor_spawn_total \
        ws_registered ws_unregistered idx_start idx_end idx_elapsed; do
      found_log=1
      p50_fmt="$(_fmt_duration "${p50}ms")"
      p95_fmt="$(_fmt_duration "${p95}ms")"
      [ "${p99}" = "NA" ] && p99_fmt="–" || p99_fmt="$(_fmt_duration "${p99}ms")"
      # Timestamp
      idx_ts_info=""
      [ -n "${idx_start:-}" ] && idx_ts_info="start=${idx_start}"
      [ -n "${idx_end:-}" ]   && idx_ts_info="${idx_ts_info:+$idx_ts_info  }end=${idx_end}"
      [ -n "${idx_elapsed:-}" ] && idx_ts_info="${idx_ts_info:+$idx_ts_info  }elapsed=${idx_elapsed}s"
      # Actors
      actors_line=""
      if [ -n "${emb_actors:-}" ] || [ -n "${main_actors:-}" ]; then
        emb_a="${emb_actors:-0}"; main_a="${main_actors:-0}"
        actors_line="actors(peak): emb=${emb_a}  main=${main_a}"
        case "$test_type" in
          spawn/*|spawn/lifecycle)
            [ -n "$actor_spawn_total" ] && [ "$actor_spawn_total" != "0" ] && \
              actors_line="${actors_line}  spawn_total(lifetime)=${actor_spawn_total}"
            ;;
        esac
      fi
      # WS extra
      extra_line=""
      [ "$test_type" = "ws/capacity" ] && [ -n "${ws_registered:-}" ] && [ "$ws_registered" != "0" ] && \
        extra_line="ws: max_concurrent=${ws_registered}  disconnected=${ws_unregistered:-0}"
      # Prometheus
      prom_parts=""
      if [ -n "${routing_p50:-}" ] && [ "${routing_p50:-0}" != "0" ]; then
        prom_parts="routing p50=$(_fmt_duration "${routing_p50}ms") p95=$(_fmt_duration "${routing_p95}ms")"
      fi
      if [ -n "${actor_p50:-}" ] && [ "${actor_p50:-0}" != "0" ]; then
        ap_p50="$(_fmt_duration "${actor_p50}ms")"
        ap_p95="$(_fmt_duration "${actor_p95}ms")"
        [ -n "$prom_parts" ] && prom_parts="${prom_parts}  |  "
        prom_parts="${prom_parts}actor_proc p50=${ap_p50} p95=${ap_p95}"
      fi
      _print_scenario "$scenario" "$test_type" "$max_vus" "$dur_s" \
        "$p50_fmt" "$p95_fmt" "$p99_fmt" "$rps" "$errors" \
        "$idx_ts_info" "$actors_line" "$prom_parts" "$extra_line"
      printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
        "$lang" "$scenario" "$p50" "$p95" "$p99" "$rps" "$errors" "$test_type" "$max_vus" >> "$ROWS"
    done < <(python3 - "$INDEX_FILE" "$lang" <<'IDXPY' 2>/dev/null
import sys, json
idx_file, lang = sys.argv[1], sys.argv[2]
with open(idx_file) as f: idx = json.load(f)
def fv(v): return f"{v:.6f}" if v is not None else "0"
def na(v): return "NA" if v is None else f"{v:.6f}"
def iv(v): return str(int(v)) if v else "0"
def sv(v): return str(v) if v is not None else ""
for s in idx.get('scenarios', []):
    if s['lang'] != lang: continue
    p = s.get('prometheus', {})
    print('\t'.join([
        s['scenario'], s['test_type'], str(s['max_vus']), s.get('duration', '?'),
        fv(s.get('p50_ms')), fv(s.get('p95_ms')), na(s.get('p99_ms')),
        f"{s.get('rps',0):.1f}", f"{s.get('errors_pct',0):.2f}",
        str(p.get('embedded_actors', 0)), str(p.get('main_actors', 0)),
        fv(p.get('routing_p50_ms')), fv(p.get('routing_p95_ms')),
        fv(p.get('actor_proc_p50_ms')), fv(p.get('actor_proc_p95_ms')),
        iv(p.get('actor_spawn_total')),
        iv(p.get('ws_thin_nodes_registered')), iv(p.get('ws_thin_nodes_unregistered')),
        sv(s.get('start_ts')), sv(s.get('end_ts')), sv(s.get('elapsed_s')),
    ]))
IDXPY
    )
  else
    # Slow path: scan log files directly
    for log_f in "$lang_dir"/*.log; do
      [ -f "$log_f" ] || continue
      [ "$(basename "$log_f")" = "server.log" ] && continue
      scenario="$(basename "$log_f" .log)"
      result=$(parse_k6_log "$log_f" || true)
      [ -z "$result" ] && continue
      found_log=1
      IFS=$'\t' read -r p50 p95 p99 rps errors test_type max_vus dur_s log_start log_end log_elapsed <<< "$result"
      p50_fmt="$(_fmt_duration "${p50}ms")"
      p95_fmt="$(_fmt_duration "${p95}ms")"
      [ "${p99:-NA}" = "NA" ] && p99_fmt="–" || p99_fmt="$(_fmt_duration "${p99}ms")"
      err_flag=""
      awk -v e="${errors:-0}" 'BEGIN{if(e+0>0){exit 0}else{exit 1}}' && err_flag="  ⚠ ERRORS"
      ts_info=""
      [ -n "${log_start:-}" ] && ts_info="start=${log_start}"
      [ -n "${log_end:-}" ]   && ts_info="${ts_info:+$ts_info  }end=${log_end}"
      [ -n "${log_elapsed:-}" ] && ts_info="${ts_info:+$ts_info  }elapsed=${log_elapsed}s"
      echo "│"
      printf "│  ┌─ %-44s [%s]  VUs=%-4s  dur=%s\n" "$scenario" "$test_type" "$max_vus" "$dur_s"
      [ -n "$ts_info" ] && printf "│  │    time:    %s\n" "$ts_info"
      printf "│  │    k6:     p50=%-10s p95=%-10s p99=%-6s  RPS=%-12s Errors=%s%%%s\n" \
        "$p50_fmt" "$p95_fmt" "$p99_fmt" "$rps" "$errors" "$err_flag"
      snap_dir="$lang_dir/snapshots"
      print_prometheus_metrics "$snap_dir" "$scenario"
      echo "│  └"
      printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
        "$lang" "$scenario" "$p50" "$p95" "$p99" "$rps" "$errors" "$test_type" "$max_vus" >> "$ROWS"
    done
  fi
  [ "$found_log" -eq 0 ] && echo "│  (no k6 logs found)"

  # per-lang metrics.csv — RSS, CPU (use index if available, else scan)
  if [ -f "$INDEX_FILE" ]; then
    python3 - "$INDEX_FILE" "$lang" <<'RSSIDX' 2>/dev/null || true
import sys, json
idx_file, lang = sys.argv[1], sys.argv[2]
with open(idx_file) as f: idx = json.load(f)
ls = idx.get('languages', {}).get(lang, {})
if ls.get('emb_rss_max'):
    print(f"│  RSS(embedded):  min={ls['emb_rss_min']:.0f}MB max={ls['emb_rss_max']:.0f}MB delta={ls['emb_rss_max']-ls['emb_rss_min']:.0f}MB  CPU peak={ls.get('emb_cpu_peak',0):.1f}%")
if ls.get('main_rss_max'):
    print(f"│  RSS(main):      min={ls['main_rss_min']:.0f}MB max={ls['main_rss_max']:.0f}MB delta={ls['main_rss_max']-ls['main_rss_min']:.0f}MB  CPU peak={ls.get('main_cpu_peak',0):.1f}%")
RSSIDX
  else
    csv="$lang_dir/metrics.csv"
    [ -f "$csv" ] && awk -F',' 'NR>1{
      e=$4+0; m=$6+0
      if(e>0){if(e>emb_max)emb_max=e; if(emb_min==""||e<emb_min)emb_min=e}
      if(m>0){if(m>main_max)main_max=m; if(main_min==""||m<main_min)main_min=m}
      if($5+0>emb_cpu)emb_cpu=$5+0; if($7+0>main_cpu)main_cpu=$7+0
      if($10+0>0){if($10+0<eap_min||eap_min==0)eap_min=$10+0; if($10+0>eap_max)eap_max=$10+0}
      if($12+0>0){if($12+0<map_min||map_min==0)map_min=$12+0; if($12+0>map_max)map_max=$12+0}
      n++
    }END{
      if(!n){exit}
      if(emb_max>0)printf "│  RSS(embedded):  min=%.0fMB max=%.0fMB delta=%.0fMB  CPU peak=%.1f%%\n",emb_min,emb_max,emb_max-emb_min,emb_cpu
      if(main_max>0)printf "│  RSS(main):      min=%.0fMB max=%.0fMB delta=%.0fMB  CPU peak=%.1f%%\n",main_min,main_max,main_max-main_min,main_cpu
      if(eap_max>0)printf "│  Actor proc(emb):%.2f–%.2fms\n",eap_min,eap_max
      if(map_max>0)printf "│  Actor proc(main):%.2f–%.2fms\n",map_min,map_max
    }' "$csv"
  fi

  # Server log — error/warn counts + first unique errors
  slog="$lang_dir/server.log"
  if [ -f "$slog" ]; then
    if [ -f "$INDEX_FILE" ]; then
      errs=$(python3 -c "import json; idx=json.load(open('$INDEX_FILE')); ls=idx.get('languages',{}).get('$lang',{}); print(ls.get('server_errors',0))" 2>/dev/null || echo "0")
      warns=$(python3 -c "import json; idx=json.load(open('$INDEX_FILE')); ls=idx.get('languages',{}).get('$lang',{}); print(ls.get('server_warnings',0))" 2>/dev/null || echo "0")
    else
      errs=$(grep -c -E 'ERROR|error\[' "$slog" 2>/dev/null; true)
      warns=$(grep -c -E 'WARN ' "$slog" 2>/dev/null; true)
    fi
    errs="${errs:-0}"; warns="${warns:-0}"
    printf "│  Server log: %s errors  %s warnings  (%s)\n" "$errs" "$warns" "$slog"
    if [ "${errs}" -gt 0 ] 2>/dev/null; then
      grep -E 'ERROR|error\[' "$slog" 2>/dev/null | sort -u | head -3 | \
        awk '{print "│    " $0}' || true
    fi
  fi

  # WASM reinstantiation (use index if available)
  if [ -f "$INDEX_FILE" ]; then
    python3 - "$INDEX_FILE" "$lang" <<'WIDX' 2>/dev/null || true
import sys, json, math
idx_file, lang = sys.argv[1], sys.argv[2]
with open(idx_file) as f: idx = json.load(f)
ls = idx.get('languages', {}).get(lang, {})
r = ls.get('wasm_reins')
if r is not None:
    m = ls.get('wasm_msgs', 0)
    ratio = ls.get('wasm_reins_ratio')
    ok = "✓" if (ratio is not None and ratio < 0.01) else "⚠ "
    ratio_str = f"{ratio:.4f}" if ratio is not None else "N/A"
    print(f"│  WASM reinstantiations: {r:.0f} / {m:.0f} msgs  ratio={ratio_str}  {ok}")
WIDX
  else
    snap_dir="$lang_dir/snapshots"
    if [ -d "$snap_dir" ] && ls "$snap_dir"/snap_*.json >/dev/null 2>&1; then
      python3 - "$snap_dir" <<'PYEOF' 2>/dev/null || true
import sys, os, json, glob
snap_dir = sys.argv[1]
files = sorted(glob.glob(os.path.join(snap_dir, "snap_*.json")))
reins_vals, msgs_vals = [], []
for f in files:
    try:
        with open(f) as fh: snap = json.load(fh)
    except Exception: continue
    for key in ("main_metrics_table", "embedded_metrics_table"):
        mt = snap.get(key)
        if not mt: continue
        rows = mt if isinstance(mt, list) else mt.get("metrics", [])
        for row in (rows or []):
            n = row.get("name","")
            try: v = float(row.get("value") or 0)
            except: continue
            if "wasm_reinstantiation_total" in n: reins_vals.append(v)
            elif "wasm_component_message_success_total" in n: msgs_vals.append(v)
if reins_vals:
    r = max(reins_vals); m = max(msgs_vals) if msgs_vals else 0
    ratio = r/m if m>0 else float("nan")
    ok = "✓" if ratio < 0.01 else "⚠ "
    print(f"│  WASM reinstantiations: {r:.0f} / {m:.0f} msgs  ratio={ratio:.4f}  {ok}")
PYEOF
    fi
  fi

  echo "└────────────────────────────────────────────────────────────────────────────"
done

# ── Cross-language comparison table ──────────────────────────────────────────
if [ -s "$ROWS" ]; then
  echo ""
  echo "╔══════════════════════════════════════════════════════════════════════════════════════════════════╗"
  echo "║  Language Comparison  (overhead vs rust-embedded baseline; sub-ms shown as µs)                 ║"
  echo "╚══════════════════════════════════════════════════════════════════════════════════════════════════╝"

  # Defined display order for test types
  TYPE_ORDER="echo/regular echo/virtual kv/http spawn/regular spawn/virtual spawn/lifecycle ws/capacity actor/capacity actor/virtual"

  # Collect actual test types present
  TEST_TYPES_PRESENT=$(awk -F'\t' '{print $8}' "$ROWS" | awk '!seen[$0]++')

  # Helper to print one comparison table for a given test type
  _print_comparison_table() {
    local ttype="$1"
    local emb_p95 emb_rps
    emb_p95=$(awk -F'\t' -v tt="$ttype" '$1=="rust-embedded" && $8==tt {print $4; exit}' "$ROWS")
    emb_rps=$(awk -F'\t'  -v tt="$ttype" '$1=="rust-embedded" && $8==tt {print $6; exit}' "$ROWS")

    echo ""
    echo "┌──────────────────────────────────────────────────────────────────────────────────────────────────"

    # ── ws/capacity: show peak connections instead of latency ────────────────
    if [ "$ttype" = "ws/capacity" ]; then
      printf "│  TEST TYPE: %-20s  (key metric: max concurrent WebSocket connections)\n" "$ttype"
      printf "│  %-18s %-36s %12s %10s %10s %10s %8s  %s\n" \
        "Language" "Scenario" "max_conns" "upgrade_p95" "reg_p95" "RPS" "Errors%" "notes"
      echo "│  ──────────────────────────────────────────────────────────────────────────────────────────"
      while IFS=$'\t' read -r lang scenario p50 p95 p99 rps errors test_type max_vus; do
        [ "$test_type" = "ws/capacity" ] || continue
        # Pull max_concurrent_thin_nodes from ws-specific row stored in ROWS col 3 (p50=max_conns)
        # and upgrade p95 from col 4 (p95=upgrade_p95), reg p95 from col 5 (p99=reg_p95)
        max_conns=$(printf '%.0f' "${p50:-0}" 2>/dev/null || echo "${p50:-?}")
        upg_p95="$(_fmt_duration "${p95}ms")"
        reg_p95_fmt=""
        [ "${p99:-NA}" != "NA" ] && reg_p95_fmt="$(_fmt_duration "${p99}ms")"
        err_flag="   "
        awk -v e="${errors:-0}" 'BEGIN{if(e+0>0) exit 0; exit 1}' 2>/dev/null && err_flag="⚠  "
        printf "│  %-18s %-36s %12s %10s %10s %10s %7s%%  %s%s\n" \
          "$lang" "$scenario" "${max_conns:-?}" "$upg_p95" "${reg_p95_fmt:--}" "${rps:-0}" "${errors:-0}" "$err_flag" ""
      done < "$ROWS"
      echo "└──────────────────────────────────────────────────────────────────────────────────────────────────"
      return
    fi

    # ── actor/capacity: show peak actors, RSS, overhead, spawn_rps ──────────────────
    if [ "$ttype" = "actor/capacity" ] || [ "$ttype" = "actor/virtual" ]; then
      printf "│  TEST TYPE: %-20s  (key metric: max live actors at OOM threshold)\n" "$ttype"
      printf "│  %-18s %-36s %12s %12s %12s %10s %8s\n" \
        "Language" "Scenario" "peak_actors" "peak_rss_MB" "KB/actor" "spawn_rps" "Errors%"
      echo "│  ─────────────────────────────────────────────────────────────────────────────────────────────────"
      while IFS=$'\t' read -r lang scenario p50 p95 p99 rps errors test_type max_vus; do
        [ "$test_type" = "$ttype" ] || continue
        # p50=peak_actors  p95=peak_rss_MB  p99=overhead_per_actor_KB  rps=spawn_rps
        peak_actors_fmt=$(printf '%.0f' "${p50:-0}" 2>/dev/null || echo "${p50:-?}")
        peak_rss_fmt=$(awk -v v="${p95:-0}" 'BEGIN{if(v+0>0){printf "%.0fMB",v+0}else{print "?"}}')
        overhead_fmt=$(awk -v v="${p99:-}" 'BEGIN{if(v+0>0){printf "%.1fKB",v+0}else{print "—"}}')
        err_flag="   "
        awk -v e="${errors:-0}" 'BEGIN{if(e+0>0) exit 0; exit 1}' 2>/dev/null && err_flag="⚠  "
        printf "│  %-18s %-36s %12s %12s %12s %10s %7s%%  %s\n" \
          "$lang" "$scenario" "$peak_actors_fmt" "$peak_rss_fmt" "$overhead_fmt" "${rps:-0}" "${errors:-0}" "$err_flag"
      done < "$ROWS"
      echo "└──────────────────────────────────────────────────────────────────────────────────────────────────"
      return
    fi

    # ── Standard latency + RPS table ─────────────────────────────────────────
    printf "│  TEST TYPE: %-20s  (baseline RPS=%s  baseline p95=%s)\n" \
      "$ttype" "${emb_rps:-N/A}" "$(_fmt_duration "${emb_p95:-0}ms")"
    printf "│  %-18s %-36s %10s %10s %10s %10s %8s  %s\n" \
      "Language" "Scenario" "p50" "p95" "p99" "RPS" "Errors%" "vs baseline"
    echo "│  ──────────────────────────────────────────────────────────────────────────────────────────"

    # echo/virtual: rust-embedded does not run virtual echo (no per-VU virtual actor support).
    # spawn/virtual: rust-embedded DOES run virtual spawn — show its data normally.
    if [ "$ttype" = "echo/virtual" ]; then
      printf "│  %-18s %-36s %10s %10s %10s %10s %7s%%  %s\n" \
        "rust-embedded" "(virtual echo: N/A)" "N/A" "N/A" "N/A" "N/A" "N/A" "(native only)"
    fi

    while IFS=$'\t' read -r lang scenario p50 p95 p99 rps errors test_type max_vus; do
      [ "$test_type" = "$ttype" ] || continue
      # echo/virtual: skip rust-embedded data row (N/A row already printed above)
      if [ "$lang" = "rust-embedded" ] && [ "$ttype" = "echo/virtual" ]; then
        continue
      fi
      p50_fmt="$(_fmt_duration "${p50}ms")"
      p95_fmt="$(_fmt_duration "${p95}ms")"
      if [ "${p99:-NA}" = "NA" ]; then
        p99_fmt="–"
      else
        p99_fmt="$(_fmt_duration "${p99}ms")"
      fi

      overhead=""
      if [ "$lang" = "rust-embedded" ]; then
        overhead="(baseline)"
      elif [ -n "${emb_p95:-}" ] && [ "${emb_p95:-0}" != "0" ]; then
        overhead=$(awk -v ep="${emb_p95:-0}" -v lp="${p95:-0}" 'BEGIN{
          if(ep+0==0||lp+0==0){print "–"}
          else{printf "%.2fx slower",lp/ep}
        }')
      else
        overhead="(no emb baseline)"
      fi
      err_flag="   "
      awk -v e="${errors:-0}" 'BEGIN{if(e+0>0) exit 0; exit 1}' 2>/dev/null && err_flag="⚠  "
      printf "│  %-18s %-36s %10s %10s %10s %10s %7s%%  %s%s\n" \
        "$lang" "$scenario" "$p50_fmt" "$p95_fmt" "$p99_fmt" "$rps" "${errors:-0}" "$err_flag" "$overhead"
    done < "$ROWS"
    echo "└──────────────────────────────────────────────────────────────────────────────────────────────────"
  }

  # Print tables in defined order, then any unexpected types
  printed=""
  for ttype in $TYPE_ORDER; do
    # virtual test types: always print if any language ran it (to show N/A row for rust-embedded)
    if [ "$ttype" = "echo/virtual" ]; then
      echo "$TEST_TYPES_PRESENT" | grep -qxF "echo/virtual" || \
        echo "$TEST_TYPES_PRESENT" | grep -qxF "echo/regular" || continue
    elif [ "$ttype" = "spawn/virtual" ]; then
      echo "$TEST_TYPES_PRESENT" | grep -qxF "spawn/virtual" || \
        echo "$TEST_TYPES_PRESENT" | grep -qxF "spawn/regular" || continue
    else
      echo "$TEST_TYPES_PRESENT" | grep -qxF "$ttype" || continue
    fi
    _print_comparison_table "$ttype"
    printed="$printed $ttype"
  done

  # Any unexpected types not in TYPE_ORDER
  for ttype in $TEST_TYPES_PRESENT; do
    echo "$printed" | grep -qw "$ttype" && continue
    _print_comparison_table "$ttype"
  done
fi

# ── Memory / profiling summary ────────────────────────────────────────────────
memsnaps=( "$RUN_DIR"/memsnap-*.txt )
svgs=( "$RUN_DIR"/flamegraph-*.svg "$RUN_DIR"/*/flamegraph-*.svg )
has_memsnap=0
has_svg=0
for f in "${memsnaps[@]}"; do [ -f "$f" ] && has_memsnap=1 && break; done
for f in "${svgs[@]}";    do [ -f "$f" ] && has_svg=1    && break; done

if [ "$has_memsnap" -eq 1 ] || [ "$has_svg" -eq 1 ]; then
  echo ""
  echo "╔══════════════════════════════════════════════════════════════════════════════╗"
  echo "║  Memory / CPU Profiles                                                      ║"
  echo "╚══════════════════════════════════════════════════════════════════════════════╝"
fi

# vmmap memsnaps + CPU peak from per-language metrics.csv
if [ "$has_memsnap" -eq 1 ]; then
  echo ""
  echo "┌─ vmmap memory snapshots + CPU ────────────────────────────────────────────────"
  for f in "$RUN_DIR"/memsnap-*.txt; do
    [ -f "$f" ] || continue
    label="$(basename "$f" .txt | sed 's/memsnap-//')"
    footprint=$(grep -m1 'Physical footprint:[[:space:]]' "$f" | sed 's/.*footprint:[[:space:]]*//' | awk '{print $1}' || echo "?")
    footprint_peak=$(grep -m1 'Physical footprint (peak):' "$f" | sed 's/.*peak):[[:space:]]*//' | awk '{print $1}' || echo "?")
    # CPU peak: find the matching lang- directory by matching label suffix
    cpu_info=""
    for lang in $LANG_ORDER; do
      csv="$RUN_DIR/lang-${lang}/metrics.csv"
      [ -f "$csv" ] || continue
      if echo "$label" | grep -qi "$lang"; then
        cpu_info=$(awk -F',' 'NR>1{if($5+0>emb)emb=$5+0; if($7+0>main)main=$7+0}END{
          s=""
          if(emb>0) s=s " emb_cpu=" emb "%"
          if(main>0) s=s " main_cpu=" main "%"
          print s}' "$csv" 2>/dev/null || echo "")
        break
      fi
    done
    printf "│  %-35s  footprint=%-10s  peak=%-10s %s\n" "$label" "$footprint" "$footprint_peak" "$cpu_info"
  done
  echo "└────────────────────────────────────────────────────────────────────────────"
fi

# CPU flamegraphs
echo ""
if [ "$has_svg" -eq 1 ]; then
  echo "┌─ CPU flamegraphs ──────────────────────────────────────────────────────────────"
  for f in "$RUN_DIR"/flamegraph-*.svg "$RUN_DIR"/*/flamegraph-*.svg; do
    [ -f "$f" ] || continue
    size=$(du -sh "$f" 2>/dev/null | awk '{print $1}' || echo "?")
    printf "│  %s  (%s)\n" "$f" "$size"
  done
  echo "└────────────────────────────────────────────────────────────────────────────"
else
  echo "No CPU flamegraphs.  Re-run with --profile to capture samply SVG."
fi

if [ "$has_memsnap" -eq 0 ] && [ "$has_svg" -eq 0 ]; then
  echo "No vmmap snapshots.  Re-run load tests to capture process memory maps."
fi

# ── Missing languages notice ─────────────────────────────────────────────────
missing=""
for lang in $LANG_ORDER; do
  [ -d "$RUN_DIR/lang-${lang}" ] || missing="$missing $lang"
done
[ -n "$missing" ] && echo "" && echo "Missing languages:$missing"

echo ""
echo "Run dir:  $RUN_DIR"
echo "Compare:  bash load-test/reports/compare.sh <before-run-dir> $RUN_DIR"
