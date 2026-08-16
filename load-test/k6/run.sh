#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# PlexSpaces Load Test Runner
#
# USAGE:
#   ./load-test/k6/run.sh [OPTIONS]
#
# OPTIONS:
#   --mode   MODE        Which test: echo | kv | spawn | ws-connections | max-actors | all
#                          ws-connections: rust-embedded only — WS capacity ramp (k6)
#                          max-actors: rust-embedded default, WASM optional (k6)
#                          ws-capacity: custom Node.js script — open WS connections until RSS limit
#                          actor-capacity: custom Node.js script — spawn actors until RSS limit
#   --lang   LANG        Language: rust-embedded | rust-wasm | go | typescript | python | all
#   --vus    N           Concurrent users for smoke run (default: 100)
#   --duration D         Duration per level e.g. 30s, 2m, 5m (default: 2m)
#   --scale-levels       Ramp through 100→500→1000→5000→10000 VUs x --duration each
#   --actor-type TYPE    regular | virtual | both (default: both)
#   --port   PORT        Main server HTTP port (default: 8091)
#   --embedded-port P    Embedded actor port (default: 8092)
#   --mem-limit MB       Kill all tests if total load-test RSS exceeds this (default: 4096 = 4GB)
#   --start-server       Build + start BOTH servers (embedded actor + main WASM server, release);
#                        deploy WASM apps; stop all on exit
#   --warm-count N       Number of pre-warmed actors for the embedded actor (default: 10)
#   --profile            Capture CPU flamegraph periodically (every --profile-interval seconds)
#   --profile-interval N Seconds between flamegraph captures (default: 30, requires --profile)
#   --no-deploy          Skip WASM actor deployment
#   --no-auth            Skip JWT auth (server must run with PLEXSPACES_DISABLE_AUTH=1)
#   --max-wasm-instances N  Cap virtual actor pool for WASM langs (default: 10, ~137MB/Python actor)
#   --output FILE        Write JSON summary to FILE
#   --help               Show this message
#
# QUICK PERF ITERATION CYCLE (single command, no pre-started server):
#   # 1. Smoke (build + start + run + stop, no auth):
#   ./load-test/k6/run.sh --start-server --no-auth --duration 30s --vus 10
#
#   # 2. Establish baseline before a fix:
#   ./load-test/k6/run.sh --start-server --no-auth --mode all --duration 2m --vus 100
#   # Note the run-id printed at the end (e.g. 20260728-143000)
#
#   # 3. Make your fix, then run again to compare:
#   ./load-test/k6/run.sh --start-server --no-auth --mode all --duration 2m --vus 100
#   bash load-test/reports/compare.sh load-test-data/20260728-143000 load-test-data/20260728-150000
#
#   # 4. Scale ramp to find the breaking point:
#   ./load-test/k6/run.sh --start-server --no-auth --scale-levels --duration 5m --profile
#
#   # 5. Compare WASM vs embedded (--start-server starts both servers + deploys apps):
#   ./load-test/k6/run.sh --start-server --no-auth --mode all --lang all --vus 100 --duration 2m
#
# WITH AUTH (production-like):
#   # Start server with JWT cert, runner auto-generates and verifies the token:
#   PLEXSPACES_JWT_PUBLIC_KEY_FILE=certs/jwt-es256-pub.pem \
#     PERF_WARM_COUNT=10 RUST_LOG=warn target/release/perf_actor 8092 &
#   ./load-test/k6/run.sh --mode echo --lang rust-embedded --duration 2m

set -euo pipefail

# Raise fd limit so all child processes (node ws_capacity, k6) can open
# tens of thousands of sockets. macOS default soft limit is 256.
# On macOS the launchd hard limit is "unlimited" but the per-process hard cap
# is kern.maxfilesperproc (default 10240). Raise both the OS limit and shell limit.
if [[ "$(uname)" == "Darwin" ]]; then
  # Raise the launchctl maxfiles limit for this session (requires no special privileges).
  launchctl limit maxfiles 200000 200000 2>/dev/null || true
fi
ulimit -n 200000 2>/dev/null || ulimit -n 65536 2>/dev/null || true
_FD_LIMIT="$(ulimit -n)"
echo "  fd limit: ${_FD_LIMIT} (needed: 200000 for ws_capacity)"
if [[ "$_FD_LIMIT" -lt 10000 ]] 2>/dev/null; then
  echo "  WARNING: fd limit ${_FD_LIMIT} is too low for ws_capacity tests."
  echo "  Run: sudo launchctl limit maxfiles 200000 200000 && ulimit -n 200000"
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
LOAD_TEST_DIR="$REPO_ROOT/load-test"
SCENARIOS_DIR="$SCRIPT_DIR/scenarios"
SCRIPTS_DIR="$LOAD_TEST_DIR/scripts"

# ─── Defaults ────────────────────────────────────────────────────────────────
MODE="echo"
LANG="rust-embedded"
# TRANSPORT variable removed — all scenarios use HTTP only
VUS=100
DURATION="2m"
SCALE_LEVELS=0
ACTOR_TYPE="both"
HTTP_PORT=8091
EMBEDDED_PORT=8092
MEM_LIMIT_MB=4096
START_SERVER=0           # 1 = build+start both servers (embedded + main WASM), deploy apps, stop on exit
CLEAN_START=0            # 1 = wipe ~/plexspaces/apps + DB before starting (fresh deploy)
WARM_COUNT=10
PROFILE=0
PROFILE_INTERVAL=30
PROFILE_MAX_CAPTURES=4    # hard cap: at most this many flamegraphs per run
NO_DEPLOY=0
NO_AUTH=0
MAX_WASM_INSTANCES=3
RUN_ID="$(date +%Y%m%d-%H%M%S)"
OUTPUT=""
MANAGED_SERVER_PID=""
MANAGED_MAIN_PID=""
NODE_CONCURRENCY="${NODE_CONCURRENCY:-200}"   # parallel workers for Node.js capacity scripts

# ─── Parse args ──────────────────────────────────────────────────────────────
while [[ $# -gt 0 ]]; do
  case "$1" in
    --mode)           MODE="$2";            shift 2 ;;
    --lang)           LANG="$2";            shift 2 ;;
    --transport)      echo "NOTE: --transport ignored; gRPC removed, HTTP only"; shift 2 ;;
    --vus)            VUS="$2";             shift 2 ;;
    --duration)       DURATION="$2";        shift 2 ;;
    --scale-levels)   SCALE_LEVELS=1;       shift   ;;
    --actor-type)     ACTOR_TYPE="$2";      shift 2 ;;
    --port)           HTTP_PORT="$2";       shift 2 ;;
    --embedded-port)  EMBEDDED_PORT="$2";   shift 2 ;;
    --mem-limit)      MEM_LIMIT_MB="$2";    shift 2 ;;
    --start-server)   START_SERVER=1;       shift   ;;
    --clean)          CLEAN_START=1;       shift   ;;
    --warm-count)     WARM_COUNT="$2";     shift 2 ;;
    --profile)        PROFILE=1;            shift   ;;
    --profile-interval) PROFILE_INTERVAL="$2"; shift 2 ;;
    --output)         OUTPUT="$2";          shift 2 ;;
    --no-deploy)      NO_DEPLOY=1;          shift   ;;
    --no-auth)        NO_AUTH=1;            shift   ;;
    --max-wasm-instances) MAX_WASM_INSTANCES="$2"; shift 2 ;;
    --concurrency)    NODE_CONCURRENCY="$2"; shift 2 ;;
    --help|-h)
      sed -n '3,58p' "$0" | grep '^#' | sed 's/^# *//'
      exit 0
      ;;
    *) echo "Unknown option: $1"; exit 1 ;;
  esac
done

# ─── Derived paths ───────────────────────────────────────────────────────────
REPORT_DIR="$REPO_ROOT/load-test-data/$RUN_ID"
mkdir -p "$REPORT_DIR"
OUTPUT="${OUTPUT:-$REPORT_DIR/summary.json}"

BASE_URL="http://localhost:$HTTP_PORT"
EMBEDDED_URL="http://localhost:$EMBEDDED_PORT"

# Expand language shorthands.
# --lang all: all five languages, each in its own isolated run
# rust-embedded: native actor (its own binary, own port)
# rust-wasm / go / typescript / python: deployed to plexspaces WASM server
ALL_LANGS="rust-embedded rust-wasm go typescript python"
RUN_LANGS="$LANG"
[ "$LANG" = "all" ] && RUN_LANGS="$ALL_LANGS"

echo "╔═══════════════════════════════════════════════════════════════╗"
echo "║        PlexSpaces Load Test Runner                            ║"
echo "╠═══════════════════════════════════════════════════════════════╣"
printf "║  Run ID:   %-51s║\n" "$RUN_ID"
printf "║  Mode:     %-10s  Lang: %-10s                     ║\n" "$MODE" "$LANG"
if [ "$SCALE_LEVELS" -eq 1 ]; then
  printf "║  Scale:    100→500→1000→5000→10000 VUs × %-18s║\n" "$DURATION each"
else
  printf "║  Load:     %-5s VUs × %-36s║\n" "$VUS" "$DURATION"
fi
printf "║  Servers:  HTTP:%-6s  Embedded:%-6s              ║\n" "$HTTP_PORT" "$EMBEDDED_PORT"
printf "║  Mem limit:%-51s║\n" "${MEM_LIMIT_MB}MB (watchdog kills k6 if exceeded)"
printf "║  Output:   %-51s║\n" "$REPORT_DIR"
echo "╚═══════════════════════════════════════════════════════════════╝"

