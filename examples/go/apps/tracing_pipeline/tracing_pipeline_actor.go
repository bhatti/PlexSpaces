// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Distributed Tracing Pipeline - Go WASM
//
// OTLP span ingestion → trace assembly (out-of-order) → tail-based sampling
// (error-biased, latency-biased, random) → service graph construction.
// Leader/worker with shard-group partitioned by trace_id, scatter/gather,
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

type Span struct {
	TraceID      string            `json:"trace_id"`
	SpanID       string            `json:"span_id"`
	ParentSpanID string            `json:"parent_span_id,omitempty"`
	ServiceName  string            `json:"service_name"`
	Operation    string            `json:"operation"`
	StatusCode   int               `json:"status_code"`
	DurationMs   float64           `json:"duration_ms"`
	StartTimeMs  int64             `json:"start_time_ms"`
	Tags         map[string]string `json:"tags,omitempty"`
}

type Trace struct {
	TraceID    string  `json:"trace_id"`
	Spans      []Span  `json:"spans"`
	SpanCount  int     `json:"span_count"`
	HasError   bool    `json:"has_error"`
	RootSpan   string  `json:"root_span,omitempty"`
	DurationMs float64 `json:"duration_ms"`
	Services   int     `json:"services"`
}

type SamplingDecision struct {
	TraceID  string `json:"trace_id"`
	Sampled  bool   `json:"sampled"`
	Reason   string `json:"reason"`
	Priority int    `json:"priority"`
}

type ServiceEdge struct {
	Source      string  `json:"source"`
	Target      string  `json:"target"`
	CallCount   int     `json:"call_count"`
	ErrorCount  int     `json:"error_count"`
	AvgLatency  float64 `json:"avg_latency_ms"`
	P99Latency  float64 `json:"p99_latency_ms"`
	TotalLatMs  float64 `json:"-"`
	Latencies   []float64 `json:"-"`
}

type ServiceNode struct {
	Name       string  `json:"name"`
	SpanCount  int     `json:"span_count"`
	ErrorCount int     `json:"error_count"`
	AvgLatency float64 `json:"avg_latency_ms"`
}

type ServiceGraph struct {
	Nodes []ServiceNode `json:"nodes"`
	Edges []ServiceEdge `json:"edges"`
}

// ─── Span Generator ─────────────────────────────────────────────────────────

var services = []struct {
	name       string
	operations []string
}{
	{"api-gateway", []string{"POST /api/orders", "GET /api/users", "POST /api/payments", "GET /api/inventory"}},
	{"user-service", []string{"GetUser", "ValidateToken", "UpdateProfile", "ListUsers"}},
	{"order-service", []string{"CreateOrder", "GetOrder", "UpdateStatus", "CancelOrder"}},
	{"payment-service", []string{"ProcessPayment", "RefundPayment", "ValidateCard", "GetBalance"}},
	{"inventory-service", []string{"CheckStock", "ReserveItem", "ReleaseItem", "UpdateQuantity"}},
	{"notification-service", []string{"SendEmail", "SendSMS", "SendPush", "QueueNotification"}},
}

// call chains: api-gateway → user-service → order-service → payment-service → inventory-service → notification-service
var callChains = [][]int{
	{0, 1, 2, 3, 4, 5}, // full order flow
	{0, 1, 2, 4},        // order + inventory check
	{0, 2, 3},            // direct payment
	{0, 1},               // user lookup
	{0, 4, 5},            // inventory + notification
}

