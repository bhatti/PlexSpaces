#!/bin/bash
# SPDX-License-Identifier: LGPL-2.1-or-later
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
IMAGE_NAME="${REGISTRY}:${TAG}"

echo "🐳 Building Docker image: ${IMAGE_NAME}"

# Build image
docker build -t "${IMAGE_NAME}" .

echo "✅ Build complete!"
echo ""
echo "📦 To run locally:"
echo "   docker run -p 9001:9001 ${IMAGE_NAME}"
echo ""
echo "📤 To push (if registry provided):"
if [[ "${REGISTRY}" != "plexspaces" ]]; then
    echo "   docker push ${IMAGE_NAME}"
else
    echo "   docker tag ${IMAGE_NAME} <registry>/plexspaces:${TAG}"
    echo "   docker push <registry>/plexspaces:${TAG}"
fi

