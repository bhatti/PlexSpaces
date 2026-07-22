#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# E2E test: Deploy WASM app and run batch prediction operations with metrics

set -euo pipefail

HTTP_PORT="${1:-8091}"
APP_ID="orleans-batch-predictor"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/batch_predictor_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
BLUE='\033[0;34m'
NC='\033[0m'

# Metrics tracking (initialize to avoid unbound variable errors)
TOTAL_START_TIME=0
TOTAL_END_TIME=0
TOTAL_TIME_MS=0
COORDINATE_TIME_MS=0
COMPUTE_TIME_MS=0
MESSAGE_COUNT=0
BATCH_COUNT=0

# Helper function to get current time in milliseconds
# Works on macOS (date) and Linux (date), with Python fallback

# Auto-generate JWT if not provided
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
if [ -z "${PLEXSPACES_TEST_TOKEN:-}" ] && [ -f "$REPO_ROOT/scripts/gen-test-jwt.sh" ]; then
  source ~/venv/bin/activate 2>/dev/null || true
  echo "Generating JWT token..."
  JWT_OUTPUT="$(PLEXSPACES_JWT_PRIVATE_KEY_FILE="$REPO_ROOT/certs/jwt-es256.pem" "$REPO_ROOT/scripts/gen-test-jwt.sh")"
  eval "$JWT_OUTPUT"
  if [ -z "${PLEXSPACES_TEST_TOKEN:-}" ]; then
    echo "ERROR: gen-test-jwt.sh failed to set PLEXSPACES_TEST_TOKEN"
    echo "Output was: $JWT_OUTPUT"
    exit 1
  fi
fi
AUTH_HEADER=""
if [ -n "${PLEXSPACES_TEST_TOKEN:-}" ]; then
  AUTH_HEADER="Authorization: Bearer $PLEXSPACES_TEST_TOKEN"
fi

get_time_ms() {
    # Try date with nanoseconds (Linux) - strip any trailing N
    local time_ns=$(date +%s%N 2>/dev/null | sed 's/N$//' || echo "")
    if [ -n "$time_ns" ] && [ "$time_ns" != "" ]; then
        # Convert nanoseconds to milliseconds
        echo "$((time_ns / 1000000))"
    else
        # Fallback: use seconds * 1000 (less precise but works everywhere)
        local time_s=$(date +%s 2>/dev/null || echo "0")
        echo "$((time_s * 1000))"
    fi
}