# ─── Managed servers (--start-server) ────────────────────────────────────────
# Starts both the embedded actor and the main WASM server (both in release mode),
# then deploys WASM apps. All processes are stopped on EXIT/INT/TERM.

# Send SIGTERM; if still alive after 3s, SIGKILL.
_stop_pid() {
  local pid="$1" name="$2"
  kill -0 "$pid" 2>/dev/null || return 0
  echo "  Stopping $name (PID $pid)..."
  kill "$pid" 2>/dev/null || true
  local i=0
  while [ $i -lt 6 ] && kill -0 "$pid" 2>/dev/null; do
    sleep 0.5; i=$((i+1))
  done
  if kill -0 "$pid" 2>/dev/null; then
    kill -9 "$pid" 2>/dev/null || true
  fi
}

stop_managed_server() {
  [ -n "$MANAGED_SERVER_PID" ] && _stop_pid "$MANAGED_SERVER_PID" "embedded actor"
  MANAGED_SERVER_PID=""
}

stop_managed_main_server() {
  [ -n "$MANAGED_MAIN_PID" ] && _stop_pid "$MANAGED_MAIN_PID" "main WASM server"
  MANAGED_MAIN_PID=""
}

# ─── Per-language isolated server lifecycle ───────────────────────────────────
# Wipe ~/plexspaces/*, start a fresh server for ONE language, deploy its app.
# Used by the per-language loop when START_SERVER=1.
_clean_plexspaces_state() {
  echo "  Clearing ~/plexspaces state for clean server start..."
  rm -rf "${HOME}/plexspaces/apps" 2>/dev/null || true
  local db_dir="${HOME}/plexspaces/db"
  rm -f "${db_dir}/plexspaces.db" "${db_dir}/plexspaces.db-wal" "${db_dir}/plexspaces.db-shm" 2>/dev/null || true
  rm -f "${db_dir}/plexspaces-${HTTP_PORT}.db" "${db_dir}/plexspaces-${HTTP_PORT}.db-wal" "${db_dir}/plexspaces-${HTTP_PORT}.db-shm" 2>/dev/null || true
  rm -f /tmp/app_*.zip 2>/dev/null || true
  echo "  Done."
}

start_embedded_actor() {
  local server_log="${1:-/dev/null}"
  local PERF_ACTOR_BIN="$REPO_ROOT/target/release/perf_actor"
  local PERF_ACTOR_SRC="$LOAD_TEST_DIR/apps/rust/embedded/perf_actor"
  lsof -ti:"$EMBEDDED_PORT" 2>/dev/null | xargs kill -9 2>/dev/null || true
  sleep 0.3
  if [ ! -f "$PERF_ACTOR_BIN" ]; then
    echo "Building embedded actor (release)..."
    CARGO_TARGET_DIR="$REPO_ROOT/target" \
      cargo build --release --manifest-path "$PERF_ACTOR_SRC/Cargo.toml"
  fi
  local HEARTBEAT_INTERVAL_MS="${PLEXSPACES_HEARTBEAT_INTERVAL_MS:-15000}"
  # PERF_MAX_VIRTUAL_POOL: cap for LRU eviction of virtual actors. 0 = unlimited.
  # Default 0 (unlimited) so lifecycle and throughput tests never hit an artificial cap.
  # Capacity tests pass explicit values via _restart_embedded_server_with_pool().
  local MAX_VIRTUAL_POOL="${PERF_MAX_VIRTUAL_POOL-0}"
  echo "Starting embedded actor on port $EMBEDDED_PORT (warm_count=$WARM_COUNT  max_virtual_pool=$MAX_VIRTUAL_POOL) → $server_log"
  if [ "$NO_AUTH" -eq 1 ]; then
    RUST_LOG=info PERF_WARM_COUNT="$WARM_COUNT" PLEXSPACES_DISABLE_AUTH=1 \
      PLEXSPACES_HEARTBEAT_INTERVAL_MS="$HEARTBEAT_INTERVAL_MS" \
      PERF_MAX_VIRTUAL_POOL="$MAX_VIRTUAL_POOL" \
      "$PERF_ACTOR_BIN" "$EMBEDDED_PORT" >> "$server_log" 2>&1 &
  else
    RUST_LOG=info PERF_WARM_COUNT="$WARM_COUNT" \
      PLEXSPACES_HEARTBEAT_INTERVAL_MS="$HEARTBEAT_INTERVAL_MS" \
      PERF_MAX_VIRTUAL_POOL="$MAX_VIRTUAL_POOL" \
      "$PERF_ACTOR_BIN" "$EMBEDDED_PORT" >> "$server_log" 2>&1 &
  fi
  MANAGED_SERVER_PID=$!
  _wait_for_http "http://localhost:$EMBEDDED_PORT/api/v1/dashboard/summary" "Embedded actor" "$MANAGED_SERVER_PID"
}

start_wasm_server() {
  local server_log="${1:-/dev/null}"
  local PLEXSPACES_BIN="$REPO_ROOT/target/release/plexspaces"
  lsof -ti:"$HTTP_PORT" 2>/dev/null | xargs kill -9 2>/dev/null || true
  sleep 0.3
  if [ ! -f "$PLEXSPACES_BIN" ]; then
    echo "Building plexspaces (release)..."
    cargo build --release -p plexspaces-cli
  fi
  local HEARTBEAT_INTERVAL_MS="${PLEXSPACES_HEARTBEAT_INTERVAL_MS:-15000}"
  echo "Starting main WASM server (release) on port $HTTP_PORT → $server_log"
  # Log level: warn globally + debug for actor/WASM/application layers so errors are
  # visible in server.log without the volume of info-level journal/sqlite chatter.
  # Matches scripts/server.sh pattern.
  local _rust_log="warn,plexspaces_actor=debug,plexspaces_node=debug,plexspaces_services=debug,plexspaces_wasm_runtime=debug,plexspaces_application=debug"
  if [ "$NO_AUTH" -eq 1 ]; then
    env -u PLEXSPACES_JWT_SECRET -u COMMON_AUTH_JWT_SECRET \
        RUST_LOG="$_rust_log" \
        RUST_BACKTRACE=1 \
        WASMTIME_BACKTRACE_DETAILS=1 \
        PLEXSPACES_SAVE_WASM_APPS=1 \
        PLEXSPACES_JWT_PRIVATE_KEY_FILE="${PLEXSPACES_JWT_PRIVATE_KEY_FILE:-$REPO_ROOT/certs/jwt-es256.pem}" \
        PLEXSPACES_DISABLE_AUTH=1 \
        PLEXSPACES_HEARTBEAT_INTERVAL_MS="$HEARTBEAT_INTERVAL_MS" \
      "$PLEXSPACES_BIN" start \
        --node-id "load-test-node-${HTTP_PORT}" \
        --listen-addr "0.0.0.0:${HTTP_PORT}" \
        --release-config "$REPO_ROOT/release.yaml" >> "$server_log" 2>&1 &
  else
    env -u PLEXSPACES_JWT_SECRET -u COMMON_AUTH_JWT_SECRET \
        RUST_LOG="$_rust_log" \
        RUST_BACKTRACE=1 \
        WASMTIME_BACKTRACE_DETAILS=1 \
        PLEXSPACES_SAVE_WASM_APPS=1 \
        PLEXSPACES_JWT_PRIVATE_KEY_FILE="${PLEXSPACES_JWT_PRIVATE_KEY_FILE:-$REPO_ROOT/certs/jwt-es256.pem}" \
        PLEXSPACES_DISABLE_AUTH=0 \
        PLEXSPACES_HEARTBEAT_INTERVAL_MS="$HEARTBEAT_INTERVAL_MS" \
      "$PLEXSPACES_BIN" start \
        --node-id "load-test-node-${HTTP_PORT}" \
        --listen-addr "0.0.0.0:${HTTP_PORT}" \
        --release-config "$REPO_ROOT/release-auth.yaml" >> "$server_log" 2>&1 &
  fi
  MANAGED_MAIN_PID=$!
  _wait_for_http "http://localhost:$HTTP_PORT/api/v1/dashboard/summary" "Main WASM server" "$MANAGED_MAIN_PID"
}

