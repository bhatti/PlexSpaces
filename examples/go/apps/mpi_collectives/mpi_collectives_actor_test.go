package main

import "testing"

func TestNormalizeWorkerPayloadUnwrapsNestedPayload(t *testing.T) {
	payload := normalizeWorkerPayload(map[string]any{
		"payload": map[string]any{
			"response": map[string]any{
				"status":     "ok",
				"latency_ms": 12,
			},
		},
	})
	if payload["status"] != "ok" {
		t.Fatalf("status = %v, want ok", payload["status"])
	}
	if payload["latency_ms"] != 12 {
		t.Fatalf("latency_ms = %v, want 12", payload["latency_ms"])
	}
}

func TestApplyMetricsDeltaAccumulatesCollectiveAndElementMetrics(t *testing.T) {
	start := map[string]any{
		"message_count": 0,
		"error_count":   0,
		"counter_metrics": map[string]any{
			"leader_messages":              0,
			"worker_messages":              0,
			"leader_collective_operations": 0,
			"worker_collective_operations": 0,
			"worker_element_operations":    0,
		},
		"latency_totals_ms": map[string]any{
			"leader":              0,
			"leader.compute":      0,
			"leader.coordination": 0,
			"worker":              0,
			"worker.compute":      0,
			"worker.coordination": 0,
		},
		"latency_max_ms": map[string]any{
			"leader": 0,
			"worker": 0,
		},
		"latency_samples": map[string]any{
			"leader": 0,
			"worker": 0,
		},
	}
	end := map[string]any{
		"message_count": 5,
		"error_count":   0,
		"counter_metrics": map[string]any{
			"leader_messages":              2,
			"worker_messages":              3,
			"leader_collective_operations": 2,
			"worker_collective_operations": 3,
			"worker_element_operations":    99,
		},
		"latency_totals_ms": map[string]any{
			"leader":              20,
			"leader.compute":      7,
			"leader.coordination": 13,
			"worker":              40,
			"worker.compute":      35,
			"worker.coordination": 5,
		},
		"latency_max_ms": map[string]any{
			"leader": 20,
			"worker": 40,
		},
		"latency_samples": map[string]any{
			"leader": 1,
			"worker": 3,
		},
	}

	nodeMetrics := map[string]map[string]int{}
	roleMetrics := map[string]map[string]int{}
	applyMetricsDelta(nodeMetrics, roleMetrics, "test-node-8091", start, end)

	node := nodeMetrics["test-node-8091"]
	if node["collective_operations"] != 5 {
		t.Fatalf("collective_operations = %d, want 5", node["collective_operations"])
	}
	if node["element_operations"] != 99 {
		t.Fatalf("element_operations = %d, want 99", node["element_operations"])
	}
	if roleMetrics["leader"]["collective_operations"] != 2 {
		t.Fatalf("leader collective_operations = %d, want 2", roleMetrics["leader"]["collective_operations"])
	}
	if roleMetrics["worker"]["collective_operations"] != 3 {
		t.Fatalf("worker collective_operations = %d, want 3", roleMetrics["worker"]["collective_operations"])
	}
}

func TestAccumulateShardStatsAcceptsReduceStyleShardEnvelope(t *testing.T) {
	nodeMetrics := map[string]bool{}
	var totalLatency uint64
	var totalResponses uint64
	var maxLatency uint64
	totalErrors := 0

	err := accumulateShardStats([]any{
		map[string]any{
			"shard_actor_id": "01B//worker::mpi-collectives-go@test-node-8093",
			"payload":        42.5,
			"latency_ms":     3.0,
			"success":        true,
		},
	}, "test-node-8091", nodeMetrics, &totalLatency, &totalResponses, &maxLatency, &totalErrors)
	if err != nil {
		t.Fatalf("accumulateShardStats error: %v", err)
	}
	if totalErrors != 0 {
		t.Fatalf("totalErrors = %d, want 0", totalErrors)
	}
	if totalResponses != 1 {
		t.Fatalf("totalResponses = %d, want 1", totalResponses)
	}
	if totalLatency != 3 || maxLatency != 3 {
		t.Fatalf("latency totals = %d/%d, want 3/3", totalLatency, maxLatency)
	}
	if !nodeMetrics["test-node-8093"] {
		t.Fatalf("expected remote node to be marked from shard_actor_id fallback")
	}
}

func TestComputeActorCountsAssignsLeaderAndWorkersToNodes(t *testing.T) {
	counts := computeActorCounts(
		"test-node-8091",
		[]string{
			"01A//worker::mpi-collectives-go@test-node-8091",
			"01B//worker::mpi-collectives-go@test-node-8093",
		},
	)
	if counts["test-node-8091"]["leader_actors"] != 1 {
		t.Fatalf("leader_actors = %d, want 1", counts["test-node-8091"]["leader_actors"])
	}
	if counts["test-node-8091"]["worker_actors"] != 1 {
		t.Fatalf("worker_actors on node1 = %d, want 1", counts["test-node-8091"]["worker_actors"])
	}
	if counts["test-node-8093"]["worker_actors"] != 1 {
		t.Fatalf("worker_actors on node2 = %d, want 1", counts["test-node-8093"]["worker_actors"])
	}
}
