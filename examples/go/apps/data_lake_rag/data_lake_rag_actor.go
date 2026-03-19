package main

import (
	"encoding/json"
	"fmt"
	"math"
	"sort"
	"strconv"
	"strings"

	"github.com/plexobject/plexspaces/sdks/go/plexspaces"
)

var host = plexspaces.NewHost()

type initConfig struct {
	ActorID string            `json:"actor_id"`
	Args    map[string]string `json:"args"`
}

type runRequest struct {
	WorkerCount    int `json:"worker_count"`
	QueryCount     int `json:"query_count"`
	ChunksPerQuery int `json:"chunks_per_query"`
	ChunkSizeBytes int `json:"chunk_size_bytes"`
	EmbeddingDim   int `json:"embedding_dim"`
	TopK           int `json:"top_k"`
}

type retrievalCandidate struct {
	ChunkID string  `json:"chunk_id"`
	Score   float64 `json:"score"`
}

type LeaderActor struct {
	plexspaces.BaseActor
	WorkerCount    int    `json:"worker_count"`
	QueryCount     int    `json:"query_count"`
	ChunksPerQuery int    `json:"chunks_per_query"`
	ChunkSizeBytes int    `json:"chunk_size_bytes"`
	EmbeddingDim   int    `json:"embedding_dim"`
	TopK           int    `json:"top_k"`
	TotalCoordMs   uint64 `json:"total_coord_ms"`
	TotalComputeMs uint64 `json:"total_compute_ms"`
}

type WorkerActor struct {
	plexspaces.BaseActor
}

func NewLeaderActor() plexspaces.Actor {
	a := &LeaderActor{
		WorkerCount:    8,
		QueryCount:     12,
		ChunksPerQuery: 48,
		ChunkSizeBytes: 2048,
		EmbeddingDim:   384,
		TopK:           5,
	}
	a.SetSelf(a)
	return a
}

func NewWorkerActor() plexspaces.Actor {
	a := &WorkerActor{}
	a.SetSelf(a)
	return a
}

func (l *LeaderActor) Init(configJSON string) string {
	var config initConfig
	if err := json.Unmarshal([]byte(configJSON), &config); err != nil {
		return "ERROR: " + err.Error()
	}
	l.SetRuntimeMetadata(config.ActorID)
	l.WorkerCount = atoiDefault(config.Args["worker_count"], l.WorkerCount)
	l.QueryCount = atoiDefault(config.Args["query_count"], l.QueryCount)
	l.ChunksPerQuery = atoiDefault(config.Args["chunks_per_query"], l.ChunksPerQuery)
	l.ChunkSizeBytes = atoiDefault(config.Args["chunk_size_bytes"], l.ChunkSizeBytes)
	l.EmbeddingDim = atoiDefault(config.Args["embedding_dim"], l.EmbeddingDim)
	l.TopK = atoiDefault(config.Args["top_k"], l.TopK)
	l.TotalCoordMs = 0
	l.TotalComputeMs = 0
	return ""
}

func (l *LeaderActor) Handle(fromActor, msgType, payloadJSON string) string {
	if msgType != "run" {
		return marshal(map[string]any{"error": "unknown_op", "op": msgType})
	}
	return l.run(payloadJSON)
}

func (w *WorkerActor) Init(configJSON string) string {
	var config initConfig
	if err := json.Unmarshal([]byte(configJSON), &config); err != nil {
		return "ERROR: " + err.Error()
	}
	w.SetRuntimeMetadata(config.ActorID)
	return ""
}

func (w *WorkerActor) Handle(fromActor, msgType, payloadJSON string) string {
	if msgType != "search_chunks" {
		return marshal(map[string]any{"error": "unknown_op", "op": msgType})
	}
	return w.searchChunks(payloadJSON)
}

