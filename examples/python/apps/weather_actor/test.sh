#!/usr/bin/env bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Weather Actor (Python WASM): get_weather, cache_stats, clear_cache.
# Usage:
#   ./test.sh [HTTP_PORT]     # contract tests + full example run
#   ./test.sh --contract-only # contract tests only
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
SDK_DIR="$PROJECT_ROOT/sdks/python"
WASM_FILE="$SCRIPT_DIR/weather_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
HTTP_PORT="${1:-8092}"
APP_ID="weather-python-test"
RUN_ID="$(date +%s)"

if [[ "${1:-}" == "--contract-only" ]]; then
  HTTP_PORT=""
fi

source "$PROJECT_ROOT/examples/rust/apps/test-common.sh"
source "$HOME/venv/bin/activate" 2>/dev/null || true

GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m'

echo "================================================================"
echo "  Weather Actor (Python WASM, service link + KV cache)"
echo "================================================================"

echo "Step 0: Contract tests"
PYTHONPATH="$SDK_DIR:$SCRIPT_DIR" python3 "$SCRIPT_DIR/test_weather_actor.py"
echo -e "  ${GREEN}Contract tests passed${NC}"
echo ""

if [[ -z "$HTTP_PORT" ]]; then
  echo -e "${GREEN}Contract-only run complete${NC}"
  exit 0
fi

echo "Step 1: Build WASM"
"$SCRIPT_DIR/build.sh"
echo ""

trap 'curl -s -X DELETE "http://localhost:$HTTP_PORT/api/v1/applications/$APP_ID" >/dev/null 2>&1 || true' EXIT

HTTP_CHECK=$(curl -s -o /dev/null -w "%{http_code}" "http://localhost:$HTTP_PORT/" 2>/dev/null) || HTTP_CHECK="000"
if [ "$HTTP_CHECK" = "000" ]; then
  echo -e "${RED}Start node: ./scripts/server.sh (from repo root)${NC}"
  exit 1
fi

echo "Step 2: Deploy"
curl -s -X DELETE "http://localhost:$HTTP_PORT/api/v1/applications/$APP_ID" >/dev/null 2>&1 || true
sleep 1
DEPLOY_OUT=$(curl -s -w "\n%{http_code}" -X POST "http://localhost:$HTTP_PORT/api/v1/applications/deploy" \
  -F "application_id=$APP_ID" \
  -F "name=$APP_ID" \
  -F "version=1.0.0" \
  -F "wasm_file=@$WASM_FILE;type=application/wasm" \
  -F "config=@$CONFIG_FILE" 2>&1)
HTTP_CODE=$(echo "$DEPLOY_OUT" | tail -n1)
RESPONSE=$(echo "$DEPLOY_OUT" | sed '$d')
if [ "$HTTP_CODE" != "200" ] || ! echo "$RESPONSE" | grep -qE '"success"[[:space:]]*:[[:space:]]*true'; then
  echo -e "${RED}Deploy failed: $RESPONSE${NC}"
  exit 1
fi
echo -e "  ${GREEN}Deployed $APP_ID${NC}"
sleep 2

send_weather() {
  local payload="$1"
  curl -s --max-time 30 -X POST "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/weather:weather-$RUN_ID/ask?timeout=30" \
    -H "Content-Type: application/json" \
    -d "$payload" 2>/dev/null || echo '{"status":"error","error":"timeout"}'
}

send_weather '{"op":"clear_cache"}' >/dev/null
LONDON_API=$(send_weather '{"op":"get_weather","city":"London"}')
LONDON_CACHE=$(send_weather '{"op":"get_weather","city":"London"}')
STATS_BEFORE=$(send_weather '{"op":"cache_stats"}')
CLEAR_RESP=$(send_weather '{"op":"clear_cache"}')
PARIS_API=$(send_weather '{"op":"get_weather","city":"Paris"}')
STATS_AFTER=$(send_weather '{"op":"cache_stats"}')
APP_METRICS=$(send_weather '{"op":"app_metrics"}')

echo "Step 3: Output and metrics"
if ! \
  LONDON_API="$LONDON_API" \
  LONDON_CACHE="$LONDON_CACHE" \
  PARIS_API="$PARIS_API" \
  STATS_BEFORE="$STATS_BEFORE" \
  STATS_AFTER="$STATS_AFTER" \
  CLEAR_RESP="$CLEAR_RESP" \
  APP_METRICS="$APP_METRICS" \
  APP_ID="$APP_ID" \
  python3 - <<'PY'
import json
import os
import sys


def payload(raw):
    top = json.loads((raw or "").strip() or "{}")
    body = top.get("payload", top)
    if isinstance(body, str):
        body = json.loads(body.strip() or "{}")
    if isinstance(top, dict) and top.get("status") == "error":
        raise SystemExit(f"actor request failed: {top}")
    return body if isinstance(body, dict) else {}


london_api = payload(os.environ["LONDON_API"])
london_cache = payload(os.environ["LONDON_CACHE"])
paris_api = payload(os.environ["PARIS_API"])
stats_before = payload(os.environ["STATS_BEFORE"])
stats_after = payload(os.environ["STATS_AFTER"])
clear_resp = payload(os.environ["CLEAR_RESP"])
metrics = payload(os.environ["APP_METRICS"])

print("  Weather responses")
print(f"  London API:   {json.dumps(london_api, sort_keys=True)}")
print(f"  London cache: {json.dumps(london_cache, sort_keys=True)}")
print(f"  Paris API:    {json.dumps(paris_api, sort_keys=True)}")
print()
print("  Cache stats")
print(f"  before_clear: {json.dumps(stats_before, sort_keys=True)}")
print(f"  after_clear:  {json.dumps(stats_after, sort_keys=True)}")
print()
print("  Application metrics")
if not metrics:
    print("  (no metrics object returned by actor)")
else:
    print(f"  message_count={metrics.get('message_count')} error_count={metrics.get('error_count')}")
    for metric_map in ("counter_metrics", "latency_totals_ms", "latency_max_ms", "latency_samples"):
        values = metrics.get(metric_map)
        if isinstance(values, dict) and values:
            print(f"  {metric_map}")
            for key in sorted(values):
                print(f"    {key}: {values[key]}")

if london_api.get("source") != "api" or london_cache.get("source") != "cache":
    print("expected first London response from api and second from cache", file=sys.stderr)
    sys.exit(1)
if clear_resp.get("cleared") is not True:
    print("expected clear_cache to return cleared=true", file=sys.stderr)
    sys.exit(1)
if int(stats_before.get("hits", 0) or 0) < 1 or int(stats_before.get("misses", 0) or 0) < 1:
    print("expected cache_stats before clear to show at least one hit and one miss", file=sys.stderr)
    sys.exit(1)
if int(stats_after.get("hits", 0) or 0) != 0 or int(stats_after.get("misses", 0) or 0) < 1:
    print("expected cache_stats after clear to reset hits and record one miss", file=sys.stderr)
    sys.exit(1)
if int(metrics.get("message_count", 0) or 0) < 1:
    print("expected unified application metrics message_count >= 1", file=sys.stderr)
    sys.exit(1)
PY
then
  echo -e "${RED}Output/metrics validation failed${NC}"
  exit 1
fi

echo ""
echo "================================================================"
echo -e "  ${GREEN}Weather Actor (Python) example test complete${NC}"
echo "================================================================"
