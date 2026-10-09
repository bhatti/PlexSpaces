// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Federated Learning Simulation - Go WASM
//
// Client actors train locally → push gradients → central aggregator
// with FedAvg + differential privacy Gaussian noise → broadcast
// updated weights → convergence tracking.
//
// Logistic regression on synthetic binary classification data.
// All math in pure Go — no external ML packages.

package main

import (
	"encoding/json"
	"fmt"
	"math"

	"github.com/bhatti/PlexSpaces/sdks/go/plexspaces"
)

var pxHost = plexspaces.NewHost()

// ─── Types ───────────────────────────────────────────────────────────────────

// TinyGo WASM constraint: keep numFeatures ≤ 10. With numFeatures=100, the
// json.Marshal call in OnRun traps in TinyGo's reflectlite.Value.IsNil path
// (strconv.ryuDigits32 → WASM unreachable). Increasing sample count and epoch
// count is the correct way to scale up compute work instead.
const numFeatures = 10

type Model struct {
	Weights []float64 `json:"weights"`
	Bias    float64   `json:"bias"`
}

type DataPoint struct {
	Features []float64 `json:"features"`
	Label    float64   `json:"label"`
}

type Gradients struct {
	WeightGrads []float64 `json:"weight_grads"`
	BiasGrad    float64   `json:"bias_grad"`
	Loss        float64   `json:"loss"`
	SampleCount int       `json:"sample_count"`
}

type RoundResult struct {
	Round      int     `json:"round"`
	AvgLoss    float64 `json:"avg_loss"`
	Accuracy   float64 `json:"accuracy"`
	GradNorm   float64 `json:"grad_norm"`
	ComputeMs  int64   `json:"compute_ms"`
	CoordMs    int64   `json:"coord_ms"`
}

// ─── Math Helpers ────────────────────────────────────────────────────────────

func sigmoid(x float64) float64 {
	if x > 500 {
		return 1.0
	}
	if x < -500 {
		return 0.0
	}
	return 1.0 / (1.0 + math.Exp(-x))
}

func predict(model *Model, features []float64) float64 {
	z := model.Bias
	for i := 0; i < len(features) && i < len(model.Weights); i++ {
		z += model.Weights[i] * features[i]
	}
	return sigmoid(z)
}

func binaryCrossEntropy(pred, label float64) float64 {
	eps := 1e-15
	pred = math.Max(eps, math.Min(1-eps, pred))
	return -(label*math.Log(pred) + (1-label)*math.Log(1-pred))
}

// Box-Muller transform for Gaussian noise
func gaussianNoise(seed int64) (float64, int64) {
	seed = (seed*1103515245 + 12345) & 0x7FFFFFFF
	u1 := float64(seed) / float64(0x7FFFFFFF)
	seed = (seed*1103515245 + 12345) & 0x7FFFFFFF
	u2 := float64(seed) / float64(0x7FFFFFFF)
	if u1 < 1e-10 {
		u1 = 1e-10
	}
	z := math.Sqrt(-2*math.Log(u1)) * math.Cos(2*math.Pi*u2)
	return z, seed
}

func vecNorm(v []float64) float64 {
	sum := 0.0
	for _, x := range v {
		sum += x * x
	}
	return math.Sqrt(sum)
}

func clipGradients(grads *Gradients, maxNorm float64) {
	norm := vecNorm(grads.WeightGrads)
	biasContrib := grads.BiasGrad * grads.BiasGrad
	norm = math.Sqrt(norm*norm + biasContrib)
	if norm > maxNorm {
		scale := maxNorm / norm
		for i := range grads.WeightGrads {
			grads.WeightGrads[i] *= scale
		}
		grads.BiasGrad *= scale
	}
}

// ─── Data Generation ────────────────────────────────────────────────────────