func (l *LeaderActor) run(payloadJSON string) string {
	request := runRequest{
		WorkerCount:    l.WorkerCount,
		QueryCount:     l.QueryCount,
		ChunksPerQuery: l.ChunksPerQuery,
		ChunkSizeBytes: l.ChunkSizeBytes,
		EmbeddingDim:   l.EmbeddingDim,
		TopK:           l.TopK,
	}
	_ = json.Unmarshal([]byte(payloadJSON), &request)
	if request.WorkerCount <= 0 || request.QueryCount <= 0 || request.ChunksPerQuery <= 0 {
		return marshal(map[string]any{"error": "worker_count, query_count, and chunks_per_query must be positive"})
	}
	if request.TopK <= 0 {
		request.TopK = 5
	}

	groupID := fmt.Sprintf("data-lake-rag-go-%d", host.NowMs())
	group, err := host.CreateShardGroup(map[string]any{
		"group_id":           groupID,
		"actor_type":         "worker",
		"shard_count":        request.WorkerCount,
		"partition_strategy": "hash",
		"rebalance_policy":   "manual",
		"placement":          map[string]any{"strategy": "from_registry"},
		"initial_state":      map[string]any{},
	})
	if err != nil {
		return marshal(map[string]any{"error": err.Error()})
	}
	shardActorIDs := stringSlice(group["shard_actor_ids"])
	if len(shardActorIDs) == 0 {
		return marshal(map[string]any{"error": "failed to create worker shard group"})
	}

	leaderNodeID := actorNodeID(l.ActorID())
	participantNodeIDs := []string{leaderNodeID}
	seenNodes := map[string]bool{leaderNodeID: true}
	for _, actorID := range shardActorIDs {
		nodeID := actorNodeID(actorID)
		if !seenNodes[nodeID] {
			seenNodes[nodeID] = true
			participantNodeIDs = append(participantNodeIDs, nodeID)
		}
	}

	startStatuses := map[string]map[string]any{}
	nodeAddresses := map[string]string{}
	for _, nodeID := range participantNodeIDs {
		status, err := host.ApplicationGetStatus(l.ApplicationID(), nodeID)
		if err != nil {
			return marshal(map[string]any{"error": fmt.Sprintf("failed to capture application status for %s: %v", nodeID, err)})
		}
		startStatuses[nodeID] = status
		if address, ok := status["node_address"].(string); ok && address != "" {
			nodeAddresses[nodeID] = address
		}
	}

	results := make([]map[string]any, 0, request.QueryCount)
	totalErrors := 0
	totalWorkerLatencyMs := uint64(0)
	totalWorkerResponses := uint64(0)
	maxWorkerLatencyMs := uint64(0)
	totalChunkOps := 0
	totalEmbedOps := 0
	totalRetrievalOps := 0
	totalBytesIngested := 0
	remoteNodesWithWork := map[string]bool{}

	for queryIndex := 0; queryIndex < request.QueryCount; queryIndex++ {
		coordStart := host.NowMs()
		response, err := host.ScatterGather(map[string]any{
			"group_id": groupID,
			"query": map[string]any{
				"op":               "search_chunks",
				"query_index":      queryIndex,
				"chunks_per_query": request.ChunksPerQuery,
				"chunk_size_bytes": request.ChunkSizeBytes,
				"embedding_dim":    request.EmbeddingDim,
				"top_k":            request.TopK,
			},
			"aggregation":   "concat",
			"min_responses": request.WorkerCount,
			"timeout_ms":    30000,
		})
		if err != nil {
			return marshal(map[string]any{"error": err.Error()})
		}
		coordMs := host.NowMs() - coordStart

		shards := anySlice(response["shard_responses"])
		candidates := make([]retrievalCandidate, 0, len(shards)*request.TopK)
		iterationErrors := 0
		iterationResponses := 0
		iterationLatencyMs := uint64(0)
		iterationMaxLatencyMs := uint64(0)
		iterationChunkOps := 0
		iterationEmbedOps := 0
		iterationRetrievalOps := 0
		iterationBytes := 0
		for _, shard := range shards {
			shardMap, _ := shard.(map[string]any)
			payloadMap := normalizeWorkerPayload(mapValue(shardMap, "payload"))
			if status, _ := payloadMap["status"].(string); status == "ok" {
				iterationResponses++
				totalWorkerResponses++
				latencyMs := uint64(anyToInt(payloadMap["latency_ms"], 0))
				iterationLatencyMs += latencyMs
				totalWorkerLatencyMs += latencyMs
				if latencyMs > iterationMaxLatencyMs {
					iterationMaxLatencyMs = latencyMs
				}
				if latencyMs > maxWorkerLatencyMs {
					maxWorkerLatencyMs = latencyMs
				}
				iterationChunkOps += anyToInt(payloadMap["chunk_operations"], 0)
				iterationEmbedOps += anyToInt(payloadMap["embedding_operations"], 0)
				iterationRetrievalOps += anyToInt(payloadMap["retrieval_operations"], 0)
				iterationBytes += anyToInt(payloadMap["bytes_ingested"], 0)
				nodeID := stringValue(payloadMap["node_id"])
				if nodeID == "" {
					nodeID = actorNodeID(stringValue(payloadMap["actor_id"]))
				}
				if nodeID != "" && nodeID != leaderNodeID {
					remoteNodesWithWork[nodeID] = true
				}
				for _, rawCandidate := range anySlice(payloadMap["top_chunks"]) {
					candidateMap, _ := rawCandidate.(map[string]any)
					candidates = append(candidates, retrievalCandidate{
						ChunkID: stringValue(candidateMap["chunk_id"]),
						Score:   anyToFloat(candidateMap["score"], 0.0),
					})
				}
			} else {
				iterationErrors++
				totalErrors++
			}
		}
		if len(candidates) == 0 {
			return marshal(map[string]any{"error": "no retrieval candidates returned"})
		}

		computeStart := host.NowMs()
		sort.SliceStable(candidates, func(i, j int) bool {
			return candidates[i].Score > candidates[j].Score
		})
		if len(candidates) > request.TopK {
			candidates = candidates[:request.TopK]
		}
		computeMs := host.NowMs() - computeStart
		l.TotalCoordMs += coordMs
		l.TotalComputeMs += computeMs

		totalChunkOps += iterationChunkOps
		totalEmbedOps += iterationEmbedOps
		totalRetrievalOps += iterationRetrievalOps
		totalBytesIngested += iterationBytes

		avgLatency := 0.0
		if iterationResponses > 0 {
			avgLatency = float64(iterationLatencyMs) / float64(iterationResponses)
		}
		results = append(results, map[string]any{
			"query_index":          queryIndex,
			"responses":            iterationResponses,
			"errors":               iterationErrors,
			"chunk_operations":     iterationChunkOps,
			"embedding_operations": iterationEmbedOps,
			"retrieval_operations": iterationRetrievalOps,
			"bytes_ingested":       iterationBytes,
			"coord_ms":             coordMs,
			"compute_ms":           computeMs,
			"avg_latency_ms":       avgLatency,
			"max_latency_ms":       iterationMaxLatencyMs,
			"top_chunks":           candidates,
		})
	}

	if _, err := host.ApplicationMetricsAdd(l.ApplicationID(), map[string]any{
		"message_count": 1,
		"counter_metrics": map[string]any{
			"leader_messages":           request.QueryCount + 1,
			"leader_runs":               1,
			"retrieval_rounds":          request.QueryCount,
			"retrieval_operation_count": request.QueryCount,
		},
		"latency_totals_ms": map[string]any{
			"leader":              l.TotalComputeMs + l.TotalCoordMs,
			"leader.compute":      l.TotalComputeMs,
			"leader.coordination": l.TotalCoordMs,
		},
		"latency_max_ms": map[string]any{
			"leader":              l.TotalComputeMs + l.TotalCoordMs,
			"leader.compute":      l.TotalComputeMs,
			"leader.coordination": l.TotalCoordMs,
		},
		"latency_samples": map[string]any{
			"leader":              1,
			"leader.compute":      1,
			"leader.coordination": 1,
		},
	}); err != nil {
		return marshal(map[string]any{"error": fmt.Sprintf("leader metrics update failed: %v", err)})
	}

	nodeMetrics := map[string]map[string]int{}
	roleMetrics := map[string]map[string]int{}
	for _, nodeID := range participantNodeIDs {
		status, err := host.ApplicationGetStatus(l.ApplicationID(), nodeID)
		if err != nil {
			return marshal(map[string]any{"error": fmt.Sprintf("failed to collect final application status for %s: %v", nodeID, err)})
		}
		if address, ok := status["node_address"].(string); ok && address != "" {
			nodeAddresses[nodeID] = address
		}
		applyStatusDelta(nodeMetrics, roleMetrics, startStatuses[nodeID], status)
	}

	for nodeID, counts := range computeActorCounts(leaderNodeID, shardActorIDs) {
		node := ensureNodeMetric(nodeMetrics, nodeID)
		node["actors"] += counts["actors"]
		node["leader_actors"] += counts["leader_actors"]
		node["worker_actors"] += counts["worker_actors"]
	}
	ensureRoleMetric(roleMetrics, "leader")["actors"] = 1
	ensureRoleMetric(roleMetrics, "worker")["actors"] = len(shardActorIDs)

	totalMessages := 0
	totalComputeMs := 0
	totalCoordinationMs := 0
	workerNodeCount := 0
	actorCounts := make([]int, 0, len(nodeMetrics))
	for nodeID, metrics := range nodeMetrics {
		totalMessages += metrics["messages"]
		totalComputeMs += metrics["compute_time_ms"]
		totalCoordinationMs += metrics["coordination_time_ms"]
		if nodeID != leaderNodeID && metrics["responses"] > 0 {
			workerNodeCount++
		}
		actorCounts = append(actorCounts, metrics["actors"])
	}
	actorDistributionSkew := 0
	if len(actorCounts) > 0 {
		minActors, maxActors := actorCounts[0], actorCounts[0]
		for _, count := range actorCounts[1:] {
			if count < minActors {
				minActors = count
			}
			if count > maxActors {
				maxActors = count
			}
		}
		actorDistributionSkew = maxActors - minActors
	}

	nodes := map[string]any{}
	for nodeID, metrics := range nodeMetrics {
		nodes[nodeID] = map[string]any{
			"actors":               metrics["actors"],
			"leader_actors":        metrics["leader_actors"],
			"worker_actors":        metrics["worker_actors"],
			"messages":             metrics["messages"],
			"leader_messages":      metrics["leader_messages"],
			"worker_messages":      metrics["worker_messages"],
			"chunk_operations":     metrics["chunk_operations"],
			"embedding_operations": metrics["embedding_operations"],
			"retrieval_operations": metrics["retrieval_operations"],
			"bytes_ingested":       metrics["bytes_ingested"],
			"compute_time_ms":      metrics["compute_time_ms"],
			"coordination_time_ms": metrics["coordination_time_ms"],
			"avg_latency_ms":       averageLatency(metrics),
			"max_latency_ms":       metrics["max_latency_ms"],
			"errors":               metrics["errors"],
		}
	}
	roles := map[string]any{}
	for role, metrics := range roleMetrics {
		roles[role] = map[string]any{
			"actors":               metrics["actors"],
			"messages":             metrics["messages"],
			"chunk_operations":     metrics["chunk_operations"],
			"embedding_operations": metrics["embedding_operations"],
			"retrieval_operations": metrics["retrieval_operations"],
			"bytes_ingested":       metrics["bytes_ingested"],
			"compute_time_ms":      metrics["compute_time_ms"],
			"coordination_time_ms": metrics["coordination_time_ms"],
			"avg_latency_ms":       averageLatency(metrics),
			"max_latency_ms":       metrics["max_latency_ms"],
			"errors":               metrics["errors"],
		}
	}

	remoteNodes := make([]string, 0, len(remoteNodesWithWork))
	for nodeID := range remoteNodesWithWork {
		remoteNodes = append(remoteNodes, nodeID)
	}
	sort.Strings(remoteNodes)

	avgWorkerLatency := 0.0
	if totalWorkerResponses > 0 {
		avgWorkerLatency = float64(totalWorkerLatencyMs) / float64(totalWorkerResponses)
	}

	return marshal(map[string]any{
		"status":                    "ok",
		"worker_count":              request.WorkerCount,
		"query_count":               request.QueryCount,
		"chunks_per_query":          request.ChunksPerQuery,
		"chunk_size_bytes":          request.ChunkSizeBytes,
		"embedding_dim":             request.EmbeddingDim,
		"top_k":                     request.TopK,
		"retrieval_rounds":          request.QueryCount,
		"leader_node_id":            leaderNodeID,
		"node_addresses":            nodeAddresses,
		"shard_actor_ids":           shardActorIDs,
		"node_count":                len(nodeMetrics),
		"worker_node_count":         workerNodeCount,
		"actor_count":               len(shardActorIDs) + 1,
		"message_count":             totalMessages,
		"chunk_operation_count":     totalChunkOps,
		"embedding_operation_count": totalEmbedOps,
		"retrieval_operation_count": totalRetrievalOps,
		"bytes_ingested":            totalBytesIngested,
		"compute_time_ms":           totalComputeMs,
		"coordination_time_ms":      totalCoordinationMs,
		"total_time_ms":             totalComputeMs + totalCoordinationMs,
		"granularity_ratio":         ratio(totalComputeMs, totalCoordinationMs),
		"avg_worker_latency_ms":     avgWorkerLatency,
		"max_worker_latency_ms":     maxWorkerLatencyMs,
		"error_count":               totalErrors,
		"remote_nodes_with_work":    remoteNodes,
		"actor_distribution_skew":   actorDistributionSkew,
		"results":                   results,
		"nodes":                     nodes,
		"roles":                     roles,
	})
}

