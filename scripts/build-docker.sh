#!/bin/bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Docker build and push script
#
# Usage:
#   ./scripts/build-docker.sh [tag] [registry] [features] [enable_firecracker]
#
# Examples:
#   ./scripts/build-docker.sh latest
#   ./scripts/build-docker.sh v0.1.0 docker.io/plexspaces
#   ./scripts/build-docker.sh latest ghcr.io/plexspaces/plexspaces
#   ./scripts/build-docker.sh latest plexspaces "" 1

set -euo pipefail

TAG="${1:-latest}"
REGISTRY="${2:-plexspaces}"
# Optional extra plexspaces-cli features; dashboard is always enabled in Dockerfile.
FEATURES="${3:-}"
# Firecracker support is opt-in for both CLI and node.
ENABLE_FIRECRACKER="${4:-0}"
IMAGE_NAME="${REGISTRY}:${TAG}"

echo "🐳 Building Docker image: ${IMAGE_NAME}"
echo "📦 Dashboard: enabled"
echo "📦 Extra CLI features: ${FEATURES:-<none>}"
echo "📦 Firecracker: ${ENABLE_FIRECRACKER}"

docker build \
  --build-arg FEATURES="${FEATURES}" \
  --build-arg ENABLE_FIRECRACKER="${ENABLE_FIRECRACKER}" \
  -t "${IMAGE_NAME}" .

echo "✅ Build complete!"
echo ""
echo "📦 To run locally:"
echo "   docker run -p 8000:8000 ${IMAGE_NAME}"
echo ""
echo "📤 To push (if registry provided):"
if [[ "${REGISTRY}" != "plexspaces" ]]; then
    echo "   docker push ${IMAGE_NAME}"
else
    echo "   docker tag ${IMAGE_NAME} <registry>/plexspaces:${TAG}"
    echo "   docker push <registry>/plexspaces:${TAG}"
fi