# Deploy and wait for ready for a single WASM language.
deploy_wasm_lang() {
  local lang_item="$1"
  local APP_DIR WASM
  case "$lang_item" in
    python)
      APP_DIR="$LOAD_TEST_DIR/apps/python/apps/perf_actor"
      WASM="$APP_DIR/perf_actor.wasm"
      ;;
    go)
      APP_DIR="$LOAD_TEST_DIR/apps/go/apps/perf_actor"
      WASM="$REPO_ROOT/target/load-test/go/perf_actor/perf_actor.wasm"
      ;;
    typescript)
      APP_DIR="$LOAD_TEST_DIR/apps/typescript/apps/perf_actor"
      WASM="$REPO_ROOT/target/load-test/typescript/perf_actor/perf_actor.wasm"
      ;;
    rust-wasm)
      APP_DIR="$LOAD_TEST_DIR/apps/rust/apps/perf_actor"
      WASM="$REPO_ROOT/target/load-test/rust/perf_actor/perf_actor.wasm"
      ;;
    *) echo "  ERROR: unknown WASM lang $lang_item"; return 1 ;;
  esac
  if [ ! -f "$APP_DIR/build.sh" ]; then
    echo "  ERROR: build.sh not found: $APP_DIR/build.sh"; return 1
  fi
  if ! (bash "$APP_DIR/build.sh" 2>&1); then
    echo "  ERROR: $lang_item build failed"; return 1
  fi
  if [ ! -f "$WASM" ]; then
    echo "  ERROR: $lang_item build ok but $WASM not found"; return 1
  fi
  deploy_app "$APP_DIR" "perf-$lang_item" "$WASM" || return 1
  # Wait for app to reach running status
  local _deadline=$((SECONDS + 60))
  local _app_id="perf-${lang_item}"
  echo "  Waiting for $lang_item app to become ready..."
  while [ $SECONDS -lt $_deadline ]; do
    local _apps_json
    _apps_json=$(curl -s -H "x-tenant-id: default" \
      "$BASE_URL/api/v1/dashboard/applications" 2>/dev/null || true)
    local _status
    _status=$(echo "$_apps_json" | python3 -c "
import json, sys
try:
  data = json.load(sys.stdin)
  apps = data.get('applications', [])
  for a in apps:
    if a.get('name') == '$_app_id' and a.get('status', 0) == 3:
      print(3); sys.exit(0)
  print(0)
except Exception:
  print(0)
" 2>/dev/null || echo "0")
    if [ "$_status" = "3" ]; then
      echo "  ✓ $lang_item app ready"
      return 0
    fi
    sleep 1
  done
  echo "  WARNING: $lang_item app not ready after 60s — proceeding"
}

# Run tests for one language with a fully isolated server lifecycle (clean → start → deploy → k6 → stop).
# When START_SERVER=0, just runs k6 (existing behaviour).
# Current language dir (set inside run_lang_isolated, used by run_k6 and collect.sh).
CURRENT_LANG_DIR=""
CURRENT_LANG_MONITOR_PID=""

stop_lang_monitor() {
  if [ -n "$CURRENT_LANG_MONITOR_PID" ]; then
    kill "$CURRENT_LANG_MONITOR_PID" 2>/dev/null || true
    CURRENT_LANG_MONITOR_PID=""
  fi
}

run_lang_isolated() {
  local lang="$1"; shift
  local run_fn="$1"; shift

  # Per-language output directory — all logs, metrics, snapshots land here.
  CURRENT_LANG_DIR="$REPORT_DIR/lang-${lang}"
  mkdir -p "$CURRENT_LANG_DIR"

  if [ "$START_SERVER" -ne 1 ]; then
    "$run_fn" "$lang" "$@"
    return
  fi

  echo ""
  echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
  echo "  Isolated run: $lang"
  echo "  Output dir:   $CURRENT_LANG_DIR"
  echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

  # Stop any previous managed servers and monitors from the prior language
  stop_lang_monitor
  stop_managed_server
  stop_managed_main_server

  _clean_plexspaces_state

  # Start server and capture its stdout+stderr to a log in the lang dir.
  local server_log="$CURRENT_LANG_DIR/server.log"
  if [ "$lang" = "rust-embedded" ]; then
    start_embedded_actor "$server_log"
  else
    start_wasm_server "$server_log"
    deploy_wasm_lang "$lang" || {
      echo "  ERROR: deploy failed for $lang — skipping test"
      stop_managed_main_server
      return 1
    }
  fi

  # Start per-language monitoring (CSV + snapshots in lang dir).
  local lang_csv="$CURRENT_LANG_DIR/metrics.csv"
  local lang_kill_file="$CURRENT_LANG_DIR/.oom_kill"
  if [ -f "$LOAD_TEST_DIR/monitoring/collect.sh" ]; then
    bash "$LOAD_TEST_DIR/monitoring/collect.sh" \
      "$EMBEDDED_URL" "$BASE_URL" "$lang_csv" \
      5 "$MEM_LIMIT_MB" "$lang_kill_file" &
    CURRENT_LANG_MONITOR_PID=$!
    echo "  Lang monitor started (PID $CURRENT_LANG_MONITOR_PID) → $lang_csv"
  fi

  "$run_fn" "$lang" "$@"
  local _exit=$?

  stop_lang_monitor

  if [ "$lang" = "rust-embedded" ]; then
    stop_managed_server
  else
    stop_managed_main_server
    sleep 3
  fi
  return $_exit
}

# Check if the managed server for the given lang is still alive; restart if not.
# For WASM langs this also re-deploys the app since a fresh server loses it.
_ensure_server_alive() {
  local lang="$1"
  [ "$START_SERVER" -ne 1 ] && return
  local _check_url
  if [ "$lang" = "rust-embedded" ]; then
    _check_url="$EMBEDDED_URL/api/v1/dashboard/summary"
  else
    _check_url="$BASE_URL/api/v1/dashboard/summary"
  fi
  if curl -sf -o /dev/null "$_check_url" -H "x-tenant-id: default" \
       ${PLEXSPACES_TEST_TOKEN:+-H "Authorization: Bearer $PLEXSPACES_TEST_TOKEN"} 2>/dev/null; then
    return 0  # still alive
  fi
  echo "  WARNING: server not responding at $_check_url — restarting for $lang"
  local server_log="${CURRENT_LANG_DIR:-$REPORT_DIR}/server.log"
  stop_lang_monitor
  if [ "$lang" = "rust-embedded" ]; then
    stop_managed_server
    start_embedded_actor "$server_log"
  else
    stop_managed_main_server
    start_wasm_server "$server_log"
    deploy_wasm_lang "$lang" || echo "  WARNING: redeploy failed for $lang after restart"
  fi
  # Resume monitoring
  local lang_csv="${CURRENT_LANG_DIR:-$REPORT_DIR}/metrics.csv"
  local lang_kill_file="${CURRENT_LANG_DIR:-$REPORT_DIR}/.oom_kill"
  if [ -f "$LOAD_TEST_DIR/monitoring/collect.sh" ]; then
    bash "$LOAD_TEST_DIR/monitoring/collect.sh" \
      "$EMBEDDED_URL" "$BASE_URL" "$lang_csv" \
      5 "$MEM_LIMIT_MB" "$lang_kill_file" &
    CURRENT_LANG_MONITOR_PID=$!
  fi
}

# Restart the embedded actor server with a specific PERF_MAX_VIRTUAL_POOL value.
# No-op when START_SERVER=0 (externally managed server — caller must set the env themselves).
# Appends to the existing server.log so the full run history is preserved.
_restart_embedded_server_with_pool() {
  local pool="$1"
  [ "$START_SERVER" -ne 1 ] && return
  [ -z "$CURRENT_LANG_DIR" ] && return
  echo ""
  echo "  ── Restarting embedded server with PERF_MAX_VIRTUAL_POOL=${pool} ──"
  stop_lang_monitor
  stop_managed_server
  local server_log="$CURRENT_LANG_DIR/server.log"
  PERF_MAX_VIRTUAL_POOL="$pool" start_embedded_actor "$server_log"
  # Resume monitoring after restart
  local lang_csv="$CURRENT_LANG_DIR/metrics.csv"
  local lang_kill_file="$CURRENT_LANG_DIR/.oom_kill"
  if [ -f "$LOAD_TEST_DIR/monitoring/collect.sh" ]; then
    bash "$LOAD_TEST_DIR/monitoring/collect.sh" \
      "$EMBEDDED_URL" "$BASE_URL" "$lang_csv" \
      5 "$MEM_LIMIT_MB" "$lang_kill_file" &
    CURRENT_LANG_MONITOR_PID=$!
  fi
}

_wait_for_http() {
  local url="$1" name="$2" pid="$3"
  for _i in $(seq 1 30); do
    sleep 0.5
    if curl -sf -o /dev/null "$url" -H "x-tenant-id: default" 2>/dev/null; then
      echo "  $name ready (PID $pid)"
      return 0
    fi
  done
  echo "  WARNING: $name did not become ready within 15s"
}

if [ "$START_SERVER" -eq 1 ]; then
  # Per-language isolation: each language gets its own clean start inside the mode loops.
  # Build binaries now (once) so the per-lang cycle only starts/stops, not rebuilds.
  PERF_ACTOR_BIN="$REPO_ROOT/target/release/perf_actor"
  PERF_ACTOR_SRC="$LOAD_TEST_DIR/apps/rust/embedded/perf_actor"
  PLEXSPACES_BIN="$REPO_ROOT/target/release/plexspaces"

  _needs_embedded_build=0
  _needs_wasm_build=0
  echo "$RUN_LANGS" | grep -qw "rust-embedded" && _needs_embedded_build=1
  for _rl in $RUN_LANGS; do [ "$_rl" != "rust-embedded" ] && _needs_wasm_build=1; done

  if [ "$_needs_embedded_build" -eq 1 ] && [ ! -f "$PERF_ACTOR_BIN" ]; then
    echo "Building embedded actor (release)..."
    CARGO_TARGET_DIR="$REPO_ROOT/target" \
      cargo build --release --manifest-path "$PERF_ACTOR_SRC/Cargo.toml"
  fi
  if [ "$_needs_wasm_build" -eq 1 ] && [ ! -f "$PLEXSPACES_BIN" ]; then
    echo "Building plexspaces (release)..."
    cargo build --release -p plexspaces-cli
  fi

  trap 'declare -f stop_lang_monitor         >/dev/null 2>&1 && stop_lang_monitor;
        declare -f stop_monitoring             >/dev/null 2>&1 && stop_monitoring;
        declare -f stop_profiling              >/dev/null 2>&1 && stop_profiling;
        declare -f stop_managed_server         >/dev/null 2>&1 && stop_managed_server;
        declare -f stop_managed_main_server    >/dev/null 2>&1 && stop_managed_main_server' EXIT INT TERM
