#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Collect PlexSpaces metrics every INTERVAL seconds during a load test.
#
# Polls:
#   /api/v1/dashboard/summary   — actor counts by type
#   OS process stats (ps)       — CPU %, RSS MB for plexspaces + perf_actor
#
# MEMORY WATCHDOG: If combined server-side RSS (perf_actor + plexspaces) exceeds
# MEM_LIMIT_MB, all k6 processes are killed and KILL_FILE is written.
# run.sh checks KILL_FILE before each new scenario and aborts the run.
#
# USAGE (called by run.sh automatically):
#   bash monitoring/collect.sh <embedded_url> <base_url> <output_csv> \
#                               [interval] [mem_limit_mb] [kill_file]
#
# Standalone:
#   bash load-test/monitoring/collect.sh http://localhost:8092 http://localhost:8091 /tmp/metrics.csv

set -euo pipefail

EMBEDDED_URL="${1:-http://localhost:8092}"
BASE_URL="${2:-http://localhost:8091}"
OUTPUT_CSV="${3:-./reports/metrics.csv}"
INTERVAL="${4:-5}"
MEM_LIMIT_MB="${5:-3072}"
KILL_FILE="${6:-}"

TENANT_HEADER="x-tenant-id: default"

mkdir -p "$(dirname "$OUTPUT_CSV")"

echo "timestamp,embedded_actors,main_actors,embedded_rss_mb,embedded_cpu_pct,main_rss_mb,main_cpu_pct,gen_server_count,emb_routing_avg_ms,emb_actor_proc_avg_ms,main_routing_avg_ms,main_actor_proc_avg_ms" \
  > "$OUTPUT_CSV"

# ─── Helpers ─────────────────────────────────────────────────────────────────

fetch() {
  local url="$1"
  curl -sf -H "$TENANT_HEADER" "$url" 2>/dev/null || echo "{}"
}

# Exact RSS KB for a single PID using ps -o rss= (works on macOS and Linux).
# Falls back to 0 if the process is gone.
pid_rss_kb() {
  local pid="$1"
  [ -z "$pid" ] && echo "0" && return
  ps -o rss= -p "$pid" 2>/dev/null | tr -d ' ' || echo "0"
}

# Exact RSS KB for all PIDs matching a name pattern (handles multi-process).
# Uses pgrep for reliable PID discovery, then ps -o rss= per PID.
pattern_rss_kb() {
  local pattern="$1"
  local total=0
  while IFS= read -r pid; do
    [ -z "$pid" ] && continue
    kb=$(ps -o rss= -p "$pid" 2>/dev/null | tr -d ' ' || echo "0")
    total=$(( total + ${kb:-0} ))
  done < <(pgrep -f "$pattern" 2>/dev/null || true)
  echo "$total"
}

# RSS MB + CPU% for a single PID
pid_stats() {
  local pid="$1"
  if [ -z "$pid" ]; then echo "0 0"; return; fi
  local line
  # Do NOT tr -d ' ' — that merges cpu and rss columns into one token
  line=$(ps -o pcpu=,rss= -p "$pid" 2>/dev/null || true)
  if [ -z "$line" ]; then echo "0 0"; return; fi
  local cpu rss_mb
  cpu=$(echo "$line" | awk '{print $1}')
  rss_mb=$(echo "$line" | awk '{printf "%.1f", $2/1024}')
  echo "${cpu:-0} ${rss_mb:-0}"
}

# Server-side RSS only (perf_actor + plexspaces).
# k6 is excluded: its memory is bounded and predictable for a fixed VU count.
combined_server_rss_mb() {
  local perf_kb plexspaces_kb
  perf_kb=$(pattern_rss_kb "perf_actor [0-9]")
  plexspaces_kb=$(pattern_rss_kb "plexspaces start")
  echo $(( (perf_kb + plexspaces_kb) / 1024 ))
}

kill_all_k6() {
  local reason="$1"
  printf "\n⚠ WATCHDOG: %s — killing k6 and node capacity scripts\n" "$reason" >&2
  pkill -9 -f "k6 run" 2>/dev/null || true
  pkill -9 -f "node.*actor_capacity" 2>/dev/null || true
  pkill -9 -f "node.*ws_capacity" 2>/dev/null || true
  [ -n "$KILL_FILE" ] && echo "$reason" > "$KILL_FILE"
}

# ─── Main loop ────────────────────────────────────────────────────────────────

printf "Monitoring → %s (every %ss, mem limit %sMB)\nPress Ctrl-C to stop.\n" \
  "$OUTPUT_CSV" "$INTERVAL" "$MEM_LIMIT_MB" >&2