# Helper function to measure operation with metrics
actor_op_with_metrics() {
    local actor_id="$1"
    local desc="$2"
    local payload="$3"
    local is_compute="${4:-false}"  # true if this is a compute operation
    
    local op_start=$(get_time_ms)
    
    # Coordination phase: HTTP request/response overhead
    local coord_start=$(get_time_ms)
    trap 'rm -f "${APP_ZIP:-}"' EXIT
    APP_ZIP="$(mktemp /tmp/app_XXXXXX.zip)"
rm -f "$APP_ZIP"
    zip -j "$APP_ZIP" "$WASM_FILE" "$CONFIG_FILE" >/dev/null
    RESPONSE=$(curl -s -w "\n%{time_total}" -X POST "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$actor_id" \
        -H "Content-Type: application/json" \
        ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
        -d "$payload" 2>/dev/null) || true
    
    local coord_end=$(get_time_ms)
    # Use bc/awk for arithmetic to handle large numbers
    local coord_time=$(echo "$coord_end - $coord_start" | bc 2>/dev/null || awk "BEGIN {print $coord_end - $coord_start}")
    
    # Extract HTTP time from curl output (last line)
    # Use sed to remove last line (works on both macOS and Linux)
    # curl -w "\n%{time_total}" outputs: response_body\n<time>
    local http_time=$(echo "$RESPONSE" | tail -1)
    local response_body=$(echo "$RESPONSE" | sed '$d')
    
    # Handle edge case: if response is empty or single line, use entire response as body
    if [ -z "$response_body" ]; then
        response_body="$RESPONSE"
    fi
    
    # Estimate coordination time (HTTP overhead + message passing)
    local estimated_coord_time=$(echo "$http_time * 1000" | bc 2>/dev/null || echo "$coord_time")
    COORDINATE_TIME_MS=$(echo "$COORDINATE_TIME_MS + $estimated_coord_time" | bc 2>/dev/null || echo "$COORDINATE_TIME_MS")
    
    # If this is a compute operation, extract compute time from response
    if [ "$is_compute" = "true" ]; then
        # Extract compute_time_ms from response if available
        local compute_time=$(echo "$response_body" | grep -o '"compute_time_ms":[0-9.]*' | cut -d':' -f2 || echo "")
        if [ -n "$compute_time" ] && [ "$compute_time" != "" ]; then
            # Convert to integer milliseconds (round)
            local compute_ms=$(echo "$compute_time" | awk '{printf "%.0f", $1}')
            COMPUTE_TIME_MS=$(echo "$COMPUTE_TIME_MS + $compute_ms" | bc 2>/dev/null || echo "$COMPUTE_TIME_MS")
        else
            # Fallback: estimate compute time as total time minus coordination overhead
            local total_op_time=$(echo "$coord_end - $op_start" | bc 2>/dev/null || awk "BEGIN {print $coord_end - $op_start}")
            local estimated_compute=$(echo "$total_op_time - $estimated_coord_time" | bc 2>/dev/null || awk "BEGIN {print $total_op_time - $estimated_coord_time}")
            if [ -n "$estimated_compute" ] && [ "$(echo "$estimated_compute > 0" | bc 2>/dev/null || awk "BEGIN {if ($estimated_compute > 0) print 1; else print 0}")" = "1" ]; then
                COMPUTE_TIME_MS=$(echo "$COMPUTE_TIME_MS + $estimated_compute" | bc 2>/dev/null || awk "BEGIN {print $COMPUTE_TIME_MS + $estimated_compute}")
            fi
        fi
        BATCH_COUNT=$((BATCH_COUNT + 1))
    fi
    
    MESSAGE_COUNT=$((MESSAGE_COUNT + 1))
    
    if echo "$response_body" | grep -q '"success":true'; then
        echo -e "  ${GREEN}✓${NC} [$actor_id] $desc"
        return 0
    else
        echo -e "  ${YELLOW}⚠${NC} [$actor_id] $desc: $response_body"
        return 1
    fi
}

# Helper function for simple operations (no compute tracking)
actor_op() {
    actor_op_with_metrics "$1" "$2" "$3" "false"
}

echo "╔════════════════════════════════════════════════════════════════╗"
echo "║     Orleans vs PlexSpaces - Batch Prediction E2E Test        ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""

# Step 1: Check if WASM file exists
if [ ! -f "$WASM_FILE" ]; then
    echo -e "${RED}❌ WASM file not found: $WASM_FILE${NC}"
    echo "   Run ./build.sh first to build the WASM component"
    exit 1
fi

# Step 2: Undeploy existing app (if any)
echo "🧹 Cleaning up existing deployment..."
"$SCRIPT_DIR/undeploy.sh" "$HTTP_PORT"
sleep 1

# Step 3: Deploy WASM app via HTTP API
echo ""
echo "📦 Deploying WASM application..."
DEPLOY_START=$(get_time_ms)
_deployed=0
for _attempt in 1 2 3; do
  if [ -f "$CONFIG_FILE" ]; then
    DEPLOY_OUT=$(curl -s -w "\n%{http_code}" -X POST "http://localhost:$HTTP_PORT/api/v1/applications/deploy" \
        ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
        -F "application_id=$APP_ID" \
        -F "name=$APP_ID" \
        -F "version=1.0.0" \
        -F "app_file=@$APP_ZIP" 2>&1) || true
  else
    APP_ZIP="$(mktemp /tmp/app_XXXXXX.zip)"
    DEPLOY_OUT=$(curl -s -w "\n%{http_code}" -X POST "http://localhost:$HTTP_PORT/api/v1/applications/deploy" \
        ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
        -F "application_id=$APP_ID" \
        -F "name=$APP_ID" \
        -F "version=1.0.0" \
        (cd "$(dirname "$WASM_FILE")" && zip -j "$APP_ZIP" "$WASM_FILE" 2>/dev/null) || zip -j "$APP_ZIP" "$WASM_FILE"
        -F "app_file=@$APP_ZIP" 2>&1)
  fi
  HTTP_CODE=$(echo "$DEPLOY_OUT" | tail -n1)
  RESPONSE=$(echo "$DEPLOY_OUT" | sed '$d')
  if [ "$HTTP_CODE" = "200" ] && echo "$RESPONSE" | grep -qE '"success"[[:space:]]*:[[:space:]]*true'; then
    _deployed=1
    break
  fi
  echo "  Deploy attempt $_attempt failed, retrying in 3s..."
  sleep 3
