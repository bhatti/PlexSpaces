package main

import (
	"encoding/json"
	"fmt"
	"math"
	"strconv"
	"strings"

	"github.com/bhatti/PlexSpaces/sdks/go/plexspaces"
)

var host = plexspaces.NewHost()

type initConfig struct {
	ActorID string            `json:"actor_id"`
	Args    map[string]string `json:"args"`
}

type LeaderActor struct {
	plexspaces.BaseActor
	InputDim       int         `json:"input_dim"`
	HiddenDim      int         `json:"hidden_dim"`
	LearningRate   float64     `json:"learning_rate"`
	Iteration      int         `json:"iteration"`
	NumWorkers     int         `json:"num_workers"`
	BatchSize      int         `json:"batch_size"`
	W1             [][]float64 `json:"w1"`
	W2             []float64   `json:"w2"`
	TotalCoordMs   uint64      `json:"total_coord_ms"`
	TotalComputeMs uint64      `json:"total_compute_ms"`
}

type WorkerActor struct {
	plexspaces.BaseActor
	BatchSize int `json:"batch_size"`
}

func NewLeaderActor() plexspaces.Actor {
	a := &LeaderActor{}
	a.SetSelf(a)
	return a
}

func NewWorkerActor() plexspaces.Actor {
	a := &WorkerActor{BatchSize: 256}
	a.SetSelf(a)
	return a
}

func (l *LeaderActor) Init(configJSON string) string {
	var config initConfig
	if err := json.Unmarshal([]byte(configJSON), &config); err != nil {
		return "ERROR: " + err.Error()
	}
	l.SetRuntimeMetadata(config.ActorID)
	l.InputDim = atoiDefault(config.Args["input_dim"], 100)
	l.HiddenDim = atoiDefault(config.Args["hidden_dim"], 64)
	l.NumWorkers = atoiDefault(config.Args["num_workers"], 4)
	l.BatchSize = atoiDefault(config.Args["batch_size"], 256)
	l.LearningRate = atofDefault(config.Args["learning_rate"], 0.01)
	l.Iteration = 0
	l.TotalCoordMs = 0
	l.TotalComputeMs = 0
	l.W1 = make([][]float64, l.HiddenDim)
	for i := 0; i < l.HiddenDim; i++ {
		row := make([]float64, l.InputDim)
		for j := 0; j < l.InputDim; j++ {
			row[j] = float64(((i*7+j*13+42)%1000)-500) / (500.0 * math.Sqrt(float64(l.InputDim)))
		}
		l.W1[i] = row
	}
	l.W2 = make([]float64, l.HiddenDim)
	for i := 0; i < l.HiddenDim; i++ {
		l.W2[i] = float64(((i*11+7)%1000)-500) / (500.0 * math.Sqrt(float64(l.HiddenDim)))
	}
	return ""
}

func (l *LeaderActor) Handle(fromActor, msgType, payloadJSON string) string {
	if msgType != "train" {
		return marshal(map[string]any{"error": "unknown_op", "op": msgType})
	}
	return l.train(payloadJSON)
}

func (w *WorkerActor) Init(configJSON string) string {
	var config initConfig
	if err := json.Unmarshal([]byte(configJSON), &config); err != nil {
		return "ERROR: " + err.Error()
	}
	w.SetRuntimeMetadata(config.ActorID)
	w.BatchSize = atoiDefault(config.Args["batch_size"], 256)
	return ""
}

func (w *WorkerActor) Handle(fromActor, msgType, payloadJSON string) string {
	if msgType != "compute_gradient" {
		return marshal(map[string]any{"error": "unknown_op", "op": msgType})
	}
	return w.computeGradient(payloadJSON)
}

