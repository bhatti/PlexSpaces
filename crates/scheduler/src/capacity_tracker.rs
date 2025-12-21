// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is free software: you can redistribute it and/or modify
// it under the terms of the GNU Lesser General Public License as published by
// the Free Software Foundation, either version 2.1 of the License, or
// (at your option) any later version.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! Capacity tracker for node resource monitoring.
//!
//! ## Purpose
//! Tracks node resource capacities by querying ObjectRegistry for node registrations
//! and extracting capacity information from node metrics.
//!
//! ## Design
//! - Queries ObjectRegistry for nodes (ObjectType::ObjectTypeService)
//! - Extracts capacity from metrics map (sent via heartbeat)
//! - Provides methods to get node capacities, filter by labels, etc.

use plexspaces_object_registry::ObjectRegistry;
use plexspaces_core::RequestContext;
use plexspaces_proto::{
    common::v1::ResourceSpec,
    node::v1::NodeCapacity,
    object_registry::v1::ObjectType,
};
use std::collections::HashMap;
use std::sync::Arc;

/// Error types for capacity tracking
#[derive(Debug, thiserror::Error)]
pub enum CapacityTrackerError {
    /// ObjectRegistry error
    #[error("ObjectRegistry error: {0}")]
    RegistryError(String),

    /// Invalid capacity data
    #[error("Invalid capacity data: {0}")]
    InvalidData(String),
}

/// Result type for capacity tracking
pub type CapacityTrackerResult<T> = Result<T, CapacityTrackerError>;

/// Capacity tracker for monitoring node resources
pub struct CapacityTracker {
    /// ObjectRegistry for querying node registrations
    registry: Arc<ObjectRegistry>,
}

impl CapacityTracker {
    /// Create a new capacity tracker
    pub fn new(registry: Arc<ObjectRegistry>) -> Self {
        Self { registry }
    }

    /// Get capacity for a specific node
    ///
    /// ## Arguments
    /// * `node_id` - Node identifier
    ///
    /// ## Returns
    /// `Ok(Some(NodeCapacity))` if node found, `Ok(None)` if not found
    pub async fn get_node_capacity(
        &self,
        ctx: &RequestContext,
        node_id: &str,
    ) -> CapacityTrackerResult<Option<NodeCapacity>> {
        let registration = self
            .registry
            .lookup(ctx, ObjectType::ObjectTypeService, node_id)
            .await
            .map_err(|e| CapacityTrackerError::RegistryError(e.to_string()))?;

        if let Some(reg) = registration {
            Ok(Some(Self::extract_capacity_from_registration(&reg)?))
        } else {
            Ok(None)
        }
    }

    /// List all node capacities (with optional filtering)
    ///
    /// ## Arguments
    /// * `node_labels` - Filter by node labels (all labels must match)
    /// * `min_available_resources` - Filter by minimum available resources
    ///
    /// ## Returns
    /// Map of node_id -> NodeCapacity
    pub async fn list_node_capacities(
        &self,
        ctx: &RequestContext,
        node_labels: Option<&HashMap<String, String>>,
        min_available_resources: Option<&ResourceSpec>,
    ) -> CapacityTrackerResult<HashMap<String, NodeCapacity>> {
        // Discover all nodes (registered as services)
        // For capacity tracking, use internal context to see all nodes
        let internal_ctx = RequestContext::internal();
        let nodes = self
            .registry
            .discover(
                &internal_ctx,
                Some(ObjectType::ObjectTypeService),
                None, // object_category
                None, // capabilities
                None, // labels (we'll filter by node labels separately)
                None, // health_status
                1000, // limit
            )
            .await
            .map_err(|e| CapacityTrackerError::RegistryError(e.to_string()))?;

        let mut capacities = HashMap::new();

        for node_registration in nodes {
            // Extract node ID from object_id (format: "node-id" or "node-id@address")
            let node_id = node_registration.object_id.clone();

            // Extract capacity from metrics
            let capacity = match Self::extract_capacity_from_registration(&node_registration) {
                Ok(cap) => cap,
                Err(e) => {
                    // Skip nodes with invalid capacity data
                    tracing::warn!("Failed to extract capacity for node {}: {}", node_id, e);
                    continue;
                }
            };

            // Filter by node labels if provided
            if let Some(ref required_labels) = node_labels {
                if !Self::matches_labels(required_labels, &capacity.labels) {
                    continue;
                }
            }

            // Filter by minimum available resources if provided
            if let Some(ref min_resources) = min_available_resources {
                if let Some(ref available) = capacity.available {
                    if available.cpu_cores < min_resources.cpu_cores
                        || available.memory_bytes < min_resources.memory_bytes
                        || available.disk_bytes < min_resources.disk_bytes
                        || available.gpu_count < min_resources.gpu_count
                    {
                        continue;
                    }
                } else {
                    continue; // No available resources data
                }
            }

            capacities.insert(node_id, capacity);
        }

        Ok(capacities)
    }