func generateData(count int, clientID int, seed int64) []DataPoint {
	data := make([]DataPoint, count)
	rng := seed + int64(clientID)*17
	// trueWeights is fixed for all data points — allocate once outside the loop
	trueWeights := make([]float64, numFeatures)
	for j := 0; j < numFeatures; j++ {
		sign := 1.0
		if j%2 == 1 {
			sign = -1.0
		}
		trueWeights[j] = sign * (0.1 + 0.8*float64(j%5)/4.0)
	}
	for i := 0; i < count; i++ {
		features := make([]float64, numFeatures)
		for j := 0; j < numFeatures; j++ {
			rng = (rng*1103515245 + 12345) & 0x7FFFFFFF
			features[j] = float64(rng)/float64(0x7FFFFFFF)*2 - 1 // [-1, 1]
			// shift distribution per client for non-IID effect
			features[j] += float64(clientID%3) * 0.3
		}
		z := 0.0
		for j := 0; j < numFeatures; j++ {
			z += features[j] * trueWeights[j]
		}
		label := 0.0
		if z > 0 {
			label = 1.0
		}
		// add noise (~5%)
		rng = (rng*1103515245 + 12345) & 0x7FFFFFFF
		if rng%20 == 0 {
			label = 1.0 - label
		}
		data[i] = DataPoint{Features: features, Label: label}
	}
	return data
}

func trainLocal(model *Model, data []DataPoint, lr float64, epochs int) Gradients {
	wGrads := make([]float64, numFeatures)
	var bGrad float64
	totalLoss := 0.0

	for e := 0; e < epochs; e++ {
		for j := range wGrads {
			wGrads[j] = 0
		}
		bGrad = 0
		totalLoss = 0

		for _, dp := range data {
			pred := predict(model, dp.Features)
			err := pred - dp.Label
			totalLoss += binaryCrossEntropy(pred, dp.Label)
			for j := 0; j < numFeatures; j++ {
				wGrads[j] += err * dp.Features[j]
			}
			bGrad += err
		}

		n := float64(len(data))
		for j := range wGrads {
			wGrads[j] /= n
			model.Weights[j] -= lr * wGrads[j]
		}
		bGrad /= n
		model.Bias -= lr * bGrad
	}

	avgLoss := totalLoss / float64(len(data))
	return Gradients{
		WeightGrads: wGrads,
		BiasGrad:    bGrad,
		Loss:        avgLoss,
		SampleCount: len(data),
	}
}

func computeAccuracy(model *Model, data []DataPoint) float64 {
	correct := 0
	for _, dp := range data {
		pred := predict(model, dp.Features)
		predictedLabel := 0.0
		if pred >= 0.5 {
			predictedLabel = 1.0
		}
		if predictedLabel == dp.Label {
			correct++
		}
	}
	return float64(correct) / float64(len(data))
}

// ─── Actors ──────────────────────────────────────────────────────────────────

type AggregatorActor struct {
	plexspaces.BaseActor
}

type ClientActor struct {
	plexspaces.BaseActor
	clientID int
	dataSize int
}

func (a *AggregatorActor) Handle(fromActor, msgType, payloadJSON string) string {
	var payload map[string]interface{}
	if err := json.Unmarshal([]byte(payloadJSON), &payload); err != nil {
		return marshal(map[string]interface{}{"error": "invalid JSON: " + err.Error()})
	}
	var result map[string]interface{}
	switch msgType {
	case "run":
		result = a.OnRun(payload)
	case "run_scaling_benchmark":
		result = a.OnRunScalingBenchmark(payload)
	case "run_weak_scaling":
		result = a.OnRunWeakScaling(payload)
	default:
		result = map[string]interface{}{"error": "unknown op: " + msgType}
	}
	return marshal(result)
}

