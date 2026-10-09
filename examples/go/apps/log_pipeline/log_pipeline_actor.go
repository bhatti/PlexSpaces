// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Log Ingestion & Routing Pipeline — Observability data processing with PlexSpaces.
//
// Architecture: Ingester → EventBreaker → Router → PipelineWorker (shard group) → SinkWriter
// Real-world analogs: Stream, Splunk Heavy Forwarder, Fluentd, Vector.

package main

import (
	"encoding/json"
	"fmt"
	"sort"
	"strconv"
	"strings"

	"github.com/bhatti/PlexSpaces/sdks/go/plexspaces"
)

// h is initialized in init() to avoid TinyGo WASM package-level init ordering issues.
// TinyGo WASM: regexp package causes WASM trap during _start in some TinyGo versions;
// use strings-based matching instead of regexp for all pattern operations.
var h *plexspaces.Host

type initConfig struct {
	ActorID string            `json:"actor_id"`
	Args    map[string]string `json:"args"`
}

// ─────────────────────────────────────────────────────────────────────────────
// Pipeline Functions
// ─────────────────────────────────────────────────────────────────────────────

func fnJSONParse(event map[string]any) map[string]any {
	raw, _ := event["_raw"].(string)
	if strings.HasPrefix(raw, "{") {
		var parsed map[string]any
		if err := json.Unmarshal([]byte(raw), &parsed); err == nil {
			for k, v := range parsed {
				event[k] = v
			}
			event["_parsed"] = true
		} else {
			event["_parse_error"] = true
		}
	}
	return event
}

// fnRegexExtract extracts ISO timestamp without regexp (TinyGo WASM regexp workaround).
func fnRegexExtract(event map[string]any) map[string]any {
	raw, _ := event["_raw"].(string)
	// Find pattern YYYY-MM-DDTHH:MM:SS by scanning for 'T' preceded by 10 date chars
	for i := 10; i < len(raw)-8; i++ {
		if raw[i] == 'T' && i >= 10 {
			candidate := raw[i-10 : i+9]
			if len(candidate) == 19 && candidate[4] == '-' && candidate[7] == '-' && candidate[13] == ':' && candidate[16] == ':' {
				event["timestamp"] = candidate
				break
			}
		}
	}
	return event
}

// fnMaskPII masks email addresses using string scanning (TinyGo WASM regexp workaround).
func fnMaskPII(event map[string]any) map[string]any {
	raw, _ := event["_raw"].(string)
	masked := 0
	// Mask email addresses: find "@" then scan left/right for word chars
	if idx := strings.Index(raw, "@"); idx > 0 {
		start := idx - 1
		for start > 0 && isEmailChar(raw[start-1]) {
			start--
		}
		end := idx + 1
		for end < len(raw) && (isEmailChar(raw[end]) || raw[end] == '.') {
			end++
		}
		if end > idx+1 {
			raw = raw[:start] + "***@***.***" + raw[end:]
			masked++
		}
	}
	if masked > 0 {
		event["_raw"] = raw
		event["_pii_masked"] = masked
	}
	return event
}

func isEmailChar(b byte) bool {
	return (b >= 'a' && b <= 'z') || (b >= 'A' && b <= 'Z') || (b >= '0' && b <= '9') ||
		b == '.' || b == '+' || b == '-' || b == '_'
}

func fnEnrich(event map[string]any) map[string]any {
	event["environment"] = "production"
	event["region"] = "us-east-1"
	event["pipeline_version"] = "3.0"
	event["_enriched"] = true
	return event
}

func fnRenameFields(event map[string]any) map[string]any {
	renames := map[string]string{"msg": "message", "lvl": "level", "ts": "timestamp"}
	for old, newName := range renames {
		if v, ok := event[old]; ok {
			event[newName] = v
			delete(event, old)
		}
	}
	return event
}

func fnDrop(event map[string]any) map[string]any {
	level, _ := event["level"].(string)
	level = strings.ToLower(level)
	if level == "debug" || level == "trace" {
		return nil
	}
	return event
}