func generateSpans(traceCount int, seed int64) []Span {
	spans := make([]Span, 0, traceCount*4)
	rng := seed
	nowMs := int64(pxHost.NowMs())

	for t := 0; t < traceCount; t++ {
		rng = nextRng(rng)
		traceID := fmt.Sprintf("trace-%08x%08x", uint32(rng), uint32(t))
		chain := callChains[int(absInt64(rng>>4))%len(callChains)]

		rng = nextRng(rng)
		baseLatency := float64(50 + absInt64(rng)%200)
		isErrorTrace := (absInt64(rng>>20) % 20) == 0 // ~5% error
		isSlow := (absInt64(rng>>16) % 100) == 0       // ~1% slow

		parentID := ""
		traceStart := nowMs + int64(t)*10

		for depth, svcIdx := range chain {
			svc := services[svcIdx]
			rng = nextRng(rng)
			op := svc.operations[int(absInt64(rng))%len(svc.operations)]

			spanID := fmt.Sprintf("span-%08x-%d", uint32(rng), depth)

			latency := baseLatency / float64(depth+1)
			rng = nextRng(rng)
			jitter := float64(absInt64(rng)%50) - 25
			latency += jitter
			if latency < 1 {
				latency = 1
			}
			if isSlow {
				latency *= 10
			}

			statusCode := 200
			if isErrorTrace && depth == len(chain)-1 {
				statusCode = 500
			}

			tags := map[string]string{
				"env":    "production",
				"region": []string{"us-east-1", "eu-west-1", "ap-south-1"}[int(absInt64(rng>>8))%3],
			}

			spans = append(spans, Span{
				TraceID:      traceID,
				SpanID:       spanID,
				ParentSpanID: parentID,
				ServiceName:  svc.name,
				Operation:    op,
				StatusCode:   statusCode,
				DurationMs:   math.Round(latency*100) / 100,
				StartTimeMs:  traceStart + int64(depth)*int64(latency/2),
				Tags:         tags,
			})
			parentID = spanID
		}
	}

	// Shuffle to simulate out-of-order arrival
	rng2 := seed + 999
	for i := len(spans) - 1; i > 0; i-- {
		rng2 = nextRng(rng2)
		j := int(absInt64(rng2)) % (i + 1)
		spans[i], spans[j] = spans[j], spans[i]
	}

	return spans
}

func nextRng(r int64) int64 {
	return (r*1103515245 + 12345) & 0x7FFFFFFF
}

func absInt64(v int64) int64 {
	if v < 0 {
		return -v
	}
	return v
}

// ─── Trace Assembly ─────────────────────────────────────────────────────────

func assembleTraces(spans []Span) []Trace {
	byTrace := map[string][]Span{}
	for _, s := range spans {
		byTrace[s.TraceID] = append(byTrace[s.TraceID], s)
	}

	traces := make([]Trace, 0, len(byTrace))
	for tid, tSpans := range byTrace {
		hasError := false
		svcs := map[string]bool{}
		var maxEnd float64
		var minStart int64 = math.MaxInt64
		rootSpan := ""

		for _, s := range tSpans {
			svcs[s.ServiceName] = true
			if s.StatusCode >= 400 {
				hasError = true
			}
			if s.ParentSpanID == "" {
				rootSpan = s.Operation
			}
			end := float64(s.StartTimeMs) + s.DurationMs
			if end > maxEnd {
				maxEnd = end
			}
			if s.StartTimeMs < minStart {
				minStart = s.StartTimeMs
			}
		}

		dur := maxEnd - float64(minStart)
		if dur < 0 {
			dur = 0
		}

		traces = append(traces, Trace{
			TraceID:    tid,
			Spans:      tSpans,
			SpanCount:  len(tSpans),
			HasError:   hasError,
			RootSpan:   rootSpan,
			DurationMs: math.Round(dur*100) / 100,
			Services:   len(svcs),
		})
	}
	return traces
}

// ─── Tail-Based Sampling ────────────────────────────────────────────────────

func tailSample(traces []Trace, errorKeepRate, latencyThresholdMs, randomRate float64) ([]SamplingDecision, int, int, int) {
	decisions := make([]SamplingDecision, 0, len(traces))
	var keptError, keptLatency, keptRandom int

	// Compute p99 latency threshold
	durations := make([]float64, len(traces))
	for i, t := range traces {
		durations[i] = t.DurationMs
	}
	sort.Float64s(durations)
	p99 := latencyThresholdMs
	if len(durations) > 0 {
		idx := int(float64(len(durations)) * 0.99)
		if idx >= len(durations) {
			idx = len(durations) - 1
		}
		p99 = durations[idx]
	}

	rng := int64(42)
	for _, t := range traces {
		rng = nextRng(rng)
		if t.HasError {
			if float64(absInt64(rng)%100)/100.0 < errorKeepRate {
				decisions = append(decisions, SamplingDecision{TraceID: t.TraceID, Sampled: true, Reason: "error", Priority: 3})
				keptError++
				continue
			}
		}
		if t.DurationMs >= p99 {
			decisions = append(decisions, SamplingDecision{TraceID: t.TraceID, Sampled: true, Reason: "high_latency", Priority: 2})
			keptLatency++
			continue
		}
		rng = nextRng(rng)
		if float64(absInt64(rng)%10000)/10000.0 < randomRate {
			decisions = append(decisions, SamplingDecision{TraceID: t.TraceID, Sampled: true, Reason: "random", Priority: 1})
			keptRandom++
			continue
		}
		decisions = append(decisions, SamplingDecision{TraceID: t.TraceID, Sampled: false, Reason: "dropped", Priority: 0})
	}
	return decisions, keptError, keptLatency, keptRandom
}