func (w *WorkerActor) searchChunks(payloadJSON string) string {
	var payload map[string]any
	_ = json.Unmarshal([]byte(payloadJSON), &payload)
	queryIndex := anyToInt(payload["query_index"], 0)
	chunksPerQuery := anyToInt(payload["chunks_per_query"], 48)
	chunkSizeBytes := anyToInt(payload["chunk_size_bytes"], 2048)
	embeddingDim := anyToInt(payload["embedding_dim"], 384)
	topK := anyToInt(payload["top_k"], 5)
	if topK <= 0 {
		topK = 5
	}

	startMs := host.NowMs()
	seed := workerSeed(w.ActorID())
	candidates := make([]retrievalCandidate, 0, topK)
	chunkOps := chunksPerQuery
	embedOps := chunksPerQuery
	retrievalOps := topK
	bytesIngested := chunksPerQuery * chunkSizeBytes
	for i := 0; i < topK; i++ {
		score := math.Mod(float64(seed+queryIndex*31+i*17), 1000.0) / 1000.0
		score += float64(embeddingDim%97) / 10_000.0
		candidates = append(candidates, retrievalCandidate{
			ChunkID: fmt.Sprintf("%s-q%d-c%d", actorRoleID(w.ActorID()), queryIndex, i),
			Score:   score,
		})
	}
	latencyMs := int(host.NowMs() - startMs)

	if _, err := host.ApplicationMetricsAdd(w.ApplicationID(), map[string]any{
		"message_count": 1,
		"counter_metrics": map[string]any{
			"worker_messages":           1,
			"chunk_operation_count":     chunkOps,
			"embedding_operation_count": embedOps,
			"retrieval_operation_count": retrievalOps,
			"bytes_ingested":            bytesIngested,
		},
		"latency_totals_ms": map[string]any{
			"worker":              latencyMs,
			"worker.compute":      latencyMs,
			"worker.coordination": 0,
		},
		"latency_max_ms": map[string]any{
			"worker":              latencyMs,
			"worker.compute":      latencyMs,
			"worker.coordination": 0,
		},
		"latency_samples": map[string]any{
			"worker":              1,
			"worker.compute":      1,
			"worker.coordination": 1,
		},
	}); err != nil {
		return marshal(map[string]any{"status": "error", "error": fmt.Sprintf("worker search metrics update: %v", err)})
	}

	outCandidates := make([]map[string]any, 0, len(candidates))
	for _, candidate := range candidates {
		outCandidates = append(outCandidates, map[string]any{
			"chunk_id": candidate.ChunkID,
			"score":    candidate.Score,
		})
	}

	return marshal(map[string]any{
		"status":               "ok",
		"actor_id":             w.ActorID(),
		"node_id":              actorNodeID(w.ActorID()),
		"latency_ms":           latencyMs,
		"chunk_operations":     chunkOps,
		"embedding_operations": embedOps,
		"retrieval_operations": retrievalOps,
		"bytes_ingested":       bytesIngested,
		"top_chunks":           outCandidates,
	})
}

