#!/usr/bin/env bash
# Weather Actor (Rust WASM): get_weather, cache_stats, clear_cache.
# Usage:
#   ./test.sh [HTTP_PORT]     # contract tests + full example run
#   ./test.sh --contract-only # contract tests only
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
WASM_FILE="$SCRIPT_DIR/weather_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
HTTP_PORT="${1:-8091}"
APP_ID="weather-rust"
ACTOR_TYPE="weather"
INSTANCE_ID="default"

if [[ "${1:-}" == "--contract-only" ]]; then
  HTTP_PORT=""
fi

_RUST_APPS_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
# shellcheck source=/dev/null
source "${_RUST_APPS_ROOT}/test-common.sh"

export CARGO_TARGET_DIR="$REPO_ROOT/target"

GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m'

echo "================================================================"
echo "  Weather Actor (Rust WASM, service link + KV cache)"
echo "================================================================"

echo "Step 0: Contract tests"
cargo test --manifest-path "$SCRIPT_DIR/Cargo.toml" --lib
echo -e "  ${GREEN}Contract tests passed${NC}"
echo ""

if [[ -z "$HTTP_PORT" ]]; then
  echo -e "${GREEN}Contract-only run complete${NC}"
  exit 0
fi

echo "Step 1: Build WASM"
"$SCRIPT_DIR/build.sh" || { echo -e "${RED}Build failed${NC}"; exit 1; }
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

send_op() {
  local payload="$1"
  local timeout="${2:-20}"
  curl -s --max-time "$timeout" -X POST "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/${ACTOR_TYPE}:${INSTANCE_ID}/ask?timeout=$timeout" \
    -H "Content-Type: application/json" \
    -d "$payload" 2>/dev/null || echo '{"status":"error","error":"timeout"}'
}

assert_actor_ok() {
  local step_name="$1"
  local json_raw="$2"
  if ! ASSERT_STEP="$step_name" ASSERT_JSON="$json_raw" python3 - <<'PY'
import json
import os
import sys

raw = os.environ.get("ASSERT_JSON", "")
step = os.environ.get("ASSERT_STEP", "?")
try:
    top = json.loads(raw)
except json.JSONDecodeError as exc:
    print(f"FAIL [{step}]: invalid JSON: {exc}", file=sys.stderr)
    sys.exit(1)

if isinstance(top, dict) and top.get("status") == "error":
    payload = top.get("payload")
    if isinstance(payload, str):
        try:
            payload = json.loads(payload)
        except Exception:
            pass
    if isinstance(payload, dict) and payload.get("error"):
        print(f"FAIL [{step}]: {payload['error']}", file=sys.stderr)
    else:
        print(f"FAIL [{step}]: {top.get('error') or top}", file=sys.stderr)
    sys.exit(1)
PY
  then
    echo -e "${RED}Assertion failed: $step_name${NC}"
    echo "  Raw: $(echo "$json_raw" | head -c 400)"
    exit 1
  fi
}

echo "Step 3: Weather requests"
LONDON_API=$(send_op '{"op":"get_weather","city":"London"}' 30)
assert_actor_ok "get_weather London API" "$LONDON_API"

LONDON_CACHE=$(send_op '{"op":"get_weather","city":"London"}' 30)
assert_actor_ok "get_weather London cache" "$LONDON_CACHE"

STATS_BEFORE_CLEAR=$(send_op '{"op":"cache_stats"}' 15)
assert_actor_ok "cache_stats before clear" "$STATS_BEFORE_CLEAR"

CLEAR_RESP=$(send_op '{"op":"clear_cache"}' 15)
assert_actor_ok "clear_cache" "$CLEAR_RESP"

SYDNEY_API=$(send_op '{"op":"get_weather","city":"Sydney"}' 30)
assert_actor_ok "get_weather Sydney API" "$SYDNEY_API"

STATS_AFTER_CLEAR=$(send_op '{"op":"cache_stats"}' 15)
assert_actor_ok "cache_stats after clear" "$STATS_AFTER_CLEAR"

APP_METRICS=$(send_op '{"op":"get_metrics"}' 15)
assert_actor_ok "get_metrics" "$APP_METRICS"

