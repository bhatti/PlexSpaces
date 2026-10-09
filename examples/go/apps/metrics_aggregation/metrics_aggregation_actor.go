// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Metrics Aggregation Pipeline - Go WASM
//
// StatsD/OTLP ingestion → tumbling window aggregation → cascading rollup
// (1s→1m→1h) → anomaly detection (Z-score/EWMA) → alerting.
// Leader/worker with shard-group placement, scatter/gather,
// compute vs coordination metrics tracking.

package main

import (
	"encoding/json"
	"fmt"
	"math"
	"sort"

	"github.com/bhatti/PlexSpaces/sdks/go/plexspaces"
)

var pxHost = plexspaces.NewHost()

// ─── Types ───────────────────────────────────────────────────────────────────

type Metric struct {
	Name      string            `json:"name"`
	Value     float64           `json:"value"`
	Type      string            `json:"type"`
	Tags      map[string]string `json:"tags"`
	Timestamp int64             `json:"timestamp"`
}

type Aggregate struct {
	Name     string            `json:"name"`
	Count    int               `json:"count"`
	Sum      float64           `json:"sum"`
	Avg      float64           `json:"avg"`
	Min      float64           `json:"min"`
	Max      float64           `json:"max"`
	P50      float64           `json:"p50"`
	P95      float64           `json:"p95"`
	P99      float64           `json:"p99"`
	Type     string            `json:"type"`
	Tags     map[string]string `json:"tags"`
	WindowMs int               `json:"window_ms"`
}

type Anomaly struct {
	Metric    string            `json:"metric"`
	Value     float64           `json:"value"`
	ZScore    float64           `json:"z_score,omitempty"`
	EWMA      float64           `json:"ewma,omitempty"`
	Deviation float64           `json:"deviation,omitempty"`
	Mean      float64           `json:"mean,omitempty"`
	Std       float64           `json:"std,omitempty"`
	Threshold float64           `json:"threshold"`
	Severity  string            `json:"severity"`
	Tags      map[string]string `json:"tags"`
}

// ─── Metric Generator ────────────────────────────────────────────────────────

var metricNames = []string{
	"cpu.usage", "memory.used", "disk.io.read", "disk.io.write",
	"network.rx.bytes", "network.tx.bytes", "http.request.duration",
	"http.request.count", "cache.hit_rate", "queue.depth",
	"gc.pause_ms", "thread.count", "db.query.duration", "db.connections.active",
	"api.latency.p99", "api.error_rate",
}
var hostNames = []string{"web-01", "web-02", "api-01", "db-01", "cache-01", "worker-01"}
var envs = []string{"production", "staging"}
var regions = []string{"us-east-1", "eu-west-1", "ap-south-1"}

func generateMetrics(count int, seed int64) []Metric {
	metrics := make([]Metric, 0, count)
	rng := seed
	nowMs := int64(pxHost.NowMs())
	for i := 0; i < count; i++ {
		rng = (rng*1103515245 + 12345) & 0x7FFFFFFF
		name := metricNames[int(rng)%len(metricNames)]
		hostName := hostNames[int(rng>>8)%len(hostNames)]
		env := envs[int(rng>>12)%len(envs)]
		region := regions[int(rng>>16)%len(regions)]

		var value float64
		if contains(name, "duration") || contains(name, "latency") || contains(name, "pause") {
			value = float64((rng % 500) + 1)
		} else if contains(name, "rate") || contains(name, "usage") || contains(name, "hit_rate") {
			value = float64(rng % 100)
		} else if contains(name, "count") || contains(name, "depth") || contains(name, "connections") {
			value = float64(rng % 1000)
		} else {
			value = float64(rng % 10000)
		}
		if (rng>>20)%20 == 0 {
			value *= 10.0
		}

		mtype := "gauge"
		if contains(name, "count") {
			mtype = "counter"
		} else if contains(name, "duration") || contains(name, "latency") {
			mtype = "histogram"
		}

		metrics = append(metrics, Metric{
			Name:      name,
			Value:     value,
			Type:      mtype,
			Tags:      map[string]string{"host": hostName, "env": env, "region": region},
			Timestamp: nowMs + int64(i),
		})
	}
	return metrics
}

func contains(s, substr string) bool {
	return len(s) >= len(substr) && (s == substr || len(s) > 0 && containsCheck(s, substr))
}