func (a *AggregatorActor) OnRun(payload map[string]interface{}) map[string]interface{} {
	clientCount := intVal(payload["client_count"], 4)
	rounds := intVal(payload["rounds"], 20)
	// Large default sample count to produce measurable compute_time_ms in TinyGo WASM.
	// numFeatures is intentionally small (10) — scale work via samples and epochs.
	samplesPerClient := intVal(payload["samples_per_client"], 50000)
	learningRate := floatVal(payload["learning_rate"], 0.1)
	localEpochs := intVal(payload["local_epochs"], 20)
	dpEpsilon := floatVal(payload["dp_epsilon"], 1.0)
	gradClipNorm := floatVal(payload["grad_clip_norm"], 1.0)
	earlyStopThreshold := floatVal(payload["early_stop_threshold"], 0.001)

	coordStart := pxHost.NowMs()
	groupID := fmt.Sprintf("fl-clients-%d", pxHost.NowMs())
	group, err := pxHost.CreateShardGroup(plexspaces.CreateShardGroupRequest{
		GroupID:           groupID,
		ActorType:         "client",
		ShardCount:        clientCount,
		PartitionStrategy: "hash",
		RebalancePolicy:   "manual",
		Placement:         plexspaces.NodePlacement{Strategy: "from_registry"},
	})
	if err != nil {
		return map[string]interface{}{"status": "error", "error": fmt.Sprintf("failed to create shard group: %v", err)}
	}
	shardIDs := group.ShardActorIDs
	if len(shardIDs) == 0 {
		return map[string]interface{}{"status": "error", "error": "failed to create shard group"}
	}
	coordCreate := pxHost.NowMs() - coordStart

	// Initialize global model
	model := &Model{
		Weights: make([]float64, numFeatures),
		Bias:    0.0,
	}

	var totalComputeMs, totalCoordMs int64
	totalCoordMs = int64(coordCreate)
	var roundResults []RoundResult
	dpSeed := int64(pxHost.NowMs())
	prevLoss := math.MaxFloat64

	for round := 0; round < rounds; round++ {
		trainPayload := map[string]interface{}{
			"op":                "train_round",
			"weights":           model.Weights,
			"bias":              model.Bias,
			"learning_rate":     learningRate,
			"local_epochs":      localEpochs,
			"samples_per_client": samplesPerClient,
			"round":             round,
		}

		sgStart := pxHost.NowMs()
		sgResult, sgErr := pxHost.ScatterGather(plexspaces.ScatterGatherRequest{
			GroupID:   groupID,
			Query:     trainPayload,
			TimeoutMs: 30000,
		})
		sgElapsed := pxHost.NowMs() - sgStart
		totalCoordMs += int64(sgElapsed)
		if sgErr != nil {
			continue
		}

		// FedAvg: weighted average of gradients
		avgWeightGrads := make([]float64, numFeatures)
		var avgBiasGrad float64
		var totalSamples int
		var totalLoss float64
		roundCompute := int64(0)

		for _, resp := range sgResult.ShardResponses {
			result := unwrapPayload(resp)
			if _, hasErr := result["error"]; hasErr {
				continue
			}
			wg := floatSlice(result["weight_grads"])
			bg := floatVal(result["bias_grad"], 0)
			loss := floatVal(result["loss"], 0)
			sc := intVal(result["sample_count"], 0)
			cm := int64(intVal(result["compute_ms"], 0))
			roundCompute += cm

			for j := 0; j < numFeatures && j < len(wg); j++ {
				avgWeightGrads[j] += wg[j] * float64(sc)
			}
			avgBiasGrad += bg * float64(sc)
			totalSamples += sc
			totalLoss += loss * float64(sc)
		}

		if totalSamples > 0 {
			n := float64(totalSamples)
			for j := range avgWeightGrads {
				avgWeightGrads[j] /= n
			}
			avgBiasGrad /= n
			totalLoss /= n
		}

		// Clip aggregated gradients
		aggGrads := &Gradients{WeightGrads: avgWeightGrads, BiasGrad: avgBiasGrad}
		clipGradients(aggGrads, gradClipNorm)

		// Add differential privacy noise (Gaussian mechanism)
		dpScale := gradClipNorm * math.Sqrt(2*math.Log(1.25/0.001)) / dpEpsilon
		for j := range aggGrads.WeightGrads {
			var noise float64
			noise, dpSeed = gaussianNoise(dpSeed)
			aggGrads.WeightGrads[j] += noise * dpScale / float64(max(totalSamples, 1))
		}
		var biasNoise float64
		biasNoise, dpSeed = gaussianNoise(dpSeed)
		aggGrads.BiasGrad += biasNoise * dpScale / float64(max(totalSamples, 1))

		// Apply gradients to global model
		for j := 0; j < numFeatures; j++ {
			model.Weights[j] -= learningRate * aggGrads.WeightGrads[j]
		}
		model.Bias -= learningRate * aggGrads.BiasGrad

		totalComputeMs += roundCompute
		gradNorm := vecNorm(aggGrads.WeightGrads)

		// Compute accuracy on test data
		testData := generateData(500, 99, int64(round)*31)
		accuracy := computeAccuracy(model, testData)

		roundResults = append(roundResults, RoundResult{
			Round:     round,
			AvgLoss:   safeFloat(math.Round(totalLoss*10000) / 10000),
			Accuracy:  safeFloat(math.Round(accuracy*10000) / 10000),
			GradNorm:  safeFloat(math.Round(gradNorm*10000) / 10000),
			ComputeMs: roundCompute, CoordMs: int64(sgElapsed),
		})

		// Early stopping
		if math.Abs(totalLoss-prevLoss) < earlyStopThreshold && round > 2 {
			break
		}
		prevLoss = totalLoss
	}

	wallTime := totalComputeMs + totalCoordMs
	total := totalComputeMs + totalCoordMs
	if total == 0 {
		total = 1
	}
	granularity := 0.0
	if totalCoordMs > 0 {
		granularity = math.Round(float64(totalComputeMs)/float64(totalCoordMs)*10) / 10
	}

	finalAcc := 0.0
	finalLoss := 0.0
	if len(roundResults) > 0 {
		last := roundResults[len(roundResults)-1]
		finalAcc = last.Accuracy
		finalLoss = last.AvgLoss
	}

	pxHost.ApplicationMetricsAdd(a.ApplicationID(), map[string]any{
		"counter_metrics": map[string]any{
			"aggregator.compute":      totalComputeMs,
			"aggregator.coordination": totalCoordMs,
		},
	})

	return map[string]interface{}{
		"status": "ok", "client_count": clientCount, "rounds_completed": len(roundResults),
		"samples_per_client": samplesPerClient, "final_accuracy": safeFloat(finalAcc),
		"final_loss": safeFloat(finalLoss), "dp_epsilon": safeFloat(dpEpsilon),
		"wall_time_ms": wallTime, "compute_time_ms": totalComputeMs,
		"coordination_time_ms": totalCoordMs, "granularity_ratio": safeFloat(granularity),
		"node_count": 1, "actor_count": len(shardIDs) + 1, "error_count": 0,
		"round_history": roundResults,
	}
}

