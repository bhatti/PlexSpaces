#!/bin/bash
# Test IoT Sensor Aggregation - Gosiris-style Actor Network (Go WASM)
#
# Usage: ./test.sh [HTTP_PORT]
#   HTTP_PORT: PlexSpaces HTTP gateway port (default: 8091)
#
# Prerequisites: Start PlexSpaces node first:
#   PLEXSPACES_JWT_SECRET=test cargo run -p plexspaces-cli -- start --node-id test-node --listen-addr 0.0.0.0:8091
# (HTTP gateway will be on 8091 + 1 = 8091)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/sensor_aggregation.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
HTTP_PORT="${1:-8091}"

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

APP_ID="gosiris-iot-sensors"
NUM_SENSORS=4
BATCH_READINGS=2000
POLL_ROUNDS=10


echo "================================================================"
echo "  Gosiris IoT Sensor Aggregation"
echo "  PlexSpaces Go WASM App"
echo "================================================================"
echo ""
echo "Configuration:"
echo "  HTTP Gateway:    localhost:$HTTP_PORT"
echo "  Sensors:         $NUM_SENSORS"
echo "  Batch readings:  $BATCH_READINGS"
echo "  Poll rounds:     $POLL_ROUNDS"
echo ""

# Build if needed
if [ ! -f "$WASM_FILE" ]; then
    echo "Building WASM actor..."
    chmod +x "$SCRIPT_DIR/build.sh"
    "$SCRIPT_DIR/build.sh" || { echo -e "${RED}Build failed${NC}"; exit 1; }
    echo ""
fi

# Step 1: Check node
echo "Step 1: Check node status"
echo "----------------------------------------------------------------"
HTTP_CHECK=$(curl -s -o /dev/null -w "%{http_code}" "http://localhost:$HTTP_PORT/" 2>/dev/null) || HTTP_CHECK="000"
if [ "$HTTP_CHECK" = "000" ]; then
    echo -e "${RED}Cannot connect to node at localhost:$HTTP_PORT${NC}"
    echo "Start node first: PLEXSPACES_JWT_SECRET=test cargo run -p plexspaces-cli -- start --node-id test-node --listen-addr 0.0.0.0:$((HTTP_PORT - 1))"
    exit 1
fi
echo -e "${GREEN}Node is running${NC}"
echo ""


# Step 2: Deploy
echo "Step 2: Deploy sensor network actors"
echo "----------------------------------------------------------------"

"$SCRIPT_DIR/undeploy.sh" "$HTTP_PORT"
sleep 1

_deployed=0
for _attempt in 1 2 3; do
DEPLOY_OUT=$(curl -s -w "\n%{http_code}" -X POST "http://localhost:$HTTP_PORT/api/v1/applications/deploy" \
    -F "application_id=$APP_ID" \
    -F "name=gosiris-iot-sensors" \
    -F "version=1.0.0" \
    -F "wasm_file=@$WASM_FILE;type=application/wasm" \
    -F "config=@$CONFIG_FILE" 2>&1) || true
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

if echo "$RESPONSE" | grep -qi '"success":\s*true'; then
    echo -e "${GREEN}Deployed $APP_ID${NC}"
    echo "  - Aggregator:       aggregator"
    echo "  - Sensors:          sensor-dc-zone-a, sensor-dc-zone-b, sensor-server-room, sensor-outdoor"
else
    echo -e "${RED}Deploy failed: $RESPONSE${NC}"
    exit 1
fi
echo ""
sleep 2

# Helper
send_op() {
    local actor="$1"
    local payload="$2"
    local timeout="${3:-60}"
    curl -s --max-time "$timeout" -X POST "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$actor/ask?timeout=$timeout" \
        -H "Content-Type: application/json" \
        -d "$payload" 2>/dev/null || echo '{"error":"timeout"}'
}

# Step 3: Verify actors
echo "Step 3: Verify all actors are responsive"
echo "----------------------------------------------------------------"