func fnSample(event map[string]any, rate float64) map[string]any {
	// TinyGo WASM: use a simple multiplicative hash instead of crypto/md5
	// to avoid TinyGo's crypto package init-time issues in WASM.
	raw, _ := event["_raw"].(string)
	h := uint64(14695981039346656037)
	for i := 0; i < len(raw); i++ {
		h ^= uint64(raw[i])
		h *= 1099511628211
	}
	if float64(h%1000)/1000.0 < rate {
		event["_sampled"] = true
		return event
	}
	return nil
}

type pipelineFunc func(map[string]any) map[string]any

// pipelineFunctions is initialized in init() to avoid TinyGo WASM package-level
// function-value map initialization issues (function values in package-level maps
// can fail silently in TinyGo before init() registers the actor router).
var pipelineFunctions map[string]pipelineFunc

// ─────────────────────────────────────────────────────────────────────────────
// Routing Rules
// ─────────────────────────────────────────────────────────────────────────────

func evaluateRouteRule(event map[string]any, rule map[string]any) bool {
	field, _ := rule["field"].(string)
	op, _ := rule["op"].(string)
	value, _ := rule["value"].(string)
	eventVal := fmt.Sprintf("%v", event[field])
	switch op {
	case "equals":
		return eventVal == value
	case "contains":
		return strings.Contains(eventVal, value)
	case "starts_with":
		return strings.HasPrefix(eventVal, value)
	case "exists":
		_, ok := event[field]
		return ok
	}
	return false
}

// ─────────────────────────────────────────────────────────────────────────────
// Event Generator
// ─────────────────────────────────────────────────────────────────────────────

func generateEvents(count int) []map[string]any {
	levels := []string{"debug", "info", "info", "info", "warn", "error"}
	services := []string{"api-gateway", "auth-service", "payment-svc", "user-svc", "inventory"}
	events := make([]map[string]any, 0, count)
	third := count / 3
	twoThird := count * 2 / 3
	eightyFivePct := count * 85 / 100
	for i := 0; i < count; i++ {
		level := levels[i%len(levels)]
		service := services[i%len(services)]
		host := fmt.Sprintf("host-%03d", i%20)
		var source, raw string
		switch {
		case i < third:
			source = "syslog"
			raw = fmt.Sprintf("<134>Oct  2 12:%02d:%02d %s %s[%d]: %s Request id=%d completed",
				(i/60)%24, i%60, host, service, 1000+i, strings.ToUpper(level), i)
		case i < twoThird:
			source = "app_json"
			raw = fmt.Sprintf(`{"ts":"2026-10-02T12:%02d:%02dZ","lvl":"%s","msg":"Request processed id=%d latency=%dms","service":"%s","trace_id":"trace-%08x"}`,
				(i/60)%24, i%60, level, i, 10+(i%200), service, i)
		case i < eightyFivePct:
			source = "security"
			raw = fmt.Sprintf(`{"ts":"2026-10-02T12:%02d:%02dZ","lvl":"warn","msg":"Auth attempt user=user%d@example.com from 10.0.%d.%d","service":"auth-service","email":"user%d@example.com"}`,
				(i/60)%24, i%60, i%100, i%256, (i*7)%256, i%100)
		default:
			source = "infra"
			// TinyGo WASM: avoid %f format verb; use integer arithmetic to prevent
			// strconv.AppendFloat → rightShift trap for certain float values.
			raw = fmt.Sprintf("2026-10-02T12:%02d:%02dZ kernel: [%d.%03d] %s CPU%d usage=%d%%",
				(i/60)%24, i%60, i/1000, i%1000, host, i%8, 50+(i%50))
		}
		events = append(events, map[string]any{
			"_raw":    raw,
			"source":  source,
			"host":    host,
			"service": service,
			"level":   level,
			"_time":   i,
			"_size":   len(raw),
		})
	}
	return events
}

// ─────────────────────────────────────────────────────────────────────────────
// PipelineWorkerActor
// ─────────────────────────────────────────────────────────────────────────────

type PipelineWorkerActor struct {
	plexspaces.BaseActor
	EventsProcessed   int      `json:"events_processed"`
	EventsDropped     int      `json:"events_dropped"`
	BytesProcessed    int      `json:"bytes_processed"`
	ComputeTimeMs     uint64   `json:"compute_time_ms"`
	PipelineFunctions []string `json:"pipeline_functions"`
}

func NewPipelineWorkerActor() plexspaces.Actor {
	a := &PipelineWorkerActor{
		PipelineFunctions: []string{"json_parse", "mask_pii", "enrich", "rename_fields"},
	}
	a.SetSelf(a)
	return a
}

