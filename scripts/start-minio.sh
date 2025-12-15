#!/bin/bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Start MinIO server for blob service testing
#
# Usage:
#   ./scripts/start-minio.sh

set -euo pipefail

CONTAINER_NAME="plexspaces-minio"
MINIO_PORT=9000
MINIO_CONSOLE_PORT=9001
MINIO_USER="minioadmin_user"
MINIO_PASS="minioadmin_pass"

echo "🚀 Starting MinIO server for blob service testing"
echo ""

# Check if MinIO is already running
if docker ps --format '{{.Names}}' | grep -q "^${CONTAINER_NAME}$"; then
    echo "✓ MinIO container '${CONTAINER_NAME}' is already running"
    echo ""
    echo "MinIO is available at:"
    echo "  API:      http://localhost:${MINIO_PORT}"
    echo "  Console:  http://localhost:${MINIO_CONSOLE_PORT}"
    echo "  User:     ${MINIO_USER}"
    echo "  Password: ${MINIO_PASS}"
    exit 0
fi

# Check if container exists but is stopped
if docker ps -a --format '{{.Names}}' | grep -q "^${CONTAINER_NAME}$"; then
    echo "Starting existing MinIO container..."
    docker start "${CONTAINER_NAME}"
else
    echo "Creating new MinIO container..."
    docker run -d \
        --name "${CONTAINER_NAME}" \
        -p "${MINIO_PORT}:9000" \
        -p "${MINIO_CONSOLE_PORT}:9001" \
        -e "MINIO_ROOT_USER=${MINIO_USER}" \
        -e "MINIO_ROOT_PASSWORD=${MINIO_PASS}" \
        minio/minio server /data --console-address ":9001"
fi

# Wait for MinIO to be ready
echo "Waiting for MinIO to be ready..."
for i in {1..30}; do
    if curl -s --max-time 1 "http://localhost:${MINIO_PORT}/minio/health/live" > /dev/null 2>&1; then
        echo "✓ MinIO is ready!"
        break
    fi
    if [ $i -eq 30 ]; then
        echo "✗ MinIO failed to start within 30 seconds"
        exit 1
    fi
    sleep 1
done

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "MinIO is running!"
echo ""
echo "Access MinIO:"
echo "  API:      http://localhost:${MINIO_PORT}"
echo "  Console:  http://localhost:${MINIO_CONSOLE_PORT}"
echo "  User:     ${MINIO_USER}"
echo "  Password: ${MINIO_PASS}"
echo ""
echo "To stop MinIO:"
echo "  docker stop ${CONTAINER_NAME}"
echo ""
echo "To remove MinIO:"
echo "  docker rm -f ${CONTAINER_NAME}"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