// ─── Service Graph ──────────────────────────────────────────────────────────

func buildServiceGraph(traces []Trace) ServiceGraph {
	nodeMap := map[string]*ServiceNode{}
	edgeKey := func(src, tgt string) string { return src + " → " + tgt }
	edgeMap := map[string]*ServiceEdge{}

	for _, t := range traces {
		spanByID := map[string]*Span{}
		for i := range t.Spans {
			spanByID[t.Spans[i].SpanID] = &t.Spans[i]
		}
		for _, s := range t.Spans {
			n, ok := nodeMap[s.ServiceName]
			if !ok {
				n = &ServiceNode{Name: s.ServiceName}
				nodeMap[s.ServiceName] = n
			}
			n.SpanCount++
			if s.StatusCode >= 400 {
				n.ErrorCount++
			}
			n.AvgLatency += s.DurationMs

			if s.ParentSpanID != "" {
				if parent, ok := spanByID[s.ParentSpanID]; ok {
					key := edgeKey(parent.ServiceName, s.ServiceName)
					e, ok := edgeMap[key]
					if !ok {
						e = &ServiceEdge{Source: parent.ServiceName, Target: s.ServiceName}
						edgeMap[key] = e
					}
					e.CallCount++
					e.TotalLatMs += s.DurationMs
					e.Latencies = append(e.Latencies, s.DurationMs)
					if s.StatusCode >= 400 {
						e.ErrorCount++
					}
				}
			}
		}
	}

	nodes := make([]ServiceNode, 0, len(nodeMap))
	for _, n := range nodeMap {
		if n.SpanCount > 0 {
			n.AvgLatency = math.Round(n.AvgLatency/float64(n.SpanCount)*100) / 100
		}
		nodes = append(nodes, *n)
	}

	edges := make([]ServiceEdge, 0, len(edgeMap))
	for _, e := range edgeMap {
		if e.CallCount > 0 {
			e.AvgLatency = math.Round(e.TotalLatMs/float64(e.CallCount)*100) / 100
		}
		if len(e.Latencies) > 0 {
			sort.Float64s(e.Latencies)
			idx := int(float64(len(e.Latencies)) * 0.99)
			if idx >= len(e.Latencies) {
				idx = len(e.Latencies) - 1
			}
			e.P99Latency = e.Latencies[idx]
		}
		edges = append(edges, *e)
	}

	return ServiceGraph{Nodes: nodes, Edges: edges}
}

// ─── Actors ─────────────────────────────────────────────────────────────────

type initConfig struct {
	ActorID string            `json:"actor_id"`
	Args    map[string]string `json:"args"`
}

type LeaderActor struct {
	plexspaces.BaseActor
}

type WorkerActor struct {
	plexspaces.BaseActor
	spansProcessed int
	computeMs      int64
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
	case "process_spans":
		return marshal(w.OnProcessSpans(payload))
	default:
		return marshal(map[string]interface{}{"error": "unknown_op", "op": op})
	}
}