func (a *AggregatorActor) OnRunScalingBenchmark(payload map[string]interface{}) map[string]interface{} {
	clientCounts := intSlice(payload["client_counts"], []int{2, 4, 8, 16})
	rounds := intVal(payload["rounds"], 10)
	samplesPerClient := intVal(payload["samples_per_client"], 20000)
	warmupRounds := intVal(payload["warmup_rounds"], 1)
	benchmarkRounds := intVal(payload["benchmark_rounds"], 2)

	var results []map[string]interface{}
	var baselineWall int64

	for _, cc := range clientCounts {
		for w := 0; w < warmupRounds; w++ {
			a.OnRun(map[string]interface{}{"client_count": cc, "rounds": 2, "samples_per_client": 50})
		}
		var tw, tc, tco int64
		var tAcc float64
		for r := 0; r < benchmarkRounds; r++ {
			res := a.OnRun(map[string]interface{}{
				"client_count": cc, "rounds": rounds, "samples_per_client": samplesPerClient,
			})
			tw += int64(intVal(res["wall_time_ms"], 0))
			tc += int64(intVal(res["compute_time_ms"], 0))
			tco += int64(intVal(res["coordination_time_ms"], 0))
			tAcc += floatVal(res["final_accuracy"], 0)
		}
		aw := tw / int64(benchmarkRounds)
		ac := tc / int64(benchmarkRounds)
		aco := tco / int64(benchmarkRounds)
		avgAcc := tAcc / float64(benchmarkRounds)

		if baselineWall == 0 {
			baselineWall = aw
		}
		speedup := 1.0
		if aw > 0 {
			speedup = float64(baselineWall) / float64(aw)
		}
		eff := speedup / (float64(cc) / float64(clientCounts[0])) * 100
		gran := 0.0
		if aco > 0 {
			gran = math.Round(float64(ac)/float64(aco)*10) / 10
		}

		results = append(results, map[string]interface{}{
			"clients": cc, "wall_time_ms": aw, "compute_time_ms": ac,
			"coordination_time_ms": aco, "granularity_ratio": gran,
			"speedup": math.Round(speedup*100) / 100,
			"efficiency_pct": math.Round(eff*10) / 10,
			"final_accuracy": math.Round(avgAcc*10000) / 10000,
			"error_count": 0,
		})
	}
	return map[string]interface{}{"status": "ok", "rounds": rounds, "results": results}
}