func (w *PipelineWorkerActor) Init(configJSON string) string {
	var config initConfig
	if err := json.Unmarshal([]byte(configJSON), &config); err != nil {
		return "ERROR: " + err.Error()
	}
	w.SetRuntimeMetadata(config.ActorID)
	if fns, ok := config.Args["pipeline_functions"]; ok && fns != "" {
		w.PipelineFunctions = strings.Split(fns, ",")
	}
	return ""
}

func (w *PipelineWorkerActor) Handle(fromActor, msgType, payloadJSON string) string {
	switch msgType {
	case "process_events":
		return w.processEvents(payloadJSON)
	case "get_stats":
		return marshal(map[string]any{
			"events_processed": w.EventsProcessed,
			"events_dropped":   w.EventsDropped,
			"bytes_processed":  w.BytesProcessed,
			"compute_time_ms":  w.ComputeTimeMs,
		})
	default:
		return marshal(map[string]any{"error": "unknown_op", "op": msgType})
	}
}

func (w *PipelineWorkerActor) processEvents(payloadJSON string) string {
	// Workers generate events locally to avoid sending large payloads across the
	// WASM boundary — TinyGo OOMs when marshaling 100k events in map[string]any.
	var req struct {
		EventCount int    `json:"event_count"`
		StartIdx   int    `json:"start_idx"`
		RouteName  string `json:"route_name"`
		BatchIndex int    `json:"batch_index"`
	}
	req.EventCount = 1000 // sensible default
	if err := json.Unmarshal([]byte(payloadJSON), &req); err != nil {
		return marshal(map[string]any{"error": err.Error()})
	}

	events := generateEvents(req.EventCount)

	tStart := h.NowMs()
	processed := 0
	dropped := 0
	batchBytes := 0
	piiMasked := 0
	jsonParsed := 0
	enriched := 0

	for _, event := range events {
		if sz, ok := event["_size"].(int); ok {
			batchBytes += sz
		} else {
			batchBytes += len(fmt.Sprintf("%v", event))
		}
		result := event
		for _, fnName := range w.PipelineFunctions {
			fn, ok := pipelineFunctions[fnName]
			if ok && result != nil {
				result = fn(result)
			}
		}
		if result == nil {
			dropped++
		} else {
			processed++
			if _, ok := result["_pii_masked"]; ok {
				piiMasked++
			}
			if _, ok := result["_parsed"]; ok {
				jsonParsed++
			}
			if _, ok := result["_enriched"]; ok {
				enriched++
			}
		}
	}

	computeMs := h.NowMs() - tStart
	w.EventsProcessed += processed
	w.EventsDropped += dropped
	w.BytesProcessed += batchBytes
	w.ComputeTimeMs += computeMs

	// TinyGo WASM: skip ApplicationMetricsAdd — nested map[string]any triggers
	// TinyGo's buggy function-table dispatch in json.Marshal/fmtsort.Sort.
	_ = computeMs

	return marshal(map[string]any{
		"status":          "ok",
		"route_name":      req.RouteName,
		"batch_index":     req.BatchIndex,
		"events_in":       req.EventCount,
		"events_out":      processed,
		"events_dropped":  dropped,
		"bytes_processed": batchBytes,
		"compute_time_ms": computeMs,
		"node_id":         actorNodeID(w.ActorID()),
		"actor_id":        w.ActorID(),
		"pii_masked":      piiMasked,
		"json_parsed":     jsonParsed,
		"enriched":        enriched,
	})
}

// ─────────────────────────────────────────────────────────────────────────────
// LeaderActor
// ─────────────────────────────────────────────────────────────────────────────

type LeaderActor struct {
	plexspaces.BaseActor
}

func NewLeaderActor() plexspaces.Actor {
	a := &LeaderActor{}
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
	switch msgType {
	case "run":
		return l.run(payloadJSON)
	case "run_scaling_benchmark":
		return l.runScalingBenchmark(payloadJSON)
	case "run_weak_scaling_benchmark":
		return l.runWeakScalingBenchmark(payloadJSON)
	case "run_pipeline_depth_benchmark":
		return l.runPipelineDepthBenchmark(payloadJSON)
	default:
		return marshal(map[string]any{"error": "unknown_op", "op": msgType})
	}
}