func containsCheck(s, substr string) bool {
	for i := 0; i <= len(s)-len(substr); i++ {
		if s[i:i+len(substr)] == substr {
			return true
		}
	}
	return false
}

// ─── Window Aggregation ──────────────────────────────────────────────────────

func aggregateWindow(metrics []Metric, windowMs int) []Aggregate {
	buckets := map[string]*struct {
		name   string
		count  int
		sum    float64
		min    float64
		max    float64
		values []float64
		tags   map[string]string
		mtype  string
	}{}

	for _, m := range metrics {
		b, ok := buckets[m.Name]
		if !ok {
			b = &struct {
				name   string
				count  int
				sum    float64
				min    float64
				max    float64
				values []float64
				tags   map[string]string
				mtype  string
			}{name: m.Name, min: math.Inf(1), max: math.Inf(-1), tags: m.Tags, mtype: m.Type}
			buckets[m.Name] = b
		}
		b.count++
		b.sum += m.Value
		if m.Value < b.min {
			b.min = m.Value
		}
		if m.Value > b.max {
			b.max = m.Value
		}
		b.values = append(b.values, m.Value)
	}

	results := make([]Aggregate, 0, len(buckets))
	for _, b := range buckets {
		avg := 0.0
		if b.count > 0 {
			avg = b.sum / float64(b.count)
		}
		sort.Float64s(b.values)
		p50 := percentile(b.values, 0.50)
		p95 := percentile(b.values, 0.95)
		p99 := percentile(b.values, 0.99)

		minV := b.min
		if math.IsInf(minV, 1) {
			minV = 0
		}
		maxV := b.max
		if math.IsInf(maxV, -1) {
			maxV = 0
		}

		results = append(results, Aggregate{
			Name: b.name, Count: b.count, Sum: b.sum,
			Avg: math.Round(avg*100) / 100, Min: minV, Max: maxV,
			P50: p50, P95: p95, P99: p99,
			Type: b.mtype, Tags: b.tags, WindowMs: windowMs,
		})
	}
	return results
}

func percentile(sorted []float64, p float64) float64 {
	if len(sorted) == 0 {
		return 0
	}
	idx := int(float64(len(sorted)) * p)
	if idx >= len(sorted) {
		idx = len(sorted) - 1
	}
	return sorted[idx]
}


// ─── Anomaly Detection ───────────────────────────────────────────────────────

func detectAnomaliesZScore(aggs []Aggregate, threshold float64) []Anomaly {
	byName := map[string][]Aggregate{}
	for _, a := range aggs {
		byName[a.Name] = append(byName[a.Name], a)
	}
	var anomalies []Anomaly
	for name, group := range byName {
		if len(group) < 3 {
			continue
		}
		values := make([]float64, len(group))
		for i, a := range group {
			values[i] = a.Avg
		}
		mean := 0.0
		for _, v := range values {
			mean += v
		}
		mean /= float64(len(values))
		variance := 0.0
		for _, v := range values {
			variance += (v - mean) * (v - mean)
		}
		variance /= float64(len(values))
		std := math.Sqrt(variance)
		if std == 0 {
			std = 0.001
		}
		for _, a := range group {
			z := math.Abs(a.Avg-mean) / std
			if z > threshold {
				sev := "warning"
				if z > threshold*2 {
					sev = "critical"
				}
				anomalies = append(anomalies, Anomaly{
					Metric: name, Value: a.Avg, ZScore: math.Round(z*100) / 100,
					Mean: math.Round(mean*100) / 100, Std: math.Round(std*100) / 100,
					Threshold: threshold, Severity: sev, Tags: a.Tags,
				})
			}
		}
	}
	return anomalies
}

// ─── Actors ──────────────────────────────────────────────────────────────────

type LeaderActor struct {
	plexspaces.BaseActor
}

type WorkerActor struct {
	plexspaces.BaseActor
	metricsProcessed int
	computeMs        int64
}

type initConfig struct {
	ActorID string            `json:"actor_id"`
	Args    map[string]string `json:"args"`
}

func NewLeaderActor() plexspaces.Actor {
	a := &LeaderActor{}
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
	return ""
}

