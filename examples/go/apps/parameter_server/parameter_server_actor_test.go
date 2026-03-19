package main

import (
	"testing"

	"github.com/plexobject/plexspaces/sdks/go/plexspaces"
)

func TestActorIDHelpers(t *testing.T) {
	actorID := "01KM1FJ3AF3BA2BDNKZPGH9K2P//worker::parameter-server-go@test-node-8093"
	base := &plexspaces.BaseActor{}
	base.SetRuntimeMetadata(actorID)
	if got := base.ApplicationID(); got != "parameter-server-go" {
		t.Fatalf("actorApplicationID() = %q, want %q", got, "parameter-server-go")
	}
	if got := actorNodeID(actorID); got != "test-node-8093" {
		t.Fatalf("actorNodeID() = %q, want %q", got, "test-node-8093")
	}
}

func TestNormalizeWorkerPayloadUnwrapsNestedPayload(t *testing.T) {
	payload := normalizeWorkerPayload(map[string]any{
		"payload": map[string]any{
			"response": map[string]any{
				"status":            "ok",
				"latency_ms":        12,
				"samples_processed": 8,
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

func TestComputeActorCountsAssignsLeaderAndWorkersToNodes(t *testing.T) {
	counts := computeActorCounts(
		"test-node-8091",
		[]string{
			"01A//worker::parameter-server-go@test-node-8091",
			"01B//worker::parameter-server-go@test-node-8093",
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