func (l *LeaderActor) OnRun(payload map[string]interface{}) map[string]interface{} {
	traceCount := intVal(payload["trace_count"], 1000)
	workerCount := intVal(payload["worker_count"], 8)
	errorKeepRate := floatVal(payload["error_keep_rate"], 1.0)
	latencyThresholdMs := floatVal(payload["latency_threshold_ms"], 500.0)
	randomRate := floatVal(payload["random_sample_rate"], 0.01)

	coordStart := pxHost.NowMs()
	groupID := fmt.Sprintf("tracing-go-%d", pxHost.NowMs())
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
	coordCreate := pxHost.NowMs() - coordStart

	// Workers generate spans locally — avoids passing float64-heavy Span structs
	// across the WASM boundary (TinyGo WASM struct-encoder crash).
	tracesPerWorker := traceCount / workerCount
	if tracesPerWorker < 1 {
		tracesPerWorker = 1
	}
	sgStart := pxHost.NowMs()
	sgResult, sgErr := pxHost.ScatterGather(plexspaces.ScatterGatherRequest{
		GroupID: groupID,
		Query: map[string]interface{}{
			"op":                     "process_spans",
			"trace_count":            tracesPerWorker,
			"seed":                   int(pxHost.NowMs() % 100000),
			"error_keep_rate_x100":   int(errorKeepRate * 100),
			"latency_threshold_ms":   int(latencyThresholdMs),
			"random_rate_x10000":     int(randomRate * 10000),
		},
		TimeoutMs: 60000,
	})
	sgElapsed := int64(pxHost.NowMs() - sgStart)

	var totalComputeMs int64
	totalCoordMs := int64(coordCreate) + sgElapsed
	var totalTraces, totalSpans, totalSampled, totalDropped, totalErrorTraces int
	var graphNodes, graphEdges int
	errorCount := 0

	if sgErr != nil {
		return map[string]interface{}{"status": "error", "error": fmt.Sprintf("scatter-gather: %v", sgErr)}
	}

	for _, resp := range sgResult.ShardResponses {
		result := unwrapPayload(resp)
		if _, hasErr := result["error"]; hasErr {
			errorCount++
			continue
		}
		totalComputeMs += int64(intVal(result["compute_ms"], 0))
		totalTraces += intVal(result["traces_assembled"], 0)
		totalSpans += intVal(result["spans_in"], 0)
		totalSampled += intVal(result["sampled_count"], 0)
		totalDropped += intVal(result["dropped_count"], 0)
		totalErrorTraces += intVal(result["error_traces"], 0)
		graphNodes += intVal(result["graph_nodes"], 0)
		graphEdges += intVal(result["graph_edges"], 0)
	}

	wallTime := totalCoordMs
	if totalComputeMs > wallTime {
		wallTime = totalComputeMs
	}
	granularity := 0.0
	if totalCoordMs > 0 {
		granularity = math.Round(float64(totalComputeMs)/float64(totalCoordMs)*10) / 10
	}
	sps := int64(0)
	if wallTime > 0 {
		sps = int64(totalSpans) * 1000 / wallTime
	}

	return map[string]interface{}{
		"status": "ok", "trace_count": totalTraces, "span_count": totalSpans,
		"worker_count": workerCount, "wall_time_ms": wallTime,
		"compute_time_ms": totalComputeMs, "coordination_time_ms": totalCoordMs,
		"granularity_ratio": granularity, "spans_per_sec": sps,
		"traces_assembled": totalTraces, "error_traces": totalErrorTraces,
		"sampled_count": totalSampled, "dropped_count": totalDropped,
		"service_graph_nodes": graphNodes, "service_graph_edges": graphEdges,
		"node_count": 1, "actor_count": len(shardIDs) + 1,
		"error_count": errorCount,
	}
}

func (l *LeaderActor) OnRunScalingBenchmark(payload map[string]interface{}) map[string]interface{} {
	traceCount := intVal(payload["trace_count"], 1000)
	shardCounts := intSlice(payload["shard_counts"], []int{2, 4, 8, 16})
	batchSize := intVal(payload["batch_size"], 500)
	warmupRounds := intVal(payload["warmup_rounds"], 1)
	benchmarkRounds := intVal(payload["benchmark_rounds"], 2)

	var results []map[string]interface{}
	var baselineWall int64

	for _, sc := range shardCounts {
		for w := 0; w < warmupRounds; w++ {
			l.OnRun(map[string]interface{}{"trace_count": min(traceCount, 100), "worker_count": sc, "batch_size": batchSize})
		}
		var tw, tc, tco, tsps int64
		var nc, ec int
		for r := 0; r < benchmarkRounds; r++ {
			res := l.OnRun(map[string]interface{}{"trace_count": traceCount, "worker_count": sc, "batch_size": batchSize})
			tw += int64(intVal(res["wall_time_ms"], 0))
			tc += int64(intVal(res["compute_time_ms"], 0))
			tco += int64(intVal(res["coordination_time_ms"], 0))
			tsps += int64(intVal(res["spans_per_sec"], 0))
			nc = intVal(res["node_count"], 0)
			ec += intVal(res["error_count"], 0)
		}
		aw := tw / int64(benchmarkRounds)
		ac := tc / int64(benchmarkRounds)
		aco := tco / int64(benchmarkRounds)
		asps := tsps / int64(benchmarkRounds)
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
			"shards": sc, "spans_per_sec": asps, "wall_time_ms": aw,
			"compute_time_ms": ac, "coordination_time_ms": aco,
			"compute_pct": float64(ac) * 100 / float64(tot), "granularity_ratio": gran,
			"speedup": math.Round(speedup*100) / 100, "efficiency_pct": math.Round(eff*10) / 10,
			"node_count": nc, "error_count": ec,
		})
	}
	return map[string]interface{}{"status": "ok", "trace_count": traceCount, "results": results}
}

