# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Tests for collective / parallel shard-group host operations.

from plexspaces.host import host
from plexspaces.shard_types import (
    AllReduceShardGroupRequest,
    BarrierShardGroupRequest,
    BroadcastShardGroupRequest,
    BulkUpdateShardGroupRequest,
    MapShardGroupRequest,
    ReduceShardGroupRequest,
)


# ---------------------------------------------------------------------------
# broadcast_shard_group
# ---------------------------------------------------------------------------

def test_broadcast_shard_group_returns_stats():
    response = host.broadcast_shard_group(BroadcastShardGroupRequest(
        group_id="workers",
        message={"op": "reset"},
        min_acks=1,
    ))
    assert "shard_responses" in response
    assert "stats" in response
    stats = response["stats"]
    assert "shards_queried" in stats
    assert "shards_responded" in stats
    assert "shards_failed" in stats


def test_broadcast_shard_group_accepts_empty_message():
    response = host.broadcast_shard_group(BroadcastShardGroupRequest(
        group_id="g1",
        message={},
    ))
    assert isinstance(response, dict)


# ---------------------------------------------------------------------------
# reduce_shard_group
# ---------------------------------------------------------------------------

def test_reduce_shard_group_returns_result_and_stats():
    response = host.reduce_shard_group(ReduceShardGroupRequest(
        group_id="workers",
        map_function={"action": "get_count"},
        reduction="SUM",
        min_responses=1,
    ))
    assert "result" in response
    assert "stats" in response
    assert "shard_responses" in response


def test_reduce_shard_group_with_target_field():
    response = host.reduce_shard_group(ReduceShardGroupRequest(
        group_id="workers",
        map_function={"action": "get_metrics"},
        reduction="MAX",
        target="count",
    ))
    assert isinstance(response, dict)


# ---------------------------------------------------------------------------
# all_reduce_shard_group
# ---------------------------------------------------------------------------

def test_all_reduce_shard_group_returns_result_and_stats():
    response = host.all_reduce_shard_group(AllReduceShardGroupRequest(
        group_id="workers",
        map_function={"action": "get_sum"},
        reduction="SUM",
        min_responses=1,
    ))
    assert "result" in response
    assert "stats" in response
    assert "shard_responses" in response


# ---------------------------------------------------------------------------
# barrier_shard_group
# ---------------------------------------------------------------------------

def test_barrier_shard_group_returns_stats():
    response = host.barrier_shard_group(BarrierShardGroupRequest(
        group_id="workers",
        barrier_id="round-1",
        round=1,
        min_acks=1,
    ))
    assert "shard_responses" in response
    assert "stats" in response


def test_barrier_shard_group_accepts_minimal_request():
    response = host.barrier_shard_group(BarrierShardGroupRequest(
        group_id="g1",
    ))
    assert isinstance(response, dict)


# ---------------------------------------------------------------------------
# spawn_actors
# ---------------------------------------------------------------------------

def test_spawn_actors_returns_results():
    response = host.spawn_actors({
        "requests": [
            {"actor_type": "counter", "actor_id": "c-0"},
            {"actor_type": "counter", "actor_id": "c-1"},
        ]
    })
    assert "results" in response
    results = response["results"]
    assert len(results) == 2
    assert all(r["success"] for r in results)


def test_spawn_actors_auto_id():
    response = host.spawn_actors({
        "requests": [
            {"actor_type": "worker"},
        ]
    })
    results = response["results"]
    assert len(results) == 1
    assert results[0]["success"]
    assert results[0]["response"]["actor_ref"].endswith("@test-node")


def test_spawn_actors_empty_request():
    response = host.spawn_actors({"requests": []})
    assert response["results"] == []


def test_spawn_actors_with_instances_count():
    response = host.spawn_actors({
        "requests": [
            {"actor_type": "worker", "actor_id": "w", "instances_count": 3},
        ]
    })
    # Mock returns one result per request entry (instances_count is
    # handled server-side); verify the wrapper round-trips cleanly.
    assert "results" in response


# ---------------------------------------------------------------------------
# bulk_update_shard_group
# ---------------------------------------------------------------------------

def test_bulk_update_shard_group_returns_stats():
    response = host.bulk_update_shard_group(BulkUpdateShardGroupRequest(
        group_id="workers",
        updates={"key1": {"payload": "data"}},
    ))
    assert "updates_sent" in response
    assert "updates_succeeded" in response


# ---------------------------------------------------------------------------
# map_shard_group
# ---------------------------------------------------------------------------

def test_map_shard_group_returns_results():
    response = host.map_shard_group(MapShardGroupRequest(
        group_id="workers",
        map_function={"action": "status"},
    ))
    assert "results" in response
    assert "stats" in response


# ---------------------------------------------------------------------------
# scatter_gather
# ---------------------------------------------------------------------------

def test_scatter_gather_returns_responses_and_stats():
    from plexspaces import ScatterGatherRequest
    response = host.scatter_gather(ScatterGatherRequest(
        group_id="workers",
        query={"action": "get_all"},
    ))
    assert hasattr(response, "shard_responses")
    assert hasattr(response, "stats")