type runParams struct {
	EventCount    int    `json:"event_count"`
	WorkerCount   int    `json:"worker_count"`
	BatchSize     int    `json:"batch_size"`
	PipelineDepth int    `json:"pipeline_depth"`
	SinkFormat    string `json:"sink_format"`
	Rounds        int    `json:"rounds"`
}

func (l *LeaderActor) run(payloadJSON string) string {
	params := runParams{
		EventCount: 10000, WorkerCount: 8, BatchSize: 500,
		PipelineDepth: 5, SinkFormat: "splunk_hec", Rounds: 1,
	}
	_ = json.Unmarshal([]byte(payloadJSON), &params)

	fnNames := []string{"json_parse", "mask_pii", "enrich", "rename_fields", "drop"}
	if params.PipelineDepth < len(fnNames) {
		fnNames = fnNames[:params.PipelineDepth]
	}
	pipelineFnStr := strings.Join(fnNames, ",")
	wallStart := h.NowMs()

	// Create shard group
	t0 := h.NowMs()
	sgResult, err := h.CreateShardGroup(plexspaces.CreateShardGroupRequest{
		GroupID:           fmt.Sprintf("log-pipeline-%d", h.NowMs()),
		ActorType:         "worker",
		ShardCount:        params.WorkerCount,
		PartitionStrategy: "hash",
		RebalancePolicy:   "manual",
		Placement:         plexspaces.NodePlacement{Strategy: "from_registry"},
	})
	if err != nil {
		return marshal(map[string]any{"error": err.Error()})
	}
	groupID := sgResult.GroupID
	coordCreateMs := h.NowMs() - t0

	// Workers generate their own events locally — avoids serializing large event
	// slices across the WASM boundary which OOMs TinyGo on 100k+ events.
	eventsPerWorker := params.EventCount / params.WorkerCount
	if eventsPerWorker < 1 {
		eventsPerWorker = 1
	}

	totalCompute := uint64(0)
	totalCoord := coordCreateMs
	totalEventsOut := 0
	totalDropped := 0
	nodeParticipation := map[string]int{}
	workerLatencies := []uint64{}

	for round := 0; round < params.Rounds; round++ {
		// Scatter/Gather — query is tiny; workers generate events themselves.
		t0 = h.NowMs()
		sgResp, err := h.ScatterGather(plexspaces.ScatterGatherRequest{
			GroupID: groupID,
			Query: map[string]any{
				"op":          "process_events",
				"event_count": eventsPerWorker,
				"start_idx":   round * eventsPerWorker,
				"route_name":  "all",
				"batch_index": round,
			},
			Aggregation:  "concat",
			TimeoutMs:    120000,
			MinResponses: params.WorkerCount,
		})
		coordScatterMs := h.NowMs() - t0
		totalCoord += coordScatterMs

		if err != nil {
			return marshal(map[string]any{"error": err.Error()})
		}

		// Aggregate worker results
		t0 = h.NowMs()
		for _, sr := range sgResp.ShardResponses {
			payload := unwrapPayload(sr)
			totalEventsOut += intVal(payload, "events_out")
			totalDropped += intVal(payload, "events_dropped")
			computeMs := uint64(intVal(payload, "compute_time_ms"))
			totalCompute += computeMs
			workerLatencies = append(workerLatencies, computeMs)
			nid := strVal(payload, "node_id")
			if nid != "" {
				nodeParticipation[nid]++
			}
		}
		totalCompute += h.NowMs() - t0
	}

	wallMs := h.NowMs() - wallStart

	avgLat := uint64(0)
	maxLat := uint64(0)
	if len(workerLatencies) > 0 {
		sum := uint64(0)
		for _, lat := range workerLatencies {
			sum += lat
			if lat > maxLat {
				maxLat = lat
			}
		}
		avgLat = sum / uint64(len(workerLatencies))
	}

	// TinyGo WASM: float64 marshaling in map[string]any uses the slow strconv path
	// (rightShift) for non-trivial decimals and traps. Store granularity as int×10
	// (e.g. 19 = 1.9x) to avoid any float marshaling in the response map.
	granularityX10 := 0
	if totalCoord > 0 {
		granularityX10 = int(totalCompute * 10 / totalCoord)
	}
	eventsPerSec := 0
	if wallMs > 0 {
		eventsPerSec = (params.EventCount * params.Rounds * 1000) / int(wallMs)
	}

	leaderNodeID := actorNodeID(l.ActorID())
	remoteNodes := []string{}
	for nid := range nodeParticipation {
		if nid != leaderNodeID {
			remoteNodes = append(remoteNodes, nid)
		}
	}
	sort.Strings(remoteNodes)

	// TinyGo WASM: skip ApplicationMetricsAdd — nested map[string]any with string keys
	// triggers TinyGo's broken fmtsort.Sort path via json.Marshal of nested maps.

	return marshal(map[string]any{
		"status":                "ok",
		"event_count":           params.EventCount,
		"worker_count":          params.WorkerCount,
		"batch_size":            params.BatchSize,
		"pipeline_depth":        params.PipelineDepth,
		"pipeline_functions":    pipelineFnStr,
		"sink_format":           params.SinkFormat,
		"rounds":                params.Rounds,
		"wall_time_ms":          wallMs,
		"compute_time_ms":       totalCompute,
		"coordination_time_ms":  totalCoord,
		"granularity_ratio":     granularityX10,
		"events_per_sec":        eventsPerSec,
		"total_events_in":       params.EventCount * params.Rounds,
		"total_events_out":      totalEventsOut,
		"total_events_dropped":  totalDropped,
		"avg_worker_latency_ms": avgLat,
		"max_worker_latency_ms": maxLat,
		"node_count":            len(nodeParticipation),
		"worker_node_count":     len(nodeParticipation),
		"actor_count":           params.WorkerCount + 1,
		"leader_node_id":        leaderNodeID,
		"remote_nodes_with_work": remoteNodes,
		"message_count":         params.EventCount * params.Rounds,
		"error_count":           0,
		"nodes":                 nodeParticipation,
	})
}