func actorNodeID(actorID string) string {
	parts := strings.Split(actorID, "@")
	if len(parts) > 1 {
		return parts[len(parts)-1]
	}
	return "local"
}

func actorRoleID(actorID string) string {
	if actorID == "" {
		return ""
	}
	if idx := strings.Index(actorID, "//"); idx >= 0 && idx+2 < len(actorID) {
		rest := actorID[idx+2:]
		if sep := strings.Index(rest, "::"); sep >= 0 {
			return rest[:sep]
		}
		if at := strings.Index(rest, "@"); at >= 0 {
			return rest[:at]
		}
		return rest
	}
	if idx := strings.Index(actorID, ":"); idx >= 0 {
		return actorID[:idx]
	}
	if idx := strings.Index(actorID, "@"); idx >= 0 {
		return actorID[:idx]
	}
	return actorID
}

func normalizeWorkerPayload(payload any) map[string]any {
	current, ok := payload.(map[string]any)
	if !ok {
		return map[string]any{}
	}
	for {
		if _, ok := current["status"]; ok {
			return current
		}
		progressed := false
		for _, key := range []string{"payload", "result", "response", "data"} {
			nested, ok := current[key].(map[string]any)
			if ok {
				current = nested
				progressed = true
				break
			}
		}
		if !progressed {
			return current
		}
	}
}