done
if [ "$_deployed" -eq 0 ]; then
  echo -e "${RED}Deploy failed: $RESPONSE${NC}"
  exit 1
fi
echo -e "  ${GREEN}✓${NC} Application deployed successfully"

# Wait for application to start
echo "  Waiting for application to initialize..."
sleep 2

# Step 4: Run E2E tests with metrics tracking
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Step 4: Batch Prediction Operations"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

TOTAL_START_TIME=$(get_time_ms)

echo "🤖 Loading model..."
actor_op "batch-predictor-model-1" "Load model ml-model-v1" '{"op":"load_model","model_id":"ml-model-v1"}'

echo ""
echo "📊 Processing batch 1 (3 data points)..."
actor_op_with_metrics "batch-predictor-model-1" "Predict batch 1" '{"op":"predict_batch","shard_path":"s3://bucket/data/shard-1.parquet","data":[{"id":"data-1","features":[1.0,2.0,3.0,4.0]},{"id":"data-2","features":[5.0,6.0,7.0,8.0]},{"id":"data-3","features":[9.0,10.0,11.0,12.0]}]}' "true"

echo ""
echo "📊 Processing batch 2 (2 data points)..."
actor_op_with_metrics "batch-predictor-model-1" "Predict batch 2" '{"op":"predict_batch","shard_path":"s3://bucket/data/shard-2.parquet","data":[{"id":"data-4","features":[13.0,14.0,15.0,16.0]},{"id":"data-5","features":[17.0,18.0,19.0,20.0]}]}' "true"

echo ""
echo "📈 Getting statistics..."
actor_op "batch-predictor-model-1" "Get stats" '{"op":"get_stats"}'

TOTAL_END_TIME=$(get_time_ms)
# Use bc/awk for arithmetic to handle large numbers
TOTAL_TIME_MS=$(echo "$TOTAL_END_TIME - $TOTAL_START_TIME" | bc 2>/dev/null || awk "BEGIN {print $TOTAL_END_TIME - $TOTAL_START_TIME}")

# Step 5: Display Performance Metrics
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Step 5: Performance Metrics"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

# Calculate percentages and ratios (use awk as fallback if bc not available)
# Ensure TOTAL_TIME_MS is set (default to 1 to avoid division by zero)
if [ -z "$TOTAL_TIME_MS" ] || [ "$TOTAL_TIME_MS" = "0" ] || [ "$TOTAL_TIME_MS" = "" ]; then
    TOTAL_TIME_MS=1
fi

if command -v bc >/dev/null 2>&1; then
    TOTAL_TIME_S=$(echo "scale=2; $TOTAL_TIME_MS / 1000" | bc 2>/dev/null || echo "0")
    COORD_PCT=$(echo "scale=1; ($COORDINATE_TIME_MS * 100) / $TOTAL_TIME_MS" | bc 2>/dev/null || echo "0")
    COMPUTE_PCT=$(echo "scale=1; ($COMPUTE_TIME_MS * 100) / $TOTAL_TIME_MS" | bc 2>/dev/null || echo "0")
    EFFICIENCY=$(echo "scale=1; ($COMPUTE_TIME_MS * 100) / $TOTAL_TIME_MS" | bc 2>/dev/null || echo "0")
    if [ -n "$COORDINATE_TIME_MS" ] && [ "$COORDINATE_TIME_MS" != "0" ] && [ "$COORDINATE_TIME_MS" != "" ]; then
        GRANULARITY_RATIO=$(echo "scale=2; $COMPUTE_TIME_MS / $COORDINATE_TIME_MS" | bc 2>/dev/null || echo "0")
    else
        GRANULARITY_RATIO="0"
    fi
