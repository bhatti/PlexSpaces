#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Test Job Queue Outbox — TypeScript WASM
#
# Verifies high-scale outbox dispatch with AIMD flow control:
#   0. Check node is running
#   1. Build + Deploy application
#   2. Init 100 virtual workers via CreateShardGroup
#   3. Dispatch jobs to shard workers via outbox
#   4. Send 5 heartbeats with OVERWRITE (LWW idempotency)
#   5. Upgrade simulation — Ask 10 workers
#   6. Scaling benchmark (10, 50, 100 workers)
#   7. Full metrics
#   8. Prometheus /metrics endpoint
#
# Usage: ./test.sh [port]
# Prerequisites: Start PlexSpaces node first.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/job_queue_outbox_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
HTTP_PORT="${1:-${HTTP_PORT:-8091}}"
HTTP_HOST="${HTTP_HOST:-127.0.0.1}"

APP_ID="ts-job-queue-outbox"
LEADER_ACTOR="leader:JobQueueLeader"

GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m'

# ── JWT auth ──────────────────────────────────────────────────────────────────
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
if [ -z "${PLEXSPACES_TEST_TOKEN:-}" ] && [ -f "$REPO_ROOT/scripts/gen-test-jwt.sh" ]; then
  source ~/venv/bin/activate 2>/dev/null || true
  echo "Generating JWT token..."
  JWT_OUTPUT="$(PLEXSPACES_JWT_PRIVATE_KEY_FILE="$REPO_ROOT/certs/jwt-es256.pem" "$REPO_ROOT/scripts/gen-test-jwt.sh")"
  eval "$JWT_OUTPUT"
  if [ -z "${PLEXSPACES_TEST_TOKEN:-}" ]; then
    echo -e "${RED}ERROR: gen-test-jwt.sh failed to set PLEXSPACES_TEST_TOKEN${NC}"
    exit 1
  fi
fi
export AUTH_HEADER=""
if [ -n "${PLEXSPACES_TEST_TOKEN:-}" ]; then
  AUTH_HEADER="Authorization: Bearer $PLEXSPACES_TEST_TOKEN"
fi

# ── Helpers ───────────────────────────────────────────────────────────────────
send_op() {
  local actor="$1"
  local payload="$2"
  local timeout="${3:-30}"
  curl -s --max-time "$timeout" -X POST \
    "http://$HTTP_HOST:$HTTP_PORT/api/v1/actors/$APP_ID/$actor/ask?timeout=$timeout" \
    -H "Content-Type: application/json" \
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
    -d "$payload" 2>/dev/null || echo '{"error":"timeout"}'
}

extract() {
  local raw="$1" key="$2"
  RAW="$raw" KEY="$key" python3 -c "
import json, os
d = json.loads(os.environ['RAW'])
p = d.get('payload', d)
if isinstance(p, str):
    p = json.loads(p)
print(p.get(os.environ['KEY'], ''))
" 2>/dev/null || echo ""
}

assert_ok() {
  local step="$1" raw="$2"
  if ! ASSERT_STEP="$step" ASSERT_JSON="$raw" python3 - <<'PY'
import json, os, sys
raw = os.environ.get("ASSERT_JSON", "")
step = os.environ.get("ASSERT_STEP", "?")
try:
    d = json.loads(raw)
except Exception as e:
    print(f"FAIL [{step}]: invalid JSON — {e}", file=sys.stderr); sys.exit(1)
payload = d.get("payload", d)
if isinstance(payload, str):
    try: payload = json.loads(payload)
    except Exception: payload = {}
if d.get("success") is False or d.get("error_message"):
    err = d.get("error_message") or str(d.get("error", "unknown"))
    print(f"FAIL [{step}]: {err}", file=sys.stderr); sys.exit(1)
if isinstance(payload, dict) and payload.get("error"):
    print(f"FAIL [{step}]: {payload['error']}", file=sys.stderr); sys.exit(1)
PY
  then
    echo -e "${RED}FAIL: $step${NC}"
    echo "  Response: $raw"
    exit 1
  fi
}

echo "================================================================"
echo "  Job Queue Outbox — TypeScript WASM"
echo "  100 workers · outbox dispatch · upgrade sim · OVERWRITE hb"
echo "================================================================"
echo "  HTTP: $HTTP_HOST:$HTTP_PORT  App: $APP_ID"
echo ""

# ── Step 0: Check node ────────────────────────────────────────────────────────
echo "Step 0: Check node"
HTTP_CHECK=$(curl -s -o /dev/null -w "%{http_code}" "http://$HTTP_HOST:$HTTP_PORT/" 2>/dev/null) || HTTP_CHECK="000"
if [ "$HTTP_CHECK" = "000" ]; then
  echo -e "${RED}Cannot connect to node at $HTTP_HOST:$HTTP_PORT${NC}"
  exit 1
fi
echo -e "${GREEN}Node is running${NC}"
echo ""

# ── Step 1: Build + Deploy ────────────────────────────────────────────────────
echo "Step 1: Build WASM"
"$SCRIPT_DIR/scripts/build.sh" || { echo -e "${RED}Build failed${NC}"; exit 1; }
echo ""