fi

# macOS ephemeral port exhaustion is prevented by using a per-VU _connected flag
# in each k6 scenario (one TCP connection per VU, no reconnect per iteration).
# No sudo/sysctl needed.

# ─── Check prerequisites ─────────────────────────────────────────────────────
if ! command -v k6 >/dev/null 2>&1; then
  echo "ERROR: k6 not found. Install with: brew install k6"
  exit 1
fi

# Node.js is needed for ws-capacity and actor-capacity modes.
if [[ "$MODE" == "ws-capacity" || "$MODE" == "actor-capacity" ]]; then
  if ! command -v node >/dev/null 2>&1; then
    echo "ERROR: node not found. Install Node.js 18+ to run capacity scripts."
    exit 1
  fi
  # Install npm deps if not present (ws package).
  if [ ! -d "$SCRIPTS_DIR/node_modules" ]; then
    echo "Installing Node.js dependencies (npm install in load-test/scripts/)..."
    npm --prefix "$SCRIPTS_DIR" install --quiet
  fi
fi

# Compute whether we need a WASM server (used by deploy check below).
_needs_wasm_check=0
for _rl in $RUN_LANGS; do [ "$_rl" != "rust-embedded" ] && _needs_wasm_check=1; done

# When START_SERVER=1, servers start per-language inside run_lang_isolated — skip pre-flight checks.
if [ "$START_SERVER" -ne 1 ]; then
  # Check embedded actor (always required for rust-embedded tests)
  _needs_embedded_check=0
  echo "$RUN_LANGS" | grep -qw "rust-embedded" && _needs_embedded_check=1
  if [ "$_needs_embedded_check" -eq 1 ]; then
    if ! curl -sf -o /dev/null "$EMBEDDED_URL/api/v1/dashboard/summary" \
         -H "x-tenant-id: default" 2>/dev/null; then
      echo "WARNING: Embedded actor not responding at $EMBEDDED_URL"
      echo "  Start it with:"
      echo "    PERF_WARM_COUNT=10 RUST_LOG=warn target/release/perf_actor $EMBEDDED_PORT &"
    fi
  fi

  # Check main server (needed for any language other than pure rust-embedded)
  if [ "$_needs_wasm_check" -eq 1 ]; then
    if ! curl -sf -o /dev/null -w "%{http_code}" "$BASE_URL/" 2>/dev/null | grep -q "^[23]"; then
      echo "WARNING: Main server not responding at $BASE_URL"
    fi
  fi
fi

# JWT token — auth is ON by default; use --no-auth only when server runs with PLEXSPACES_DISABLE_AUTH=1
if [ "$NO_AUTH" -eq 0 ]; then
  if [ -z "${PLEXSPACES_TEST_TOKEN:-}" ] && [ -f "$REPO_ROOT/scripts/gen-test-jwt.sh" ]; then
    source ~/venv/bin/activate 2>/dev/null || true
    JWT_OUTPUT="$(PLEXSPACES_JWT_PRIVATE_KEY_FILE="$REPO_ROOT/certs/jwt-es256.pem" \
      "$REPO_ROOT/scripts/gen-test-jwt.sh" 2>/dev/null)" || true
    eval "$JWT_OUTPUT" 2>/dev/null || true
  fi
  # When START_SERVER=1, servers start per-language — skip live verification here.
  if [ "$START_SERVER" -ne 1 ] && [ -n "${PLEXSPACES_TEST_TOKEN:-}" ]; then
    CHECK_URL="$EMBEDDED_URL/api/v1/dashboard/summary"
    HTTP_CODE=$(curl -sf -o /dev/null -w "%{http_code}" "$CHECK_URL" \
      -H "Authorization: Bearer $PLEXSPACES_TEST_TOKEN" 2>/dev/null || echo "000")
    if [[ "$HTTP_CODE" != "200" && "$HTTP_CODE" != "204" ]]; then
      echo "ERROR: JWT auth verification failed (HTTP $HTTP_CODE)."
      echo "  Re-run with PLEXSPACES_TEST_TOKEN='' to regenerate, or use --no-auth"
      exit 1
    fi
    echo "  Auth: JWT verified OK (HTTP $HTTP_CODE)"
  elif [ -z "${PLEXSPACES_TEST_TOKEN:-}" ]; then
    echo "WARNING: No JWT token available. Requests will be sent without auth."
  fi
fi

# ─── Build common k6 env args ────────────────────────────────────────────────
COMMON_ARGS=(
  -e "BASE_URL=$BASE_URL"
  -e "EMBEDDED_URL=$EMBEDDED_URL"
  -e "WARM_COUNT=$WARM_COUNT"
)
[ "$NO_AUTH" -eq 1 ] && COMMON_ARGS+=(-e "PLEXSPACES_NO_AUTH=1")
[ "$NO_AUTH" -eq 0 ] && [ -n "${PLEXSPACES_TEST_TOKEN:-}" ] && COMMON_ARGS+=(-e "PLEXSPACES_TEST_TOKEN=$PLEXSPACES_TEST_TOKEN")
COMMON_ARGS+=(-e "MAX_WASM_INSTANCES=$MAX_WASM_INSTANCES")

# ─── Monitoring + memory watchdog ────────────────────────────────────────────
MONITOR_PID=""
KILL_FILE="$REPORT_DIR/.oom_kill"

start_monitoring() {
  if [ -f "$LOAD_TEST_DIR/monitoring/collect.sh" ]; then
    bash "$LOAD_TEST_DIR/monitoring/collect.sh" \
      "$EMBEDDED_URL" "$BASE_URL" "$REPORT_DIR/metrics.csv" \
      5 "$MEM_LIMIT_MB" "$KILL_FILE" &
    MONITOR_PID=$!
    echo "  Monitoring started (PID $MONITOR_PID, mem limit ${MEM_LIMIT_MB}MB) → $REPORT_DIR/metrics.csv"
  fi
}
stop_monitoring() {
  if [ -n "$MONITOR_PID" ]; then
    kill "$MONITOR_PID" 2>/dev/null || true
    MONITOR_PID=""
  fi
}

# Check if watchdog fired; call before starting a new scenario.
check_oom() {
  if [ -f "$KILL_FILE" ]; then
    echo ""
    echo "╔═══════════════════════════════════════════════════════════════╗"
    echo "║  ⚠ MEMORY WATCHDOG FIRED — load test aborted                 ║"
    printf "║  Reason: %-53s║\n" "$(cat "$KILL_FILE")"
    echo "╠═══════════════════════════════════════════════════════════════╣"
    printf "║  Report saved: %-47s║\n" "$REPORT_DIR"
    echo "╚═══════════════════════════════════════════════════════════════╝"
    stop_monitoring
    stop_profiling
    exit 2
  fi
}

# ─── Flamegraph ──────────────────────────────────────────────────────────────
# --profile: capture a CPU flamegraph every PROFILE_INTERVAL seconds for the
# lifetime of the test run.  Each capture is a 30s sample saved to the report
# dir as cpu-profile-<pid>-<seq>.json (samply) or cpu-sample-<pid>-<seq>.txt
# (macOS built-in 'sample').  The loop runs in the background and exits when
# stop_profiling() is called (via the PROFILE_STOP_FILE sentinel).
PROFILE_INTERVAL=${PROFILE_INTERVAL:-30}   # seconds between consecutive captures; set by --profile-interval
PROFILE_STOP_FILE=""      # set in start_profiling; checked by the loop

start_profiling() {
  if [ "$PROFILE" -eq 1 ] && [ -f "$LOAD_TEST_DIR/profiling/capture.sh" ]; then
    PROFILE_STOP_FILE="$REPORT_DIR/.profile_stop"
    echo "  Flamegraph: capturing every ${PROFILE_INTERVAL}s (stop file: $PROFILE_STOP_FILE)"
    (
      seq=0
      # Wait for a server process to appear before starting
      for _ in $(seq 1 12); do
        pgrep -f "perf_actor [0-9]" >/dev/null 2>&1 && break
        pgrep -f "plexspaces start" >/dev/null 2>&1 && break
        sleep 5
      done
      while [ ! -f "$PROFILE_STOP_FILE" ] && [ "$seq" -lt "$PROFILE_MAX_CAPTURES" ]; do
        seq=$(( seq + 1 ))
        CAPTURE_DIR="$REPORT_DIR/flamegraph-${seq}"
        mkdir -p "$CAPTURE_DIR"
        bash "$LOAD_TEST_DIR/profiling/capture.sh" "$CAPTURE_DIR" 30 2>/dev/null &
        # Wait for the next interval (or stop signal)
        end=$(( $(date +%s) + PROFILE_INTERVAL ))
        while [ "$(date +%s)" -lt "$end" ] && [ ! -f "$PROFILE_STOP_FILE" ]; do
          sleep 2
        done
      done
    ) &
  fi
}

stop_profiling() {
  if [ -n "$PROFILE_STOP_FILE" ]; then
    touch "$PROFILE_STOP_FILE"
  fi
}

