#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Live load-test monitor — refreshes every INTERVAL seconds.
#
# Shows: CPU, RSS, actor counts, k6 error rate, hot k6 metrics, top processes.
#
# USAGE:
#   bash load-test/monitoring/monitor.sh [OPTIONS]
#
# OPTIONS:
#   --embedded-url URL   Embedded actor server URL  (default: http://localhost:8092)
#   --base-url URL       Main server URL            (default: http://localhost:8091)
#   --interval N         Refresh interval in seconds (default: 15)
#   --report-dir DIR     k6 report dir to tail for live error/latency (default: none)
#   --mem-limit MB       RSS warning threshold in MB (default: 3072)
#   --help               Show this message
#
# EXAMPLES:
#   # Watch a running load test (point at its report dir for live k6 metrics):
#   bash load-test/monitoring/monitor.sh \
#     --report-dir load-test-data/20260727-140000 \
#     --interval 15
#
#   # Watch just the servers, no k6:
#   bash load-test/monitoring/monitor.sh --interval 10
#
# REQUIRES: jq, ps, curl  (no python, no node)

set -euo pipefail

EMBEDDED_URL="http://localhost:8092"
BASE_URL="http://localhost:8091"
INTERVAL=15
REPORT_DIR=""
MEM_LIMIT_MB=3072
TENANT_HEADER="x-tenant-id: default"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --embedded-url) EMBEDDED_URL="$2";  shift 2 ;;
    --base-url)     BASE_URL="$2";      shift 2 ;;
    --interval)     INTERVAL="$2";      shift 2 ;;
    --report-dir)   REPORT_DIR="$2";    shift 2 ;;
    --mem-limit)    MEM_LIMIT_MB="$2";  shift 2 ;;
    --help|-h)
      sed -n '3,30p' "$0" | grep '^#' | sed 's/^# *//'
      exit 0
      ;;
    *) echo "Unknown option: $1"; exit 1 ;;
  esac
done

# ─── Helpers ─────────────────────────────────────────────────────────────────

fetch_json() {
  curl -sf -H "$TENANT_HEADER" "$1" 2>/dev/null || echo "{}"
}

# Exact RSS KB for a single PID; 0 if gone.
_pid_rss_kb() {
  [ -z "$1" ] && echo "0" && return
  ps -o rss= -p "$1" 2>/dev/null | tr -d ' ' || echo "0"
}

# Sum exact RSS KB across all PIDs matching a pattern → MB integer
total_rss_mb() {
  local total=0
  while IFS= read -r pid; do
    [ -z "$pid" ] && continue
    kb=$(_pid_rss_kb "$pid")
    total=$(( total + ${kb:-0} ))
  done < <(pgrep -f "$1" 2>/dev/null || true)
  echo $(( total / 1024 ))
}

# RSS MB + CPU% for a process matching a pattern (first PID, exact RSS)
proc_stats() {
  local pid
  pid=$(pgrep -f "$1" 2>/dev/null | head -1 || true)
  if [ -z "$pid" ]; then echo "0.0 0.0"; return; fi
  local line
  line=$(ps -o pcpu=,rss= -p "$pid" 2>/dev/null || true)
  if [ -z "$line" ]; then echo "0.0 0.0"; return; fi
  # Do NOT tr -d ' ' — that merges the two columns into one token
  echo "$line" | awk '{printf "%.1f %.1f", $2/1024, $1}'
}

# Colorize a number: green if below warn, red if at/above
color_rss() {
  local val="$1" limit="$2"
  if [ "$val" -ge "$limit" ] 2>/dev/null; then
    printf '\033[1;31m%s\033[0m' "$val"
  elif [ "$(echo "$val * 100 / $limit" | bc 2>/dev/null || echo 0)" -ge 75 ] 2>/dev/null; then
    printf '\033[1;33m%s\033[0m' "$val"
  else
    printf '\033[1;32m%s\033[0m' "$val"
  fi
}

color_pct() {
  local val="$1"
  local int_val
  int_val=$(echo "$val" | awk '{printf "%d", $1}')
  if [ "$int_val" -ge 80 ] 2>/dev/null; then
    printf '\033[1;31m%s\033[0m' "$val"
  elif [ "$int_val" -ge 50 ] 2>/dev/null; then
    printf '\033[1;33m%s\033[0m' "$val"
  else
    printf '\033[0;37m%s\033[0m' "$val"
  fi
}