type scalingParams struct {
	EventCount      int   `json:"event_count"`
	ShardCounts     []int `json:"shard_counts"`
	BatchSize       int   `json:"batch_size"`
	PipelineDepth   int   `json:"pipeline_depth"`
	WarmupRounds    int   `json:"warmup_rounds"`
	BenchmarkRounds int   `json:"benchmark_rounds"`
}

type weakScalingParams struct {
	EventsPerShard  int   `json:"events_per_shard"`
	ShardCounts     []int `json:"shard_counts"`
	BatchSize       int   `json:"batch_size"`
	PipelineDepth   int   `json:"pipeline_depth"`
	WarmupRounds    int   `json:"warmup_rounds"`
	BenchmarkRounds int   `json:"benchmark_rounds"`
}

type depthParams struct {
	EventCount  int   `json:"event_count"`
	WorkerCount int   `json:"worker_count"`
	BatchSize   int   `json:"batch_size"`
	Depths      []int `json:"depths"`
}

func (l *LeaderActor) runScalingBenchmark(payloadJSON string) string {
	params := scalingParams{
		EventCount: 10000, ShardCounts: []int{1, 2, 4, 8, 16},
		BatchSize: 500, PipelineDepth: 5, WarmupRounds: 1, BenchmarkRounds: 2,
	}
	_ = json.Unmarshal([]byte(payloadJSON), &params)

	results := []map[string]any{}
	baselineThroughput := 0

	for _, numShards := range params.ShardCounts {
		// Warmup
		for i := 0; i < params.WarmupRounds; i++ {
			runJSON, _ := json.Marshal(runParams{
				EventCount: params.EventCount / 4, WorkerCount: numShards,
				BatchSize: params.BatchSize, PipelineDepth: params.PipelineDepth,
				SinkFormat: "splunk_hec", Rounds: 1,
			})
			l.run(string(runJSON))
		}

		// Benchmark
		var bestResult map[string]any
		bestThroughput := 0
		for i := 0; i < params.BenchmarkRounds; i++ {
			runJSON, _ := json.Marshal(runParams{
				EventCount: params.EventCount, WorkerCount: numShards,
				BatchSize: params.BatchSize, PipelineDepth: params.PipelineDepth,
				SinkFormat: "splunk_hec", Rounds: 1,
			})
			raw := l.run(string(runJSON))
			var r map[string]any
			_ = json.Unmarshal([]byte(raw), &r)
			throughput := intVal(r, "events_per_sec")
			if throughput > bestThroughput {
				bestThroughput = throughput
				bestResult = r
			}
		}

		if baselineThroughput == 0 {
			baselineThroughput = bestThroughput
		}

		// TinyGo WASM: store all ratios as int×100 (speedup) or int×10 (pct/ratio)
		// to avoid float64 marshaling trap in map[string]any.
		speedupX100 := 100
		if baselineThroughput > 0 {
			speedupX100 = bestThroughput * 100 / baselineThroughput
		}
		efficiencyX10 := 0
		if numShards > 0 {
			efficiencyX10 = speedupX100 * 10 / numShards
		}

		compMs := uint64(intVal(bestResult, "compute_time_ms"))
		coordMs := uint64(intVal(bestResult, "coordination_time_ms"))
		total := compMs + coordMs
		compPctX10 := 0
		if total > 0 {
			compPctX10 = int(compMs * 1000 / total)
		}

		results = append(results, map[string]any{
			"shards":               numShards,
			"events_per_sec":       bestThroughput,
			"wall_time_ms":         intVal(bestResult, "wall_time_ms"),
			"compute_time_ms":      compMs,
			"coordination_time_ms": coordMs,
			"compute_pct":          compPctX10,
			"granularity_ratio":    intVal(bestResult, "granularity_ratio"),
			"speedup":              speedupX100,
			"efficiency_pct":       efficiencyX10,
			"node_count":           intVal(bestResult, "node_count"),
			"error_count":          intVal(bestResult, "error_count"),
		})
	}

	return marshal(map[string]any{
		"status":         "ok",
		"benchmark_type": "strong_scaling",
		"event_count":    params.EventCount,
		"pipeline_depth": params.PipelineDepth,
		"batch_size":     params.BatchSize,
		"shard_counts":   params.ShardCounts,
		"results":        results,
	})
}