else
    # Fallback to awk for calculations
    TOTAL_TIME_S=$(awk "BEGIN {if ($TOTAL_TIME_MS > 0) printf \"%.2f\", $TOTAL_TIME_MS / 1000; else print \"0\"}")
    COORD_PCT=$(awk "BEGIN {if ($TOTAL_TIME_MS > 0) printf \"%.1f\", ($COORDINATE_TIME_MS * 100) / $TOTAL_TIME_MS; else print \"0\"}")
    COMPUTE_PCT=$(awk "BEGIN {if ($TOTAL_TIME_MS > 0) printf \"%.1f\", ($COMPUTE_TIME_MS * 100) / $TOTAL_TIME_MS; else print \"0\"}")
    EFFICIENCY=$(awk "BEGIN {if ($TOTAL_TIME_MS > 0) printf \"%.1f\", ($COMPUTE_TIME_MS * 100) / $TOTAL_TIME_MS; else print \"0\"}")
    if [ -n "$COORDINATE_TIME_MS" ] && [ "$COORDINATE_TIME_MS" != "0" ] && [ "$COORDINATE_TIME_MS" != "" ]; then
        GRANULARITY_RATIO=$(awk "BEGIN {if ($COORDINATE_TIME_MS > 0) printf \"%.2f\", $COMPUTE_TIME_MS / $COORDINATE_TIME_MS; else print \"0\"}")
    else
        GRANULARITY_RATIO="0"
    fi
fi

# Ensure all values are set (default to 0 if empty)
TOTAL_TIME_S=${TOTAL_TIME_S:-0}
COORD_PCT=${COORD_PCT:-0}
COMPUTE_PCT=${COMPUTE_PCT:-0}
EFFICIENCY=${EFFICIENCY:-0}
GRANULARITY_RATIO=${GRANULARITY_RATIO:-0}

echo "Execution Summary:"
echo "  Total execution time: ${TOTAL_TIME_MS}ms (${TOTAL_TIME_S}s)"
echo "  Operations completed: $MESSAGE_COUNT"
echo "  Batch predictions: $BATCH_COUNT"
echo "  Data points processed: $((BATCH_COUNT * 5))"  # Approximate: 3 + 2 batches
echo ""

echo "Coordination vs Computation Breakdown:"
printf "  Coordination time: %.2fms (%.1f%%)\n" "$COORDINATE_TIME_MS" "$COORD_PCT"
printf "  Computation time: %.2fms (%.1f%%)\n" "$COMPUTE_TIME_MS" "$COMPUTE_PCT"
printf "  Efficiency (compute/total): %.1f%%\n" "$EFFICIENCY"
echo ""

echo "Message Metrics:"
echo "  Total messages sent: $MESSAGE_COUNT"
if [ "$MESSAGE_COUNT" -gt 0 ]; then
    if command -v bc >/dev/null 2>&1; then
        AVG_LATENCY=$(echo "scale=2; $TOTAL_TIME_MS / $MESSAGE_COUNT" | bc)
        THROUGHPUT=$(echo "scale=1; $MESSAGE_COUNT / $TOTAL_TIME_S" | bc)
    else
        AVG_LATENCY=$(awk "BEGIN {printf \"%.2f\", $TOTAL_TIME_MS / $MESSAGE_COUNT}")
        THROUGHPUT=$(awk "BEGIN {printf \"%.1f\", $MESSAGE_COUNT / $TOTAL_TIME_S}")
    fi
    printf "  Average latency per message: %.2fms\n" "$AVG_LATENCY"
    printf "  Message throughput: %.1f msg/s\n" "$THROUGHPUT"
fi
echo ""

