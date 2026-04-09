package main

import (
	"encoding/json"
	"testing"
)

func TestShardTransportFailedVariants(t *testing.T) {
	t.Parallel()
	if !shardTransportFailed(map[string]any{"success": false}) {
		t.Fatal("bool false => failed")
	}
	if shardTransportFailed(map[string]any{"success": true}) {
		t.Fatal("bool true => ok")
	}
	if !shardTransportFailed(map[string]any{"success": float64(0)}) {
		t.Fatal("float64(0) => failed")
	}
	if shardTransportFailed(map[string]any{"success": float64(1)}) {
		t.Fatal("float64(1) => ok")
	}
	if !shardTransportFailed(nil) {
		t.Fatal("nil shard map should be transport failed")
	}
}

func TestRemoteWorkerShardHostNodeIDsMatchesMultinodePlacement(t *testing.T) {
	t.Parallel()
	leader := "test-node-8091"
	shards := []string{
		"g-1//worker::data-lake-rag-go@test-node-8093",
		"g-1//worker::data-lake-rag-go@test-node-8091",
		"g-1//worker::data-lake-rag-go@test-node-8093",
		"g-1//worker::data-lake-rag-go@test-node-8091",
	}
	got := remoteWorkerShardHostNodeIDs(leader, shards)
	if len(got) != 1 || got[0] != "test-node-8093" {
		t.Fatalf("remoteWorkerShardHostNodeIDs = %v, want [test-node-8093]", got)
	}
}

// TestGoldenAskPayloadTopology uses a trimmed capture of a real multinode run (shard ids only).
func TestGoldenAskPayloadTopology(t *testing.T) {
	t.Parallel()
	const golden = `{
  "success": true,
  "payload": {
    "leader_node_id": "test-node-8091",
    "shard_actor_ids": [
      "data-lake-rag-go-1//worker::data-lake-rag-go@test-node-8093",
      "data-lake-rag-go-1//worker::data-lake-rag-go@test-node-8091",
      "data-lake-rag-go-1//worker::data-lake-rag-go@test-node-8093",
      "data-lake-rag-go-1//worker::data-lake-rag-go@test-node-8091",
      "data-lake-rag-go-1//worker::data-lake-rag-go@test-node-8093",
      "data-lake-rag-go-1//worker::data-lake-rag-go@test-node-8091",
      "data-lake-rag-go-1//worker::data-lake-rag-go@test-node-8093",
      "data-lake-rag-go-1//worker::data-lake-rag-go@test-node-8091"
    ],
    "error_count": 48,
    "remote_nodes_with_work": [],
    "worker_node_count": 0
  }
}`
	var outer map[string]any
	if err := json.Unmarshal([]byte(golden), &outer); err != nil {
		t.Fatal(err)
	}
	payload, _ := outer["payload"].(map[string]any)
	leader, _ := payload["leader_node_id"].(string)
	rawShards, _ := payload["shard_actor_ids"].([]any)
	shardIDs := make([]string, 0, len(rawShards))
	for _, x := range rawShards {
		if s, ok := x.(string); ok {
			shardIDs = append(shardIDs, s)
		}
	}
	remoteHosts := remoteWorkerShardHostNodeIDs(leader, shardIDs)
	if len(remoteHosts) != 1 || remoteHosts[0] != "test-node-8093" {
		t.Fatalf("golden placement remote hosts = %v, want [test-node-8093]", remoteHosts)
	}
	statusBased := int(anyToInt(payload["worker_node_count"], 0))
	placementCount := len(remoteHosts)
	merged := statusBased
	if placementCount > merged {
		merged = placementCount
	}
	if merged < 1 {
		t.Fatalf("merged worker_node_count = %d, want >= 1 for two-node cluster", merged)
	}
}