# ─── Pre-warm WASM actors from bash before k6 starts ────────────────────────
# Sends warmup requests directly (not via k6) so the WASM module is guaranteed
# to be in the kernel page cache when k6 VUs start. This matters when multiple
# languages run back-to-back: previous tests can evict the next lang's WASM pages.
prewarm_wasm() {
  local app_id="$1" pool="$2" rounds="${3:-5}"
  [[ "$NO_AUTH" -eq 1 ]] || return 0  # Only needed in no-auth test mode
  local auth_hdr=""
  [ -n "${PLEXSPACES_TEST_TOKEN:-}" ] && auth_hdr="-H 'Authorization: Bearer $PLEXSPACES_TEST_TOKEN'"
  echo "  Pre-warming $app_id (pool=$pool, rounds=$rounds)..."
  for i in $(seq 1 "$pool"); do
    for r in $(seq 1 "$rounds"); do
      curl -sf -o /dev/null -X POST \
        "$BASE_URL/api/v1/actors/$app_id/kv-vu${i}:PerfActor/ask?timeout=35" \
        -H "x-tenant-id: default" \
        ${PLEXSPACES_TEST_TOKEN:+-H "Authorization: Bearer $PLEXSPACES_TEST_TOKEN"} \
        -H "Content-Type: application/json" \
        -d "{\"op\":\"kv_put\",\"key\":\"_prewarm_${i}_${r}\",\"value\":\"w\"}" 2>/dev/null || \
        echo "    [warn] prewarm $app_id kv-vu${i} round $r failed"
    done
  done
  echo "  Pre-warm done."
}

# ─── Snapshot helpers ────────────────────────────────────────────────────────

# Convert a duration string (30s, 2m, 5m, 1h) to integer seconds.
duration_to_seconds() {
  local d="$1"
  case "$d" in
    *h) echo $(( ${d%h} * 3600 )) ;;
    *m) echo $(( ${d%m} * 60 )) ;;
    *s) echo "${d%s}" ;;
    *)  echo "$d" ;;   # already numeric
  esac
}

# Take a single Prometheus / metrics-table snapshot into the lang snapshots dir.
# Called from a background subshell inside run_k6 after sleeping to midpoint.
take_snap() {
  local snap_dir="$1"
  local label="$2"
  mkdir -p "$snap_dir"
  local TS; TS="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  local snap_file="$snap_dir/snap_${label}_${TS//:/-}.json"

  local EMB_JSON MAIN_JSON EMB_ACTORS MAIN_ACTORS EMB_RSS MAIN_RSS
  EMB_JSON="$(curl -sf -H "x-tenant-id: default" "$EMBEDDED_URL/api/v1/dashboard/summary" 2>/dev/null || echo '{}')"
  MAIN_JSON="$(curl -sf -H "x-tenant-id: default" "$BASE_URL/api/v1/dashboard/summary" 2>/dev/null || echo '{}')"
  EMB_ACTORS=$(echo "$EMB_JSON"  | jq '[.actors_by_type // {} | to_entries[].value] | add // 0' 2>/dev/null || echo 0)
  MAIN_ACTORS=$(echo "$MAIN_JSON" | jq '[.actors_by_type // {} | to_entries[].value] | add // 0' 2>/dev/null || echo 0)

  local EMB_PID MAIN_PID EMB_RSS_MB MAIN_RSS_MB
  EMB_PID=$(pgrep -f "perf_actor [0-9]" 2>/dev/null | head -1 || true)
  MAIN_PID=$(pgrep -f "plexspaces start" 2>/dev/null | head -1 || true)
  EMB_RSS_MB=$(ps -o rss= -p "$EMB_PID" 2>/dev/null | awk '{printf "%.1f", $1/1024}' || echo "0")
  MAIN_RSS_MB=$(ps -o rss= -p "$MAIN_PID" 2>/dev/null | awk '{printf "%.1f", $1/1024}' || echo "0")

  {
    echo "{"
    echo "  \"timestamp\": \"$TS\","
    echo "  \"main_actors\": $MAIN_ACTORS,"
    echo "  \"main_rss_mb\": $MAIN_RSS_MB,"
    echo "  \"embedded_actors\": $EMB_ACTORS,"
    echo "  \"embedded_rss_mb\": $EMB_RSS_MB,"
    printf '  "main_metrics_table": '
    curl -sf -H "x-tenant-id: default" "$BASE_URL/api/v1/dashboard/metrics-table" 2>/dev/null || echo "null"
    echo ","
    printf '  "embedded_metrics_table": '
    curl -sf -H "x-tenant-id: default" "$EMBEDDED_URL/api/v1/dashboard/metrics-table" 2>/dev/null || echo "null"
    echo ","
    printf '  "main_summary": '
    curl -sf -H "x-tenant-id: default" "$BASE_URL/api/v1/dashboard/summary" 2>/dev/null || echo "null"
    echo ""
    echo "}"
  } > "$snap_file" 2>/dev/null || true
}

# ─── Run a single k6 scenario ────────────────────────────────────────────────
# Logs go to CURRENT_LANG_DIR if set (per-lang isolation), else REPORT_DIR.
run_k6() {
  check_oom   # bail immediately if watchdog already fired

  local scenario="$1"; shift
  local label="$1"; shift
  local extra_args=("$@")
  local out_dir="${CURRENT_LANG_DIR:-$REPORT_DIR}"
  local log_file="$out_dir/${label}.log"
  local json_file="$out_dir/${label}.json"

  local start_ts; start_ts="$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
  local start_epoch; start_epoch="$(date +%s)"
  echo ""
  echo "─── $label ────────────────────────────────────────────────────"
  echo "  Start: $start_ts"

  # Pre-flight: confirm the target server is reachable. Skip the scenario if not.
  # Determine which URL to check based on the extra args passed to k6.
  local _check_url="$BASE_URL/api/v1/dashboard/summary"
  local _ea
  for _ea in "${extra_args[@]}"; do
    [[ "$_ea" == "LANG=rust-embedded" || "$_ea" == *"embedded"* ]] && _check_url="$EMBEDDED_URL/api/v1/dashboard/summary"
  done
  if ! curl -sf -o /dev/null "$_check_url" -H "x-tenant-id: default" \
       ${PLEXSPACES_TEST_TOKEN:+-H "Authorization: Bearer $PLEXSPACES_TEST_TOKEN"} 2>/dev/null; then
    echo "  SKIP: server not reachable at $_check_url — skipping $label"
    return 0
  fi

  # Schedule one snapshot at ~55% through the test duration (just past midpoint).
  local snap_dir="${CURRENT_LANG_DIR:-$REPORT_DIR}/snapshots"
  local dur_secs; dur_secs="$(duration_to_seconds "$DURATION")"
  local snap_delay=$(( dur_secs * 55 / 100 ))
  ( sleep "$snap_delay"; take_snap "$snap_dir" "$label" ) &
  local snap_pid=$!

  local k6_exit_file; k6_exit_file="$(mktemp)"
  { k6 run "${COMMON_ARGS[@]}" "${extra_args[@]}" \
      --out "json=$json_file" \
      "$SCENARIOS_DIR/${scenario}.js" 2>&1; echo "$?" > "$k6_exit_file"; } | \
    tee "$log_file" | \
    grep -v "^running (" | \
    grep -v -E "^\w.*\[  *[0-9]+%.*\] [0-9]+ VUs" | \
    grep -v "reflect.methodValueCall (native)" | \
    grep -v "at default (file://" | \
    grep -v "^$" || true
  local k6_exit; k6_exit="$(cat "$k6_exit_file")"; rm -f "$k6_exit_file"
  # Wait for snapshot to finish (it's fast — at most a few seconds behind k6).
  wait "$snap_pid" 2>/dev/null || true
  local end_ts; end_ts="$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
  local elapsed_s=$(( $(date +%s) - start_epoch ))
  echo "  Saved: $log_file"
  echo "  k6 JSON: $json_file"
  echo "  End: $end_ts  (${elapsed_s}s)"
  # Cooldown: skip for capacity tests (they run their own timing and have no immediate successor).
  # For regular scenarios, wait 7s for in-flight actor replies to expire (actor ask timeout = 5s).
  case "$scenario" in
    actor_capacity|ws_connections) ;;
    *) sleep 7 ;;
  esac
  if [[ "$k6_exit" -ne 0 ]]; then
    echo ""
    echo "╔══════════════════════════════════════════════════════════════╗" >&2
    echo "║  SCENARIO FAILED: $label" >&2
    echo "╠══════════════════════════════════════════════════════════════╣" >&2
    echo "║  Exit code: $k6_exit   Full log: $log_file" >&2
    echo "╠══════════════════════════════════════════════════════════════╣" >&2
    grep -iE "ERRO|level=error|threshold.*crossed|GoError" "$log_file" \
      | grep -v "reflect.methodValueCall" | grep -v "at default (file://" \
      | sort -u | head -10 | sed 's/^/║  /' >&2
    echo "╚══════════════════════════════════════════════════════════════╝" >&2
    return "$k6_exit"
  fi
  check_oom   # if watchdog fired while this scenario was running, abort now
}

# ─── Scale level runner ───────────────────────────────────────────────────────
# Runs the same scenario at 100 → 500 → 1000 → 5000 → 10000 VUs, each for DURATION.
run_scale() {
  local scenario="$1"; shift
  local label_prefix="$1"; shift
  local base_extra=("$@")
  local levels=(100 500 1000 5000 10000)

  for level_vus in "${levels[@]}"; do
    check_oom   # stop stepping up if watchdog fired mid-ramp
    local label="${label_prefix}_${level_vus}vu"
    echo ""
    echo "  ▶ Scale level: ${level_vus} VUs × $DURATION"
    run_k6 "$scenario" "$label" "${base_extra[@]}" \
      -e "VUS=$level_vus" -e "DURATION=$DURATION"
  done
}

