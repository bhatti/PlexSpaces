#!/usr/bin/env bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

set -e

# Colors for output
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m' # No Color

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

echo -e "${BLUE}=== Starting Genomics Pipeline Docker Cluster ===${NC}"
echo ""

# Check if docker-compose is installed
if ! command -v docker-compose &> /dev/null; then
    echo -e "${RED}Error: docker-compose not found${NC}"
    echo "Install with: brew install docker-compose (macOS) or apt-get install docker-compose (Linux)"
    exit 1
fi

# Check if Docker is running
if ! docker info &> /dev/null; then
    echo -e "${RED}Error: Docker daemon is not running${NC}"
    echo "Start Docker Desktop or run: sudo systemctl start docker"
    exit 1
fi

cd "$PROJECT_DIR"

# Create necessary directories for volumes
echo -e "${BLUE}Creating volume directories...${NC}"
mkdir -p data/genomics
mkdir -p reference_data
mkdir -p annotation_databases
mkdir -p report_templates

# Build Docker image (if needed)
if [ "$1" == "--build" ]; then
    echo -e "${BLUE}Building Docker image...${NC}"
    docker build -t plexspaces/genomics-pipeline:latest .
fi

# Start cluster
echo -e "${BLUE}Starting 4-node cluster...${NC}"
docker-compose up -d

# Wait for health checks
echo -e "${BLUE}Waiting for cluster to be healthy...${NC}"
sleep 5

# Check service status
echo ""
echo -e "${GREEN}=== Cluster Status ===${NC}"
docker-compose ps

# Show resource usage
echo ""
echo -e "${GREEN}=== Resource Usage ===${NC}"
docker stats --no-stream

# Show connection info
echo ""
echo -e "${GREEN}=== Node Endpoints ===${NC}"
echo "  Coordinator:   http://localhost:8000 (gRPC)"
echo "  QC/Alignment:  http://localhost:8001 (gRPC)"
echo "  Chromosomes:   http://localhost:8002 (gRPC)"
echo "  Annotation:    http://localhost:8003 (gRPC)"

echo ""
echo -e "${GREEN}=== Useful Commands ===${NC}"
echo "  View logs:           docker-compose logs -f coordinator"
echo "  View all logs:       docker-compose logs -f"
echo "  Check health:        docker-compose ps"
echo "  Resource usage:      docker stats"
echo "  Stop cluster:        ./scripts/docker_down.sh"
echo ""
echo -e "${YELLOW}Note: Actual CLI commands pending PlexSpaces node implementation${NC}"
echo -e "${GREEN}✅ Cluster is running!${NC}"