func ensureNodeMetric(metrics map[string]map[string]int, nodeID string) map[string]int {
	node, ok := metrics[nodeID]
	if ok {
		return node
	}
	node = map[string]int{
		"actors":               0,
		"leader_actors":        0,
		"worker_actors":        0,
		"messages":             0,
		"leader_messages":      0,
		"worker_messages":      0,
		"chunk_operations":     0,
		"embedding_operations": 0,
		"retrieval_operations": 0,
		"bytes_ingested":       0,
		"compute_time_ms":      0,
		"coordination_time_ms": 0,
		"total_latency_ms":     0,
		"max_latency_ms":       0,
		"responses":            0,
		"errors":               0,
	}
	metrics[nodeID] = node
	return node
}

func ensureRoleMetric(metrics map[string]map[string]int, role string) map[string]int {
	entry, ok := metrics[role]
	if ok {
		return entry
	}
	entry = map[string]int{
		"actors":               0,
		"messages":             0,
		"chunk_operations":     0,
		"embedding_operations": 0,
		"retrieval_operations": 0,
		"bytes_ingested":       0,
		"compute_time_ms":      0,
		"coordination_time_ms": 0,
		"total_latency_ms":     0,
		"max_latency_ms":       0,
		"responses":            0,
		"errors":               0,
	}
	metrics[role] = entry
	return entry
}