# ─── Deploy WASM actors (skip for rust-embedded) ─────────────────────────────
deploy_app() {
  local app_dir="$1" app_id="$2" wasm_file="$3"
  local config_file="$app_dir/app-config.toml"
  if [ ! -f "$wasm_file" ]; then
    echo "  ERROR: $app_id: WASM not found: $wasm_file"
    return 1
  fi
  if [ ! -f "$config_file" ]; then
    echo "  ERROR: $app_id: app-config.toml not found: $config_file"
    return 1
  fi
  echo "  Deploying $app_id..."
  # zip appends .zip when the destination has no extension, so name the file explicitly.
  local tmp_base; tmp_base="$(mktemp /tmp/app_XXXXXX)"
  local tmp_zip="${tmp_base}.zip"
  rm -f "$tmp_base"   # remove the 0-byte placeholder mktemp created
  if ! zip -j "$tmp_zip" "$wasm_file" "$config_file" >/dev/null 2>&1; then
    rm -f "$tmp_zip"
    echo "  ERROR: $app_id: zip failed"
    return 1
  fi
  local deploy_out http_code
  deploy_out=$(curl -s -w "\n%{http_code}" -X POST "$BASE_URL/api/v1/applications/deploy" \
    -H "x-tenant-id: default" \
    ${PLEXSPACES_TEST_TOKEN:+-H "Authorization: Bearer $PLEXSPACES_TEST_TOKEN"} \
    -F "application_id=$app_id" -F "name=$app_id" -F "version=1.0.0" \
    -F "app_file=@$tmp_zip" 2>/dev/null) || deploy_out="error\n000"
  http_code=$(echo "$deploy_out" | tail -n1)
  rm -f "$tmp_zip"
  if [ "$http_code" = "200" ]; then
    echo "  ✓ $app_id"
  else
    local body; body=$(echo "$deploy_out" | sed '$d')
    echo "  ERROR: $app_id deploy HTTP $http_code: $body"
    return 1
  fi
}

# Batch deploy (only when START_SERVER=0 and server is pre-running externally).
# When START_SERVER=1, deploy is done per-language inside run_lang_isolated.
if [ "$START_SERVER" -eq 0 ] && [ "$NO_DEPLOY" -eq 0 ] && [ "$_needs_wasm_check" -eq 1 ]; then
  echo ""; echo "Deploying WASM actors (batch, server already running)..."
  DEPLOY_FAILED=0
  for lang_item in python go typescript rust-wasm; do
    echo "$RUN_LANGS" | grep -qw "$lang_item" || continue
    deploy_wasm_lang "$lang_item" || { DEPLOY_FAILED=1; break; }
  done
  if [ "$DEPLOY_FAILED" -eq 1 ]; then
    echo "ERROR: WASM build/deploy failed. Fix the errors above before running load tests."
    exit 1
  fi
fi

# ─── Start monitoring + profiling ────────────────────────────────────────────
# When START_SERVER=1, monitoring runs per-language inside run_lang_isolated.
# Start global monitoring only when using a pre-running external server.
[ "$START_SERVER" -ne 1 ] && start_monitoring
start_profiling

# ─── Which VUs list to run ───────────────────────────────────────────────────
# ─── Scenario dispatch ───────────────────────────────────────────────────────
# ─── Memory snapshot (for debugging wasmtime#8943 Store retention) ────────────
# Captures vmmap (macOS) or /proc/smaps_rollup (Linux) for the main WASM server.
# Saved to $REPORT_DIR/memsnap-<tag>.txt — open manually to see what's using memory.
capture_memory_snapshot() {
  local tag="${1:-unknown}"
  local main_pid
  main_pid=$(pgrep -f "plexspaces start" 2>/dev/null | head -1 || true)
  if [[ -z "$main_pid" ]]; then
    echo "  [memsnap] main server not running, skipping"
    return
  fi
  local snap="$REPORT_DIR/memsnap-${tag}.txt"
  echo "  [memsnap] capturing memory snapshot for PID $main_pid → $snap"
  if [[ "$(uname)" == "Darwin" ]]; then
    vmmap --wide "$main_pid" > "$snap" 2>&1 || true
  elif [[ -r "/proc/$main_pid/smaps" ]]; then
    cat "/proc/$main_pid/smaps" > "$snap" 2>&1 || true
  fi
  # Print top-10 regions by size
  if [[ -s "$snap" ]]; then
    echo "  [memsnap] top 10 regions by dirty size:"
    awk '/^[A-Za-z]/ { region=$0 }
         /Dirty:/ { gsub(" kB",""); gsub("Dirty:",""); dirty=$NF+0;
                    if (dirty>0) printf "  %8d kB  %s\n", dirty, region }' "$snap" \
      2>/dev/null | sort -rn | head -10 || true
  fi
}

# Wait for the main WASM server's RSS to drop below 1.5GB or for 60s max.
# WASM languages release their wasmtime Store heap during GC after the test ends.
wait_memory_recovery() {
  local limit_mb=1500
  local max_wait=60
  local elapsed=0
  local main_pid
  main_pid=$(pgrep -f "plexspaces start" 2>/dev/null | head -1 || true)
  [[ -z "$main_pid" ]] && return
  echo "  [recovery] waiting for server RSS to drop below ${limit_mb}MB (max ${max_wait}s)..."
  while [[ "$elapsed" -lt "$max_wait" ]]; do
    local rss_mb
    rss_mb=$(ps -o rss= -p "$main_pid" 2>/dev/null | awk '{printf "%d", $1/1024}' || echo "0")
    if [[ "$rss_mb" -lt "$limit_mb" ]]; then
      echo "  [recovery] RSS=${rss_mb}MB — OK, proceeding"
      return
    fi
    echo "  [recovery] RSS=${rss_mb}MB — waiting..."
    sleep 10
    elapsed=$((elapsed + 10))
  done
  # Capture a snapshot if we timed out — memory is still high, useful for debug
  local rss_mb
  rss_mb=$(ps -o rss= -p "$main_pid" 2>/dev/null | awk '{printf "%d", $1/1024}' || echo "0")
  echo "  [recovery] WARNING: RSS still ${rss_mb}MB after ${max_wait}s — capturing snapshot"
  capture_memory_snapshot "oom-wait-timeout"
}

run_echo() {
  local lang="$1"
  local wasm_pool="$MAX_WASM_INSTANCES"
  local actor_types
  if [[ "$lang" == "rust-embedded" ]]; then
    # Embedded actors are always regular (pre-spawned); virtual not applicable
    actor_types=("regular")
  else
    # WASM languages: respect --actor-type flag; default to virtual only
    case "$ACTOR_TYPE" in
      regular) actor_types=("regular"); wasm_pool=1 ;;
      both)    actor_types=("regular" "virtual"); wasm_pool=1 ;;
      *)       actor_types=("virtual"); wasm_pool=1 ;;
    esac
  fi

  for at in "${actor_types[@]}"; do
    local label_suffix="${lang}_${at}"
    local ask_timeout=5
    [[ "$at" == "virtual" ]] && ask_timeout=30
    test_type_banner "echo/http (actor_type=$at)"
    if [ "$SCALE_LEVELS" -eq 1 ]; then
      run_scale "echo_http" "echo_http_${label_suffix}" \
        -e "LANG=$lang" -e "ACTOR_TYPE=$at" -e "MAX_WASM_INSTANCES=$wasm_pool" \
        -e "ASK_TIMEOUT=$ask_timeout"
    else
      run_k6 "echo_http" "echo_http_${label_suffix}" \
        -e "LANG=$lang" -e "VUS=$VUS" -e "DURATION=$DURATION" -e "ACTOR_TYPE=$at" \
        -e "MAX_WASM_INSTANCES=$wasm_pool" -e "ASK_TIMEOUT=$ask_timeout"
    fi
  done
}

# Per-language start epoch stored in global vars (bash 3.2 compatible, no declare -A).
# Variable name: _LANG_EPOCH_<slug> — only [A-Za-z0-9_] allowed; all other chars replaced with _.
_lang_epoch_var() { echo "_LANG_EPOCH_$(echo "$1" | tr -cs 'A-Za-z0-9' '_')"; }

lang_banner_start() {
  local lang="$1"
  local ts; ts="$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
  local _var; _var="$(_lang_epoch_var "$lang")"
  printf -v "$_var" '%s' "$(date +%s)"
  echo ""
  echo "╔══════════════════════════════════════════════════════════════╗"
  printf "║  ▶ START  Language: %-43s║\n" "$lang"
  printf "║    Mode: %-12s  VUs: %-6s  Duration: %-14s║\n" "$MODE" "$VUS" "$DURATION"
  printf "║    Start: %-52s║\n" "$ts"
  echo "╚══════════════════════════════════════════════════════════════╝"
}

lang_banner_end() {
  local lang="$1"
  local ts; ts="$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
  local elapsed=""
  local _var; _var="$(_lang_epoch_var "$lang")"
  local _start="${!_var:-}"
  if [[ -n "$_start" ]]; then
    elapsed="  ($(( $(date +%s) - _start ))s)"
  fi
  echo ""
  echo "╔══════════════════════════════════════════════════════════════╗"
  printf "║  ✓ END    Language: %-43s║\n" "$lang"
  printf "║    End: %-54s║\n" "${ts}${elapsed}"
  echo "╚══════════════════════════════════════════════════════════════╝"
}