    /// Extract NodeCapacity from ObjectRegistration metrics
    ///
    /// ## Metrics Format
    /// Metrics are stored as f64 values in the metrics map:
    /// - `total_cpu_cores`, `total_memory_mb`, `total_disk_mb`, `total_gpu_count`
    /// - `allocated_cpu_cores`, `allocated_memory_mb`, `allocated_disk_mb`, `allocated_gpu_count`
    /// - `available_cpu_cores`, `available_memory_mb`, `available_disk_mb`, `available_gpu_count`
    fn extract_capacity_from_registration(
        registration: &plexspaces_proto::object_registry::v1::ObjectRegistration,
    ) -> CapacityTrackerResult<NodeCapacity> {
        // Extract total resources
        let total = ResourceSpec {
            cpu_cores: Self::get_metric_f64(&registration.metrics, "total_cpu_cores")
                .unwrap_or(0.0),
            memory_bytes: Self::get_metric_f64(&registration.metrics, "total_memory_mb")
                .map(|mb| (mb * 1024.0 * 1024.0) as u64)
                .unwrap_or(0),
            disk_bytes: Self::get_metric_f64(&registration.metrics, "total_disk_mb")
                .map(|mb| (mb * 1024.0 * 1024.0) as u64)
                .unwrap_or(0),
            gpu_count: Self::get_metric_f64(&registration.metrics, "total_gpu_count")
                .map(|g| g as u32)
                .unwrap_or(0),
            gpu_type: String::new(),
        };

        // Extract allocated resources
        let allocated = ResourceSpec {
            cpu_cores: Self::get_metric_f64(&registration.metrics, "allocated_cpu_cores")
                .unwrap_or(0.0),
            memory_bytes: Self::get_metric_f64(&registration.metrics, "allocated_memory_mb")
                .map(|mb| (mb * 1024.0 * 1024.0) as u64)
                .unwrap_or(0),
            disk_bytes: Self::get_metric_f64(&registration.metrics, "allocated_disk_mb")
                .map(|mb| (mb * 1024.0 * 1024.0) as u64)
                .unwrap_or(0),
            gpu_count: Self::get_metric_f64(&registration.metrics, "allocated_gpu_count")
                .map(|g| g as u32)
                .unwrap_or(0),
            gpu_type: String::new(),
        };

        // Extract available resources (or calculate from total - allocated)
        let available = ResourceSpec {
            cpu_cores: Self::get_metric_f64(&registration.metrics, "available_cpu_cores")
                .unwrap_or_else(|| total.cpu_cores - allocated.cpu_cores),
            memory_bytes: Self::get_metric_f64(&registration.metrics, "available_memory_mb")
                .map(|mb| (mb * 1024.0 * 1024.0) as u64)
                .unwrap_or_else(|| {
                    total.memory_bytes.saturating_sub(allocated.memory_bytes)
                }),
            disk_bytes: Self::get_metric_f64(&registration.metrics, "available_disk_mb")
                .map(|mb| (mb * 1024.0 * 1024.0) as u64)
                .unwrap_or_else(|| {
                    total.disk_bytes.saturating_sub(allocated.disk_bytes)
                }),
            gpu_count: Self::get_metric_f64(&registration.metrics, "available_gpu_count")
                .map(|g| g as u32)
                .unwrap_or_else(|| total.gpu_count.saturating_sub(allocated.gpu_count)),
            gpu_type: String::new(),
        };

        // Extract labels from metadata (nodes store labels in metadata)
        let labels = if let Some(ref metadata) = registration.metadata {
            metadata.labels.clone()
        } else {
            HashMap::new()
        };

        Ok(NodeCapacity {
            total: Some(total),
            allocated: Some(allocated),
            available: Some(available),
            labels,
        })
    }