func applyStatusDelta(
	nodeMetrics map[string]map[string]int,
	roleMetrics map[string]map[string]int,
	startStatus map[string]any,
	endStatus map[string]any,
) {
	counterDelta := saturatingMapDelta(statusMetricsMap(endStatus, "counter_metrics"), statusMetricsMap(startStatus, "counter_metrics"))
	latencyTotalsDelta := saturatingMapDelta(statusMetricsMap(endStatus, "latency_totals_ms"), statusMetricsMap(startStatus, "latency_totals_ms"))
	latencyMaxEnd := statusMetricsMap(endStatus, "latency_max_ms")
	latencyMaxStart := statusMetricsMap(startStatus, "latency_max_ms")
	latencySamplesDelta := saturatingMapDelta(statusMetricsMap(endStatus, "latency_samples"), statusMetricsMap(startStatus, "latency_samples"))
	messageDelta := maxInt(anyToInt(statusMetricsValue(endStatus, "message_count"), 0)-anyToInt(statusMetricsValue(startStatus, "message_count"), 0), 0)
	errorDelta := maxInt(anyToInt(statusMetricsValue(endStatus, "error_count"), 0)-anyToInt(statusMetricsValue(startStatus, "error_count"), 0), 0)
	nodeID := stringValue(endStatus["node_id"])
	node := ensureNodeMetric(nodeMetrics, nodeID)
	node["messages"] += messageDelta
	node["leader_messages"] += counterDelta["leader_messages"]
	node["worker_messages"] += counterDelta["worker_messages"]
	node["chunk_operations"] += counterDelta["chunk_operation_count"]
	node["embedding_operations"] += counterDelta["embedding_operation_count"]
	node["retrieval_operations"] += counterDelta["retrieval_operation_count"]
	node["bytes_ingested"] += counterDelta["bytes_ingested"]
	node["compute_time_ms"] += latencyTotalsDelta["worker.compute"] + latencyTotalsDelta["leader.compute"]
	node["coordination_time_ms"] += latencyTotalsDelta["worker.coordination"] + latencyTotalsDelta["leader.coordination"]
	node["total_latency_ms"] += latencyTotalsDelta["worker"] + latencyTotalsDelta["leader"]
	node["max_latency_ms"] = maxInt(node["max_latency_ms"], latencyMaxEnd["worker"], latencyMaxStart["worker"], latencyMaxEnd["leader"], latencyMaxStart["leader"])
	node["responses"] += latencySamplesDelta["worker"]
	node["errors"] += errorDelta

	leader := ensureRoleMetric(roleMetrics, "leader")
	leader["messages"] += counterDelta["leader_messages"]
	leader["retrieval_operations"] += counterDelta["retrieval_operation_count"]
	leader["compute_time_ms"] += latencyTotalsDelta["leader.compute"]
	leader["coordination_time_ms"] += latencyTotalsDelta["leader.coordination"]
	leader["total_latency_ms"] += latencyTotalsDelta["leader"]
	leader["max_latency_ms"] = maxInt(leader["max_latency_ms"], latencyMaxEnd["leader"], latencyMaxStart["leader"])
	leader["responses"] += latencySamplesDelta["leader"]

	worker := ensureRoleMetric(roleMetrics, "worker")
	worker["messages"] += counterDelta["worker_messages"]
	worker["chunk_operations"] += counterDelta["chunk_operation_count"]
	worker["embedding_operations"] += counterDelta["embedding_operation_count"]
	worker["retrieval_operations"] += counterDelta["retrieval_operation_count"]
	worker["bytes_ingested"] += counterDelta["bytes_ingested"]
	worker["compute_time_ms"] += latencyTotalsDelta["worker.compute"]
	worker["coordination_time_ms"] += latencyTotalsDelta["worker.coordination"]
	worker["total_latency_ms"] += latencyTotalsDelta["worker"]
	worker["max_latency_ms"] = maxInt(worker["max_latency_ms"], latencyMaxEnd["worker"], latencyMaxStart["worker"])
	worker["responses"] += latencySamplesDelta["worker"]
	worker["errors"] += errorDelta
}

