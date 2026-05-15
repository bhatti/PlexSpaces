#!/bin/bash
# Start a PlexSpaces node. Optional port (default 8091).
# gRPC and HTTP/REST share a single port (single-port server).
# Usage: ./scripts/server.sh [PORT]
#   ./scripts/server.sh       # port 8091
#   ./scripts/server.sh 8093   # port 8093
# Log: plexspaces-node-{PORT}.log so multiple servers don't overwrite.
set -e
clear
GRPC_PORT="${1:-8091}"
NODE_LOG="plexspaces-node-${GRPC_PORT}.log"
NODE_META="plexspaces-node-${GRPC_PORT}.meta"
rm -f plexspaces-node-${GRPC_PORT}.log
rm -f "$NODE_META"

# Kill only processes on this node's port (so multiple nodes can run)
lsof -ti:${GRPC_PORT} | xargs kill -9 2>/dev/null || true
sleep 1

cd "$(dirname "$0")/.."

#echo "Building..."
###make build-fast build-cli
#make build

if [ ! -f "target/debug/plexspaces" ]; then
    echo "ERROR: target/debug/plexspaces not found after make build"
    exit 1
fi
echo "Binary built at: $(stat -f '%Sm' target/debug/plexspaces 2>/dev/null || stat -c '%y' target/debug/plexspaces 2>/dev/null || echo 'unknown')"
BINARY_MTIME_EPOCH="$(python3 - <<'PY'
import os
print(int(os.path.getmtime("target/debug/plexspaces")))
PY
)"

# Generate mTLS certs if needed
mkdir -p ./certs
if [ ! -f "./certs/ca.crt" ]; then
    target/debug/plexspaces generate-mtls \
      --output ./certs \
      --ca-common-name "PlexObject Solutions CA" \
      --server-common-name "my-node.plexobject.com" \
      --validity-days 365
fi

# Optional: clear apps for this run (comment out to keep apps across restarts)
# rm -rf ~/plexspaces/apps/*

export RUST_BACKTRACE=1
export PLEXSPACES_SAVE_WASM_APPS=1
export WASMTIME_BACKTRACE_DETAILS=1
export RUST_LOG="${RUST_LOG:-warn,plexspaces_actor=debug,plexspaces_node=debug,plexspaces_services=debug,plexspaces_wasm_runtime=debug,plexspaces_core=debug,plexspaces_application=debug,plexspaces_facet=debug,plexspaces_mailbox=debug}"
export PLEXSPACES_MTLS_CA_CERT="./certs/ca.crt"
export PLEXSPACES_MTLS_SERVER_CERT="./certs/server.crt"
export PLEXSPACES_MTLS_SERVER_KEY="./certs/server.key"
export PLEXSPACES_JWT_SECRET="${PLEXSPACES_JWT_SECRET:-test-secret}"
export PLEXSPACES_DISABLE_AUTH="${PLEXSPACES_DISABLE_AUTH:-1}"
# Same value on every node for multinode (registry labels, PingResponse, from_registry placement).
# Matches config_manager default when unset; override for multiple clusters.
#export PLEXSPACES_CLUSTER_NAME="${PLEXSPACES_CLUSTER_NAME:-default}"

RELEASE_CONFIG="${RELEASE_CONFIG:-release.yaml}"
if [ ! -f "$RELEASE_CONFIG" ]; then
    echo "ERROR: release config not found: $RELEASE_CONFIG (run from repo root)"
    exit 1
fi

echo "Starting PlexSpaces server (port: ${GRPC_PORT}, gRPC+HTTP single port) with config: $RELEASE_CONFIG"
: > "$NODE_LOG"
nohup target/debug/plexspaces start --node-id "test-node-${GRPC_PORT}" --listen-addr "0.0.0.0:${GRPC_PORT}" --release-config "$RELEASE_CONFIG" >> "$NODE_LOG" 2>&1 &
SERVER_PID="$!"
echo "Server PID: $SERVER_PID"
cat > "$NODE_META" <<EOF
binary_path=$(pwd)/target/debug/plexspaces
binary_mtime_epoch=$BINARY_MTIME_EPOCH
grpc_port=$GRPC_PORT
http_port=$GRPC_PORT
pid=$SERVER_PID
EOF
echo "Log: $NODE_LOG"
echo "Meta: $NODE_META"
echo "Waiting for server to start..."
sleep 5

SERVER_READY=0
for i in 1 2 3 4 5 6 7 8 9 10; do
    if curl -sf -o /dev/null "http://localhost:${GRPC_PORT}/" 2>/dev/null; then
        echo "Server is ready at http://localhost:${GRPC_PORT}/"
        SERVER_READY=1
        break
    fi
    echo "Waiting... ($i/10)"
    sleep 2
done

if [ "$SERVER_READY" -eq 0 ]; then
    echo ""
    echo "ERROR: Server did not become ready. Last 20 lines of $NODE_LOG:"
    tail -20 "$NODE_LOG"
    exit 1
fi

echo ""
echo "To view logs: tail -f $NODE_LOG"
echo "To stop this node: lsof -ti:${GRPC_PORT} | xargs kill -9"

tail -f plexspaces-node-${GRPC_PORT}.log