echo "Step 4: Output and metrics"
if ! \
  LONDON_API="$LONDON_API" \
  LONDON_CACHE="$LONDON_CACHE" \
  SYDNEY_API="$SYDNEY_API" \
  STATS_BEFORE_CLEAR="$STATS_BEFORE_CLEAR" \
  STATS_AFTER_CLEAR="$STATS_AFTER_CLEAR" \
  CLEAR_RESP="$CLEAR_RESP" \
  APP_METRICS="$APP_METRICS" \
  APP_ID="$APP_ID" \
  python3 - <<'PY'
import json
import os
import sys
from typing import Any, Dict, Optional


def unwrap_payload(raw: str) -> Dict[str, Any]:
    top = json.loads((raw or "").strip() or "{}")
    payload = top.get("payload", top)
    if isinstance(payload, str):
        payload = json.loads(payload.strip() or "{}")
    return payload if isinstance(payload, dict) else {}


def print_map(metrics: Dict[str, Any], key: str) -> None:
    values = metrics.get(key)
    if not isinstance(values, dict) or not values:
        return
    print(f"  {key}")
    for name in sorted(values):
        print(f"    {name}: {values[name]}")


london_api = unwrap_payload(os.environ["LONDON_API"])
london_cache = unwrap_payload(os.environ["LONDON_CACHE"])
sydney_api = unwrap_payload(os.environ["SYDNEY_API"])
stats_before = unwrap_payload(os.environ["STATS_BEFORE_CLEAR"])
stats_after = unwrap_payload(os.environ["STATS_AFTER_CLEAR"])
clear_resp = unwrap_payload(os.environ["CLEAR_RESP"])

print("  Weather responses")
print("  ────────────────────────────────────────────")
print(
    "  London API:   city={city} temp_c={temp_c} wind_kph={wind_kph} source={source}".format(
        city=london_api.get("city"),
        temp_c=london_api.get("temp_c"),
        wind_kph=london_api.get("wind_kph"),
        source=london_api.get("source"),
    )
)
print(
    "  London cache: city={city} temp_c={temp_c} wind_kph={wind_kph} source={source}".format(
        city=london_cache.get("city"),
        temp_c=london_cache.get("temp_c"),
        wind_kph=london_cache.get("wind_kph"),
        source=london_cache.get("source"),
    )
)
print(
    "  Sydney API:   city={city} temp_c={temp_c} wind_kph={wind_kph} source={source}".format(
        city=sydney_api.get("city"),
        temp_c=sydney_api.get("temp_c"),
        wind_kph=sydney_api.get("wind_kph"),
        source=sydney_api.get("source"),
    )
)

if london_api.get("source") != "api":
    print("Expected first London response from API", file=sys.stderr)
    sys.exit(1)
if london_cache.get("source") != "cache":
    print("Expected second London response from cache", file=sys.stderr)
    sys.exit(1)
if clear_resp.get("cleared") is not True:
    print("Expected clear_cache to return cleared=true", file=sys.stderr)
    sys.exit(1)

print()
print("  Cache stats")
print("  ────────────────────────────────────────────")
print(f"  before_clear: hits={stats_before.get('hits')} misses={stats_before.get('misses')}")
print(f"  after_clear:  hits={stats_after.get('hits')} misses={stats_after.get('misses')}")

if int(stats_before.get("hits", 0) or 0) < 1 or int(stats_before.get("misses", 0) or 0) < 1:
    print("Expected cache_stats before clear to show at least one hit and miss", file=sys.stderr)
    sys.exit(1)
if int(stats_after.get("hits", 0) or 0) != 0 or int(stats_after.get("misses", 0) or 0) < 1:
    print("Expected cache_stats after clear to reset hits and record one new miss", file=sys.stderr)
    sys.exit(1)

metrics = unwrap_payload(os.environ["APP_METRICS"])

print()
print("  Application metrics")
print("  ────────────────────────────────────────────")
if not metrics:
    print("  (no metrics payload returned)")
else:
    print(f"  message_count: {metrics.get('message_count')}")
    print(f"  error_count:   {metrics.get('error_count')}")
    print_map(metrics, "counter_metrics")
    print_map(metrics, "latency_totals_ms")
    print_map(metrics, "latency_max_ms")
    print_map(metrics, "latency_samples")
PY
then
  echo -e "${RED}Output/metrics validation failed${NC}"
  exit 1
fi

echo ""
echo "================================================================"
echo -e "  ${GREEN}Weather Actor (Rust) example test complete${NC}"
echo "================================================================"