test_type_banner() {
  local test_type="$1"
  local ts; ts="$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
  echo ""
  echo "┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄"
  printf "  TEST TYPE: %s  [%s]\n" "$test_type" "$ts"
  echo "┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄"
}

# ─── Per-language k6 test functions (called by run_lang_isolated) ────────────

_kv_pool_for_lang() {
  local lang="$1"
  local pool="$MAX_WASM_INSTANCES"
  if [[ "$lang" == "go" || "$lang" == "typescript" ]]; then
    pool=2
  elif [[ "$lang" == "rust-wasm" || "$lang" == "python" ]]; then
    pool=1
  fi
  echo "$pool"
}

_run_kv_for_lang() {
  local lang="$1"
  local kv_pool
  kv_pool=$(_kv_pool_for_lang "$lang")
  if [[ "$lang" != "rust-embedded" ]]; then
    prewarm_wasm "perf-${lang}" "$kv_pool" 5
  fi
  test_type_banner "kv/http"
  if [ "$SCALE_LEVELS" -eq 1 ]; then
    run_scale "kv_http" "kv_http_$lang" -e "LANG=$lang" -e "MAX_WASM_INSTANCES=$kv_pool"
  else
    run_k6 "kv_http" "kv_http_$lang" -e "LANG=$lang" -e "VUS=$VUS" -e "DURATION=$DURATION" -e "MAX_WASM_INSTANCES=$kv_pool"
  fi
}

_run_all_for_lang() {
  local lang="$1"
  local cap="$NODE_CONCURRENCY"
  [[ "$lang" != "rust-embedded" ]] && cap="$MAX_WASM_INSTANCES"
  local target_url="$EMBEDDED_URL"
  [[ "$lang" != "rust-embedded" ]] && target_url="$BASE_URL"

  _ensure_server_alive "$lang"
  run_echo "$lang"
  local kv_pool
  kv_pool=$(_kv_pool_for_lang "$lang")
  _ensure_server_alive "$lang"
  if [[ "$lang" != "rust-embedded" ]]; then
    prewarm_wasm "perf-${lang}" "$kv_pool" 5
  fi
  test_type_banner "kv/http"
  run_k6 "kv_http" "kv_http_$lang" -e "LANG=$lang" -e "VUS=$VUS" -e "DURATION=$DURATION" -e "MAX_WASM_INSTANCES=$kv_pool"
  if [[ "$lang" != "rust-embedded" ]]; then
    capture_memory_snapshot "post-kv-${lang}"
  fi
  # ── Capacity tests (only for embedded; WASM capacity runs separately) ─────
  if [[ "$lang" == "rust-embedded" ]]; then
    # Ensure node deps are available.
    if command -v node >/dev/null 2>&1; then
      if [ ! -d "$SCRIPTS_DIR/node_modules" ]; then
        echo "  Installing Node.js dependencies..."
        npm --prefix "$SCRIPTS_DIR" install --quiet
      fi

      # actor/capacity regular: spawn unique actors until RSS limit (no eviction).
      test_type_banner "actor/capacity (regular — accumulate to memory limit)"
      _ensure_server_alive "$lang"
      _restart_embedded_server_with_pool 0
      run_node_script "actor_capacity.js" "actor_capacity_node_${lang}_regular" \
        "ACTOR_LANG=$lang" "BASE_URL=$target_url" "ACTOR_MODE=regular" \
        "CONCURRENCY=500" "MEMORY_LIMIT_MB=2048" "MAX_DURATION_S=1800"

      # actor/capacity virtual: unique virtual actors with eager activation until RSS limit.
      test_type_banner "actor/capacity (virtual — eager activation, accumulate to memory limit)"
      _restart_embedded_server_with_pool 200000
      run_node_script "actor_capacity.js" "actor_capacity_node_${lang}_virtual" \
        "ACTOR_LANG=$lang" "BASE_URL=$target_url" "ACTOR_MODE=virtual" \
        "CONCURRENCY=500" "MEMORY_LIMIT_MB=2048" "MAX_DURATION_S=1800"

      # ws/capacity: ramp WebSocket thin-node connections to RSS memory limit.
      test_type_banner "ws/capacity (ramp WS connections to memory limit)"
      _restart_embedded_server_with_pool 0
      run_node_script "ws_capacity.js" "ws_capacity_${lang}" \
        "WS_URL=$(echo "$EMBEDDED_URL" | sed 's|^http|ws|')/ws" \
        "TARGET=500000" "BATCH_SIZE=200" "RAMP_INTERVAL_MS=300" \
        "HB_INTERVAL_MS=15000" "MEMORY_LIMIT_MB=$MEM_LIMIT_MB"
    else
      echo "  NOTE: node not found — skipping actor/capacity and ws/capacity tests"
    fi
  fi
}

_run_ws_for_lang() {
  local lang="$1"
  # ws-capacity: ramp WebSocket thin-node connections to RSS memory limit.
  # Uses the Node.js ws_capacity.js script which ramps in batches until RSS > STOP_AT_MB.
  # Each connection performs the thin-node registration handshake and holds open with heartbeats.
  # This is the real capacity ceiling test — no artificial somaxconn cap.
  test_type_banner "ws/capacity (rust-embedded — ramp to memory limit)"

  # Ensure node deps are available (ws package).
  if ! command -v node >/dev/null 2>&1; then
    echo "  ERROR: node not found — cannot run ws/capacity. Install Node.js 18+."
    return 1
  fi
  if [ ! -d "$SCRIPTS_DIR/node_modules" ]; then
    echo "  Installing Node.js dependencies..."
    npm --prefix "$SCRIPTS_DIR" install --quiet
  fi

  run_node_script "ws_capacity.js" "ws_capacity_${lang}" \
    "WS_URL=$(echo "$EMBEDDED_URL" | sed 's|^http|ws|')/ws" \
    "TARGET=500000" \
    "BATCH_SIZE=200" \
    "RAMP_INTERVAL_MS=300" \
    "HB_INTERVAL_MS=15000" \
    "MEMORY_LIMIT_MB=$MEM_LIMIT_MB"
}

_run_max_actors_for_lang() {
  local lang="$1"
  if [[ "$lang" == "rust-embedded" ]]; then
    # Embedded: full OOM ramp — spawn until memory limit, both eager and virtual.
    local run_modes=()
    case "${ACTOR_TYPE:-both}" in
      virtual) run_modes=("virtual") ;;
      regular) run_modes=("eager") ;;
      *)       run_modes=("eager" "virtual") ;;
    esac
    for _am in "${run_modes[@]}"; do
      test_type_banner "actor/capacity (lang=${lang} mode=${_am})"
      run_k6 "actor_capacity" "actor_capacity_${lang}_${_am}" \
        -e "LANG=${lang}" -e "ACTOR_MODE=${_am}" -e "MEMORY_LIMIT_MB=${MEM_LIMIT_MB}" \
        -e "MAX_DURATION=60m"
    done
  else
    # WASM: sanity check only — verify a small pool of actors can activate and respond.
    # Full OOM ramp is not run for WASM; use --mode actor-capacity (Node.js) for that.
    test_type_banner "actor/capacity (lang=${lang} — WASM sanity check, ${MAX_WASM_INSTANCES} actors)"
    run_k6 "actor_capacity" "actor_capacity_${lang}_sanity" \
      -e "LANG=${lang}" -e "ACTOR_MODE=eager" \
      -e "VUS=${MAX_WASM_INSTANCES}" -e "MAX_ACTORS=${MAX_WASM_INSTANCES}" \
      -e "MEMORY_LIMIT_MB=512" -e "MAX_DURATION=5m"
  fi
}

# ─── Node.js capacity script runner ─────────────────────────────────────────
# Runs one of the scripts in load-test/scripts/ with environment forwarded from
# the same flags used by run.sh (--mem-limit, --embedded-port, --lang, etc.).
# Logs are written to $CURRENT_LANG_DIR (set by run_lang_isolated) or $REPORT_DIR.
run_node_script() {
  local script_name="$1"   # actor_capacity.js | ws_capacity.js
  local label="$2"
  local extra_env=("${@:3}")   # KEY=VALUE pairs passed as env vars

  check_oom

  local out_dir="${CURRENT_LANG_DIR:-$REPORT_DIR}"
  local log_file="$out_dir/${label}.log"

  local start_ts; start_ts="$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
  local start_epoch; start_epoch="$(date +%s)"
  echo ""
  echo "─── $label (node) ──────────────────────────────────────────────"
  echo "  Start: $start_ts"

  # Build env: inherit EMBEDDED_URL / BASE_URL and no-auth settings, then add extras.
  # Note: callers may override BASE_URL via extra_env — keep EMBEDDED_URL as a separate key
  # but do NOT set BASE_URL here so callers can pass the correct value for their language.
  local node_env=(
    "EMBEDDED_URL=$EMBEDDED_URL"
    "MEMORY_LIMIT_MB=$MEM_LIMIT_MB"
    "CONCURRENCY=$NODE_CONCURRENCY"
  )
  [ "$NO_AUTH" -eq 1 ] && node_env+=("NO_AUTH=1")
  [ -n "${PLEXSPACES_TEST_TOKEN:-}" ] && node_env+=("AUTH_TOKEN=$PLEXSPACES_TEST_TOKEN")
  node_env+=("${extra_env[@]}")

  # Raise fd limit for ws_capacity (needs tens of thousands of open sockets).
  # Try 200000; fall back to 65536; ignore if already at max (non-root on macOS caps at kern.maxfilesperproc).
  (ulimit -n 200000 2>/dev/null || ulimit -n 65536 2>/dev/null || true
   env "${node_env[@]}" node "$SCRIPTS_DIR/$script_name") 2>&1 | tee "$log_file" || true
  local end_ts; end_ts="$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
  local elapsed_s=$(( $(date +%s) - start_epoch ))
  echo "  Saved: $log_file"
  echo "  End: $end_ts  (${elapsed_s}s)"
}