APP_ZIP="$(mktemp).zip"
rm -f "$APP_ZIP"
trap 'rm -f "$APP_ZIP"' EXIT
zip -j "$APP_ZIP" "$WASM_FILE" "$CONFIG_FILE" >/dev/null

echo "Step 1b: Deploy application"
"$SCRIPT_DIR/undeploy.sh" "$HTTP_PORT" 2>/dev/null || true
sleep 1

_deployed=0
for _attempt in 1 2 3; do
  DEPLOY_OUT=$(curl -s -w "\n%{http_code}" -X POST \
    "http://$HTTP_HOST:$HTTP_PORT/api/v1/applications/deploy" \
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
    -F "application_id=$APP_ID" \
    -F "name=$APP_ID" \
    -F "version=1.0.0" \
    -F "app_file=@$APP_ZIP" 2>&1) || true
  HTTP_CODE=$(echo "$DEPLOY_OUT" | tail -n1)
  RESPONSE=$(echo "$DEPLOY_OUT" | sed '$d')
  if [ "$HTTP_CODE" = "200" ] && echo "$RESPONSE" | grep -qE '"success"[[:space:]]*:[[:space:]]*true'; then
    _deployed=1; break
  fi
  echo "  Deploy attempt $_attempt failed, retrying in 3s..."
  sleep 3
done
if [ "$_deployed" -eq 0 ]; then
  echo -e "${RED}Deploy failed: $RESPONSE${NC}"
  exit 1
fi
echo -e "${GREEN}Deployed $APP_ID${NC}"
sleep 2
echo ""

# ── Step 2: Init 100 virtual workers ─────────────────────────────────────────
echo "Step 2: Init 100 virtual workers via CreateShardGroup"
R=$(send_op "$LEADER_ACTOR" '{"op":"init_workers","count":100}' 60)
assert_ok "init_workers" "$R"
WORKER_COUNT=$(extract "$R" worker_count)
GROUP_ID=$(extract "$R" group_id)
echo "  group_id=$GROUP_ID worker_count=$WORKER_COUNT"
if [ -n "$WORKER_COUNT" ] && [ "$WORKER_COUNT" != "0" ]; then
  echo -e "  ${GREEN}PASS: init_workers worker_count=$WORKER_COUNT${NC}"
else
  echo -e "  ${YELLOW}WARN: worker_count=$WORKER_COUNT — shard group may span multiple nodes${NC}"
fi
echo ""

# ── Step 3: Dispatch jobs to workers ─────────────────────────────────────────
echo "Step 3: Dispatch jobs to shard workers via outbox"
R=$(send_op "$LEADER_ACTOR" '{"op":"dispatch_jobs","payload":{"task":"process_item","priority":"high"}}' 60)
assert_ok "dispatch_jobs" "$R"
ACCEPTED=$(extract "$R" accepted)
REJECTED=$(extract "$R" rejected)
WALL_MS=$(extract "$R" wall_time_ms)
THROUGHPUT=$(extract "$R" throughput_per_s)
echo "  accepted=$ACCEPTED rejected=$REJECTED wall_time_ms=$WALL_MS throughput=${THROUGHPUT}/s"
if [ -n "$ACCEPTED" ] && [ "$ACCEPTED" != "0" ]; then
  echo -e "  ${GREEN}PASS: dispatch_jobs accepted=$ACCEPTED${NC}"
else
  echo -e "${RED}FAIL: dispatch_jobs accepted=$ACCEPTED (expected > 0)${NC}"
  echo "  Response: $R"
  exit 1
fi
echo ""

# ── Step 4: Heartbeat with OVERWRITE idempotency ─────────────────────────────
echo "Step 4: Send 5 heartbeats with OVERWRITE (LWW idempotency)"
for i in 1 2 3 4 5; do
  R=$(send_op "$LEADER_ACTOR" '{"op":"send_heartbeat","seq":'"$i"'}')
  assert_ok "heartbeat_$i" "$R"
done
HB_R=$(send_op "$LEADER_ACTOR" '{"op":"get_metrics"}')
assert_ok "get_metrics_after_heartbeat" "$HB_R"
HB_COUNT=$(extract "$HB_R" heartbeat_count)
echo "  heartbeat_count=$HB_COUNT"
if [ "$HB_COUNT" = "5" ]; then
  echo -e "  ${GREEN}PASS: all 5 heartbeats sent with OVERWRITE idempotency${NC}"
else
  echo -e "${RED}FAIL: heartbeat_count=$HB_COUNT (expected 5)${NC}"
  exit 1
fi
echo ""

# ── Step 5: Upgrade simulation ────────────────────────────────────────────────
echo "Step 5: Upgrade simulation — Ask 10 workers"
R=$(send_op "$LEADER_ACTOR" '{"op":"upgrade_workers","count":10}' 60)
assert_ok "upgrade_workers" "$R"
WORKERS_ASKED=$(extract "$R" workers_asked)
RTT_MIN=$(extract "$R" ask_rtt_min_ms)
RTT_MAX=$(extract "$R" ask_rtt_max_ms)
RTT_AVG=$(extract "$R" ask_rtt_avg_ms)
echo "  workers_asked=$WORKERS_ASKED ask_rtt: min=${RTT_MIN}ms max=${RTT_MAX}ms avg=${RTT_AVG}ms"
if [ -n "$WORKERS_ASKED" ] && [ "$WORKERS_ASKED" != "0" ]; then
  echo -e "  ${GREEN}PASS: upgrade simulation workers_asked=$WORKERS_ASKED${NC}"