func (l *LeaderActor) runWeakScalingBenchmark(payloadJSON string) string {
	params := weakScalingParams{
		EventsPerShard: 10000, ShardCounts: []int{2, 4, 8, 16},
		BatchSize: 500, PipelineDepth: 5, WarmupRounds: 1, BenchmarkRounds: 2,
	}
	_ = json.Unmarshal([]byte(payloadJSON), &params)

	results := []map[string]any{}
	baselineThroughput := 0

	for _, numShards := range params.ShardCounts {
		totalEvents := params.EventsPerShard * numShards

		for i := 0; i < params.WarmupRounds; i++ {
			runJSON, _ := json.Marshal(runParams{
				EventCount: totalEvents / 4, WorkerCount: numShards,
				BatchSize: params.BatchSize, PipelineDepth: params.PipelineDepth,
				SinkFormat: "splunk_hec", Rounds: 1,
			})
			l.run(string(runJSON))
		}

		var bestResult map[string]any
		bestThroughput := 0
		for i := 0; i < params.BenchmarkRounds; i++ {
			runJSON, _ := json.Marshal(runParams{
				EventCount: totalEvents, WorkerCount: numShards,
				BatchSize: params.BatchSize, PipelineDepth: params.PipelineDepth,
				SinkFormat: "splunk_hec", Rounds: 1,
			})
			raw := l.run(string(runJSON))
			var r map[string]any
			_ = json.Unmarshal([]byte(raw), &r)
			tp := intVal(r, "events_per_sec")
			if tp > bestThroughput {
				bestThroughput = tp
				bestResult = r
			}
		}

		if baselineThroughput == 0 {
			baselineThroughput = bestThroughput
		}
		// TinyGo WASM: store as int×10 to avoid float64 marshaling trap.
		efficiencyX10 := 0
		if baselineThroughput > 0 {
			efficiencyX10 = bestThroughput * 1000 / baselineThroughput
		}
		compMs := uint64(intVal(bestResult, "compute_time_ms"))
		coordMs := uint64(intVal(bestResult, "coordination_time_ms"))
		granX10 := 0
		if coordMs > 0 {
			granX10 = int(compMs * 10 / coordMs)
		}
		results = append(results, map[string]any{
			"shards":               numShards,
			"total_events":         totalEvents,
			"events_per_sec":       bestThroughput,
			"wall_time_ms":         intVal(bestResult, "wall_time_ms"),
			"compute_time_ms":      compMs,
			"coordination_time_ms": coordMs,
			"granularity_ratio":    granX10,
			"efficiency_pct":       efficiencyX10,
			"node_count":           intVal(bestResult, "node_count"),
			"error_count":          intVal(bestResult, "error_count"),
		})
	}

	return marshal(map[string]any{
		"status":          "ok",
		"benchmark_type":  "weak_scaling",
		"events_per_shard": params.EventsPerShard,
		"pipeline_depth":  params.PipelineDepth,
		"results":         results,
	})
}