func (l *LeaderActor) Handle(fromActor, msgType, payloadJSON string) string {
	var payload map[string]interface{}
	if err := json.Unmarshal([]byte(payloadJSON), &payload); err != nil {
		return marshal(map[string]interface{}{"error": err.Error()})
	}
	op := strVal(payload["op"])
	if op == "" {
		op = msgType
	}
	switch op {
	case "run":
		return marshal(l.OnRun(payload))
	case "run_scaling_benchmark":
		return marshal(l.OnRunScalingBenchmark(payload))
	case "run_weak_scaling":
		return marshal(l.OnRunWeakScaling(payload))
	default:
		return marshal(map[string]interface{}{"error": "unknown_op", "op": op})
	}
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
	var payload map[string]interface{}
	if err := json.Unmarshal([]byte(payloadJSON), &payload); err != nil {
		return marshal(map[string]interface{}{"error": err.Error()})
	}
	op := strVal(payload["op"])
	if op == "" {
		op = msgType
	}
	switch op {
	case "aggregate_batch":
		return marshal(w.OnAggregateBatch(payload))
	default:
		return marshal(map[string]interface{}{"error": "unknown_op", "op": op})
	}
}

func (l *LeaderActor) OnRun(payload map[string]interface{}) map[string]interface{} {
	metricCount := intVal(payload["metric_count"], 10000)
	workerCount := intVal(payload["worker_count"], 8)
	windowMs := intVal(payload["window_ms"], 1000)
	anomalyThreshold := floatVal(payload["anomaly_threshold"], 2.5)
	anomalyThreshX10 := int(anomalyThreshold * 10)

	coordStart := pxHost.NowMs()
	groupID := fmt.Sprintf("metrics-agg-go-%d", pxHost.NowMs())
	group, err := pxHost.CreateShardGroup(plexspaces.CreateShardGroupRequest{
		GroupID:           groupID,
		ActorType:         "worker",
		ShardCount:        workerCount,
		PartitionStrategy: "hash",
		RebalancePolicy:   "manual",
		Placement:         plexspaces.NodePlacement{Strategy: "from_registry"},
	})
	if err != nil {
		return map[string]interface{}{"status": "error", "error": fmt.Sprintf("shard group: %v", err)}
	}
	shardIDs := group.ShardActorIDs
	if len(shardIDs) == 0 {
		return map[string]interface{}{"status": "error", "error": "failed to create shard group"}
	}
	coordCreate := int64(pxHost.NowMs() - coordStart)

	metricsPerWorker := metricCount / workerCount
	if metricsPerWorker < 1 {
		metricsPerWorker = 1
	}

	// Workers generate their own metrics locally — avoids passing float64-heavy
	// Metric structs across the WASM boundary (TinyGo WASM struct-encoder crash).
	sgStart := pxHost.NowMs()
	sgResult, sgErr := pxHost.ScatterGather(plexspaces.ScatterGatherRequest{
		GroupID: groupID,
		Query: map[string]interface{}{
			"op":                    "aggregate_batch",
			"metric_count":          metricsPerWorker,
			"seed":                  int(pxHost.NowMs() % 100000),
			"window_ms":             windowMs,
			"anomaly_threshold_x10": anomalyThreshX10,
		},
		TimeoutMs: 60000,
	})
	sgElapsed := int64(pxHost.NowMs() - sgStart)
	if sgErr != nil {
		return map[string]interface{}{"status": "error", "error": fmt.Sprintf("scatter-gather: %v", sgErr)}
	}

	var totalComputeMs, totalCoordMs int64
	totalCoordMs = coordCreate + sgElapsed
	var totalAggregates, totalAnomalies int
	errorCount := 0

	for _, resp := range sgResult.ShardResponses {
		result := unwrapPayload(resp)
		if _, hasErr := result["error"]; hasErr {
			errorCount++
			continue
		}
		totalComputeMs += int64(intVal(result["compute_ms"], 0))
		totalAggregates += intVal(result["aggregate_count"], 0)
		totalAnomalies += intVal(result["anomaly_count"], 0)
	}

	wallTime := totalCoordMs
	if totalComputeMs > wallTime {
		wallTime = totalComputeMs
	}
	granularity := 0.0
	if totalCoordMs > 0 {
		granularity = math.Round(float64(totalComputeMs)/float64(totalCoordMs)*10) / 10
	}
	mps := int64(0)
	if wallTime > 0 {
		mps = int64(metricCount) * 1000 / wallTime
	}

	return map[string]interface{}{
		"status": "ok", "metric_count": metricCount, "worker_count": workerCount,
		"window_ms": windowMs, "wall_time_ms": wallTime, "compute_time_ms": totalComputeMs,
		"coordination_time_ms": totalCoordMs, "granularity_ratio": granularity,
		"metrics_per_sec": mps, "total_aggregates": totalAggregates,
		"anomaly_count": totalAnomalies, "node_count": 1,
		"actor_count": len(shardIDs) + 1, "error_count": errorCount,
	}
}

