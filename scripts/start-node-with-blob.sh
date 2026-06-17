#!/bin/bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Start an empty PlexSpaces node with blob service enabled
#
# Usage:
#   ./scripts/start-node-with-blob.sh [OPTIONS]
#
# Options:
#   --blob-url URL      Object store endpoint (default: auto-start embedded)
#   --blob-key KEY      Object store access key (default: empty)
#   --blob-secret SEC   Object store secret key (default: empty)
#   --bucket NAME       S3 bucket name (default: plexspaces)
#   --port PORT         Node gRPC port (default: 0.0.0.0:8000)
#   --help              Show this help message

set -euo pipefail

# Default values — empty endpoint means embedded store is auto-started by the node
BLOB_URL="${BLOB_ENDPOINT:-}"
BLOB_KEY="${BLOB_ACCESS_KEY_ID:-}"
BLOB_SECRET="${BLOB_SECRET_ACCESS_KEY:-}"
BUCKET="${BLOB_BUCKET:-plexspaces}"
PORT="${PLEXSPACES_LISTEN_ADDR:-0.0.0.0:8000}"
NODE_ID="${PLEXSPACES_NODE_ID:-node-$(date +%s)}"
BACKEND="${BLOB_BACKEND:-embedded}"

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --blob-url)
            BLOB_URL="$2"
            shift 2
            ;;
        --blob-key)
            BLOB_KEY="$2"
            shift 2
            ;;
        --blob-secret)
            BLOB_SECRET="$2"
            shift 2
            ;;
        --bucket)
            BUCKET="$2"
            shift 2
            ;;
        --port)
            PORT="$2"
            shift 2
            ;;
        --help)
            echo "Usage: $0 [OPTIONS]"
            echo ""
            echo "Start an empty PlexSpaces node with blob service enabled"
            echo ""
            echo "Options:"
            echo "  --blob-url URL      Object store endpoint (default: auto-start embedded)"
            echo "  --blob-key KEY      Object store access key (default: empty)"
            echo "  --blob-secret SEC   Object store secret key (default: empty)"
            echo "  --bucket NAME       S3 bucket name (default: plexspaces)"
            echo "  --port PORT         Node gRPC port (default: 0.0.0.0:9000)"
            echo "  --help              Show this help message"
            echo ""
            echo "Environment variables:"
            echo "  BLOB_ENDPOINT                  Object store endpoint (empty = auto-start embedded)"
            echo "  BLOB_ACCESS_KEY_ID             Object store access key"
            echo "  BLOB_SECRET_ACCESS_KEY         Object store secret key"
            echo "  BLOB_BUCKET                    S3 bucket name"
            echo "  BLOB_BACKEND                   Backend type: embedded (default), s3, gcp, azure"
            echo "  EMBEDDED_OBJECT_STORE_BIN      Binary for embedded store (default: rustfs)"
            echo "  PLEXSPACES_LISTEN_ADDR         Node listen address"
            echo "  PLEXSPACES_NODE_ID             Node ID"
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            echo "Use --help for usage information"
            exit 1
            ;;
    esac
done

# Colors
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}Starting PlexSpaces Node with Blob Service${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo ""
echo "Configuration:"
echo "  Node ID:        $NODE_ID"
echo "  Listen Address: $PORT"
echo "  Blob Backend:   $BACKEND"
if [ -n "$BLOB_URL" ]; then
    echo "  Blob Endpoint:  $BLOB_URL"
else
    echo "  Blob Endpoint:  (auto-start embedded object store)"
fi
echo "  Bucket:         $BUCKET"
echo ""

# Set environment variables
export PLEXSPACES_NODE_ID="$NODE_ID"
export PLEXSPACES_LISTEN_ADDR="$PORT"
export BLOB_ENABLED="true"
export BLOB_BACKEND="$BACKEND"
export BLOB_BUCKET="$BUCKET"
export BLOB_PREFIX="${BLOB_PREFIX:-$HOME/plexspaces/blob}"
export BLOB_DATABASE_URL="sqlite:blob_metadata.db"
export RUST_LOG="${RUST_LOG:-info}"

if [ -n "$BLOB_URL" ]; then
    export BLOB_ENDPOINT="$BLOB_URL"
fi
if [ -n "$BLOB_KEY" ]; then
    export BLOB_ACCESS_KEY_ID="$BLOB_KEY"
fi
if [ -n "$BLOB_SECRET" ]; then
    export BLOB_SECRET_ACCESS_KEY="$BLOB_SECRET"
fi

# Build CLI if needed
echo -e "${YELLOW}Building CLI (if needed)...${NC}"
cargo build --package plexspaces-cli --bin plexspaces --quiet 2>/dev/null || {
    echo "Building CLI..."
    cargo build --package plexspaces-cli --bin plexspaces
}
echo ""

echo -e "${GREEN}Starting node...${NC}"
echo ""

# Extract port from address (e.g., "0.0.0.0:8000" -> "8000")
GRPC_PORT="${PORT##*:}"
HTTP_PORT=$((GRPC_PORT + 100))
GRPC_HOST="${PORT%:*}"
if [ "$GRPC_HOST" = "$PORT" ]; then
    GRPC_HOST="localhost"
fi

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "API Endpoints:"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
echo "📡 gRPC Server: ${GRPC_HOST}:${GRPC_PORT}"
echo "   - All gRPC services"
echo "   - gRPC-Gateway HTTP endpoints"
echo ""
echo "🌐 Blob HTTP Server: http://${GRPC_HOST}:${HTTP_PORT}"
echo "   - POST /api/v1/blobs/upload (multipart upload)"
echo "   - GET  /api/v1/blobs/{blob_id}/download/raw (raw download)"
echo ""
echo "📋 Metadata APIs (via gRPC-Gateway on gRPC port):"
echo "   - GET    http://${GRPC_HOST}:${GRPC_PORT}/api/v1/blobs/{blob_id} (get metadata)"
echo "   - GET    http://${GRPC_HOST}:${GRPC_PORT}/api/v1/blobs?tenant_id=...&namespace=... (list)"
echo "   - DELETE http://${GRPC_HOST}:${GRPC_PORT}/api/v1/blobs/{blob_id} (delete)"
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
echo -e "${YELLOW}Press Ctrl+C to stop${NC}"
echo ""

# Run the node using CLI
exec cargo run --package plexspaces-cli --bin plexspaces -- start --node-id "$NODE_ID" --listen-addr "$PORT"