for SENSOR in "sensor-dc-zone-a" "sensor-dc-zone-b" "sensor-server-room" "sensor-outdoor"; do
    RESP=$(send_op "$SENSOR" '{"op":"stats"}' 5)
    if echo "$RESP" | grep -q '"status":"ok"'; then
        echo -e "  ${GREEN}$SENSOR OK${NC}"
    else
        echo -e "  ${RED}$SENSOR failed: $RESP${NC}"
        exit 1
    fi
done

AGG_RESP=$(send_op "aggregator" '{"op":"stats"}' 5)
if echo "$AGG_RESP" | grep -q '"status":"ok"'; then
    echo -e "  ${GREEN}aggregator OK${NC}"
else
    echo -e "  ${RED}aggregator failed: $AGG_RESP${NC}"
    exit 1
fi
echo ""

# Step 4: Individual sensor reads
echo "Step 4: Individual Sensor Reading Test"
echo "----------------------------------------------------------------"
echo ""

for SENSOR in "sensor-dc-zone-a" "sensor-dc-zone-b" "sensor-server-room" "sensor-outdoor"; do
    RESP=$(send_op "$SENSOR" '{"op":"read"}' 5)
    if echo "$RESP" | grep -q '"status":"ok"'; then
        echo "$RESP" | python3 -c "
import sys, json
d = json.load(sys.stdin)
p = d.get('payload', d)
r = p.get('reading', {})
print(f'  {r.get(\"sensor_id\",\"?\"):25s}  temp={r.get(\"temp_c\",0):6.2f}C  humidity={r.get(\"humidity\",0):5.2f}%')
" 2>/dev/null || echo "  $SENSOR: reading received"
    else
        echo -e "  ${RED}$SENSOR read failed: $RESP${NC}"
    fi
done
echo ""

# Step 5: Poll sensors via process group (aggregator asks each sensor)
echo "Step 5: Aggregator Polling via Process Groups ($POLL_ROUNDS rounds)"
echo "----------------------------------------------------------------"
echo ""

POLL_START=$(date +%s%N)

for i in $(seq 1 $POLL_ROUNDS); do
    RESP=$(send_op "aggregator" '{"op":"poll_sensors","group":"sensors"}' 10)
    if echo "$RESP" | grep -q '"status":"ok"'; then
        if [ "$i" -eq 1 ] || [ "$i" -eq $POLL_ROUNDS ]; then
            POLLED=$(echo "$RESP" | python3 -c "import sys,json; d=json.load(sys.stdin); p=d.get('payload',d); print(p.get('polled',0))" 2>/dev/null || echo "?")
            echo -e "  Round $i: ${GREEN}polled $POLLED sensors${NC}"
        fi
    else
        echo -e "  Round $i: ${RED}failed: $RESP${NC}"
    fi
done

POLL_END=$(date +%s%N)
POLL_WALL_MS=$(( (POLL_END - POLL_START) / 1000000 ))
echo -e "  ${CYAN}$POLL_ROUNDS rounds in ${POLL_WALL_MS}ms${NC}"
echo ""

# Step 6: Network-wide statistics
echo "Step 6: Network-Wide Sensor Statistics"
echo "----------------------------------------------------------------"
echo ""

NET_RESP=$(send_op "aggregator" '{"op":"get_network_stats"}')
if echo "$NET_RESP" | grep -q '"status":"ok"'; then
    echo "$NET_RESP" | python3 -c "
import sys, json
d = json.load(sys.stdin)
p = d.get('payload', d)
ts = p.get('network_temp', {})
hs = p.get('network_humid', {})

