#!/bin/bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Deploy PlexSpaces dashboard test environment (2 nodes + object-store + PostgreSQL)
#
# Prerequisites:
#   - Kubernetes cluster (minikube, kind, or cloud)
#   - kubectl configured
#   - Docker image built: docker build -t plexspaces:latest -f Dockerfile .
#   - Or use: make docker-build (if Makefile has docker-build target)

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
K8S_DIR="${SCRIPT_DIR}/../tests/k8s"

echo "🚀 Deploying PlexSpaces dashboard test environment..."
echo ""
echo "📋 Prerequisites check..."

# Check kubectl
if ! command -v kubectl &> /dev/null; then
    echo "❌ kubectl not found. Please install kubectl."
    exit 1
fi

# Check if kubectl can connect
if ! kubectl cluster-info &> /dev/null; then
    echo "❌ Cannot connect to Kubernetes cluster. Please configure kubectl."
    exit 1
fi

echo "✅ kubectl configured"

# Check if Docker image exists (optional, will use IfNotPresent)
if docker images | grep -q "^plexspaces.*latest"; then
    echo "✅ Docker image found: plexspaces:latest"
else
    echo "⚠️  Docker image 'plexspaces:latest' not found locally"
    echo "   Will attempt to pull from registry or build if needed"
    echo "   To build: docker build -t plexspaces:latest -f Dockerfile . --features dashboard"
fi

echo ""

# Create namespace
echo "📦 Creating namespace..."
kubectl apply -f "${K8S_DIR}/namespace.yaml"

# Deploy object store
echo "📦 Deploying object store..."
kubectl apply -f "${K8S_DIR}/object-store.yaml"

# Deploy PostgreSQL
echo "📦 Deploying PostgreSQL..."
kubectl apply -f "${K8S_DIR}/postgres.yaml"

# Wait for object store and PostgreSQL to be ready
echo "⏳ Waiting for object store and PostgreSQL to be ready..."
kubectl wait --for=condition=available --timeout=300s deployment/object-store -n plexspaces-test || true
kubectl wait --for=condition=available --timeout=300s deployment/postgres -n plexspaces-test || true

# Deploy PlexSpaces nodes
echo "📦 Deploying PlexSpaces nodes..."
kubectl apply -f "${K8S_DIR}/nodes.yaml"

# Deploy services
echo "📦 Deploying services..."
kubectl apply -f "${K8S_DIR}/services.yaml"

# Wait for nodes to be ready
echo "⏳ Waiting for nodes to be ready..."
kubectl wait --for=condition=available --timeout=300s deployment/plexspaces-node-1 -n plexspaces-test || true
kubectl wait --for=condition=available --timeout=300s deployment/plexspaces-node-2 -n plexspaces-test || true

echo "✅ Deployment complete!"
echo ""
echo "📊 Dashboard URLs:"
echo "  Node 1: http://localhost:8000"
echo "    Port-forward: kubectl port-forward -n plexspaces-test svc/plexspaces-node-1 8000:8000"
echo "  Node 2: http://localhost:8002"
echo "    Port-forward: kubectl port-forward -n plexspaces-test svc/plexspaces-node-2 8093:8000"
echo ""
echo "🚀 Next Steps:"
echo "  1. Port-forward to access dashboard:"
echo "     kubectl port-forward -n plexspaces-test svc/plexspaces-node-1 8000:8000"
echo "  2. Open browser: http://localhost:8000/"
echo "  3. Deploy test application:"
echo "     ./scripts/deploy-wasm-app-test.sh http://localhost:8000 wasm-calculator"
echo "  4. Test dashboard APIs:"
echo "     ./scripts/test-dashboard.sh http://localhost:8000"
echo ""
echo "🔍 Check status:"
echo "  kubectl get pods -n plexspaces-test"
echo "  kubectl logs -n plexspaces-test deployment/plexspaces-node-1 --tail=50"
echo "  kubectl logs -n plexspaces-test deployment/plexspaces-node-2 --tail=50"