    /// Get metric value as f64 from metrics map
    fn get_metric_f64(
        metrics: &HashMap<String, f64>,
        key: &str,
    ) -> Option<f64> {
        metrics.get(key).copied()
    }

    /// Check if node labels match required labels
    fn matches_labels(
        required_labels: &HashMap<String, String>,
        node_labels: &HashMap<String, String>,
    ) -> bool {
        // All required labels must be present and match
        required_labels.iter().all(|(key, value)| {
            node_labels.get(key).map_or(false, |v| v == value)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plexspaces_keyvalue::InMemoryKVStore;
    use plexspaces_proto::object_registry::v1::ObjectRegistration;

    fn create_test_registration(
        node_id: &str,
        metrics: HashMap<String, f64>,
        labels: HashMap<String, String>,
    ) -> ObjectRegistration {
        use plexspaces_proto::common::v1::Metadata;
        
        ObjectRegistration {
            object_id: node_id.to_string(),
            object_name: node_id.to_string(),
            object_type: ObjectType::ObjectTypeService as i32,
            version: "1.0.0".to_string(),
            tenant_id: "internal".to_string(),
            namespace: "system".to_string(),
            node_id: node_id.to_string(),
            grpc_address: format!("http://{}:9001", node_id),
            object_category: "node".to_string(),
            capabilities: vec![],
            metadata: Some(Metadata {
                labels,
                annotations: HashMap::new(),
                create_time: None,
                created_by: String::new(),
                update_time: None,
                updated_by: String::new(),
            }),
            health_status: 1, // Healthy
            labels: vec![], // Labels are in metadata, not here
            metrics,
            created_at: None,
            updated_at: None,
            last_heartbeat: None,
        }
    }

    #[tokio::test]
    async fn test_extract_capacity_from_registration() {
        let mut metrics = HashMap::new();
        metrics.insert("total_cpu_cores".to_string(), 4.0);
        metrics.insert("total_memory_mb".to_string(), 8192.0);
        metrics.insert("total_disk_mb".to_string(), 1000.0);
        metrics.insert("total_gpu_count".to_string(), 0.0);
        metrics.insert("allocated_cpu_cores".to_string(), 1.0);
        metrics.insert("allocated_memory_mb".to_string(), 1024.0);
        metrics.insert("allocated_disk_mb".to_string(), 0.0);
        metrics.insert("allocated_gpu_count".to_string(), 0.0);
        metrics.insert("available_cpu_cores".to_string(), 3.0);
        metrics.insert("available_memory_mb".to_string(), 7168.0);
        metrics.insert("available_disk_mb".to_string(), 1000.0);
        metrics.insert("available_gpu_count".to_string(), 0.0);

        let labels = HashMap::new();
        let registration = create_test_registration("node-1", metrics, labels);

        let capacity = CapacityTracker::extract_capacity_from_registration(&registration).unwrap();

        assert!(capacity.total.is_some());
        assert_eq!(capacity.total.as_ref().unwrap().cpu_cores, 4.0);
        assert_eq!(capacity.total.as_ref().unwrap().memory_bytes, 8192 * 1024 * 1024);

        assert!(capacity.allocated.is_some());
        assert_eq!(capacity.allocated.as_ref().unwrap().cpu_cores, 1.0);
        assert_eq!(capacity.allocated.as_ref().unwrap().memory_bytes, 1024 * 1024 * 1024);

        assert!(capacity.available.is_some());
        assert_eq!(capacity.available.as_ref().unwrap().cpu_cores, 3.0);
        assert_eq!(capacity.available.as_ref().unwrap().memory_bytes, 7168 * 1024 * 1024);
    }

    #[tokio::test]
    async fn test_extract_capacity_calculates_available() {
        let mut metrics = HashMap::new();
        metrics.insert("total_cpu_cores".to_string(), 4.0);
        metrics.insert("total_memory_mb".to_string(), 8192.0);
        metrics.insert("allocated_cpu_cores".to_string(), 1.0);
        metrics.insert("allocated_memory_mb".to_string(), 1024.0);
        // No available_* metrics - should calculate from total - allocated

        let labels = HashMap::new();
        let registration = create_test_registration("node-1", metrics, labels);

        let capacity = CapacityTracker::extract_capacity_from_registration(&registration).unwrap();

        assert!(capacity.available.is_some());
        assert_eq!(capacity.available.as_ref().unwrap().cpu_cores, 3.0);
        assert_eq!(capacity.available.as_ref().unwrap().memory_bytes, 7168 * 1024 * 1024);
    }

    #[tokio::test]
    async fn test_get_node_capacity() {
        use plexspaces_core::RequestContext;
        let kv = Arc::new(InMemoryKVStore::new());
        let registry = Arc::new(ObjectRegistry::new(kv));

        // Register a node
        let mut metrics = HashMap::new();
        metrics.insert("total_cpu_cores".to_string(), 4.0);
        metrics.insert("total_memory_mb".to_string(), 8192.0);
        metrics.insert("allocated_cpu_cores".to_string(), 1.0);
        metrics.insert("allocated_memory_mb".to_string(), 1024.0);

        let labels = HashMap::new();
        let registration = create_test_registration("node-1", metrics, labels);
        let ctx = RequestContext::internal();
        registry.register(&ctx, registration).await.unwrap();

        let tracker = CapacityTracker::new(registry);
        let capacity = tracker.get_node_capacity(&ctx, "node-1").await.unwrap();

        assert!(capacity.is_some());
        assert!(capacity.unwrap().total.is_some());
    }

    #[tokio::test]
    async fn test_get_node_capacity_not_found() {
        let kv = Arc::new(InMemoryKVStore::new());
        let registry = Arc::new(ObjectRegistry::new(kv));
        let tracker = CapacityTracker::new(registry);
        let ctx = RequestContext::internal();

        let capacity = tracker.get_node_capacity(&ctx, "nonexistent").await.unwrap();
        assert!(capacity.is_none());
    }

    #[tokio::test]
    async fn test_list_node_capacities() {
        use plexspaces_core::RequestContext;
        let kv = Arc::new(InMemoryKVStore::new());
        let registry = Arc::new(ObjectRegistry::new(kv));

        // Register two nodes
        let mut metrics1 = HashMap::new();
        metrics1.insert("total_cpu_cores".to_string(), 4.0);
        metrics1.insert("total_memory_mb".to_string(), 8192.0);
        metrics1.insert("allocated_cpu_cores".to_string(), 1.0);
        metrics1.insert("allocated_memory_mb".to_string(), 1024.0);

        let mut labels1 = HashMap::new();
        labels1.insert("zone".to_string(), "us-west".to_string());
        let registration1 = create_test_registration("node-1", metrics1, labels1);
        let ctx = RequestContext::internal();
        registry.register(&ctx, registration1).await.unwrap();

        let mut metrics2 = HashMap::new();
        metrics2.insert("total_cpu_cores".to_string(), 8.0);
        metrics2.insert("total_memory_mb".to_string(), 16384.0);
        metrics2.insert("allocated_cpu_cores".to_string(), 2.0);
        metrics2.insert("allocated_memory_mb".to_string(), 2048.0);

        let mut labels2 = HashMap::new();
        labels2.insert("zone".to_string(), "us-east".to_string());
        let registration2 = create_test_registration("node-2", metrics2, labels2);
        registry.register(&ctx, registration2).await.unwrap();

        let tracker = CapacityTracker::new(registry);
        let capacities = tracker.list_node_capacities(&ctx, None, None).await.unwrap();

        assert_eq!(capacities.len(), 2);
        assert!(capacities.contains_key("node-1"));
        assert!(capacities.contains_key("node-2"));
    }

    #[tokio::test]
    async fn test_list_node_capacities_filter_by_labels() {
        let kv = Arc::new(InMemoryKVStore::new());
        let registry = Arc::new(ObjectRegistry::new(kv));

        // Register two nodes with different labels
        let mut metrics1 = HashMap::new();
        metrics1.insert("total_cpu_cores".to_string(), 4.0);
        metrics1.insert("total_memory_mb".to_string(), 8192.0);
        let ctx = RequestContext::internal();
        let mut labels1 = HashMap::new();
        labels1.insert("zone".to_string(), "us-west".to_string());
        let registration1 = create_test_registration("node-1", metrics1, labels1);
        registry.register(&ctx, registration1).await.unwrap();

        let mut metrics2 = HashMap::new();
        metrics2.insert("total_cpu_cores".to_string(), 8.0);
        metrics2.insert("total_memory_mb".to_string(), 16384.0);
        let mut labels2 = HashMap::new();
        labels2.insert("zone".to_string(), "us-east".to_string());
        let registration2 = create_test_registration("node-2", metrics2, labels2);
        registry.register(&ctx, registration2).await.unwrap();

        let tracker = CapacityTracker::new(registry);

        // Filter by zone=us-west
        let mut filter_labels = HashMap::new();
        filter_labels.insert("zone".to_string(), "us-west".to_string());
        let ctx = RequestContext::internal();
        let capacities = tracker
            .list_node_capacities(&ctx, Some(&filter_labels), None)
            .await
            .unwrap();

        assert_eq!(capacities.len(), 1);
        assert!(capacities.contains_key("node-1"));
        assert!(!capacities.contains_key("node-2"));
    }

    #[tokio::test]
    async fn test_list_node_capacities_filter_by_min_resources() {
        let kv = Arc::new(InMemoryKVStore::new());
        let registry = Arc::new(ObjectRegistry::new(kv));

        // Register two nodes with different capacities
        let mut metrics1 = HashMap::new();
        metrics1.insert("total_cpu_cores".to_string(), 4.0);
        metrics1.insert("total_memory_mb".to_string(), 8192.0);
        metrics1.insert("allocated_cpu_cores".to_string(), 3.0);
        metrics1.insert("allocated_memory_mb".to_string(), 7000.0);
        // Available: 1 CPU, ~1GB memory
        let ctx = RequestContext::internal();
        let registration1 = create_test_registration("node-1", metrics1, HashMap::new());
        registry.register(&ctx, registration1).await.unwrap();

        let mut metrics2 = HashMap::new();
        metrics2.insert("total_cpu_cores".to_string(), 8.0);
        metrics2.insert("total_memory_mb".to_string(), 16384.0);
        metrics2.insert("allocated_cpu_cores".to_string(), 2.0);
        metrics2.insert("allocated_memory_mb".to_string(), 2048.0);
        // Available: 6 CPU, ~14GB memory
        let registration2 = create_test_registration("node-2", metrics2, HashMap::new());
        registry.register(&ctx, registration2).await.unwrap();

        let tracker = CapacityTracker::new(registry);

        // Filter by minimum 4 CPU cores
        let min_resources = ResourceSpec {
            cpu_cores: 4.0,
            memory_bytes: 0,
            disk_bytes: 0,
            gpu_count: 0,
            gpu_type: String::new(),
        };

        let capacities = tracker
            .list_node_capacities(&RequestContext::internal(), None, Some(&min_resources))
            .await
            .unwrap();

        assert_eq!(capacities.len(), 1);
        assert!(capacities.contains_key("node-2"));
        assert!(!capacities.contains_key("node-1"));
    }
}