# Latest k6 JSON output: extract key metrics
k6_summary() {
  local dir="$1"
  [ -z "$dir" ] && return
  # Find the most recently written k6 JSON report
  local f
  f=$(ls -t "$dir"/*.json 2>/dev/null | grep -v summary | head -1 || true)
  [ -z "$f" ] && return
  # k6 JSON output is newline-delimited; last Point metric per name
  # Extract p(95), p(99), rate, count from the final summary lines
  jq -r '
    select(.type == "Point") |
    select(.metric | test("perf_echo|perf_kv|perf_error|http_reqs|grpc_req")) |
    "\(.metric) \(.data.value)"
  ' "$f" 2>/dev/null | awk '
    {
      vals[$1] = $2
    }
    END {
      for (k in vals) printf "  %-35s %s\n", k, vals[k]
    }
  ' | sort || true
}

# Pull the last N rows of metrics.csv and compute stats
csv_tail_stats() {
  local dir="$1"
  [ -z "$dir" ] && return
  local csv="$dir/metrics.csv"
  [ -f "$csv" ] || return
  # last 12 rows = ~1 minute at 5s interval
  tail -12 "$csv" | awk -F',' '
    NR > 0 {
      rss_sum += $4; rss_count++
      cpu_sum += $5; cpu_count++
      act_sum += $2; act_count++
      if ($4 > rss_max) rss_max = $4
      if ($5 > cpu_max) cpu_max = $5
    }
    END {
      if (rss_count > 0)
        printf "  Emb RSS  avg=%.0fMB  max=%.0fMB  (last %d samples)\n",
          rss_sum/rss_count, rss_max, rss_count
      if (cpu_count > 0)
        printf "  Emb CPU  avg=%.1f%%  max=%.1f%%\n",
          cpu_sum/cpu_count, cpu_max
      if (act_count > 0)
        printf "  Actors   avg=%.0f\n", act_sum/act_count
    }
  '
}

# Top 5 processes by RSS
top_procs() {
  ps aux 2>/dev/null \
    | awk 'NR>1 && $6 > 50000 {printf "  %6.0fMB  %5.1f%%cpu  %s %s %s\n", $6/1024, $3, $11, $12, $13}' \
    | sort -rn \
    | head -5 || true
}

# k6 processes
k6_procs() {
  ps aux 2>/dev/null | grep -E "k6 run" | grep -v grep \
    | awk '{printf "  PID=%-6s VUs≈running  RSS=%.0fMB  CPU=%.1f%%\n", $2, $6/1024, $3}' || true
}

# ─── Render one screen ───────────────────────────────────────────────────────

render() {
  local ts
  ts="$(date '+%Y-%m-%d %H:%M:%S')"

  # Fetch server info
  local emb_json main_json
  emb_json="$(fetch_json "$EMBEDDED_URL/api/v1/dashboard/summary")"
  main_json="$(fetch_json "$BASE_URL/api/v1/dashboard/summary")"

  local emb_actors main_actors gen_server virtual_actor
  emb_actors=$(echo "$emb_json"  | jq '[.actors_by_type // {} | to_entries[].value] | add // 0' 2>/dev/null || echo 0)
  main_actors=$(echo "$main_json" | jq '[.actors_by_type // {} | to_entries[].value] | add // 0' 2>/dev/null || echo 0)
  gen_server=$(echo "$emb_json"  | jq '.actors_by_type.gen_server // 0' 2>/dev/null || echo 0)
  virtual_actor=$(echo "$emb_json" | jq '.actors_by_type.virtual_actor // 0' 2>/dev/null || echo 0)

  local emb_stats main_stats
  emb_stats=$(proc_stats "perf_actor [0-9]")
  main_stats=$(proc_stats "plexspaces start")
  local emb_rss emb_cpu main_rss main_cpu
  emb_rss=$(echo "$emb_stats"  | awk '{print $1}')
  emb_cpu=$(echo "$emb_stats"  | awk '{print $2}')
  main_rss=$(echo "$main_stats" | awk '{print $1}')
  main_cpu=$(echo "$main_stats" | awk '{print $2}')

  local server_rss
  server_rss=$(( ${emb_rss%.*} + ${main_rss%.*} ))

  # Clear screen and draw
  clear
  printf '\033[1;36m══════════════════════════════════════════════════════════════\033[0m\n'
  printf '\033[1;36m  PlexSpaces Load Monitor   %s   (refresh: %ss)\033[0m\n' "$ts" "$INTERVAL"
  printf '\033[1;36m══════════════════════════════════════════════════════════════\033[0m\n'
  echo ""

  # ── Servers ──
  printf '\033[1;37m── Servers ──────────────────────────────────────────────────\033[0m\n'
  printf '  %-30s  RSS=' "embedded[$EMBEDDED_URL]"; color_rss "$emb_rss" "$MEM_LIMIT_MB"
  printf 'MB  CPU=%s%%  actors=%s (gen_server=%s virtual=%s)\n' \
    "$(color_pct "$emb_cpu")" "$emb_actors" "$gen_server" "$virtual_actor"
  printf '  %-30s  RSS=%sMB  CPU=%s%%  actors=%s\n' \
    "main[$BASE_URL]" "$main_rss" "$(color_pct "$main_cpu")" "$main_actors"
  printf '  %-24s  combined='; color_rss "$server_rss" "$MEM_LIMIT_MB"
  printf 'MB / limit=%sMB\n' "$MEM_LIMIT_MB"
  echo ""

  # ── k6 processes ──
  local k6_lines
  k6_lines="$(k6_procs)"
  if [ -n "$k6_lines" ]; then
    printf '\033[1;37m── k6 processes ────────────────────────────────────────────\033[0m\n'
    echo "$k6_lines"
    echo ""
  fi

  # ── metrics.csv trend (last ~1 min) ──
  if [ -n "$REPORT_DIR" ] && [ -f "$REPORT_DIR/metrics.csv" ]; then
    printf '\033[1;37m── Server trend (last ~1 min from metrics.csv) ─────────────\033[0m\n'
    csv_tail_stats "$REPORT_DIR"
    echo ""
  fi

  # ── Top processes by RSS ──
  printf '\033[1;37m── Top processes by RSS ────────────────────────────────────\033[0m\n'
  top_procs
  echo ""

  # ── Watchdog status ──
  if [ -n "$REPORT_DIR" ] && [ -f "$REPORT_DIR/.oom_kill" ]; then
    printf '\033[1;31m⚠  WATCHDOG FIRED: %s\033[0m\n' "$(cat "$REPORT_DIR/.oom_kill")"
    echo ""
  fi

  printf '\033[0;90m  Ctrl-C to quit  ·  --interval %s  ·  --mem-limit %sMB\033[0m\n' \
    "$INTERVAL" "$MEM_LIMIT_MB"
}

# ─── Loop ────────────────────────────────────────────────────────────────────

trap 'printf "\nMonitor stopped.\n"; exit 0' INT TERM

while true; do
  render
  sleep "$INTERVAL"
done
