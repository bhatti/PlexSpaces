// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// MPI-style collective operations demo using PlexSpaces shard-group APIs.
//
// Leader orchestrates 5 collective phases per round:
//   BroadcastShardGroup  → MPI_Bcast   (broadcast scale factor)
//   ScatterGather        → MPI_Scatter + MPI_Gather (distribute work)
//   ReduceShardGroup     → MPI_Reduce  (partial sums → global sum at leader)
//   AllReduceShardGroup  → MPI_Allreduce (global sum at ALL workers via "event")
//   BarrierShardGroup    → MPI_Barrier (end-of-round synchronisation)

package main

import (
	"encoding/json"
	"fmt"
	"math"
	"sort"
	"strconv"
	"strings"

	"github.com/bhatti/PlexSpaces/sdks/go/plexspaces"
)

var host = plexspaces.NewHost()

type initConfig struct {
	ActorID string            `json:"actor_id"`
	Args    map[string]string `json:"args"`
}

type runRequest struct {
	WorkerCount       int `json:"worker_count"`
	ElementsPerWorker int `json:"elements_per_worker"`
	Rounds            int `json:"rounds"`
}

type LeaderActor struct {
	plexspaces.BaseActor
	WorkerCount       int    `json:"worker_count"`
	ElementsPerWorker int    `json:"elements_per_worker"`
	Rounds            int    `json:"rounds"`
	TotalCoordMs      uint64 `json:"total_coord_ms"`
	TotalComputeMs    uint64 `json:"total_compute_ms"`
}

type WorkerActor struct {
	plexspaces.BaseActor
	BroadcastScale float64 `json:"broadcast_scale"`
	LastRound      int     `json:"last_round"`
	LastPartialSum float64 `json:"last_partial_sum"`
	LastReducedSum float64 `json:"last_reduced_sum"`
}

func NewLeaderActor() plexspaces.Actor {
	a := &LeaderActor{
		WorkerCount:       8,
		ElementsPerWorker: 24000,
		Rounds:            8,
	}
	a.SetSelf(a)
	return a
}

func NewWorkerActor() plexspaces.Actor {
	a := &WorkerActor{BroadcastScale: 1.0}
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
	l.ElementsPerWorker = atoiDefault(config.Args["elements_per_worker"], l.ElementsPerWorker)
	l.Rounds = atoiDefault(config.Args["rounds"], l.Rounds)
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
	w.BroadcastScale = 1.0
	w.LastRound = -1
	w.LastPartialSum = 0
	w.LastReducedSum = 0
	return ""
}

func (w *WorkerActor) Handle(fromActor, msgType, payloadJSON string) string {
	switch msgType {
	case "apply_broadcast":
		return w.applyBroadcast(payloadJSON)
	case "process_scatter_chunk":
		return w.processScatterChunk(payloadJSON)
	case "partial_reduce":
		return w.partialReduce(payloadJSON)
	case "apply_allreduce":
		// Apply the all-reduce result delivered through the worker message path.
		return w.applyAllreduce(payloadJSON)
	case "event":
		// AllReduceShardGroup broadcasts the reduced result to all workers with
		// message_type "event" (header: plexspaces-collective-op=all-reduce-result).
		return w.applyAllreduceEvent(payloadJSON)
	default:
		return marshal(map[string]any{"error": "unknown_op", "op": msgType})
	}
}

// ---------------------------------------------------------------------------
// Leader: orchestrate all 5 collective phases per round
// ---------------------------------------------------------------------------