func (l *LeaderActor) OnRunWeakScaling(payload map[string]interface{}) map[string]interface{} {
	tracesPerShard := intVal(payload["traces_per_shard"], 500)
	shardCounts := intSlice(payload["shard_counts"], []int{2, 4, 8, 16})
	batchSize := intVal(payload["batch_size"], 500)
	warmupRounds := intVal(payload["warmup_rounds"], 1)
	benchmarkRounds := intVal(payload["benchmark_rounds"], 2)

	var results []map[string]interface{}
	var baselineSPS int64

	for _, sc := range shardCounts {
		totalTraces := tracesPerShard * sc
		for w := 0; w < warmupRounds; w++ {
			l.OnRun(map[string]interface{}{"trace_count": min(totalTraces, 100), "worker_count": sc, "batch_size": batchSize})
		}
		var tsps, tw, tc, tco int64
		var nc, ec int
		for r := 0; r < benchmarkRounds; r++ {
			res := l.OnRun(map[string]interface{}{"trace_count": totalTraces, "worker_count": sc, "batch_size": batchSize})
			tsps += int64(intVal(res["spans_per_sec"], 0))
			tw += int64(intVal(res["wall_time_ms"], 0))
			tc += int64(intVal(res["compute_time_ms"], 0))
			tco += int64(intVal(res["coordination_time_ms"], 0))
			nc = intVal(res["node_count"], 0)
			ec += intVal(res["error_count"], 0)
		}
		asps := tsps / int64(benchmarkRounds)
		aw := tw / int64(benchmarkRounds)
		ac := tc / int64(benchmarkRounds)
		aco := tco / int64(benchmarkRounds)
		if baselineSPS == 0 {
			baselineSPS = asps
		}
		eff := 100.0
		if baselineSPS > 0 {
			eff = float64(asps) / float64(baselineSPS) * 100
		}
		gran := 0.0
		if aco > 0 {
			gran = math.Round(float64(ac)/float64(aco)*10) / 10
		}
		results = append(results, map[string]interface{}{
			"shards": sc, "total_traces": totalTraces, "spans_per_sec": asps,
			"wall_time_ms": aw, "compute_time_ms": ac, "coordination_time_ms": aco,
			"granularity_ratio": gran, "efficiency_pct": math.Round(eff*10) / 10,
			"node_count": nc, "error_count": ec,
		})
	}
	return map[string]interface{}{"status": "ok", "traces_per_shard": tracesPerShard, "results": results}
}

func (w *WorkerActor) OnProcessSpans(payload map[string]interface{}) map[string]interface{} {
	// Workers generate spans locally — avoids passing float64-heavy Span/Trace structs
	// across the WASM boundary (TinyGo WASM struct-encoder crash).
	traceCount := intVal(payload["trace_count"], 100)
	seed := int64(intVal(payload["seed"], 42))
	errorKeepRate := float64(intVal(payload["error_keep_rate_x100"], 100)) / 100.0
	latencyThresholdMs := float64(intVal(payload["latency_threshold_ms"], 500))
	randomRate := float64(intVal(payload["random_rate_x10000"], 100)) / 10000.0

	compStart := pxHost.NowMs()
	spans := generateSpans(traceCount, seed)
	traces := assembleTraces(spans)
	decisions, _, _, _ := tailSample(traces, errorKeepRate, latencyThresholdMs, randomRate)
	graph := buildServiceGraph(traces)

	computeMs := int64(pxHost.NowMs() - compStart)
	w.spansProcessed += len(spans)
	w.computeMs += computeMs

	sampled := 0
	errorTraces := 0
	for _, d := range decisions {
		if d.Sampled {
			sampled++
		}
	}
	for _, t := range traces {
		if t.HasError {
			errorTraces++
		}
	}

	return map[string]interface{}{
		"spans_in":         len(spans),
		"traces_assembled": len(traces),
		"error_traces":     errorTraces,
		"sampled_count":    sampled,
		"dropped_count":    len(decisions) - sampled,
		"graph_nodes":      len(graph.Nodes),
		"graph_edges":      len(graph.Edges),
		"compute_ms":       computeMs,
	}
}

// ─── Helpers ────────────────────────────────────────────────────────────────

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


func min(a, b int) int {
	if a < b {
		return a
	}
	return b
}

// ─── Router ─────────────────────────────────────────────────────────────────

func init() {
	router := plexspaces.NewActorRouter()
	router.Route("leader", NewLeaderActor)
	router.Route("worker", NewWorkerActor)
	plexspaces.Register(router)
}

func main() {}