_run_node_ws_capacity() {
  local lang="$1"
  test_type_banner "ws/capacity (node — ramp to memory limit)"
  run_node_script "ws_capacity.js" "ws_capacity_${lang}" \
    "WS_URL=$(echo "$EMBEDDED_URL" | sed 's|^http|ws|')/ws" \
    "TARGET=500000" \
    "BATCH_SIZE=200" \
    "RAMP_INTERVAL_MS=300" \
    "HB_INTERVAL_MS=15000"
}

_run_node_actor_capacity() {
  local lang="$1"
  local target_url="$EMBEDDED_URL"
  [[ "$lang" != "rust-embedded" ]] && target_url="$BASE_URL"
  local cap="$NODE_CONCURRENCY"
  [[ "$lang" != "rust-embedded" ]] && cap="$MAX_WASM_INSTANCES"

  # Determine which modes to run (respects --actor-type flag).
  local run_modes=()
  case "${ACTOR_TYPE:-both}" in
    virtual) run_modes=("virtual") ;;
    regular) run_modes=("regular") ;;
    *)       run_modes=("regular" "virtual") ;;
  esac

  for _am in "${run_modes[@]}"; do
    test_type_banner "actor/capacity (node — ${_am}, spawn to memory limit)"

    if [[ "$_am" == "virtual" ]]; then
      # Virtual mode: eager activation, accumulate virtual actors until RSS limit.
      # Pool cap set very high so LRU only kicks in near memory limit (not artificially early).
      # Embedded: ~2-5KB/actor so 200K actors fit in 2GB before LRU is needed.
      # WASM: much larger per-actor cost, cap at MAX_WASM_INSTANCES to avoid OOM during instantiation.
      local virtual_pool=200000
      [[ "$lang" != "rust-embedded" ]] && virtual_pool="$MAX_WASM_INSTANCES"
      # Restart server with the correct pool cap so PERF_MAX_VIRTUAL_POOL takes effect.
      _restart_embedded_server_with_pool "$virtual_pool"
      run_node_script "actor_capacity.js" "actor_capacity_node_${lang}_virtual" \
        "ACTOR_LANG=$lang" \
        "BASE_URL=$target_url" \
        "ACTOR_MODE=virtual" \
        "CONCURRENCY=500" \
        "MEMORY_LIMIT_MB=2048" \
        "MAX_DURATION_S=1800"
    else
      # Regular mode: no eviction, unique actors accumulate until RSS limit.
      _restart_embedded_server_with_pool 0
      run_node_script "actor_capacity.js" "actor_capacity_node_${lang}_regular" \
        "ACTOR_LANG=$lang" \
        "BASE_URL=$target_url" \
        "ACTOR_MODE=regular" \
        "CONCURRENCY=500" \
        "MEMORY_LIMIT_MB=2048" \
        "MAX_DURATION_S=1800"
    fi
  done
}

case "$MODE" in
  echo)
    for lang in $RUN_LANGS; do
      lang_banner_start "$lang"
      run_lang_isolated "$lang" run_echo
      lang_banner_end "$lang"
    done
    ;;
  kv)
    for lang in $RUN_LANGS; do
      lang_banner_start "$lang"
      run_lang_isolated "$lang" _run_kv_for_lang
      lang_banner_end "$lang"
    done
    ;;
  spawn)
    _run_spawn_for_lang() {
      local lang="$1"
      if [[ "$lang" == "rust-embedded" ]]; then
        # Embedded: spawn unique actors continuously until RSS limit — no eviction, no pool cap.
        # This is the scalability proof: show how many actors the node can hold.
        test_type_banner "spawn/capacity (rust-embedded — accumulate actors until OOM)"
        _run_node_actor_capacity "$lang"
      else
        # WASM: bounded virtual pool k6 test (instantiation is expensive, cap to MAX_WASM_INSTANCES).
        test_type_banner "spawn/lifecycle (virtual — WASM bounded pool)"
        local _svus="$MAX_WASM_INSTANCES"
        run_k6 "spawn_lifecycle" "lifecycle_${lang}_virtual" \
          -e "LANG=$lang" -e "ACTOR_TYPE=virtual" -e "VUS=$_svus" -e "DURATION=$DURATION" \
          -e "MAX_WASM_INSTANCES=$MAX_WASM_INSTANCES"
      fi
    }
    for lang in $RUN_LANGS; do
      lang_banner_start "$lang"
      # Disable LRU eviction before server starts so actors accumulate without bound.
      [[ "$lang" == "rust-embedded" ]] && export PERF_MAX_VIRTUAL_POOL=0
      run_lang_isolated "$lang" _run_spawn_for_lang
      unset PERF_MAX_VIRTUAL_POOL
      lang_banner_end "$lang"
    done
    ;;
  ws-connections|ws-capacity)
    # Both modes run the Node.js ws_capacity.js: ramp WS connections to RSS memory limit.
    export PERF_MAX_VIRTUAL_POOL=0
    lang_banner_start "rust-embedded"
    run_lang_isolated "rust-embedded" _run_ws_for_lang
    lang_banner_end "rust-embedded"
    ;;
  actor-capacity)
    # Node.js actor capacity test: spawn actors until RSS limit.
    # Regular pass: no eviction (PERF_MAX_VIRTUAL_POOL=0), accumulate to OOM.
    # Virtual pass: pool cap=200000, eager activation, accumulate to OOM.
    # _run_node_actor_capacity calls _restart_embedded_server_with_pool between passes.
    export PERF_MAX_VIRTUAL_POOL=0  # server starts with pool=0 for the first (regular) pass
    for lang in $RUN_LANGS; do
      lang_banner_start "$lang"
      run_lang_isolated "$lang" _run_node_actor_capacity
      lang_banner_end "$lang"
    done
    ;;
  max-actors)
    # embedded: full OOM ramp via _run_max_actors_for_lang (no eviction, runs to memory limit).
    # WASM: sanity check only (a few actors, verifies activation works).
    export PERF_MAX_VIRTUAL_POOL=0
    for lang in $RUN_LANGS; do
      lang_banner_start "$lang"
      run_lang_isolated "$lang" _run_max_actors_for_lang
      lang_banner_end "$lang"
    done
    ;;
  all)
    # all mode: echo + kv + spawn/lifecycle + actor/capacity (regular+virtual) + ws/capacity.
    # Capacity tests run last per language; embedded server restarts between capacity modes.
    for lang in $RUN_LANGS; do
      lang_banner_start "$lang"
      run_lang_isolated "$lang" _run_all_for_lang
      lang_banner_end "$lang"
    done
    ;;
  *)
    echo "Unknown mode: $MODE"; exit 1 ;;
esac

# ─── Stop monitoring + profiling ─────────────────────────────────────────────
stop_monitoring
stop_profiling

# ─── Summary ─────────────────────────────────────────────────────────────────
echo ""
echo "╔═══════════════════════════════════════════════════════════════╗"
echo "║  Load test complete.                                          ║"
echo "╠═══════════════════════════════════════════════════════════════╣"
printf "║  Run ID:      %-47s║\n" "$RUN_ID"
printf "║  Report dir:  %-47s║\n" "$REPORT_DIR"

# RSS delta from metrics.csv — proves whether the server leaked memory
if [ -f "$REPORT_DIR/metrics.csv" ]; then
  printf "║  Metrics CSV: %-47s║\n" "$REPORT_DIR/metrics.csv"
  RSS_LINE=$(awk -F',' 'NR>1 && $4!="" {
      if ($4+0 > max) max=$4+0
      if (min=="" || $4+0 < min) min=$4+0
    } END {
      if (min!="") printf "embedded RSS: min=%.0fMB max=%.0fMB delta=%.0fMB", min, max, max-min
    }' "$REPORT_DIR/metrics.csv" 2>/dev/null || true)
  [ -n "$RSS_LINE" ] && printf "║  %-61s║\n" "$RSS_LINE"
fi

[ "$PROFILE" -eq 1 ] && \
  printf "║  Flamegraph:  %-47s║\n" "$REPORT_DIR/flamegraph-*/"
echo "╠═══════════════════════════════════════════════════════════════╣"
echo "║  Next steps:                                                  ║"
printf "║    Analyze:  bash load-test/reports/analyze.sh %-13s║\n" "$RUN_ID"
echo "║    Compare:  bash load-test/reports/compare.sh \\             ║"
printf "║      <prev-run-dir> load-test-data/%-27s║\n" "$RUN_ID"
echo "╚═══════════════════════════════════════════════════════════════╝"

# Build per-run index for fast analysis
"$REPO_ROOT/load-test/reports/build-index.sh" "$REPORT_DIR" 2>/dev/null || true