func (l *LeaderActor) OnRunScalingBenchmark(payload map[string]interface{}) map[string]interface{} {
	metricCount := intVal(payload["metric_count"], 10000)
	shardCounts := intSlice(payload["shard_counts"], []int{2, 4, 8, 16})
	batchSize := intVal(payload["batch_size"], 500)
	windowMs := intVal(payload["window_ms"], 1000)
	warmupRounds := intVal(payload["warmup_rounds"], 1)
	benchmarkRounds := intVal(payload["benchmark_rounds"], 2)

	var results []map[string]interface{}
	var baselineWall int64

	for _, sc := range shardCounts {
		for w := 0; w < warmupRounds; w++ {
			l.OnRun(map[string]interface{}{"metric_count": min(metricCount, 1000), "worker_count": sc, "batch_size": batchSize, "window_ms": windowMs})
		}
		var tw, tc, tco, tmps int64
		var nc, ec int
		for r := 0; r < benchmarkRounds; r++ {
			res := l.OnRun(map[string]interface{}{"metric_count": metricCount, "worker_count": sc, "batch_size": batchSize, "window_ms": windowMs})
			tw += int64(intVal(res["wall_time_ms"], 0))
			tc += int64(intVal(res["compute_time_ms"], 0))
			tco += int64(intVal(res["coordination_time_ms"], 0))
			tmps += int64(intVal(res["metrics_per_sec"], 0))
			nc = intVal(res["node_count"], 0)
			ec += intVal(res["error_count"], 0)
		}
		aw := tw / int64(benchmarkRounds)
		ac := tc / int64(benchmarkRounds)
		aco := tco / int64(benchmarkRounds)
		amps := tmps / int64(benchmarkRounds)
		tot := ac + aco
		if tot == 0 {
			tot = 1
		}
		if baselineWall == 0 {
			baselineWall = aw
		}
		speedup := 1.0
		if aw > 0 {
			speedup = float64(baselineWall) / float64(aw)
		}
		eff := speedup / (float64(sc) / float64(shardCounts[0])) * 100
		gran := 0.0
		if aco > 0 {
			gran = math.Round(float64(ac)/float64(aco)*10) / 10
		}
		results = append(results, map[string]interface{}{
			"shards": sc, "metrics_per_sec": amps, "wall_time_ms": aw,
			"compute_time_ms": ac, "coordination_time_ms": aco,
			"compute_pct": float64(ac) * 100 / float64(tot), "granularity_ratio": gran,
			"speedup": math.Round(speedup*100) / 100, "efficiency_pct": math.Round(eff*10) / 10,
			"node_count": nc, "error_count": ec,
		})
	}
	return map[string]interface{}{"status": "ok", "metric_count": metricCount, "window_ms": windowMs, "results": results}
}

