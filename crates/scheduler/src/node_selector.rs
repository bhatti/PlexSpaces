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

//! Node selector for resource-aware scheduling.
//!
//! ## Purpose
//! Selects the best matching node for an actor based on:
//! - Resource requirements (CPU, memory, disk, GPU)
//! - Node labels (Kubernetes-inspired label selectors)
//! - Placement preferences (preferred node, affinity/anti-affinity)
//!
//! ## Design
//! - **Scoring Algorithm**: Bin-packing with resource utilization scoring
//! - **Label Matching**: All required labels must match
//! - **Resource Matching**: Available resources must meet requirements
//! - **Placement Preferences**: Preferred nodes get higher scores, avoided nodes are excluded

use plexspaces_proto::{
    v1::actor::{ActorResourceRequirements, PlacementPreferences, PlacementStrategy},
    common::v1::ResourceSpec,
    node::v1::NodeCapacity,
};
use std::collections::HashMap;

/// Error types for node selection
#[derive(Debug, thiserror::Error)]
pub enum NodeSelectionError {
    /// No matching node found
    #[error("No matching node found: {0}")]
    NoMatchingNode(String),

    /// Invalid resource requirements
    #[error("Invalid resource requirements: {0}")]
    InvalidRequirements(String),
}

/// Result type for node selection
pub type NodeSelectionResult<T> = Result<T, NodeSelectionError>;

/// Node selector for resource-aware scheduling
pub struct NodeSelector;

impl NodeSelector {
    /// Select the best matching node from available node capacities
    ///
    /// ## Arguments
    /// * `requirements` - Actor resource requirements
    /// * `node_capacities` - Map of node_id -> NodeCapacity
    ///
    /// ## Returns
    /// `Ok((node_id, score))` - Selected node ID and its score
    /// `Err(NodeSelectionError)` - If no matching node found
    ///
    /// ## Algorithm
    /// 1. Filter nodes by label matching (all required labels must match)
    /// 2. Filter nodes by resource availability (must meet requirements)
    /// 3. Apply placement preferences (preferred nodes, affinity/anti-affinity)
    /// 4. Score remaining nodes using bin-packing algorithm
    /// 5. Return highest scoring node
    pub fn select_node(
        requirements: &ActorResourceRequirements,
        node_capacities: &HashMap<String, NodeCapacity>,
    ) -> NodeSelectionResult<(String, f64)> {
        // Validate requirements
        if let Some(ref resources) = requirements.resources {
            if resources.cpu_cores < 0.0
                || resources.memory_bytes < 0
                || resources.disk_bytes < 0
                || resources.gpu_count < 0
            {
                return Err(NodeSelectionError::InvalidRequirements(
                    "Resource values must be non-negative".to_string(),
                ));
            }
        }

        // Filter nodes by labels
        let mut candidates: Vec<(&String, &NodeCapacity)> = node_capacities
            .iter()
            .filter(|(_, capacity)| {
                Self::matches_labels(&requirements.required_labels, &capacity.labels)
            })
            .collect();

        // Filter nodes by resource availability
        if let Some(ref required_resources) = requirements.resources {
            candidates.retain(|(_, capacity)| {
                Self::has_sufficient_resources(required_resources, capacity)
            });
        }

        if candidates.is_empty() {
            return Err(NodeSelectionError::NoMatchingNode(
                "No nodes match resource requirements and labels".to_string(),
            ));
        }

        // Apply placement preferences and separate preferred nodes
        let (preferred_nodes, other_nodes) = if let Some(ref placement) = requirements.placement {
            let filtered = Self::apply_placement_preferences(candidates, placement);
            if filtered.is_empty() {
                return Err(NodeSelectionError::NoMatchingNode(
                    "No nodes match placement preferences".to_string(),
                ));
            }
            
            // Separate preferred nodes from others
            let mut preferred = Vec::new();
            let mut others = Vec::new();
            
            for (node_id, capacity) in filtered {
                if placement.preferred_node_ids.contains(node_id) {
                    preferred.push((node_id, capacity));
                } else {
                    others.push((node_id, capacity));
                }
            }
            
            (preferred, others)
        } else {
            (candidates, Vec::new())
        };

        // Score nodes using bin-packing algorithm
        let mut scored: Vec<(String, f64)> = preferred_nodes
            .into_iter()
            .map(|(node_id, capacity)| {
                let score = Self::calculate_score(requirements, capacity);
                (node_id.clone(), score)
            })
            .collect();

        // If no preferred nodes, score other nodes
        if scored.is_empty() {
            scored = other_nodes
                .into_iter()
                .map(|(node_id, capacity)| {
                    let score = Self::calculate_score(requirements, capacity);
                    (node_id.clone(), score)
                })
                .collect();
        }

        // Sort by score (descending) and return best node
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        Ok(scored[0].clone())
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

    /// Check if node has sufficient resources
    fn has_sufficient_resources(
        required: &ResourceSpec,
        capacity: &NodeCapacity,
    ) -> bool {
        if let Some(ref available) = capacity.available {
            available.cpu_cores >= required.cpu_cores
                && available.memory_bytes >= required.memory_bytes
                && available.disk_bytes >= required.disk_bytes
                && available.gpu_count >= required.gpu_count
        } else {
            false
        }
    }

    /// Apply placement preferences (preferred nodes, affinity/anti-affinity)
    fn apply_placement_preferences<'a>(
        candidates: Vec<(&'a String, &'a NodeCapacity)>,
        placement: &PlacementPreferences,
    ) -> Vec<(&'a String, &'a NodeCapacity)> {
        // First, exclude nodes in avoid_node_ids (anti-affinity)
        let mut filtered: Vec<(&'a String, &'a NodeCapacity)> = candidates
            .into_iter()
            .filter(|(node_id, _)| !placement.avoid_node_ids.contains(node_id))
            .collect();

        // If preferred_node_ids is specified, prioritize those nodes
        if !placement.preferred_node_ids.is_empty() {
            let mut preferred = Vec::new();
            let mut others = Vec::new();

            for (node_id, capacity) in filtered {
                if placement.preferred_node_ids.contains(node_id) {
                    preferred.push((node_id, capacity));
                } else {
                    others.push((node_id, capacity));
                }
            }

            // Return preferred nodes first, then others
            preferred.extend(others);
            preferred
        } else {
            filtered
        }
    }

