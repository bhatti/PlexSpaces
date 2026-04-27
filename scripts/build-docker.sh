#!/bin/bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Docker build and push script
#
# Usage:
#   ./scripts/build-docker.sh [tag] [registry]
#
# Examples:
#   ./scripts/build-docker.sh latest
#   ./scripts/build-docker.sh v0.1.0 docker.io/plexspaces
#   ./scripts/build-docker.sh latest ghcr.io/plexspaces/plexspaces

set -euo pipefail

TAG="${1:-latest}"
REGISTRY="${2:-plexspaces}"
# Note: FEATURES arg is for plexspaces-cli features (firecracker)
# plexspaces-node features (dashboard, firecracker) are always enabled in Dockerfile
FEATURES="${3:-firecracker}"  # Default: build with all features
IMAGE_NAME="${REGISTRY}:${TAG}"

echo "🐳 Building Docker image: ${IMAGE_NAME}"
echo "📦 Features: ${FEATURES} (plexspaces-cli)"
echo "📦 All features enabled: plexspaces-cli/firecracker, plexspaces-node/dashboard, plexspaces-node/firecracker"

# Build image with all features by default
docker build --build-arg FEATURES="${FEATURES}" -t "${IMAGE_NAME}" .

echo "✅ Build complete!"
echo ""
echo "📦 To run locally:"
echo "   docker run -p 8000:8000 -p 8001:8001 ${IMAGE_NAME}"
echo ""
echo "📤 To push (if registry provided):"
if [[ "${REGISTRY}" != "plexspaces" ]]; then
    echo "   docker push ${IMAGE_NAME}"
else
    echo "   docker tag ${IMAGE_NAME} <registry>/plexspaces:${TAG}"
    echo "   docker push <registry>/plexspaces:${TAG}"
fi

