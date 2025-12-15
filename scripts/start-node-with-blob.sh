#!/bin/bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Start an empty PlexSpaces node with blob service enabled
#
# Usage:
#   ./scripts/start-node-with-blob.sh [OPTIONS]
#
# Options:
#   --minio-url URL     MinIO endpoint (default: http://localhost:9000)
#   --minio-key KEY     MinIO access key (default: minioadmin_user)
#   --minio-secret SEC  MinIO secret key (default: minioadmin_pass)
#   --bucket NAME       S3 bucket name (default: plexspaces)
#   --port PORT         Node gRPC port (default: 9000)
#   --help              Show this help message

set -euo pipefail

# Default values
MINIO_URL="${BLOB_ENDPOINT:-http://localhost:9000}"
MINIO_KEY="${BLOB_ACCESS_KEY_ID:-minioadmin_user}"
MINIO_SECRET="${BLOB_SECRET_ACCESS_KEY:-minioadmin_pass}"
BUCKET="${BLOB_BUCKET:-plexspaces}"
PORT="${PLEXSPACES_LISTEN_ADDR:-0.0.0.0:9000}"
NODE_ID="${PLEXSPACES_NODE_ID:-node-$(date +%s)}"

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --minio-url)
            MINIO_URL="$2"
            shift 2
            ;;
        --minio-key)
            MINIO_KEY="$2"
            shift 2
            ;;
        --minio-secret)
            MINIO_SECRET="$2"
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
            echo "  --minio-url URL     MinIO endpoint (default: http://localhost:9000)"
            echo "  --minio-key KEY     MinIO access key (default: minioadmin_user)"
            echo "  --minio-secret SEC  MinIO secret key (default: minioadmin_pass)"
            echo "  --bucket NAME       S3 bucket name (default: plexspaces)"
            echo "  --port PORT         Node gRPC port (default: 0.0.0.0:9000)"
            echo "  --help              Show this help message"
            echo ""
            echo "Environment variables:"
            echo "  BLOB_ENDPOINT           MinIO endpoint"
            echo "  BLOB_ACCESS_KEY_ID      MinIO access key"
            echo "  BLOB_SECRET_ACCESS_KEY  MinIO secret key"
            echo "  BLOB_BUCKET             S3 bucket name"
            echo "  PLEXSPACES_LISTEN_ADDR  Node listen address"
            echo "  PLEXSPACES_NODE_ID      Node ID"
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
echo "  Blob Backend:   MinIO"
echo "  MinIO URL:      $MINIO_URL"
echo "  Bucket:         $BUCKET"
echo ""

# Check if MinIO is available
echo -e "${YELLOW}Checking MinIO availability...${NC}"
if curl -s --max-time 2 "${MINIO_URL}/minio/health/live" > /dev/null 2>&1; then
    echo -e "${GREEN}✓${NC} MinIO is available at $MINIO_URL"
else
    echo -e "${YELLOW}⚠${NC} MinIO not available at $MINIO_URL"
    echo "  You may need to start MinIO first:"
    echo "  docker run -d -p 9000:9000 -p 9001:9001 \\"
    echo "    -e MINIO_ROOT_USER=minioadmin_user \\"
    echo "    -e MINIO_ROOT_PASSWORD=minioadmin_pass \\"
    echo "    minio/minio server /data --console-address \":9001\""
    echo ""
    read -p "Continue anyway? (y/N) " -n 1 -r
    echo
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        exit 1
    fi
fi
echo ""

# Set environment variables
export PLEXSPACES_NODE_ID="$NODE_ID"
export PLEXSPACES_LISTEN_ADDR="$PORT"
export BLOB_ENABLED="true"
export BLOB_BACKEND="minio"
export BLOB_ENDPOINT="$MINIO_URL"
export BLOB_ACCESS_KEY_ID="$MINIO_KEY"
export BLOB_SECRET_ACCESS_KEY="$MINIO_SECRET"
export BLOB_BUCKET="$BUCKET"
export BLOB_PREFIX="/plexspaces"
export BLOB_DATABASE_URL="sqlite:blob_metadata.db"
export RUST_LOG="${RUST_LOG:-info}"

# Build if needed
echo -e "${YELLOW}Building CLI (if needed)...${NC}"
cargo build --package plexspaces-cli --bin plexspaces --quiet 2>/dev/null || cargo build --package plexspaces-cli --bin plexspaces
echo ""

echo -e "${GREEN}Starting node...${NC}"
echo ""
echo "API Endpoints:"
HTTP_PORT="${PORT#*:}"
echo "  gRPC:        ${PORT}"
echo "  HTTP:        http://${HTTP_PORT}"
echo "  Blob Upload: POST http://${HTTP_PORT}/api/v1/blobs/upload"
echo "  Blob Download: GET http://${HTTP_PORT}/api/v1/blobs/{blob_id}/download/raw"
echo "  Blob Metadata: GET http://${HTTP_PORT}/api/v1/blobs/{blob_id}"
echo "  List Blobs:  GET http://${HTTP_PORT}/api/v1/blobs?tenant_id=...&namespace=..."
echo ""
echo -e "${YELLOW}Press Ctrl+C to stop${NC}"
echo ""

# Run the node using CLI
exec cargo run --package plexspaces-cli --bin plexspaces --quiet -- start --node-id "$NODE_ID" --listen-addr "$PORT"
