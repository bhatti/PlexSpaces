#!/bin/bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Kubernetes deployment automation script
#
# Usage:
#   ./scripts/deploy-k8s.sh [namespace] [image-tag]
#
# Examples:
#   ./scripts/deploy-k8s.sh default latest
#   ./scripts/deploy-k8s.sh plexspaces v0.1.0

set -euo pipefail

NAMESPACE="${1:-default}"
IMAGE_TAG="${2:-latest}"
IMAGE_NAME="plexspaces:${IMAGE_TAG}"

echo "🚀 Deploying PlexSpaces to Kubernetes"
echo "   Namespace: ${NAMESPACE}"
echo "   Image: ${IMAGE_NAME}"

# Create namespace if it doesn't exist
kubectl create namespace "${NAMESPACE}" --dry-run=client -o yaml | kubectl apply -f -

# Apply ConfigMap
echo "📝 Applying ConfigMap..."
kubectl apply -f k8s/deployment.yaml -n "${NAMESPACE}"

# Apply Secrets (update with your values)
echo "🔐 Applying Secrets..."
kubectl apply -f k8s/secrets.yaml -n "${NAMESPACE}"

# Apply Deployment or StatefulSet
echo "📦 Applying Deployment..."
kubectl apply -f k8s/deployment.yaml -n "${NAMESPACE}"

# Wait for deployment to be ready
echo "⏳ Waiting for deployment to be ready..."
kubectl wait --for=condition=available --timeout=300s deployment/plexspaces-node -n "${NAMESPACE}"

# Show status
echo "✅ Deployment complete!"
kubectl get pods -n "${NAMESPACE}" -l app=plexspaces
kubectl get svc -n "${NAMESPACE}" -l app=plexspaces

echo ""
echo "📊 To check logs:"
echo "   kubectl logs -f -l app=plexspaces -n ${NAMESPACE}"
echo ""
echo "🔍 To check health:"
echo "   kubectl get pods -n ${NAMESPACE} -l app=plexspaces"
echo ""
echo "🗑️  To delete:"
echo "   kubectl delete -f k8s/deployment.yaml -n ${NAMESPACE}"