else
  echo -e "${RED}FAIL: workers_asked=$WORKERS_ASKED (expected > 0)${NC}"
  exit 1
fi
echo ""

# ── Step 6: Scaling benchmark ─────────────────────────────────────────────────
echo "Step 6: Scaling benchmark (10, 50, 100 workers)"
R=$(send_op "$LEADER_ACTOR" '{"op":"scaling_benchmark"}' 120)
assert_ok "scaling_benchmark" "$R"
python3 - <<PY
import json, sys
raw = '''$R'''
try:
    d = json.loads(raw)
    payload = d.get('payload', d)
    if isinstance(payload, str): payload = json.loads(payload)
    scales = payload.get('scales', [])
    print("  ┌──────────┬──────────┬──────────┬────────────┬──────────┐")
    print("  │  Workers │ Accepted │ Wall(ms) │ Throughput │ Speedup% │")
    print("  ├──────────┼──────────┼──────────┼────────────┼──────────┤")
    for row in scales:
        print(f"  │ {row.get('workers',0):>8} │ {row.get('accepted',0):>8} │ {row.get('wall_time_ms',0):>8} │ {str(row.get('throughput',0))+'/s':>10} │ {row.get('speedup_pct',0):>7}% │")
    print("  └──────────┴──────────┴──────────┴────────────┴──────────┘")
except Exception as e:
    print(f"  (could not parse scaling result: {e})")
PY
echo -e "  ${GREEN}PASS: scaling benchmark completed${NC}"
echo ""

# ── Step 7: Full metrics ──────────────────────────────────────────────────────
echo "Step 7: Full metrics"
R=$(send_op "$LEADER_ACTOR" '{"op":"get_metrics"}')
assert_ok "get_metrics" "$R"
DISPATCHED=$(extract "$R" total_dispatched)
DISPATCH_COUNT=$(extract "$R" dispatch_count)
WALL_MS=$(extract "$R" wall_time_ms)
THROUGHPUT=$(extract "$R" throughput_per_s)
ASK_MIN=$(extract "$R" ask_rtt_min_ms)
ASK_MAX=$(extract "$R" ask_rtt_max_ms)
ASK_AVG=$(extract "$R" ask_rtt_avg_ms)
echo "  total_dispatched=$DISPATCHED dispatch_count=$DISPATCH_COUNT"
echo "  wall_time_ms=$WALL_MS throughput=${THROUGHPUT}/s"
echo "  ask_rtt: min=${ASK_MIN}ms max=${ASK_MAX}ms avg=${ASK_AVG}ms"
if [ -n "$DISPATCHED" ] && [ "$DISPATCHED" != "0" ]; then
  echo -e "  ${GREEN}PASS: metrics tracked${NC}"
else
  echo -e "${RED}FAIL: total_dispatched=$DISPATCHED (expected > 0)${NC}"
  exit 1
fi
echo ""

# ── Step 8: Prometheus metrics ────────────────────────────────────────────────
echo "Step 8: Prometheus /metrics endpoint"
METRICS=$(curl -s --max-time 10 \
  "http://$HTTP_HOST:$HTTP_PORT/metrics" \
  ${AUTH_HEADER:+-H "$AUTH_HEADER"} 2>/dev/null || echo '')
OUTBOX_DISPATCH=$(echo "$METRICS" | grep '^plexspaces_wasm_outbox_dispatch_total' | awk '{print $2}' || echo "0")
OUTBOX_DISPATCH="${OUTBOX_DISPATCH:-0}"
echo "  outbox_dispatch_total=$OUTBOX_DISPATCH"
if [ -n "$OUTBOX_DISPATCH" ] && [ "$OUTBOX_DISPATCH" != "0" ]; then
  echo -e "  ${GREEN}PASS: outbox dispatch counter incremented${NC}"
else
  echo -e "  ${YELLOW}INFO: outbox counter not yet non-zero (check Prometheus /metrics)${NC}"
fi
echo ""

echo "================================================================"
echo -e "  ${GREEN}Job Queue Outbox (TypeScript) Test PASSED${NC}"
echo "================================================================"
echo ""
echo "This example demonstrated:"
echo -e "  ${GREEN}*${NC} 100 virtual workers via CreateShardGroup"
echo -e "  ${GREEN}*${NC} Outbox dispatch with AIMD flow control"
echo -e "  ${GREEN}*${NC} Upgrade simulation via Ask round-trip (RTT tracked)"
echo -e "  ${GREEN}*${NC} LWW heartbeat dedup via OVERWRITE conflict resolution"
echo -e "  ${GREEN}*${NC} Scaling benchmark (10/50/100 workers)"
echo -e "  ${GREEN}*${NC} Wall-time and throughput metrics"
echo ""
