// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors

package plexspaces

// NodePlacement configures shard placement across nodes.
type NodePlacement struct {
	Strategy       string            `json:"strategy,omitempty"`
	NodeIDs        []string          `json:"node_ids,omitempty"`
	Cluster        string            `json:"cluster,omitempty"`
	RequiredLabels map[string]string `json:"required_labels,omitempty"`
	AvoidNodeIDs   []string          `json:"avoid_node_ids,omitempty"`
	AffinityLabels map[string]string `json:"affinity_labels,omitempty"`
}

// CreateShardGroupRequest creates a data-parallel shard group.
type CreateShardGroupRequest struct {
	GroupID           string            `json:"group_id"`
	ActorType         string            `json:"actor_type"`
	ShardCount        int               `json:"shard_count"`
	PartitionStrategy string            `json:"partition_strategy,omitempty"`
	RebalancePolicy   string            `json:"rebalance_policy,omitempty"`
	Placement         NodePlacement     `json:"placement,omitempty"`
	InitialState      map[string]any    `json:"initial_state,omitempty"`
	Metadata          map[string]string `json:"metadata,omitempty"`
}

// CreateShardGroupResponse is the result of CreateShardGroup.
type CreateShardGroupResponse struct {
	GroupID       string   `json:"group_id"`
	ActorType     string   `json:"actor_type"`
	ShardActorIDs []string `json:"shard_actor_ids"`
	ShardCount    int      `json:"shard_count"`
}

// ScatterGatherRequest runs a scatter/gather across a shard group.
type ScatterGatherRequest struct {
	GroupID      string         `json:"group_id"`
	Query        map[string]any `json:"query"`
	Aggregation  string         `json:"aggregation,omitempty"`
	TimeoutMs    int            `json:"timeout_ms,omitempty"`
	MinResponses int            `json:"min_responses,omitempty"`
}

// ScatterGatherResponse is the result of ScatterGather.
type ScatterGatherResponse struct {
	ShardResponses []map[string]any `json:"shard_responses"`
	Stats          map[string]any   `json:"stats"`
}

// BroadcastShardGroupRequest broadcasts a message to all shards.
type BroadcastShardGroupRequest struct {
	GroupID   string         `json:"group_id"`
	Message   map[string]any `json:"message"`
	TimeoutMs int            `json:"timeout_ms,omitempty"`
	MinAcks   int            `json:"min_acks,omitempty"`
}

// ReduceShardGroupRequest maps a function across shards and reduces the results.
type ReduceShardGroupRequest struct {
	GroupID      string         `json:"group_id"`
	MapFunction  map[string]any `json:"map_function"`
	Reduction    string         `json:"reduction"`
	Target       string         `json:"target,omitempty"`
	TimeoutMs    int            `json:"timeout_ms,omitempty"`
	MinResponses int            `json:"min_responses,omitempty"`
}

// AllReduceShardGroupRequest is the same as ReduceShardGroupRequest.
type AllReduceShardGroupRequest = ReduceShardGroupRequest

// BarrierShardGroupRequest synchronizes shards at a barrier round.
type BarrierShardGroupRequest struct {
	GroupID   string `json:"group_id"`
	BarrierID string `json:"barrier_id"`
	Round     uint64 `json:"round,omitempty"`
	TimeoutMs int    `json:"timeout_ms,omitempty"`
	MinAcks   int    `json:"min_acks,omitempty"`
}

// MapShardGroupRequest maps a function across shards and collects results.
type MapShardGroupRequest struct {
	GroupID      string         `json:"group_id"`
	MapFunction  map[string]any `json:"map_function"`
	TimeoutMs    int            `json:"timeout_ms,omitempty"`
	MinResponses int            `json:"min_responses,omitempty"`
}

// BulkUpdateShardGroupRequest routes N updates to their respective shards.
type BulkUpdateShardGroupRequest struct {
	GroupID          string                    `json:"group_id"`
	Updates          map[string]map[string]any `json:"updates"`
	ConsistencyLevel string                    `json:"consistency_level,omitempty"`
	TimeoutMs        int                       `json:"timeout_ms,omitempty"`
	WaitForResponses bool                      `json:"wait_for_responses,omitempty"`
}

// Placement strategy constants.
const (
	PlacementFromRegistry = "from_registry"
	PlacementSameNode     = "same_node"
	PlacementNodeIDs      = "node_ids"

	PartitionStrategyHash           = "hash"
	PartitionStrategyRange          = "range"
	PartitionStrategyConsistentHash = "consistent_hash"

	RebalancePolicyManual    = "manual"
	RebalancePolicyOnScale   = "on_scale"
	RebalancePolicyLoadBased = "load_based"

	AggregationConcat   = "concat"
	AggregationMerge    = "merge"
	AggregationFirst    = "first"
	AggregationMajority = "majority"

	ReductionSum     = "sum"
	ReductionMin     = "min"
	ReductionMax     = "max"
	ReductionProduct = "product"
	ReductionConcat  = "concat"
	ReductionBoolAnd = "bool_and"
	ReductionBoolOr  = "bool_or"
)