    /// Calculate node score using bin-packing algorithm
    ///
    /// ## Scoring Algorithm
    /// Higher score = better fit
    /// - Resource utilization: Prefer nodes with higher utilization (bin-packing)
    /// - Resource balance: Prefer nodes with balanced resource usage
    /// - Available capacity: Prefer nodes with more available resources (for future actors)
    ///
    /// Score = (cpu_utilization * 0.4) + (memory_utilization * 0.4) + (balance_score * 0.2)
    fn calculate_score(
        requirements: &ActorResourceRequirements,
        capacity: &NodeCapacity,
    ) -> f64 {
        if let (Some(ref required), Some(ref total), Some(ref allocated)) = (
            requirements.resources.as_ref(),
            capacity.total.as_ref(),
            capacity.allocated.as_ref(),
        ) {
            // Calculate resource utilization after placing this actor
            let new_allocated_cpu = allocated.cpu_cores + required.cpu_cores;
            let new_allocated_memory = allocated.memory_bytes + required.memory_bytes;
            let _new_allocated_disk = allocated.disk_bytes + required.disk_bytes;

            let cpu_utilization = if total.cpu_cores > 0.0 {
                new_allocated_cpu / total.cpu_cores
            } else {
                0.0
            };

            let memory_utilization = if total.memory_bytes > 0 {
                new_allocated_memory as f64 / total.memory_bytes as f64
            } else {
                0.0
            };

            // Balance score: Prefer nodes where resources are balanced
            let balance_score = {
                let cpu_ratio = cpu_utilization;
                let memory_ratio = memory_utilization;
                // Lower variance = more balanced = higher score
                1.0 - (cpu_ratio - memory_ratio).abs()
            };

            // Weighted score
            (cpu_utilization * 0.4) + (memory_utilization * 0.4) + (balance_score * 0.2)
        } else {
            0.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_node_capacity(
        _node_id: &str,
        total_cpu: f64,
        total_memory: u64,
        allocated_cpu: f64,
        allocated_memory: u64,
        labels: HashMap<String, String>,
    ) -> NodeCapacity {
        let total = ResourceSpec {
            cpu_cores: total_cpu,
            memory_bytes: total_memory,
            disk_bytes: 1000 * 1024 * 1024, // 1GB
            gpu_count: 0,
            gpu_type: String::new(),
        };

        let allocated = ResourceSpec {
            cpu_cores: allocated_cpu,
            memory_bytes: allocated_memory,
            disk_bytes: 0,
            gpu_count: 0,
            gpu_type: String::new(),
        };

        let available = ResourceSpec {
            cpu_cores: total_cpu - allocated_cpu,
            memory_bytes: total_memory - allocated_memory,
            disk_bytes: 1000 * 1024 * 1024,
            gpu_count: 0,
            gpu_type: String::new(),
        };

        NodeCapacity {
            total: Some(total),
            allocated: Some(allocated),
            available: Some(available),
            labels,
        }
    }

    fn create_test_requirements(
        cpu: f64,
        memory: u64,
        required_labels: HashMap<String, String>,
    ) -> ActorResourceRequirements {
        let resources = ResourceSpec {
            cpu_cores: cpu,
            memory_bytes: memory,
            disk_bytes: 0,
            gpu_count: 0,
            gpu_type: String::new(),
        };

        ActorResourceRequirements {
            resources: Some(resources),
            required_labels,
            placement: None,
            actor_groups: vec![],
        }
    }

    #[test]
    fn test_select_node_basic() {
        let mut capacities = HashMap::new();
        let mut labels1 = HashMap::new();
        labels1.insert("zone".to_string(), "us-west".to_string());
        capacities.insert(
            "node-1".to_string(),
            create_test_node_capacity("node-1", 4.0, 8192, 1.0, 1024, labels1),
        );

        let mut labels2 = HashMap::new();
        labels2.insert("zone".to_string(), "us-east".to_string());
        capacities.insert(
            "node-2".to_string(),
            create_test_node_capacity("node-2", 8.0, 16384, 2.0, 2048, labels2),
        );

        let mut required_labels = HashMap::new();
        required_labels.insert("zone".to_string(), "us-west".to_string());
        let requirements = create_test_requirements(1.0, 512, required_labels);

        let result = NodeSelector::select_node(&requirements, &capacities);
        assert!(result.is_ok());
        let (node_id, _score) = result.unwrap();
        assert_eq!(node_id, "node-1");
    }

    #[test]
    fn test_select_node_no_matching_labels() {
        let mut capacities = HashMap::new();
        let mut labels = HashMap::new();
        labels.insert("zone".to_string(), "us-west".to_string());
        capacities.insert(
            "node-1".to_string(),
            create_test_node_capacity("node-1", 4.0, 8192, 0.0, 0, labels),
        );

        let mut required_labels = HashMap::new();
        required_labels.insert("zone".to_string(), "us-east".to_string());
        let requirements = create_test_requirements(1.0, 512, required_labels);

        let result = NodeSelector::select_node(&requirements, &capacities);
        assert!(matches!(result, Err(NodeSelectionError::NoMatchingNode(_))));
    }

    #[test]
    fn test_select_node_insufficient_resources() {
        let mut capacities = HashMap::new();
        let labels = HashMap::new();
        capacities.insert(
            "node-1".to_string(),
            create_test_node_capacity("node-1", 1.0, 512, 0.5, 256, labels),
        );

        let requirements = create_test_requirements(2.0, 1024, HashMap::new());

        let result = NodeSelector::select_node(&requirements, &capacities);
        assert!(matches!(result, Err(NodeSelectionError::NoMatchingNode(_))));
    }

    #[test]
    fn test_select_node_preferred() {
        let mut capacities = HashMap::new();
        capacities.insert(
            "node-1".to_string(),
            create_test_node_capacity("node-1", 4.0, 8192, 0.0, 0, HashMap::new()),
        );
        capacities.insert(
            "node-2".to_string(),
            create_test_node_capacity("node-2", 8.0, 16384, 0.0, 0, HashMap::new()),
        );

        let mut requirements = create_test_requirements(1.0, 512, HashMap::new());
        requirements.placement = Some(PlacementPreferences {
            strategy: PlacementStrategy::PlacementStrategyResourceBased as i32,
            preferred_node_ids: vec!["node-2".to_string()],
            avoid_node_ids: vec![],
        });

        let result = NodeSelector::select_node(&requirements, &capacities);
        assert!(result.is_ok());
        let (node_id, _score) = result.unwrap();
        // Preferred node should be selected (even if scoring would prefer another)
        assert_eq!(node_id, "node-2");
    }

    #[test]
    fn test_select_node_avoid() {
        let mut capacities = HashMap::new();
        capacities.insert(
            "node-1".to_string(),
            create_test_node_capacity("node-1", 4.0, 8192, 0.0, 0, HashMap::new()),
        );
        capacities.insert(
            "node-2".to_string(),
            create_test_node_capacity("node-2", 8.0, 16384, 0.0, 0, HashMap::new()),
        );

        let mut requirements = create_test_requirements(1.0, 512, HashMap::new());
        requirements.placement = Some(PlacementPreferences {
            strategy: PlacementStrategy::PlacementStrategyResourceBased as i32,
            preferred_node_ids: vec![],
            avoid_node_ids: vec!["node-1".to_string()],
        });

        let result = NodeSelector::select_node(&requirements, &capacities);
        assert!(result.is_ok());
        let (node_id, _score) = result.unwrap();
        assert_eq!(node_id, "node-2");
    }

    #[test]
    fn test_select_node_bin_packing() {
        // Node 1: 50% utilized (after placement: 75% utilized)
        let mut capacities = HashMap::new();
        capacities.insert(
            "node-1".to_string(),
            create_test_node_capacity("node-1", 4.0, 8192, 2.0, 4096, HashMap::new()),
        );

        // Node 2: 25% utilized (after placement: 50% utilized)
        // Bin-packing prefers higher utilization, so node-1 should be selected
        capacities.insert(
            "node-2".to_string(),
            create_test_node_capacity("node-2", 4.0, 8192, 1.0, 2048, HashMap::new()),
        );

        let requirements = create_test_requirements(1.0, 512, HashMap::new());

        let result = NodeSelector::select_node(&requirements, &capacities);
        assert!(result.is_ok());
        let (node_id, _score) = result.unwrap();
        // Node 1 should be selected (higher utilization after placement = better bin-packing)
        assert_eq!(node_id, "node-1");
    }

    #[test]
    fn test_select_node_invalid_requirements() {
        let capacities = HashMap::new();
        let requirements = create_test_requirements(-1.0, 512, HashMap::new());

        let result = NodeSelector::select_node(&requirements, &capacities);
        assert!(matches!(result, Err(NodeSelectionError::InvalidRequirements(_))));
    }

    #[test]
    fn test_select_node_empty_capacities() {
        let capacities = HashMap::new();
        let requirements = create_test_requirements(1.0, 512, HashMap::new());

        let result = NodeSelector::select_node(&requirements, &capacities);
        assert!(matches!(result, Err(NodeSelectionError::NoMatchingNode(_))));
    }

    #[test]
    fn test_matches_labels_all_match() {
        let mut required = HashMap::new();
        required.insert("zone".to_string(), "us-west".to_string());
        required.insert("env".to_string(), "prod".to_string());

        let mut node_labels = HashMap::new();
        node_labels.insert("zone".to_string(), "us-west".to_string());
        node_labels.insert("env".to_string(), "prod".to_string());
        node_labels.insert("team".to_string(), "platform".to_string());

        assert!(NodeSelector::matches_labels(&required, &node_labels));
    }

    #[test]
    fn test_matches_labels_partial_match() {
        let mut required = HashMap::new();
        required.insert("zone".to_string(), "us-west".to_string());
        required.insert("env".to_string(), "prod".to_string());

        let mut node_labels = HashMap::new();
        node_labels.insert("zone".to_string(), "us-west".to_string());
        // Missing "env" label

        assert!(!NodeSelector::matches_labels(&required, &node_labels));
    }

    #[test]
    fn test_matches_labels_value_mismatch() {
        let mut required = HashMap::new();
        required.insert("zone".to_string(), "us-west".to_string());

        let mut node_labels = HashMap::new();
        node_labels.insert("zone".to_string(), "us-east".to_string());

        assert!(!NodeSelector::matches_labels(&required, &node_labels));
    }

    #[test]
    fn test_has_sufficient_resources() {
        let required = ResourceSpec {
            cpu_cores: 1.0,
            memory_bytes: 512,
            disk_bytes: 0,
            gpu_count: 0,
            gpu_type: String::new(),
        };

        let capacity = create_test_node_capacity("node-1", 4.0, 8192, 0.0, 0, HashMap::new());
        assert!(NodeSelector::has_sufficient_resources(&required, &capacity));
    }

    #[test]
    fn test_has_sufficient_resources_insufficient() {
        let required = ResourceSpec {
            cpu_cores: 5.0,
            memory_bytes: 10000,
            disk_bytes: 0,
            gpu_count: 0,
            gpu_type: String::new(),
        };

        let capacity = create_test_node_capacity("node-1", 4.0, 8192, 0.0, 0, HashMap::new());
        assert!(!NodeSelector::has_sufficient_resources(&required, &capacity));
    }

    #[test]
    fn test_has_sufficient_resources_partial_allocated() {
        let required = ResourceSpec {
            cpu_cores: 2.0,
            memory_bytes: 2048,
            disk_bytes: 0,
            gpu_count: 0,
            gpu_type: String::new(),
        };

        // Node has 4 CPU, 8192 memory total
        // Already allocated: 1 CPU, 1024 memory
        // Available: 3 CPU, 7168 memory
        // Required: 2 CPU, 2048 memory
        // Should pass
        let capacity = create_test_node_capacity("node-1", 4.0, 8192, 1.0, 1024, HashMap::new());
        assert!(NodeSelector::has_sufficient_resources(&required, &capacity));
    }
}