func (l *LeaderActor) runPipelineDepthBenchmark(payloadJSON string) string {
	params := depthParams{
		EventCount: 50000, WorkerCount: 8, BatchSize: 500,
		Depths: []int{1, 2, 3, 4, 5},
	}
	_ = json.Unmarshal([]byte(payloadJSON), &params)

	allFunctions := []string{"json_parse", "mask_pii", "enrich", "rename_fields", "drop"}
	results := []map[string]any{}

	for _, depth := range params.Depths {
		fns := allFunctions
		if depth < len(fns) {
			fns = fns[:depth]
		}
		runJSON, _ := json.Marshal(runParams{
			EventCount: params.EventCount, WorkerCount: params.WorkerCount,
			BatchSize: params.BatchSize, PipelineDepth: depth,
			SinkFormat: "splunk_hec", Rounds: 1,
		})
		raw := l.run(string(runJSON))
		var r map[string]any
		_ = json.Unmarshal([]byte(raw), &r)
		results = append(results, map[string]any{
			"depth":             depth,
			"functions":         strings.Join(fns, ","),
			"events_per_sec":    intVal(r, "events_per_sec"),
			"wall_time_ms":      intVal(r, "wall_time_ms"),
			"granularity_ratio": intVal(r, "granularity_ratio"),
			"error_count":       intVal(r, "error_count"),
		})
	}

	return marshal(map[string]any{
		"status":       "ok",
		"event_count":  params.EventCount,
		"worker_count": params.WorkerCount,
		"results":      results,
	})
}

// ─────────────────────────────────────────────────────────────────────────────
// Router
// ─────────────────────────────────────────────────────────────────────────────

func init() {
	h = plexspaces.NewHost()
	pipelineFunctions = map[string]pipelineFunc{
		"json_parse":    fnJSONParse,
		"regex_extract": fnRegexExtract,
		"mask_pii":      fnMaskPII,
		"enrich":        fnEnrich,
		"rename_fields": fnRenameFields,
		"drop":          fnDrop,
	}
	router := plexspaces.NewActorRouter()
	router.Route("leader", NewLeaderActor)
	router.Route("worker", NewPipelineWorkerActor)
	plexspaces.Register(router)
}

func main() {}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

func marshal(v any) string {
	b, _ := json.Marshal(v)
	return string(b)
}

func actorNodeID(actorID string) string {
	parts := strings.Split(actorID, "::")
	if len(parts) >= 2 {
		nodeAndRest := parts[0]
		if idx := strings.LastIndex(nodeAndRest, ":"); idx >= 0 {
			return nodeAndRest
		}
		return nodeAndRest
	}
	return "local"
}

func atoiDefault(s string, def int) int {
	if v, err := strconv.Atoi(s); err == nil {
		return v
	}
	return def
}

func intVal(m map[string]any, key string) int {
	if m == nil {
		return 0
	}
	switch v := m[key].(type) {
	case float64:
		return int(v)
	case int:
		return v
	case json.Number:
		n, _ := v.Int64()
		return int(n)
	}
	return 0
}

func strVal(m map[string]any, key string) string {
	if m == nil {
		return ""
	}
	s, _ := m[key].(string)
	return s
}

func unwrapPayload(v any) map[string]any {
	m, ok := v.(map[string]any)
	if !ok {
		return nil
	}
	for _, key := range []string{"payload", "result", "response", "data"} {
		if inner, ok := m[key]; ok {
			if innerMap, ok := inner.(map[string]any); ok {
				m = innerMap
			}
		}
	}
	return m
}

func anySliceOf(m map[string]any, key string) []any {
	if m == nil {
		return nil
	}
	if s, ok := m[key].([]any); ok {
		return s
	}
	return nil
}
