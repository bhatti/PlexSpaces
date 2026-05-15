#!/bin/bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Build and publish PlexSpaces Docker image to Docker Hub
#
# Usage:
#   ./scripts/publish-docker.sh [VERSION] [FEATURES] [ENABLE_FIRECRACKER]
#
# Examples:
#   ./scripts/publish-docker.sh v0.1.0
#   ./scripts/publish-docker.sh latest
#   ./scripts/publish-docker.sh v0.1.0 "" 1

set -e

# Configuration
IMAGE_NAME="plexobject/plexspaces"
VERSION="${1:-latest}"
FEATURES="${2:-}"
ENABLE_FIRECRACKER="${3:-0}"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo -e "${GREEN}╔════════════════════════════════════════════════════════════════╗${NC}"
echo -e "${GREEN}║  Building and Publishing PlexSpaces Docker Image              ║${NC}"
echo -e "${GREEN}╚════════════════════════════════════════════════════════════════╝${NC}"
echo ""

# Validate version format (if not "latest")
if [ "$VERSION" != "latest" ] && [[ ! "$VERSION" =~ ^v[0-9]+\.[0-9]+\.[0-9]+(-.*)?$ ]]; then
    echo -e "${YELLOW}Warning: Version '$VERSION' doesn't follow semantic versioning (vX.Y.Z)${NC}"
    read -p "Continue anyway? (y/N) " -n 1 -r
    echo
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        exit 1
    fi
fi

echo "Image: ${IMAGE_NAME}"
echo "Version: ${VERSION}"
echo "Dashboard: enabled"
echo "Extra CLI features: ${FEATURES:-<none>}"
echo "Firecracker: ${ENABLE_FIRECRACKER}"
echo ""

# Step 1: Build image
echo -e "${GREEN}[1/4] Building Docker image...${NC}"
BUILD_ARGS=(
  --build-arg "FEATURES=${FEATURES}"
  --build-arg "ENABLE_FIRECRACKER=${ENABLE_FIRECRACKER}"
)

if [ "$VERSION" == "latest" ]; then
    docker build "${BUILD_ARGS[@]}" -t ${IMAGE_NAME}:latest .
else
    docker build "${BUILD_ARGS[@]}" -t ${IMAGE_NAME}:${VERSION} -t ${IMAGE_NAME}:latest .
fi

if [ $? -ne 0 ]; then
    echo -e "${RED}❌ Build failed${NC}"
    exit 1
fi

echo -e "${GREEN}✅ Build successful${NC}"
echo ""

# Step 2: Verify image
echo -e "${GREEN}[2/4] Verifying image...${NC}"
docker images | grep "${IMAGE_NAME}" | grep -E "(${VERSION}|latest)" || {
    echo -e "${RED}❌ Image not found${NC}"
    exit 1
}

IMAGE_SIZE=$(docker images ${IMAGE_NAME}:${VERSION} --format "{{.Size}}")
echo -e "${GREEN}✅ Image verified (Size: ${IMAGE_SIZE})${NC}"
echo ""

# Step 3: Login to Docker Hub
echo -e "${GREEN}[3/4] Logging in to Docker Hub...${NC}"
if ! docker info | grep -q "Username:"; then
    echo "Please login to Docker Hub:"
    docker login
else
    echo -e "${GREEN}✅ Already logged in${NC}"
fi
echo ""

# Step 4: Push to Docker Hub
echo -e "${GREEN}[4/4] Pushing to Docker Hub...${NC}"

# Push version tag
if [ "$VERSION" != "latest" ]; then
    echo "Pushing ${IMAGE_NAME}:${VERSION}..."
    docker push ${IMAGE_NAME}:${VERSION}
    if [ $? -ne 0 ]; then
        echo -e "${RED}❌ Failed to push ${IMAGE_NAME}:${VERSION}${NC}"
        exit 1
    fi
    echo -e "${GREEN}✅ Pushed ${IMAGE_NAME}:${VERSION}${NC}"
fi

# Push latest tag
echo "Pushing ${IMAGE_NAME}:latest..."
docker push ${IMAGE_NAME}:latest
if [ $? -ne 0 ]; then
    echo -e "${RED}❌ Failed to push ${IMAGE_NAME}:latest${NC}"
    exit 1
fi
echo -e "${GREEN}✅ Pushed ${IMAGE_NAME}:latest${NC}"
echo ""

# Success message
echo -e "${GREEN}╔════════════════════════════════════════════════════════════════╗${NC}"
echo -e "${GREEN}║  ✅ Successfully published ${IMAGE_NAME}:${VERSION}${NC}"
if [ "$VERSION" != "latest" ]; then
    echo -e "${GREEN}║  ✅ Also tagged and pushed as latest${NC}"
fi
echo -e "${GREEN}╚════════════════════════════════════════════════════════════════╝${NC}"
echo ""
echo "View on Docker Hub: https://hub.docker.com/r/${IMAGE_NAME}/tags"
echo ""
echo "Test the published image:"
echo "  docker pull ${IMAGE_NAME}:${VERSION}"
echo "  docker run -d --name plexspaces-test -p 8000:8000 -p 8000:8000 -e PLEXSPACES_DISABLE_AUTH=1 ${IMAGE_NAME}:${VERSION}"