func statusMetricsValue(status map[string]any, field string) any {
	application, _ := status["application"].(map[string]any)
	metrics, _ := application["metrics"].(map[string]any)
	return metrics[field]
}

func statusMetricsMap(status map[string]any, field string) map[string]int {
	value, _ := statusMetricsValue(status, field).(map[string]any)
	result := map[string]int{}
	for key, raw := range value {
		result[key] = anyToInt(raw, 0)
	}
	return result
}

func saturatingMapDelta(end, start map[string]int) map[string]int {
	result := map[string]int{}
	for key, endValue := range end {
		startValue := start[key]
		if endValue > startValue {
			result[key] = endValue - startValue
		} else {
			result[key] = 0
		}
	}
	for key := range start {
		if _, ok := result[key]; !ok {
			result[key] = 0
		}
	}
	return result
}

func computeActorCounts(leaderNodeID string, shardActorIDs []string) map[string]map[string]int {
	nodes := map[string]map[string]int{
		leaderNodeID: {
			"actors":        1,
			"leader_actors": 1,
			"worker_actors": 0,
		},
	}
	for _, actorID := range shardActorIDs {
		nodeID := actorNodeID(actorID)
		node, ok := nodes[nodeID]
		if !ok {
			node = map[string]int{
				"actors":        0,
				"leader_actors": 0,
				"worker_actors": 0,
			}
			nodes[nodeID] = node
		}
		node["actors"]++
		node["worker_actors"]++
	}
	return nodes
}