while true; do
  TS="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

  EMB_JSON="$(fetch "$EMBEDDED_URL/api/v1/dashboard/summary")"
  MAIN_JSON="$(fetch "$BASE_URL/api/v1/dashboard/summary")"
  EMB_SVC_JSON="$(fetch "$EMBEDDED_URL/api/v1/dashboard/local-recorder-summary")"
  MAIN_SVC_JSON="$(fetch "$BASE_URL/api/v1/dashboard/local-recorder-summary")"

  # Actor counts via jq; fall back to 0 on parse failure
  EMB_ACTORS=$(echo "$EMB_JSON"  | jq '[.actors_by_type // {} | to_entries[].value] | add // 0' 2>/dev/null || echo 0)
  MAIN_ACTORS=$(echo "$MAIN_JSON" | jq '[.actors_by_type // {} | to_entries[].value] | add // 0' 2>/dev/null || echo 0)
  GEN_SERVER_COUNT=$(echo "$EMB_JSON" | jq '.actors_by_type.gen_server // 0' 2>/dev/null || echo 0)

  # Server-side latency metrics from Prometheus recorder summary
  EMB_ROUTING_AVG=$(echo "$EMB_SVC_JSON"  | jq '.message_routing_latency_avg_ms // 0 | . * 100 | round / 100' 2>/dev/null || echo 0)
  EMB_ACTOR_AVG=$(echo "$EMB_SVC_JSON"    | jq '.actor_message_processing_latency_avg_ms // 0 | . * 100 | round / 100' 2>/dev/null || echo 0)
  MAIN_ROUTING_AVG=$(echo "$MAIN_SVC_JSON" | jq '.message_routing_latency_avg_ms // 0 | . * 100 | round / 100' 2>/dev/null || echo 0)
  MAIN_ACTOR_AVG=$(echo "$MAIN_SVC_JSON"   | jq '.actor_message_processing_latency_avg_ms // 0 | . * 100 | round / 100' 2>/dev/null || echo 0)

  # Use pgrep + ps -o rss= for accurate per-process RSS (not pattern column parsing)
  EMB_PID=$(pgrep -f "perf_actor [0-9]" 2>/dev/null | head -1 || true)
  EMB_STATS=$(pid_stats "$EMB_PID")
  EMB_CPU=$(echo "$EMB_STATS" | awk '{print $1}')
  EMB_RSS=$(echo "$EMB_STATS" | awk '{printf "%.1f", $2}')

  MAIN_PID=$(pgrep -f "plexspaces start" 2>/dev/null | head -1 || true)
  MAIN_STATS=$(pid_stats "$MAIN_PID")
  MAIN_CPU=$(echo "$MAIN_STATS" | awk '{print $1}')
  MAIN_RSS=$(echo "$MAIN_STATS" | awk '{printf "%.1f", $2}')

  echo "$TS,$EMB_ACTORS,$MAIN_ACTORS,$EMB_RSS,$EMB_CPU,$MAIN_RSS,$MAIN_CPU,$GEN_SERVER_COUNT,$EMB_ROUTING_AVG,$EMB_ACTOR_AVG,$MAIN_ROUTING_AVG,$MAIN_ACTOR_AVG" \
    >> "$OUTPUT_CSV"

  # ── Watchdog ────────────────────────────────────────────────────────────────
  TOTAL_RSS=$(combined_server_rss_mb)
  if [ "$TOTAL_RSS" -ge "$MEM_LIMIT_MB" ] 2>/dev/null; then
    kill_all_k6 "total load-test RSS ${TOTAL_RSS}MB >= limit ${MEM_LIMIT_MB}MB"
    MEM_LIMIT_MB=999999   # self-disarm; keep monitoring alive for the final report
  fi

  printf "\r[%s] actors: emb=%s main=%s | CPU: emb=%s%% main=%s%% | RSS: emb=%sMB main=%sMB | routing: emb=%sms main=%sms | actor_proc: emb=%sms main=%sms | total_rss=%sMB/%sMB" \
    "$TS" "$EMB_ACTORS" "$MAIN_ACTORS" "$EMB_CPU" "$MAIN_CPU" "$EMB_RSS" "$MAIN_RSS" \
    "$EMB_ROUTING_AVG" "$MAIN_ROUTING_AVG" "$EMB_ACTOR_AVG" "$MAIN_ACTOR_AVG" \
    "$TOTAL_RSS" "$MEM_LIMIT_MB" >&2

  sleep "$INTERVAL"
done