func (a *AggregatorActor) OnRunWeakScaling(payload map[string]interface{}) map[string]interface{} {
	clientCounts := intSlice(payload["client_counts"], []int{2, 4, 8, 16})
	rounds := intVal(payload["rounds"], 10)
	samplesPerClient := intVal(payload["samples_per_client"], 20000)
	warmupRounds := intVal(payload["warmup_rounds"], 1)
	benchmarkRounds := intVal(payload["benchmark_rounds"], 2)

	var results []map[string]interface{}
	var baselineWall int64

	for _, cc := range clientCounts {
		for w := 0; w < warmupRounds; w++ {
			a.OnRun(map[string]interface{}{"client_count": cc, "rounds": 2, "samples_per_client": 50})
		}
		var tw, tc, tco int64
		var tAcc float64
		for r := 0; r < benchmarkRounds; r++ {
			res := a.OnRun(map[string]interface{}{
				"client_count": cc, "rounds": rounds, "samples_per_client": samplesPerClient,
			})
			tw += int64(intVal(res["wall_time_ms"], 0))
			tc += int64(intVal(res["compute_time_ms"], 0))
			tco += int64(intVal(res["coordination_time_ms"], 0))
			tAcc += floatVal(res["final_accuracy"], 0)
		}
		aw := tw / int64(benchmarkRounds)
		ac := tc / int64(benchmarkRounds)
		aco := tco / int64(benchmarkRounds)
		avgAcc := tAcc / float64(benchmarkRounds)

		if baselineWall == 0 {
			baselineWall = aw
		}
		eff := 100.0
		if baselineWall > 0 && aw > 0 {
			eff = float64(baselineWall) / float64(aw) * 100
		}
		gran := 0.0
		if aco > 0 {
			gran = math.Round(float64(ac)/float64(aco)*10) / 10
		}

		results = append(results, map[string]interface{}{
			"clients": cc, "total_samples": cc * samplesPerClient,
			"wall_time_ms": aw, "compute_time_ms": ac,
			"coordination_time_ms": aco, "granularity_ratio": gran,
			"efficiency_pct": math.Round(eff*10) / 10,
			"final_accuracy": math.Round(avgAcc*10000) / 10000,
			"error_count": 0,
		})
	}
	return map[string]interface{}{"status": "ok", "rounds": rounds, "samples_per_client": samplesPerClient, "results": results}
}

func (c *ClientActor) Handle(fromActor, msgType, payloadJSON string) string {
	var payload map[string]interface{}
	if err := json.Unmarshal([]byte(payloadJSON), &payload); err != nil {
		return marshal(map[string]interface{}{"error": "invalid JSON: " + err.Error()})
	}
	var result map[string]interface{}
	switch msgType {
	case "train_round":
		result = c.OnTrainRound(payload)
	default:
		result = map[string]interface{}{"error": "unknown op: " + msgType}
	}
	return marshal(result)
}

