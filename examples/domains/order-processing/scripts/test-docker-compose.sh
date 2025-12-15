#!/usr/bin/env bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Test multi-node deployment with Docker Compose
#
# Usage:
#   ./scripts/test-docker-compose.sh [up|down|logs|status]

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

COMPOSE_FILE="docker-compose.yml"
PROJECT_NAME="plexspaces-order-processing"

function show_usage() {
    echo -e "${BLUE}=== PlexSpaces Order Processing - Docker Compose Test ===${NC}"
    echo ""
    echo "Usage: $0 [command]"
    echo ""
    echo "Commands:"
    echo "  up      - Build and start all 3 nodes"
    echo "  down    - Stop and remove all containers"
    echo "  logs    - Follow logs from all nodes"
    echo "  status  - Show status of all containers"
    echo "  clean   - Stop containers and remove volumes"
    echo ""
    echo "Examples:"
    echo "  $0 up             # Start the cluster"
    echo "  $0 logs           # Watch logs"
    echo "  $0 down           # Stop the cluster"
}

function docker_compose_up() {
    echo -e "${YELLOW}Building and starting multi-node cluster...${NC}"
    echo ""

    # Build and start services
    docker-compose -f "$COMPOSE_FILE" -p "$PROJECT_NAME" up --build -d

    echo ""
    echo -e "${GREEN}Cluster started successfully!${NC}"
    echo ""
    echo -e "${BLUE}=== Node Information ===${NC}"
    docker-compose -f "$COMPOSE_FILE" -p "$PROJECT_NAME" ps
    echo ""
    echo -e "${BLUE}=== Ports ===${NC}"
    echo "  Node 1 (Orders+Payment): localhost:9001"
    echo "  Node 2 (Inventory):      localhost:9002"
    echo "  Node 3 (Shipping):       localhost:9003"
    echo ""
    echo -e "${BLUE}=== Next Steps ===${NC}"
    echo "  View logs:   $0 logs"
    echo "  Check status: $0 status"
    echo "  Stop cluster: $0 down"
}

function docker_compose_down() {
    echo -e "${YELLOW}Stopping multi-node cluster...${NC}"
    docker-compose -f "$COMPOSE_FILE" -p "$PROJECT_NAME" down
    echo -e "${GREEN}Cluster stopped.${NC}"
}

function docker_compose_logs() {
    echo -e "${YELLOW}Following logs from all nodes (Ctrl+C to exit)...${NC}"
    echo ""
    docker-compose -f "$COMPOSE_FILE" -p "$PROJECT_NAME" logs -f
}

function docker_compose_status() {
    echo -e "${BLUE}=== Cluster Status ===${NC}"
    echo ""
    docker-compose -f "$COMPOSE_FILE" -p "$PROJECT_NAME" ps
    echo ""
    echo -e "${BLUE}=== Health Checks ===${NC}"

    # Check if nodes are healthy
    for node in node1 node2 node3; do
        container="${PROJECT_NAME}-${node}-1"
        if docker ps --filter "name=$container" --filter "status=running" | grep -q "$container"; then
            echo -e "  ${GREEN}✓${NC} $node is running"
        else
            echo -e "  ${RED}✗${NC} $node is NOT running"
        fi
    done
}

function docker_compose_clean() {
    echo -e "${YELLOW}Cleaning up cluster (including volumes)...${NC}"
    docker-compose -f "$COMPOSE_FILE" -p "$PROJECT_NAME" down -v
    echo -e "${GREEN}Cleanup complete.${NC}"
}

# Main command router
case "${1:-help}" in
    up)
        docker_compose_up
        ;;
    down)
        docker_compose_down
        ;;
    logs)
        docker_compose_logs
        ;;
    status)
        docker_compose_status
        ;;
    clean)
        docker_compose_clean
        ;;
    help|--help|-h)
        show_usage
        ;;
    *)
        echo -e "${RED}Error: Unknown command '${1}'${NC}"
        echo ""
        show_usage
        exit 1
        ;;
esac