echo "Granularity Analysis:"
if command -v bc >/dev/null 2>&1; then
    GRANULARITY_CHECK=$(echo "$GRANULARITY_RATIO > 0" | bc 2>/dev/null || echo "0")
else
    GRANULARITY_CHECK=$(awk "BEGIN {if ($GRANULARITY_RATIO > 0) print 1; else print 0}")
fi

if [ "$GRANULARITY_CHECK" = "1" ]; then
    printf "  Granularity ratio (compute/coordinate): %.2f\n" "$GRANULARITY_RATIO"
    if command -v bc >/dev/null 2>&1; then
        RATIO_100=$(echo "$GRANULARITY_RATIO >= 100" | bc)
        RATIO_10=$(echo "$GRANULARITY_RATIO >= 10" | bc)
        RATIO_1=$(echo "$GRANULARITY_RATIO >= 1" | bc)
    else
        RATIO_100=$(awk "BEGIN {if ($GRANULARITY_RATIO >= 100) print 1; else print 0}")
        RATIO_10=$(awk "BEGIN {if ($GRANULARITY_RATIO >= 10) print 1; else print 0}")
        RATIO_1=$(awk "BEGIN {if ($GRANULARITY_RATIO >= 1) print 1; else print 0}")
    fi
    
    if [ "$RATIO_100" = "1" ]; then
        echo "  ${GREEN}✅ Excellent granularity (coordination overhead is negligible)${NC}"
    elif [ "$RATIO_10" = "1" ]; then
        echo "  ${GREEN}✅ Good granularity (coordination overhead is low)${NC}"
    elif [ "$RATIO_1" = "1" ]; then
        echo "  ${YELLOW}⚠️  Moderate granularity (coordination overhead is noticeable)${NC}"
    else
        echo "  ${RED}❌ Poor granularity (coordination overhead dominates)${NC}"
    fi
else
    echo "  Granularity ratio: N/A (no coordination overhead measured)"
fi
echo ""

send_actor() {
  local actor="$1" payload="$2" timeout="${3:-20}"
  curl -s --max-time "$timeout" -X POST \
    "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$actor/ask?timeout=$timeout" \
    -H "Content-Type: application/json" \
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
    -d "$payload" 2>/dev/null || echo '{"error":"timeout"}'
}

assert_actor_ok() {
  local step="$1" raw="$2"
  if ! ASSERT_STEP="$step" ASSERT_JSON="$raw" python3 - <<'PY'
import json, os, sys
raw = os.environ.get("ASSERT_JSON", "")
step = os.environ.get("ASSERT_STEP", "?")
try:
    d = json.loads(raw)
except Exception as e:
    print(f"FAIL [{step}]: invalid JSON: {e}", file=sys.stderr); sys.exit(1)
def find_err(obj):
    if isinstance(obj, dict):
        if obj.get("success") is False: return str(obj.get("error") or "request failed")
        if obj.get("status") == "error": return str(obj.get("error") or "error status")
        if obj.get("error"): return str(obj["error"])
        for v in obj.values():
            e = find_err(v)
            if e: return e
    return None
err = find_err(d)
if err: print(f"FAIL [{step}]: {err}", file=sys.stderr); sys.exit(1)
PY
  then
    echo -e "${RED}Assertion failed: $step${NC}"
    echo "  Raw: $(echo "$raw" | head -c 400)"
    exit 1
  fi
}

echo ""
echo "Step 6: Smoke test batch predictor"
echo "  Testing get_stats..."
R="$(send_actor "batch-predictor-model-1" '{"op":"get_stats"}' 20)"
assert_actor_ok "get_stats" "$R"
echo "  ✓ get_stats OK"

echo "  Testing predict_batch..."
R="$(send_actor "batch-predictor-model-1" '{"op":"predict_batch","data":[{"id":"smoke-1","features":[1.0,2.0,3.0,4.0]}]}' 20)"
assert_actor_ok "predict_batch" "$R"
echo "  ✓ predict_batch OK"

echo -e "${GREEN}✅ E2E test complete!${NC}"