print(f'  Total sensors:     {p.get(\"total_sensors\", 0)}')
print(f'  Total readings:    {p.get(\"total_readings\", 0)}')
print(f'  Total anomalies:   {p.get(\"total_anomalies\", 0)}')
print()
print(f'  Temperature (all sensors)')
print(f'    Mean:   {ts.get(\"mean\",0):6.2f}C')
print(f'    StdDev: {ts.get(\"std_dev\",0):6.2f}C')
print(f'    Range:  {ts.get(\"min\",0):.2f}C - {ts.get(\"max\",0):.2f}C')
print()
print(f'  Humidity (all sensors)')
print(f'    Mean:   {hs.get(\"mean\",0):6.2f}%')
print(f'    StdDev: {hs.get(\"std_dev\",0):6.2f}%')
print(f'    Range:  {hs.get(\"min\",0):.2f}% - {hs.get(\"max\",0):.2f}%')
" 2>/dev/null || echo "  (Could not parse network stats)"
else
    echo -e "  ${RED}Network stats failed: $NET_RESP${NC}"
fi
echo ""

# Step 7: Batch Sensor Reading Benchmark
echo "Step 7: Batch Sensor Reading Benchmark ($BATCH_READINGS readings per sensor)"
echo "----------------------------------------------------------------"
echo ""

BATCH_START=$(date +%s%N)

for SENSOR in "sensor-dc-zone-a" "sensor-dc-zone-b" "sensor-server-room" "sensor-outdoor"; do
    RESP=$(send_op "$SENSOR" "{\"op\":\"read_batch\",\"count\":$BATCH_READINGS}" 120)
    if echo "$RESP" | grep -q '"status":"ok"'; then
        echo "$RESP" | python3 -c "
import sys, json
d = json.load(sys.stdin)
p = d.get('payload', d)
sensor = '$SENSOR'
dur = float(p.get('duration_ms', 0))
ops = float(p.get('ops_per_sec', 0))
hist = p.get('history_size', 0)
print(f'  {sensor:25s}  {dur:7.1f}ms  {ops:>10,.0f} readings/sec  history={hist}')
" 2>/dev/null || echo "  $SENSOR: batch completed"
    else
        echo -e "  ${RED}$SENSOR batch failed: $RESP${NC}"
    fi
done

BATCH_END=$(date +%s%N)
BATCH_WALL_MS=$(( (BATCH_END - BATCH_START) / 1000000 ))
echo ""
echo -e "  ${CYAN}Total: $((BATCH_READINGS * NUM_SENSORS)) readings in ${BATCH_WALL_MS}ms${NC}"
echo ""

# Step 8: Batch Ingestion Benchmark (aggregator processes readings)
echo "Step 8: Batch Ingestion Benchmark (aggregator ingests sensor data)"
echo "----------------------------------------------------------------"
echo ""