func (l *LeaderActor) train(payloadJSON string) string {
	var payload map[string]any
	_ = json.Unmarshal([]byte(payloadJSON), &payload)
	iterations := anyToInt(payload["iterations"], 10)
	groupID := fmt.Sprintf("parameter-server-go-%d", host.NowMs())
	group, err := host.CreateShardGroup(map[string]any{
		"group_id":           groupID,
		"actor_type":         "worker",
		"shard_count":        l.NumWorkers,
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

	startMetrics := map[string]map[string]any{}
	nodeAddresses := map[string]string{}
	for _, nodeID := range participantNodeIDs {
		status, err := host.ApplicationGetStatus(l.ApplicationID(), nodeID)
		if err != nil {
			return marshal(map[string]any{"error": fmt.Sprintf("failed to capture application status for %s: %v", nodeID, err)})
		}
		if address, ok := status["node_address"].(string); ok && address != "" {
			nodeAddresses[nodeID] = address
		}
		metrics, err := host.ApplicationGetMetrics(l.ApplicationID(), nodeID)
		if err != nil {
			return marshal(map[string]any{"error": fmt.Sprintf("failed to capture application metrics for %s: %v", nodeID, err)})
		}
		startMetrics[nodeID] = metrics
	}

	results := make([]map[string]any, 0, iterations)
	totalSamples := 0
	totalErrors := 0
	totalWorkerLatencyMs := uint64(0)
	totalWorkerResponses := uint64(0)
	maxWorkerLatencyMs := uint64(0)
	remoteNodesWithWork := map[string]bool{}

	for i := 0; i < iterations; i++ {
		coordStart := host.NowMs()
		response, err := host.ScatterGather(map[string]any{
			"group_id": groupID,
			"query": map[string]any{
				"op":         "compute_gradient",
				"weights":    map[string]any{"w1": l.W1, "w2": l.W2},
				"input_dim":  l.InputDim,
				"hidden_dim": l.HiddenDim,
			},
			"aggregation":   "concat",
			"min_responses": l.NumWorkers,
			"timeout_ms":    30000,
		})
		if err != nil {
			return marshal(map[string]any{"error": err.Error()})
		}
		coordMs := host.NowMs() - coordStart

		shards := anySlice(response["shard_responses"])
		gradients := make([]map[string]any, 0, len(shards))
		iterationErrors := 0
		iterationResponses := 0
		iterationLatencyMs := uint64(0)
		iterationMaxLatencyMs := uint64(0)
		iterationSamples := 0
		for _, shard := range shards {
			shardMap, _ := shard.(map[string]any)
			payloadMap := normalizeWorkerPayload(mapValue(shardMap, "payload"))
			if status, _ := payloadMap["status"].(string); status == "ok" {
				if gradientsMap, ok := payloadMap["gradients"].(map[string]any); ok {
					gradients = append(gradients, gradientsMap)
				}
				samples := anyToInt(payloadMap["samples_processed"], anyToInt(payloadMap["samples"], 0))
				latencyMs := uint64(anyToInt(payloadMap["latency_ms"], 0))
				totalSamples += samples
				iterationSamples += samples
				iterationResponses++
				totalWorkerResponses++
				iterationLatencyMs += latencyMs
				totalWorkerLatencyMs += latencyMs
				if latencyMs > iterationMaxLatencyMs {
					iterationMaxLatencyMs = latencyMs
				}
				if latencyMs > maxWorkerLatencyMs {
					maxWorkerLatencyMs = latencyMs
				}
				nodeID := stringValue(payloadMap["node_id"])
				if nodeID == "" {
					nodeID = actorNodeID(stringValue(payloadMap["actor_id"]))
				}
				if nodeID != "" && nodeID != leaderNodeID {
					remoteNodesWithWork[nodeID] = true
				}
			} else {
				iterationErrors++
				totalErrors++
			}
		}

		if len(gradients) == 0 {
			return marshal(map[string]any{"error": "no gradients returned"})
		}

		computeStart := host.NowMs()
		nWorkers := len(gradients)
		aggW1 := make([][]float64, l.HiddenDim)
		aggW2 := make([]float64, l.HiddenDim)
		for idx := range aggW1 {
			aggW1[idx] = make([]float64, l.InputDim)
		}
		for _, gradient := range gradients {
			dW1 := nestedFloat64Slice(gradient["d_w1"])
			dW2 := float64Slice(gradient["d_w2"])
			for rowIdx := 0; rowIdx < minInt(l.HiddenDim, len(dW1)); rowIdx++ {
				row := dW1[rowIdx]
				for colIdx := 0; colIdx < minInt(l.InputDim, len(row)); colIdx++ {
					aggW1[rowIdx][colIdx] += row[colIdx]
				}
				if rowIdx < len(dW2) {
					aggW2[rowIdx] += dW2[rowIdx]
				}
			}
		}
		for rowIdx := 0; rowIdx < l.HiddenDim; rowIdx++ {
			for colIdx := 0; colIdx < l.InputDim; colIdx++ {
				aggW1[rowIdx][colIdx] /= float64(nWorkers)
				l.W1[rowIdx][colIdx] -= l.LearningRate * aggW1[rowIdx][colIdx]
			}
			aggW2[rowIdx] /= float64(nWorkers)
			l.W2[rowIdx] -= l.LearningRate * aggW2[rowIdx]
		}
		computeMs := host.NowMs() - computeStart
		l.TotalCoordMs += coordMs
		l.TotalComputeMs += computeMs
		l.Iteration++

		avgLatency := 0.0
		if iterationResponses > 0 {
			avgLatency = float64(iterationLatencyMs) / float64(iterationResponses)
		}
		results = append(results, map[string]any{
			"iteration":         l.Iteration,
			"workers":           nWorkers,
			"coord_ms":          coordMs,
			"compute_ms":        computeMs,
			"samples_processed": iterationSamples,
			"responses":         iterationResponses,
			"errors":            iterationErrors,
			"avg_latency_ms":    avgLatency,
			"max_latency_ms":    iterationMaxLatencyMs,
		})
	}

	if _, err := host.ApplicationMetricsAdd(l.ApplicationID(), map[string]any{
		"message_count": 1,
		"counter_metrics": map[string]any{
			"leader_messages":     iterations + 1,
			"leader_runs":         1,
			"weight_update_count": iterations,
			"training_rounds":     iterations,
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
	totalGradientOps := 0
	totalComputeMs := 0
	totalCoordinationMs := 0
	workerNodeCount := 0
	actorCounts := make([]int, 0, len(nodeMetrics))
	for nodeID, metrics := range nodeMetrics {
		totalMessages += metrics["messages"]
		totalGradientOps += metrics["gradient_operations"]
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
			"gradient_operations":  metrics["gradient_operations"],
			"samples_processed":    metrics["samples_processed"],
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
			"gradient_operations":  metrics["gradient_operations"],
			"samples_processed":    metrics["samples_processed"],
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

	avgWorkerLatency := 0.0
	if totalWorkerResponses > 0 {
		avgWorkerLatency = float64(totalWorkerLatencyMs) / float64(totalWorkerResponses)
	}

	return marshal(map[string]any{
		"status":                   "ok",
		"param_count":              l.InputDim*l.HiddenDim + l.HiddenDim,
		"worker_count":             l.NumWorkers,
		"iterations":               iterations,
		"batch_size":               l.BatchSize,
		"training_rounds":          iterations,
		"leader_node_id":           leaderNodeID,
		"node_addresses":           nodeAddresses,
		"shard_actor_ids":          shardActorIDs,
		"node_count":               len(nodeMetrics),
		"worker_node_count":        workerNodeCount,
		"actor_count":              len(shardActorIDs) + 1,
		"message_count":            totalMessages,
		"gradient_operation_count": totalGradientOps,
		"samples_processed":        totalSamples,
		"weight_update_count":      iterations,
		"compute_time_ms":          totalComputeMs,
		"coordination_time_ms":     totalCoordinationMs,
		"total_time_ms":            totalComputeMs + totalCoordinationMs,
		"granularity_ratio":        ratio(totalComputeMs, totalCoordinationMs),
		"avg_worker_latency_ms":    avgWorkerLatency,
		"max_worker_latency_ms":    maxWorkerLatencyMs,
		"error_count":              totalErrors,
		"remote_nodes_with_work":   remoteNodes,
		"actor_distribution_skew":  actorDistributionSkew,
		"results":                  results,
		"nodes":                    nodes,
		"roles":                    roles,
	})
}

func (w *WorkerActor) computeGradient(payloadJSON string) string {
	var payload map[string]any
	_ = json.Unmarshal([]byte(payloadJSON), &payload)
	inputDim := anyToInt(payload["input_dim"], 100)
	hiddenDim := anyToInt(payload["hidden_dim"], 64)
	weights, _ := payload["weights"].(map[string]any)
	if weights == nil {
		return marshal(map[string]any{"status": "error", "error": "missing weights"})
	}
	if len(nestedFloat64Slice(weights["w1"])) == 0 || len(float64Slice(weights["w2"])) == 0 {
		return marshal(map[string]any{"status": "error", "error": "invalid weights"})
	}

	startMs := host.NowMs()
	dW1 := make([][]float64, hiddenDim)
	for i := 0; i < hiddenDim; i++ {
		dW1[i] = make([]float64, inputDim)
	}
	dW2 := make([]float64, hiddenDim)
	seed := workerSeed(w.ActorID())
	for sampleIdx := 0; sampleIdx < w.BatchSize; sampleIdx++ {
		for i := 0; i < hiddenDim; i++ {
			scale := float64(((seed + sampleIdx + i*17) % 1000)) / 1000.0
			scale -= 0.5
			dW2[i] += scale
			for j := 0; j < inputDim; j++ {
				dW1[i][j] += scale * (float64((seed+j*13)%1000)/1000.0 - 0.5)
			}
		}
	}
	for i := 0; i < hiddenDim; i++ {
		for j := 0; j < inputDim; j++ {
			dW1[i][j] /= float64(w.BatchSize)
		}
		dW2[i] /= float64(w.BatchSize)
	}
	latencyMs := int(host.NowMs() - startMs)

	gradientChecksum := uint64(0)
	gradientScale := 0
	for _, row := range dW1 {
		for _, value := range row {
			scaled := int(math.Round(value * 1_000_000))
			gradientChecksum += uint64(int64(scaled))
			gradientScale += absInt(scaled)
		}
	}
	for _, value := range dW2 {
		scaled := int(math.Round(value * 1_000_000))
		gradientChecksum += uint64(int64(scaled))
		gradientScale += absInt(scaled)
	}

	if _, err := host.ApplicationMetricsAdd(w.ApplicationID(), map[string]any{
		"message_count": 1,
		"counter_metrics": map[string]any{
			"worker_messages":          1,
			"gradient_operation_count": 1,
			"samples_processed":        w.BatchSize,
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
		return marshal(map[string]any{"status": "error", "error": fmt.Sprintf("worker compute metrics update: %v", err)})
	}

	return marshal(map[string]any{
		"status":            "ok",
		"actor_id":          w.ActorID(),
		"node_id":           actorNodeID(w.ActorID()),
		"worker_id":         w.ActorID(),
		"samples":           w.BatchSize,
		"samples_processed": w.BatchSize,
		"latency_ms":        latencyMs,
		"gradient_checksum": gradientChecksum,
		"gradient_scale":    gradientScale,
		"gradients":         map[string]any{"d_w1": dW1, "d_w2": dW2},
	})
}

func actorNodeID(actorID string) string {
	parts := strings.Split(actorID, "@")
	if len(parts) > 1 {
		return parts[len(parts)-1]
	}
	return "local"
}

func workerSeed(actorID string) int {
	total := 0
	for _, ch := range actorID {
		total += int(ch)
	}
	return total % 10000
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
		"gradient_operations":  0,
		"samples_processed":    0,
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
		"gradient_operations":  0,
		"samples_processed":    0,
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
	node["gradient_operations"] += counterDelta["gradient_operation_count"]
	node["samples_processed"] += counterDelta["samples_processed"]
	node["compute_time_ms"] += latencyTotalsDelta["worker.compute"] + latencyTotalsDelta["leader.compute"]
	node["coordination_time_ms"] += latencyTotalsDelta["worker.coordination"] + latencyTotalsDelta["leader.coordination"]
	node["total_latency_ms"] += latencyTotalsDelta["worker"] + latencyTotalsDelta["leader"]
	node["max_latency_ms"] = maxInt(node["max_latency_ms"], latencyMaxEnd["worker"], latencyMaxStart["worker"], latencyMaxEnd["leader"], latencyMaxStart["leader"])
	node["responses"] += latencySamplesDelta["worker"]
	node["errors"] += errorDelta

	leader := ensureRoleMetric(roleMetrics, "leader")
	leader["messages"] += counterDelta["leader_messages"]
	leader["compute_time_ms"] += latencyTotalsDelta["leader.compute"]
	leader["coordination_time_ms"] += latencyTotalsDelta["leader.coordination"]
	leader["total_latency_ms"] += latencyTotalsDelta["leader"]
	leader["max_latency_ms"] = maxInt(leader["max_latency_ms"], latencyMaxEnd["leader"], latencyMaxStart["leader"])
	leader["responses"] += latencySamplesDelta["leader"]

	worker := ensureRoleMetric(roleMetrics, "worker")
	worker["messages"] += counterDelta["worker_messages"]
	worker["gradient_operations"] += counterDelta["gradient_operation_count"]
	worker["samples_processed"] += counterDelta["samples_processed"]
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

func nestedFloat64Slice(value any) [][]float64 {
	rows := anySlice(value)
	result := make([][]float64, 0, len(rows))
	for _, row := range rows {
		result = append(result, float64Slice(row))
	}
	return result
}

func float64Slice(value any) []float64 {
	items := anySlice(value)
	result := make([]float64, 0, len(items))
	for _, item := range items {
		switch v := item.(type) {
		case float64:
			result = append(result, v)
		case int:
			result = append(result, float64(v))
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

func atoiDefault(value string, fallback int) int {
	if parsed, err := strconv.Atoi(value); err == nil {
		return parsed
	}
	return fallback
}

func atofDefault(value string, fallback float64) float64 {
	if parsed, err := strconv.ParseFloat(value, 64); err == nil {
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

func minInt(a, b int) int {
	if a < b {
		return a
	}
	return b
}

func absInt(value int) int {
	if value < 0 {
		return -value
	}
	return value
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
