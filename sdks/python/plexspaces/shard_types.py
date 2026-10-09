# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2025 PlexSpaces Contributors

from __future__ import annotations
from dataclasses import dataclass, field
from typing import Any


@dataclass
class NodePlacement:
    strategy: str = "from_registry"
    node_ids: list[str] = field(default_factory=list)
    cluster: str = ""
    required_labels: dict[str, str] = field(default_factory=dict)
    avoid_node_ids: list[str] = field(default_factory=list)
    affinity_labels: dict[str, str] = field(default_factory=dict)


@dataclass
class CreateShardGroupRequest:
    group_id: str = ""
    actor_type: str = ""
    shard_count: int = 0
    partition_strategy: str = "hash"
    rebalance_policy: str = "manual"
    placement: NodePlacement = field(default_factory=NodePlacement)
    initial_state: dict[str, Any] = field(default_factory=dict)
    metadata: dict[str, str] = field(default_factory=dict)


@dataclass
class CreateShardGroupResponse:
    group_id: str = ""
    actor_type: str = ""
    shard_actor_ids: list[str] = field(default_factory=list)
    shard_count: int = 0


@dataclass
class ScatterGatherRequest:
    group_id: str = ""
    query: dict[str, Any] = field(default_factory=dict)
    aggregation: str = "concat"
    timeout_ms: int = 30_000
    min_responses: int = 0


@dataclass
class ScatterGatherResponse:
    shard_responses: list[dict[str, Any]] = field(default_factory=list)
    stats: dict[str, Any] = field(default_factory=dict)


@dataclass
class BroadcastShardGroupRequest:
    group_id: str = ""
    message: dict[str, Any] = field(default_factory=dict)
    timeout_ms: int = 30_000
    min_acks: int = 0


@dataclass
class ReduceShardGroupRequest:
    group_id: str = ""
    map_function: dict[str, Any] = field(default_factory=dict)
    reduction: str = ""
    target: str = ""
    timeout_ms: int = 30_000
    min_responses: int = 0


# AllReduce has the same shape as Reduce
AllReduceShardGroupRequest = ReduceShardGroupRequest


@dataclass
class BarrierShardGroupRequest:
    group_id: str = ""
    barrier_id: str = ""
    round: int = 0
    timeout_ms: int = 30_000
    min_acks: int = 0


@dataclass
class MapShardGroupRequest:
    group_id: str = ""
    map_function: dict[str, Any] = field(default_factory=dict)
    timeout_ms: int = 30_000
    min_responses: int = 0


@dataclass
class BulkUpdateShardGroupRequest:
    group_id: str = ""
    updates: dict[str, dict[str, Any]] = field(default_factory=dict)
    consistency_level: str = "eventual"
    timeout_ms: int = 30_000
    wait_for_responses: bool = False