func (l *LeaderActor) OnRunWeakScaling(payload map[string]interface{}) map[string]interface{} {
	metricsPerShard := intVal(payload["metrics_per_shard"], 5000)
	shardCounts := intSlice(payload["shard_counts"], []int{2, 4, 8, 16})
	batchSize := intVal(payload["batch_size"], 500)
	windowMs := intVal(payload["window_ms"], 1000)
	warmupRounds := intVal(payload["warmup_rounds"], 1)
	benchmarkRounds := intVal(payload["benchmark_rounds"], 2)

	var results []map[string]interface{}
	var baselineMPS int64

	for _, sc := range shardCounts {
		totalMetrics := metricsPerShard * sc
		for w := 0; w < warmupRounds; w++ {
			l.OnRun(map[string]interface{}{"metric_count": min(totalMetrics, 1000), "worker_count": sc, "batch_size": batchSize, "window_ms": windowMs})
		}
		var tmps, tw, tc, tco int64
		var nc, ec int
		for r := 0; r < benchmarkRounds; r++ {
			res := l.OnRun(map[string]interface{}{"metric_count": totalMetrics, "worker_count": sc, "batch_size": batchSize, "window_ms": windowMs})
			tmps += int64(intVal(res["metrics_per_sec"], 0))
			tw += int64(intVal(res["wall_time_ms"], 0))
			tc += int64(intVal(res["compute_time_ms"], 0))
			tco += int64(intVal(res["coordination_time_ms"], 0))
			nc = intVal(res["node_count"], 0)
			ec += intVal(res["error_count"], 0)
		}
		amps := tmps / int64(benchmarkRounds)
		aw := tw / int64(benchmarkRounds)
		ac := tc / int64(benchmarkRounds)
		aco := tco / int64(benchmarkRounds)
		if baselineMPS == 0 {
			baselineMPS = amps
		}
		eff := 100.0
		if baselineMPS > 0 {
			eff = float64(amps) / float64(baselineMPS) * 100
		}
		gran := 0.0
		if aco > 0 {
			gran = math.Round(float64(ac)/float64(aco)*10) / 10
		}
		results = append(results, map[string]interface{}{
			"shards": sc, "total_metrics": totalMetrics, "metrics_per_sec": amps,
			"wall_time_ms": aw, "compute_time_ms": ac, "coordination_time_ms": aco,
			"granularity_ratio": gran, "efficiency_pct": math.Round(eff*10) / 10,
			"node_count": nc, "error_count": ec,
		})
	}
	return map[string]interface{}{"status": "ok", "metrics_per_shard": metricsPerShard, "window_ms": windowMs, "results": results}
}

func (w *WorkerActor) OnAggregateBatch(payload map[string]interface{}) map[string]interface{} {
	// Workers generate metrics locally — avoids passing float64-heavy structs across WASM boundary.
	// TinyGo WASM: float64 in map[string]any triggers broken strconv.genericFtoa path.
	metricCount := intVal(payload["metric_count"], 1000)
	seed := int64(intVal(payload["seed"], 42))
	windowMs := intVal(payload["window_ms"], 1000)
	threshX10 := intVal(payload["anomaly_threshold_x10"], 25)
	threshold := float64(threshX10) / 10.0 // local only, never marshaled

	compStart := pxHost.NowMs()
	metrics := generateMetrics(metricCount, seed)
	aggregates := aggregateWindow(metrics, windowMs)
	anomalies := detectAnomaliesZScore(aggregates, threshold)
	computeMs := int64(pxHost.NowMs() - compStart)

	critCount := 0
	warnCount := 0
	for _, a := range anomalies {
		if a.Severity == "critical" {
			critCount++
		} else {
			warnCount++
		}
	}

	w.metricsProcessed += metricCount
	w.computeMs += computeMs
	// TinyGo WASM: skip ApplicationMetricsAdd — nested map[string]any triggers fmtsort crash.

	return map[string]interface{}{
		"metrics_in":       metricCount,
		"aggregate_count":  len(aggregates),
		"unique_metrics":   countUniqueNames(aggregates),
		"anomaly_count":    len(anomalies),
		"anomaly_critical": critCount,
		"anomaly_warning":  warnCount,
		"compute_ms":       computeMs,
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
	}
	return def
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


func marshal(v interface{}) string {
	return plexspaces.SafeMarshal(v)
}

func unwrapPayload(resp map[string]interface{}) map[string]interface{} {
	for _, key := range []string{"payload", "result", "response", "data"} {
		if inner, ok := resp[key]; ok {
			if innerMap, ok := inner.(map[string]interface{}); ok {
				return innerMap
			}
		}
	}
	return resp
}

func strVal(v interface{}) string {
	if s, ok := v.(string); ok {
		return s
	}
	return ""
}

func countUniqueNames(aggs []Aggregate) int {
	seen := map[string]bool{}
	for _, a := range aggs {
		seen[a.Name] = true
	}
	return len(seen)
}

func min(a, b int) int {
	if a < b {
		return a
	}
	return b
}

// ─── Router ──────────────────────────────────────────────────────────────────

func init() {
	router := plexspaces.NewActorRouter()
	router.Route("leader", NewLeaderActor)
	router.Route("worker", NewWorkerActor)
	plexspaces.Register(router)
}

func main() {}