func (c *ClientActor) OnTrainRound(payload map[string]interface{}) map[string]interface{} {
	compStart := pxHost.NowMs()
	weights := floatSlice(payload["weights"])
	bias := floatVal(payload["bias"], 0)
	lr := floatVal(payload["learning_rate"], 0.1)
	localEpochs := intVal(payload["local_epochs"], 20)
	samplesPerClient := intVal(payload["samples_per_client"], 50000)
	round := intVal(payload["round"], 0)

	model := &Model{
		Weights: make([]float64, numFeatures),
		Bias:    bias,
	}
	for j := 0; j < numFeatures && j < len(weights); j++ {
		model.Weights[j] = weights[j]
	}

	data := generateData(samplesPerClient, c.clientID, int64(round)*13+int64(c.clientID)*7)
	grads := trainLocal(model, data, lr, localEpochs)

	computeMs := int64(pxHost.NowMs() - compStart)

	pxHost.ApplicationMetricsAdd(c.ApplicationID(), map[string]any{
		"counter_metrics": map[string]any{"client.compute": computeMs},
	})

	return map[string]interface{}{
		"weight_grads": grads.WeightGrads, "bias_grad": grads.BiasGrad,
		"loss": grads.Loss, "sample_count": grads.SampleCount,
		"compute_ms": computeMs, "client_id": c.clientID,
	}
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

func intVal(v interface{}, def int) int {
	switch val := v.(type) {
	case float64:
		return int(val)
	case int:
		return val
	case int64:
		return int(val)
	case json.Number:
		if i, err := val.Int64(); err == nil {
			return int(i)
		}
	}
	return def
}

func floatVal(v interface{}, def float64) float64 {
	switch val := v.(type) {
	case float64:
		return val
	case int:
		return float64(val)
	case int64:
		return float64(val)
	case json.Number:
		if f, err := val.Float64(); err == nil {
			return f
		}
	}
	return def
}

func floatSlice(v interface{}) []float64 {
	arr, ok := v.([]interface{})
	if !ok {
		return nil
	}
	result := make([]float64, 0, len(arr))
	for _, item := range arr {
		result = append(result, floatVal(item, 0))
	}
	return result
}

func intSlice(v interface{}, def []int) []int {
	arr, ok := v.([]interface{})
	if !ok {
		return def
	}
	result := make([]int, 0, len(arr))
	for _, item := range arr {
		result = append(result, intVal(item, 0))
	}
	return result
}

func strSlice(v interface{}) []string {
	arr, ok := v.([]interface{})
	if !ok {
		return nil
	}
	result := make([]string, 0, len(arr))
	for _, item := range arr {
		if s, ok := item.(string); ok {
			result = append(result, s)
		}
	}
	return result
}

func mapSlice(v interface{}) []map[string]interface{} {
	arr, ok := v.([]interface{})
	if !ok {
		return nil
	}
	result := make([]map[string]interface{}, 0, len(arr))
	for _, item := range arr {
		if m, ok := item.(map[string]interface{}); ok {
			result = append(result, m)
		}
	}
	return result
}

// TinyGo WASM constraint: json.Marshal traps (WASM unreachable) on NaN/Inf
// float64 values — TinyGo's reflectlite path panics instead of returning an
// error like standard Go. Always sanitize floats before marshaling.
func safeFloat(f float64) float64 {
	if math.IsNaN(f) || math.IsInf(f, 0) {
		return 0.0
	}
	return f
}

func marshal(v interface{}) string {
	b, _ := json.Marshal(v)
	return string(b)
}

func unwrapPayload(resp map[string]interface{}) map[string]interface{} {
	// Scatter-gather shard responses wrap actor output under "payload".
	if r, ok := resp["payload"].(map[string]interface{}); ok {
		return r
	}
	// Fallback: payload may be a JSON string; try to decode it.
	if s, ok := resp["payload"].(string); ok && s != "" {
		var m map[string]interface{}
		if err := json.Unmarshal([]byte(s), &m); err == nil {
			return m
		}
	}
	return resp
}

func max(a, b int) int {
	if a > b {
		return a
	}
	return b
}

// ─── Router ──────────────────────────────────────────────────────────────────

var clientCounter = 0

func NewAggregatorActor() plexspaces.Actor {
	a := &AggregatorActor{}
	a.SetSelf(a)
	return a
}

func NewClientActor() plexspaces.Actor {
	clientCounter++
	a := &ClientActor{clientID: clientCounter}
	a.SetSelf(a)
	return a
}

func init() {
	router := plexspaces.NewActorRouter()
	router.Route("aggregator", NewAggregatorActor)
	router.Route("client", NewClientActor)
	plexspaces.Register(router)
}

func main() {}