func (l *LeaderActor) run(payloadJSON string) string {
	request := runRequest{
		WorkerCount:       l.WorkerCount,
		ElementsPerWorker: l.ElementsPerWorker,
		Rounds:            l.Rounds,
	}
	_ = json.Unmarshal([]byte(payloadJSON), &request)
	if request.WorkerCount <= 0 || request.ElementsPerWorker <= 0 || request.Rounds <= 0 {
		return marshal(map[string]any{"error": "worker_count, elements_per_worker, and rounds must be positive"})
	}

	groupID := fmt.Sprintf("mpi-collectives-go-%d", host.NowMs())
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
	startMetrics := map[string]map[string]any{}
	nodeAddresses := map[string]string{}
	for _, nodeID := range participantNodeIDs {
		status, err := host.ApplicationGetStatus(l.ApplicationID(), nodeID)
		if err != nil {
			return marshal(map[string]any{"error": fmt.Sprintf("failed to capture application status for %s: %v", nodeID, err)})
		}
		startStatuses[nodeID] = status
		metrics, err := host.ApplicationGetMetrics(l.ApplicationID(), nodeID)
		if err != nil {
			return marshal(map[string]any{"error": fmt.Sprintf("failed to capture application metrics for %s: %v", nodeID, err)})
		}
		startMetrics[nodeID] = metrics
		if address, ok := status["node_address"].(string); ok && address != "" {
			nodeAddresses[nodeID] = address
		}
	}

	totalWorkerLatencyMs := uint64(0)
	totalWorkerResponses := uint64(0)
	maxWorkerLatencyMs := uint64(0)
	totalElements := 0
	totalCollectiveOps := 0
	totalErrors := 0
	remoteNodesWithWork := map[string]bool{}
	roundSummaries := make([]map[string]any, 0, request.Rounds)
	finalReducedTotal := 0.0
	allreduceConsistent := true

	for round := 0; round < request.Rounds; round++ {
		scale := 1.0 + float64(round+1)*0.125

		// ── Phase 1: MPI_Bcast ─────────────────────────────────────────────────
		// Broadcast the round's scale factor to all worker shards.
		broadcastStart := host.NowMs()
		broadcastResponse, err := host.BroadcastShardGroup(map[string]any{
			"group_id":     groupID,
			"message_type": "apply_broadcast",
			"message":      map[string]any{"round": round, "scale": scale},
			"min_acks":     request.WorkerCount,
			"timeout_ms":   30000,
		})
		if err != nil {
			return marshal(map[string]any{"error": err.Error()})
		}
		broadcastCoordMs := host.NowMs() - broadcastStart
		l.TotalCoordMs += broadcastCoordMs
		totalCollectiveOps++
		if err := accumulateShardStats(
			broadcastResponse["shard_responses"],
			leaderNodeID, remoteNodesWithWork,
			&totalWorkerLatencyMs, &totalWorkerResponses, &maxWorkerLatencyMs, &totalErrors,
		); err != nil {
			return marshal(map[string]any{"error": err.Error()})
		}

		// ── Phase 2: MPI_Scatter + MPI_Gather ─────────────────────────────────
		// Each worker receives a unique chunk descriptor and computes a local sum.
		scatterStart := host.NowMs()
		scatterResponse, err := host.ScatterGather(map[string]any{
			"group_id":     groupID,
			"message_type": "process_scatter_chunk",
			"query": map[string]any{
				"round":               round,
				"elements_per_worker": request.ElementsPerWorker,
				"base_value":          float64((round + 1) * 7),
				"scale":               scale,
			},
			"aggregation":   "concat",
			"min_responses": request.WorkerCount,
			"timeout_ms":    30000,
		})
		if err != nil {
			return marshal(map[string]any{"error": err.Error()})
		}
		scatterCoordMs := host.NowMs() - scatterStart
		l.TotalCoordMs += scatterCoordMs
		totalCollectiveOps++

		scatterElements := 0
		scatterChecksum := 0.0
		if err := accumulateScatterStats(
			scatterResponse["shard_responses"],
			leaderNodeID, remoteNodesWithWork,
			&totalWorkerLatencyMs, &totalWorkerResponses, &maxWorkerLatencyMs,
			&scatterElements, &scatterChecksum, &totalErrors,
		); err != nil {
			return marshal(map[string]any{"error": err.Error()})
		}
		totalElements += scatterElements

		// ── Phase 3: MPI_Reduce ────────────────────────────────────────────────
		// Leader queries each worker's partial_sum and reduces to a global total.
		// Result is available ONLY at the leader (root).
		reduceStart := host.NowMs()
		reduceResponse, err := host.ReduceShardGroup(map[string]any{
			"group_id":      groupID,
			"message_type":  "partial_reduce",
			"map_function":  map[string]any{"round": round},
			"target":        "partial_sum",
			"reduction":     "sum",
			"min_responses": request.WorkerCount,
			"timeout_ms":    30000,
		})
		if err != nil {
			return marshal(map[string]any{"error": err.Error()})
		}
		reduceCoordMs := host.NowMs() - reduceStart
		l.TotalCoordMs += reduceCoordMs
		totalCollectiveOps++
		reducedTotal := anyToFloat(reduceResponse["result"], 0)
		finalReducedTotal = reducedTotal
		if err := accumulateShardStats(
			reduceResponse["shard_responses"],
			leaderNodeID, remoteNodesWithWork,
			&totalWorkerLatencyMs, &totalWorkerResponses, &maxWorkerLatencyMs, &totalErrors,
		); err != nil {
			return marshal(map[string]any{"error": err.Error()})
		}

		// ── Phase 4: MPI_Allreduce ─────────────────────────────────────────────
		// Same reduction, but the framework also broadcasts the final result back
		// to ALL workers as a message_type="event" message so every shard holds
		// the global total (handled by worker applyAllreduceEvent).
		allreduceStart := host.NowMs()
		allreduceResponse, err := host.AllReduceShardGroup(map[string]any{
			"group_id":      groupID,
			"message_type":  "partial_reduce",
			"map_function":  map[string]any{"round": round},
			"target":        "partial_sum",
			"reduction":     "sum",
			"min_responses": request.WorkerCount,
			"timeout_ms":    30000,
		})
		if err != nil {
			return marshal(map[string]any{"error": err.Error()})
		}
		allreduceCoordMs := host.NowMs() - allreduceStart
		l.TotalCoordMs += allreduceCoordMs
		totalCollectiveOps++
		allreducedTotal := anyToFloat(allreduceResponse["result"], 0)
		consistent := math.Abs(allreducedTotal-reducedTotal) < 1e-9
		allreduceConsistent = allreduceConsistent && consistent

		// ── Phase 5: MPI_Barrier ───────────────────────────────────────────────
		// All workers must acknowledge before the next round begins.
		barrierStart := host.NowMs()
		_, err = host.BarrierShardGroup(map[string]any{
			"group_id":   groupID,
			"barrier_id": fmt.Sprintf("barrier-round-%d", round),
			"round":      uint64(round),
			"min_acks":   request.WorkerCount,
			"timeout_ms": 30000,
		})
		if err != nil {
			return marshal(map[string]any{"error": err.Error()})
		}
		barrierCoordMs := host.NowMs() - barrierStart
		l.TotalCoordMs += barrierCoordMs
		totalCollectiveOps++

		roundSummaries = append(roundSummaries, map[string]any{
			"round":              round + 1,
			"broadcast_coord_ms": broadcastCoordMs,
			"scatter_coord_ms":   scatterCoordMs,
			"reduce_coord_ms":    reduceCoordMs,
			"allreduce_coord_ms": allreduceCoordMs,
			"barrier_coord_ms":   barrierCoordMs,
			"elements":           scatterElements,
			"scatter_checksum":   scatterChecksum,
			"reduced_total":      reducedTotal,
			"allreduce_total":    allreducedTotal,
			"allreduce_ok":       consistent,
		})
	}

	if _, err := host.ApplicationMetricsAdd(l.ApplicationID(), map[string]any{
		"message_count": 1,
		"counter_metrics": map[string]any{
			"leader_messages":              request.Rounds*5 + 1,
			"leader_broadcast_operations":  request.Rounds,
			"leader_scatter_operations":    request.Rounds,
			"leader_reduce_operations":     request.Rounds,
			"leader_allreduce_operations":  request.Rounds,
			"leader_barrier_operations":    request.Rounds,
			"leader_collective_operations": request.Rounds * 5,
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
		metrics, err := host.ApplicationGetMetrics(l.ApplicationID(), nodeID)
		if err != nil {
			return marshal(map[string]any{"error": fmt.Sprintf("failed to collect final application metrics for %s: %v", nodeID, err)})
		}
		if address, ok := status["node_address"].(string); ok && address != "" {
			nodeAddresses[nodeID] = address
		}
		_ = startStatuses[nodeID]
		applyMetricsDelta(nodeMetrics, roleMetrics, nodeID, startMetrics[nodeID], metrics)
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
	totalCollectiveMetricOps := 0
	totalElementOps := 0
	totalComputeMs := 0
	totalCoordinationMs := 0
	workerNodeCount := 0
	actorCounts := make([]int, 0, len(nodeMetrics))
	for nodeID, metrics := range nodeMetrics {
		totalMessages += metrics["messages"]
		totalCollectiveMetricOps += metrics["collective_operations"]
		totalElementOps += metrics["element_operations"]
		totalComputeMs += metrics["compute_time_ms"]
		totalCoordinationMs += metrics["coordination_time_ms"]
		if nodeID != leaderNodeID && metrics["responses"] > 0 {
			workerNodeCount++
		}
		actorCounts = append(actorCounts, metrics["actors"])
	}
	if placementCount := len(remoteWorkerShardHostNodeIDs(leaderNodeID, shardActorIDs)); placementCount > workerNodeCount {
		workerNodeCount = placementCount
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
			"actors":                metrics["actors"],
			"leader_actors":         metrics["leader_actors"],
			"worker_actors":         metrics["worker_actors"],
			"messages":              metrics["messages"],
			"leader_messages":       metrics["leader_messages"],
			"worker_messages":       metrics["worker_messages"],
			"collective_operations": metrics["collective_operations"],
			"element_operations":    metrics["element_operations"],
			"compute_time_ms":       metrics["compute_time_ms"],
			"coordination_time_ms":  metrics["coordination_time_ms"],
			"avg_latency_ms":        averageLatency(metrics),
			"max_latency_ms":        metrics["max_latency_ms"],
			"errors":                metrics["errors"],
		}
	}
	roles := map[string]any{}
	for role, metrics := range roleMetrics {
		roles[role] = map[string]any{
			"actors":                metrics["actors"],
			"messages":              metrics["messages"],
			"collective_operations": metrics["collective_operations"],
			"element_operations":    metrics["element_operations"],
			"compute_time_ms":       metrics["compute_time_ms"],
			"coordination_time_ms":  metrics["coordination_time_ms"],
			"avg_latency_ms":        averageLatency(metrics),
			"max_latency_ms":        metrics["max_latency_ms"],
			"errors":                metrics["errors"],
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
		"status":                     "ok",
		"worker_count":               request.WorkerCount,
		"elements_per_worker":        request.ElementsPerWorker,
		"rounds":                     request.Rounds,
		"total_elements":             totalElements,
		"collective_rounds":          request.Rounds,
		"leader_node_id":             leaderNodeID,
		"node_addresses":             nodeAddresses,
		"shard_actor_ids":            shardActorIDs,
		"node_count":                 len(nodeMetrics),
		"worker_node_count":          workerNodeCount,
		"actor_count":                len(shardActorIDs) + 1,
		"message_count":              totalMessages,
		"collective_operation_count": totalCollectiveMetricOps,
		"element_operation_count":    totalElementOps,
		"compute_time_ms":            totalComputeMs,
		"coordination_time_ms":       totalCoordinationMs,
		"total_time_ms":              totalComputeMs + totalCoordinationMs,
		"granularity_ratio":          ratio(totalComputeMs, totalCoordinationMs),
		"avg_worker_latency_ms":      avgWorkerLatency,
		"max_worker_latency_ms":      maxWorkerLatencyMs,
		"error_count":                totalErrors,
		"remote_nodes_with_work":     remoteNodes,
		"actor_distribution_skew":    actorDistributionSkew,
		"reduced_total":              finalReducedTotal,
		"allreduce_consistent":       allreduceConsistent,
		"total_collective_ops":       totalCollectiveOps,
		"results":                    roundSummaries,
		"nodes":                      nodes,
		"roles":                      roles,
	})
}

// ---------------------------------------------------------------------------
// Worker handlers
// ---------------------------------------------------------------------------

func (w *WorkerActor) applyBroadcast(payloadJSON string) string {
	var payload map[string]any
	_ = json.Unmarshal([]byte(payloadJSON), &payload)
	round := anyToInt(payload["round"], 0)
	scale := anyToFloat(payload["scale"], 1.0)
	startMs := host.NowMs()
	w.LastRound = round
	w.BroadcastScale = scale
	latencyMs := int(host.NowMs() - startMs)
	if err := w.recordMetrics("broadcast", latencyMs, 0); err != nil {
		return marshal(map[string]any{"status": "error", "error": err.Error()})
	}
	return marshal(map[string]any{
		"status":     "ok",
		"actor_id":   w.ActorID(),
		"node_id":    actorNodeID(w.ActorID()),
		"round":      round,
		"scale":      scale,
		"latency_ms": latencyMs,
	})
}

func (w *WorkerActor) processScatterChunk(payloadJSON string) string {
	var payload map[string]any
	_ = json.Unmarshal([]byte(payloadJSON), &payload)
	round := anyToInt(payload["round"], 0)
	elements := anyToInt(payload["elements_per_worker"], 1024)
	baseValue := anyToFloat(payload["base_value"], 1.0)
	scale := anyToFloat(payload["scale"], w.BroadcastScale)
	startMs := host.NowMs()
	seed := workerSeed(w.ActorID()) + round*37
	localSum := 0.0
	checksum := 0.0
	for idx := 0; idx < elements; idx++ {
		value := baseValue + scale*float64(((seed+idx*17)%997)-498)/37.0
		localSum += value
		checksum += math.Mod(math.Abs(value)*float64((idx%11)+1), 97.0)
	}
	latencyMs := int(host.NowMs() - startMs)
	w.LastRound = round
	w.LastPartialSum = localSum
	if err := w.recordMetrics("scatter", latencyMs, elements); err != nil {
		return marshal(map[string]any{"status": "error", "error": err.Error()})
	}
	return marshal(map[string]any{
		"status":             "ok",
		"actor_id":           w.ActorID(),
		"node_id":            actorNodeID(w.ActorID()),
		"round":              round,
		"elements_processed": elements,
		"partial_sum":        localSum,
		"scatter_checksum":   checksum,
		"latency_ms":         latencyMs,
	})
}

func (w *WorkerActor) partialReduce(payloadJSON string) string {
	var payload map[string]any
	_ = json.Unmarshal([]byte(payloadJSON), &payload)
	round := anyToInt(payload["round"], 0)
	startMs := host.NowMs()
	partial := w.LastPartialSum
	if w.LastRound != round {
		partial = 0
	}
	latencyMs := int(host.NowMs() - startMs)
	if err := w.recordMetrics("reduce", latencyMs, 0); err != nil {
		return marshal(map[string]any{"status": "error", "error": err.Error()})
	}
	return marshal(map[string]any{
		"status":      "ok",
		"actor_id":    w.ActorID(),
		"node_id":     actorNodeID(w.ActorID()),
		"round":       round,
		"partial_sum": partial,
		"latency_ms":  latencyMs,
	})
}

// applyAllreduce handles a direct "apply_allreduce" message (legacy / manual path).
func (w *WorkerActor) applyAllreduce(payloadJSON string) string {
	var payload map[string]any
	_ = json.Unmarshal([]byte(payloadJSON), &payload)
	round := anyToInt(payload["round"], 0)
	reduced := anyToFloat(payload["reduced_sum"], 0)
	startMs := host.NowMs()
	w.LastRound = round
	w.LastReducedSum = reduced
	latencyMs := int(host.NowMs() - startMs)
	if err := w.recordMetrics("allreduce", latencyMs, 0); err != nil {
		return marshal(map[string]any{"status": "error", "error": err.Error()})
	}
	return marshal(map[string]any{
		"status":          "ok",
		"actor_id":        w.ActorID(),
		"node_id":         actorNodeID(w.ActorID()),
		"round":           round,
		"reduced_sum":     reduced,
		"latency_ms":      latencyMs,
		"consistent":      math.Abs(w.LastReducedSum-reduced) < 1e-9,
		"broadcast_scale": w.BroadcastScale,
	})
}

// applyAllreduceEvent handles the "event" message that AllReduceShardGroup
// broadcasts back to ALL workers after computing the global reduction.
// The payload is the raw JSON-encoded reduced value (e.g. 42.5).
func (w *WorkerActor) applyAllreduceEvent(payloadJSON string) string {
	// AllReduceShardGroup broadcasts the reduced Message back with
	// message_type="event".  The payload bytes contain the JSON-encoded
	// reduced value produced by reduce_values() — a plain JSON number.
	var result float64
	if err := json.Unmarshal([]byte(payloadJSON), &result); err != nil {
		// Fallback: value might be wrapped in {"result": X} or {"value": X}.
		var obj map[string]any
		if err2 := json.Unmarshal([]byte(payloadJSON), &obj); err2 == nil {
			result = anyToFloat(obj["result"], anyToFloat(obj["value"], 0))
		}
	}
	startMs := host.NowMs()
	w.LastReducedSum = result
	latencyMs := int(host.NowMs() - startMs)
	if err := w.recordMetrics("allreduce", latencyMs, 0); err != nil {
		return marshal(map[string]any{"status": "error", "error": err.Error()})
	}
	return marshal(map[string]any{
		"status":      "ok",
		"actor_id":    w.ActorID(),
		"node_id":     actorNodeID(w.ActorID()),
		"reduced_sum": result,
		"latency_ms":  latencyMs,
	})
}

func (w *WorkerActor) recordMetrics(op string, latencyMs int, elements int) error {
	counterMetrics := map[string]any{
		"worker_messages":              1,
		"worker_collective_operations": 1,
	}
	latencyTotals := map[string]any{
		"worker": latencyMs,
	}
	latencyMax := map[string]any{
		"worker": latencyMs,
	}
	latencySamples := map[string]any{
		"worker": 1,
	}
	switch op {
	case "broadcast":
		counterMetrics["worker_broadcast_operations"] = 1
		latencyTotals["worker.compute"] = 0
		latencyTotals["worker.coordination"] = latencyMs
		latencyMax["worker.compute"] = 0
		latencyMax["worker.coordination"] = latencyMs
		latencySamples["worker.compute"] = 1
		latencySamples["worker.coordination"] = 1
	case "scatter":
		counterMetrics["worker_scatter_operations"] = 1
		counterMetrics["worker_element_operations"] = elements
		latencyTotals["worker.compute"] = latencyMs
		latencyTotals["worker.coordination"] = 0
		latencyMax["worker.compute"] = latencyMs
		latencyMax["worker.coordination"] = 0
		latencySamples["worker.compute"] = 1
		latencySamples["worker.coordination"] = 1
	case "reduce":
		counterMetrics["worker_reduce_operations"] = 1
		latencyTotals["worker.compute"] = latencyMs
		latencyTotals["worker.coordination"] = 0
		latencyMax["worker.compute"] = latencyMs
		latencyMax["worker.coordination"] = 0
		latencySamples["worker.compute"] = 1
		latencySamples["worker.coordination"] = 1
	case "allreduce":
		counterMetrics["worker_allreduce_operations"] = 1
		latencyTotals["worker.compute"] = 0
		latencyTotals["worker.coordination"] = latencyMs
		latencyMax["worker.compute"] = 0
		latencyMax["worker.coordination"] = latencyMs
		latencySamples["worker.compute"] = 1
		latencySamples["worker.coordination"] = 1
	}
	_, err := host.ApplicationMetricsAdd(w.ApplicationID(), map[string]any{
		"message_count":     1,
		"counter_metrics":   counterMetrics,
		"latency_totals_ms": latencyTotals,
		"latency_max_ms":    latencyMax,
		"latency_samples":   latencySamples,
	})
	return err
}

// ---------------------------------------------------------------------------
// Stats helpers
// ---------------------------------------------------------------------------

// accumulateShardStats walks the shard_responses array returned by
// BroadcastShardGroup, ReduceShardGroup, and AllReduceShardGroup.
func accumulateShardStats(
	rawShards any,
	leaderNodeID string,
	remoteNodesWithWork map[string]bool,
	totalWorkerLatencyMs *uint64,
	totalWorkerResponses *uint64,
	maxWorkerLatencyMs *uint64,
	totalErrors *int,
) error {
	for _, shard := range anySlice(rawShards) {
		shardMap, _ := shard.(map[string]any)
		payloadMap := normalizeWorkerPayload(mapValue(shardMap, "payload"))
		if shardFailed(shardMap, payloadMap) {
			*totalErrors++
			continue
		}
		latencyMs := uint64(anyToInt(payloadMap["latency_ms"], anyToInt(shardMap["latency_ms"], 0)))
		*totalWorkerLatencyMs += latencyMs
		*totalWorkerResponses++
		if latencyMs > *maxWorkerLatencyMs {
			*maxWorkerLatencyMs = latencyMs
		}
		nodeID := stringValue(payloadMap["node_id"])
		if nodeID == "" {
			nodeID = actorNodeID(stringValue(payloadMap["actor_id"]))
		}
		if nodeID == "" {
			nodeID = actorNodeID(stringValue(shardMap["shard_actor_id"]))
		}
		if nodeID != "" && nodeID != leaderNodeID {
			remoteNodesWithWork[nodeID] = true
		}
	}
	return nil
}

// accumulateScatterStats walks the shard_responses from ScatterGather and
// additionally extracts per-shard element counts and checksum values.
func accumulateScatterStats(
	rawShards any,
	leaderNodeID string,
	remoteNodesWithWork map[string]bool,
	totalWorkerLatencyMs *uint64,
	totalWorkerResponses *uint64,
	maxWorkerLatencyMs *uint64,
	totalElements *int,
	checksum *float64,
	totalErrors *int,
) error {
	for _, shard := range anySlice(rawShards) {
		shardMap, _ := shard.(map[string]any)
		payloadMap := normalizeWorkerPayload(mapValue(shardMap, "payload"))
		if shardFailed(shardMap, payloadMap) {
			*totalErrors++
			continue
		}
		latencyMs := uint64(anyToInt(payloadMap["latency_ms"], anyToInt(shardMap["latency_ms"], 0)))
		*totalWorkerLatencyMs += latencyMs
		*totalWorkerResponses++
		if latencyMs > *maxWorkerLatencyMs {
			*maxWorkerLatencyMs = latencyMs
		}
		*totalElements += anyToInt(payloadMap["elements_processed"], 0)
		*checksum += anyToFloat(payloadMap["scatter_checksum"], 0)
		nodeID := stringValue(payloadMap["node_id"])
		if nodeID == "" {
			nodeID = actorNodeID(stringValue(payloadMap["actor_id"]))
		}
		if nodeID == "" {
			nodeID = actorNodeID(stringValue(shardMap["shard_actor_id"]))
		}
		if nodeID != "" && nodeID != leaderNodeID {
			remoteNodesWithWork[nodeID] = true
		}
	}
	return nil
}

func shardFailed(shardMap map[string]any, payloadMap map[string]any) bool {
	if errText := stringValue(shardMap["error"]); errText != "" {
		return true
	}
	if success, ok := shardMap["success"].(bool); ok {
		return !success
	}
	if errText := stringValue(payloadMap["error"]); errText != "" {
		return true
	}
	if status, ok := payloadMap["status"].(string); ok {
		return status != "ok"
	}
	return false
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
		if _, ok := current["latency_ms"]; ok {
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

// ---------------------------------------------------------------------------
// Node / role metrics helpers (unchanged)
// ---------------------------------------------------------------------------

func ensureNodeMetric(metrics map[string]map[string]int, nodeID string) map[string]int {
	node, ok := metrics[nodeID]
	if ok {
		return node
	}
	node = map[string]int{
		"actors":                0,
		"leader_actors":         0,
		"worker_actors":         0,
		"messages":              0,
		"leader_messages":       0,
		"worker_messages":       0,
		"collective_operations": 0,
		"element_operations":    0,
		"compute_time_ms":       0,
		"coordination_time_ms":  0,
		"total_latency_ms":      0,
		"max_latency_ms":        0,
		"responses":             0,
		"errors":                0,
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
		"actors":                0,
		"messages":              0,
		"collective_operations": 0,
		"element_operations":    0,
		"compute_time_ms":       0,
		"coordination_time_ms":  0,
		"total_latency_ms":      0,
		"max_latency_ms":        0,
		"responses":             0,
		"errors":                0,
	}
	metrics[role] = entry
	return entry
}

func applyMetricsDelta(
	nodeMetrics map[string]map[string]int,
	roleMetrics map[string]map[string]int,
	nodeID string,
	startMetrics map[string]any,
	endMetrics map[string]any,
) {
	counterDelta := saturatingMapDelta(metricsMap(endMetrics, "counter_metrics"), metricsMap(startMetrics, "counter_metrics"))
	latencyTotalsDelta := saturatingMapDelta(metricsMap(endMetrics, "latency_totals_ms"), metricsMap(startMetrics, "latency_totals_ms"))
	latencyMaxEnd := metricsMap(endMetrics, "latency_max_ms")
	latencyMaxStart := metricsMap(startMetrics, "latency_max_ms")
	latencySamplesDelta := saturatingMapDelta(metricsMap(endMetrics, "latency_samples"), metricsMap(startMetrics, "latency_samples"))
	messageDelta := maxInt(anyToInt(endMetrics["message_count"], 0)-anyToInt(startMetrics["message_count"], 0), 0)
	errorDelta := maxInt(anyToInt(endMetrics["error_count"], 0)-anyToInt(startMetrics["error_count"], 0), 0)
	node := ensureNodeMetric(nodeMetrics, nodeID)
	node["messages"] += messageDelta
	node["leader_messages"] += counterDelta["leader_messages"]
	node["worker_messages"] += counterDelta["worker_messages"]
	node["collective_operations"] += counterDelta["leader_collective_operations"] + counterDelta["worker_collective_operations"]
	node["element_operations"] += counterDelta["worker_element_operations"]
	node["compute_time_ms"] += latencyTotalsDelta["worker.compute"] + latencyTotalsDelta["leader.compute"]
	node["coordination_time_ms"] += latencyTotalsDelta["worker.coordination"] + latencyTotalsDelta["leader.coordination"]
	node["total_latency_ms"] += latencyTotalsDelta["worker"] + latencyTotalsDelta["leader"]
	node["max_latency_ms"] = maxInt(node["max_latency_ms"], latencyMaxEnd["worker"], latencyMaxStart["worker"], latencyMaxEnd["leader"], latencyMaxStart["leader"])
	node["responses"] += latencySamplesDelta["worker"] + latencySamplesDelta["leader"]
	node["errors"] += errorDelta

	leader := ensureRoleMetric(roleMetrics, "leader")
	leader["messages"] += counterDelta["leader_messages"]
	leader["collective_operations"] += counterDelta["leader_collective_operations"]
	leader["compute_time_ms"] += latencyTotalsDelta["leader.compute"]
	leader["coordination_time_ms"] += latencyTotalsDelta["leader.coordination"]
	leader["total_latency_ms"] += latencyTotalsDelta["leader"]
	leader["max_latency_ms"] = maxInt(leader["max_latency_ms"], latencyMaxEnd["leader"], latencyMaxStart["leader"])
	leader["responses"] += latencySamplesDelta["leader"]

	worker := ensureRoleMetric(roleMetrics, "worker")
	worker["messages"] += counterDelta["worker_messages"]
	worker["collective_operations"] += counterDelta["worker_collective_operations"]
	worker["element_operations"] += counterDelta["worker_element_operations"]
	worker["compute_time_ms"] += latencyTotalsDelta["worker.compute"]
	worker["coordination_time_ms"] += latencyTotalsDelta["worker.coordination"]
	worker["total_latency_ms"] += latencyTotalsDelta["worker"]
	worker["max_latency_ms"] = maxInt(worker["max_latency_ms"], latencyMaxEnd["worker"], latencyMaxStart["worker"])
	worker["responses"] += latencySamplesDelta["worker"]
	worker["errors"] += errorDelta
}

func metricsMap(metrics map[string]any, field string) map[string]int {
	value, _ := metrics[field].(map[string]any)
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

// remoteWorkerShardHostNodeIDs returns sorted unique node IDs that host worker shards.
// The leader node and synthetic "local" placement are excluded.
func remoteWorkerShardHostNodeIDs(leaderNodeID string, shardActorIDs []string) []string {
	seen := map[string]struct{}{}
	for _, actorID := range shardActorIDs {
		nodeID := actorNodeID(actorID)
		if nodeID == "" || nodeID == "local" || nodeID == leaderNodeID {
			continue
		}
		seen[nodeID] = struct{}{}
	}
	out := make([]string, 0, len(seen))
	for nodeID := range seen {
		out = append(out, nodeID)
	}
	sort.Strings(out)
	return out
}

func averageLatency(metric map[string]int) float64 {
	if metric["responses"] <= 0 {
		return 0.0
	}
	return float64(metric["total_latency_ms"]) / float64(metric["responses"])
}

// ---------------------------------------------------------------------------
// Generic helpers
// ---------------------------------------------------------------------------

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

func mapValue(value any, key string) any {
	if m, ok := value.(map[string]any); ok {
		return m[key]
	}
	return nil
}

func stringValue(value any) string {
	if s, ok := value.(string); ok {
		return s
	}
	return ""
}

func anyToInt(value any, defaultValue int) int {
	switch v := value.(type) {
	case int:
		return v
	case int32:
		return int(v)
	case int64:
		return int(v)
	case float64:
		return int(v)
	case string:
		if parsed, err := strconv.Atoi(v); err == nil {
			return parsed
		}
	}
	return defaultValue
}

func anyToFloat(value any, defaultValue float64) float64 {
	switch v := value.(type) {
	case float64:
		return v
	case int:
		return float64(v)
	case int32:
		return float64(v)
	case int64:
		return float64(v)
	case string:
		if parsed, err := strconv.ParseFloat(v, 64); err == nil {
			return parsed
		}
	}
	return defaultValue
}

func atoiDefault(raw string, fallback int) int {
	if raw == "" {
		return fallback
	}
	value, err := strconv.Atoi(raw)
	if err != nil {
		return fallback
	}
	return value
}

func marshal(value any) string {
	bytes, err := json.Marshal(value)
	if err != nil {
		return fmt.Sprintf(`{"status":"error","error":%q}`, err.Error())
	}
	return string(bytes)
}

func actorNodeID(actorID string) string {
	parts := strings.Split(actorID, "@")
	if len(parts) < 2 {
		return ""
	}
	return parts[len(parts)-1]
}

func workerSeed(actorID string) int {
	seed := 17
	for _, r := range actorID {
		seed = (seed*31 + int(r)) % 104729
	}
	return seed
}

func ratio(computeMs, coordMs int) float64 {
	if coordMs <= 0 {
		if computeMs <= 0 {
			return 0
		}
		return float64(computeMs)
	}
	return float64(computeMs) / float64(coordMs)
}

func maxInt(values ...int) int {
	if len(values) == 0 {
		return 0
	}
	maxValue := values[0]
	for _, value := range values[1:] {
		if value > maxValue {
			maxValue = value
		}
	}
	return maxValue
}

func init() {
	router := plexspaces.NewActorRouter()
	router.Route("leader", NewLeaderActor)
	router.Route("worker", NewWorkerActor)
	plexspaces.Register(router)
}

func main() {}