# First read batch from each sensor, then feed to aggregator
for SENSOR in "sensor-dc-zone-a" "sensor-server-room"; do
    HIST_RESP=$(send_op "$SENSOR" '{"op":"get_history","limit":200}' 30)
    if echo "$HIST_RESP" | grep -q '"status":"ok"'; then
        # Extract readings array and send to aggregator
        READINGS_JSON=$(echo "$HIST_RESP" | python3 -c "
import sys, json
d = json.load(sys.stdin)
p = d.get('payload', d)
readings = p.get('readings', [])
print(json.dumps({'op':'ingest_batch','readings':readings}))
" 2>/dev/null)
        if [ -n "$READINGS_JSON" ]; then
            INGEST_START=$(date +%s%N)
            INGEST_RESP=$(send_op "aggregator" "$READINGS_JSON" 60)
            INGEST_END=$(date +%s%N)
            INGEST_WALL_MS=$(( (INGEST_END - INGEST_START) / 1000000 ))

            if echo "$INGEST_RESP" | grep -q '"status":"ok"'; then
                echo "$INGEST_RESP" | python3 -c "
import sys, json
d = json.load(sys.stdin)
p = d.get('payload', d)
ingested = p.get('ingested', 0)
compute = float(p.get('compute_ms', 0))
ops = float(p.get('ops_per_sec', 0))
sensors = p.get('sensors', 0)
wall = $INGEST_WALL_MS
coord = wall - compute if wall > compute else 0
print(f'  $SENSOR -> aggregator')
print(f'    Ingested:    {ingested} readings')
print(f'    Compute:     {compute:.1f}ms')
print(f'    Coordination:{coord:.1f}ms')
print(f'    Throughput:  {ops:,.0f} ingest/sec')
" 2>/dev/null || echo "  Ingested readings from $SENSOR"
            else
                echo -e "  ${RED}Ingest from $SENSOR failed: $INGEST_RESP${NC}"
            fi
        fi
    fi
done
echo ""

# Step 9: Anomaly Detection
echo "Step 9: Anomaly Detection Report"
echo "----------------------------------------------------------------"
echo ""

ALERTS_RESP=$(send_op "aggregator" '{"op":"get_alerts"}')
if echo "$ALERTS_RESP" | grep -q '"status":"ok"'; then
    echo "$ALERTS_RESP" | python3 -c "
import sys, json
d = json.load(sys.stdin)
p = d.get('payload', d)
total = p.get('total_alerts', 0)
sensors_with = p.get('sensors_with_anomalies', 0)
alerts = p.get('alerts', [])
print(f'  Total alerts:             {total}')
print(f'  Sensors with anomalies:   {sensors_with}')
for a in alerts[:5]:
    print(f'    {a.get(\"sensor_id\",\"?\")}:  {a.get(\"anomalies\",0)} anomalies  temp_mean={a.get(\"temp_mean\",0):.2f}C  humid_mean={a.get(\"humid_mean\",0):.2f}%')
" 2>/dev/null || echo "  (Could not parse alerts)"
else
    echo -e "  ${RED}Alerts failed: $ALERTS_RESP${NC}"
fi
echo ""

# Step 10: Final Statistics & Benchmarks
echo "Step 10: Final Statistics & Benchmarks"
echo "================================================================"
echo ""

STATS_RESP=$(send_op "aggregator" '{"op":"stats"}')

if echo "$STATS_RESP" | grep -q '"status":"ok"'; then
    echo "$STATS_RESP" | python3 -c "
import sys, json
d = json.load(sys.stdin)
p = d.get('payload', d)
counters = p.get('counters', {})
bench = p.get('benchmarks', {})

print('  Aggregator Counters')
print('  ────────────────────────────────────────────')
print(f'  Total polls:        {counters.get(\"total_polls\", 0)}')
print(f'  Total readings:     {counters.get(\"total_readings\", 0):,}')
print(f'  Total alerts:       {counters.get(\"total_alerts\", 0)}')
print(f'  Active sensors:     {counters.get(\"active_sensors\", 0)}')
print()
print('  Performance Benchmarks')
print('  ────────────────────────────────────────────')
total_ms = float(bench.get('total_ms', 0))
compute_ms = float(bench.get('compute_ms', 0))
coord_ms = float(bench.get('coord_ms', 0))
compute_pct = float(bench.get('compute_pct', 0))
coord_pct = float(bench.get('coord_pct', 0))
gran = float(bench.get('granularity', 0))
ops = float(bench.get('ops_per_sec', 0))
mem = float(bench.get('memory_kb', 0))
stored = bench.get('readings_stored', 0)
print(f'  Total time:          {total_ms:.1f}ms')
print(f'  Compute time:        {compute_ms:.1f}ms ({compute_pct:.1f}%)')
print(f'  Coordination time:   {coord_ms:.1f}ms ({coord_pct:.1f}%)')
print(f'  Granularity:         {gran:.1f}x (compute/coordinate)')
print(f'  Throughput:          {ops:,.0f} readings/sec')
print(f'  Memory:              {mem:.1f} KB ({stored} readings stored)')
" 2>/dev/null || echo "  (Could not parse stats)"
else
    echo -e "  ${RED}Stats failed: $STATS_RESP${NC}"
fi

echo ""
echo "================================================================"
echo -e "  ${GREEN}Gosiris IoT Sensor Aggregation Test Complete${NC}"
echo "================================================================"