func averageLatency(metric map[string]int) float64 {
	if metric["responses"] <= 0 {
		return 0.0
	}
	return float64(metric["total_latency_ms"]) / float64(metric["responses"])
}

func anySlice(value any) []any {
	if value == nil {
		return nil
	}
	if out, ok := value.([]any); ok {
		return out
	}
	return nil
}

func stringSlice(value any) []string {
	raw := anySlice(value)
	result := make([]string, 0, len(raw))
	for _, item := range raw {
		if s, ok := item.(string); ok {
			result = append(result, s)
		}
	}
	return result
}

func anyToInt(value any, fallback int) int {
	switch v := value.(type) {
	case int:
		return v
	case int32:
		return int(v)
	case int64:
		return int(v)
	case float64:
		return int(v)
	case json.Number:
		if parsed, err := v.Int64(); err == nil {
			return int(parsed)
		}
	case string:
		if parsed, err := strconv.Atoi(v); err == nil {
			return parsed
		}
	}
	return fallback
}

func anyToFloat(value any, fallback float64) float64 {
	switch v := value.(type) {
	case float64:
		return v
	case int:
		return float64(v)
	case json.Number:
		if parsed, err := v.Float64(); err == nil {
			return parsed
		}
	}
	return fallback
}

func atoiDefault(value string, fallback int) int {
	if parsed, err := strconv.Atoi(value); err == nil {
		return parsed
	}
	return fallback
}

func stringValue(value any) string {
	if s, ok := value.(string); ok {
		return s
	}
	return ""
}

func mapValue(value any, key string) any {
	if typed, ok := value.(map[string]any); ok {
		return typed[key]
	}
	return nil
}

func ratio(num, den int) float64 {
	if den == 0 {
		return 0.0
	}
	return float64(num) / float64(den)
}

func maxInt(values ...int) int {
	if len(values) == 0 {
		return 0
	}
	max := values[0]
	for _, value := range values[1:] {
		if value > max {
			max = value
		}
	}
	return max
}

func workerSeed(actorID string) int {
	total := 0
	for _, ch := range actorID {
		total += int(ch)
	}
	return total % 10000
}

func marshal(v map[string]any) string {
	bytes, _ := json.Marshal(v)
	return string(bytes)
}

func init() {
	router := plexspaces.NewActorRouter()
	router.Route("leader", NewLeaderActor)
	router.Route("worker", NewWorkerActor)
	plexspaces.Register(router)
}

func main() {}
