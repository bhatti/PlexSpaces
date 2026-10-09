// SPDX-License-Identifier: AGPL-3.0-or-later
//
// CDC (Change Data Capture) Pipeline - Go WASM
//
// PostgreSQL-style WAL events → CDC position tracking → transformation
// → fan-out to search index, analytics, cache invalidation.
// Leader/worker with shard-group placement, scatter/gather,
// compute vs coordination metrics tracking.

package main

import (
	"encoding/json"
	"fmt"
	"math"

	"github.com/bhatti/PlexSpaces/sdks/go/plexspaces"
)

var pxHost = plexspaces.NewHost()

// ─── Types ───────────────────────────────────────────────────────────────────

type WALEvent struct {
	EventID   string                 `json:"event_id"`
	LSN       int64                  `json:"lsn"`
	Table     string                 `json:"table"`
	Operation string                 `json:"operation"` // INSERT, UPDATE, DELETE
	Before    map[string]interface{} `json:"before,omitempty"`
	After     map[string]interface{} `json:"after,omitempty"`
	Timestamp int64                  `json:"timestamp"`
}

type TransformedEvent struct {
	EventID     string                 `json:"event_id"`
	LSN         int64                  `json:"lsn"`
	Table       string                 `json:"table"`
	Operation   string                 `json:"operation"`
	Data        map[string]interface{} `json:"data"`
	SinkType    string                 `json:"sink_type"`
	Timestamp   int64                  `json:"timestamp"`
	ProcessedAt int64                  `json:"processed_at"`
}

type CDCPosition struct {
	Table    string `json:"table"`
	LastLSN  int64  `json:"last_lsn"`
	EventCnt int    `json:"event_count"`
}

type SinkStats struct {
	SearchIndex       int `json:"search_index"`
	Analytics         int `json:"analytics"`
	CacheInvalidation int `json:"cache_invalidation"`
}

// ─── WAL Event Generator ────────────────────────────────────────────────────

var tables = []string{"users", "orders", "products", "inventory"}

var tableSchemas = map[string][]string{
	"users":     {"id", "first_name", "last_name", "email", "status"},
	"orders":    {"id", "user_id", "total", "status", "created_at"},
	"products":  {"id", "name", "price", "stock", "category"},
	"inventory": {"product_id", "warehouse", "quantity", "reserved"},
}

var operations = []string{"INSERT", "UPDATE", "DELETE"}

var firstNames = []string{"Alice", "Bob", "Charlie", "Diana", "Eve", "Frank", "Grace", "Hank"}
var lastNames = []string{"Smith", "Jones", "Brown", "Davis", "Wilson", "Moore", "Taylor", "Anderson"}
var statuses = []string{"active", "inactive", "pending", "suspended"}
var orderStatuses = []string{"pending", "processing", "shipped", "delivered", "cancelled"}
var categories = []string{"electronics", "clothing", "food", "books", "tools", "home"}
var warehouses = []string{"us-east-1", "us-west-2", "eu-west-1", "ap-south-1"}

func generateWALEvents(count int, seed int64) []WALEvent {
	events := make([]WALEvent, 0, count)
	rng := seed
	baseLSN := int64(1000000)
	nowMs := pxHost.NowMs()

	for i := 0; i < count; i++ {
		rng = (rng*1103515245 + 12345) & 0x7FFFFFFF
		table := tables[int(rng)%len(tables)]
		opIdx := int((rng >> 4) % 10)
		var op string
		if opIdx < 5 {
			op = "INSERT"
		} else if opIdx < 8 {
			op = "UPDATE"
		} else {
			op = "DELETE"
		}

		lsn := baseLSN + int64(i)
		eventID := fmt.Sprintf("evt-%d-%d", lsn, rng%10000)
		rng = (rng*1103515245 + 12345) & 0x7FFFFFFF

		var before, after map[string]interface{}

		switch table {
		case "users":
			rowID := int(rng%1000) + 1
			fn := firstNames[int(rng>>3)%len(firstNames)]
			ln := lastNames[int(rng>>6)%len(lastNames)]
			email := fmt.Sprintf("%s.%s@example.com", lower(fn), lower(ln))
			st := statuses[int(rng>>9)%len(statuses)]
			row := map[string]interface{}{
				"id": rowID, "first_name": fn, "last_name": ln,
				"email": email, "status": st,
			}
			if op == "DELETE" || op == "UPDATE" {
				before = row
			}
			if op == "INSERT" || op == "UPDATE" {
				if op == "UPDATE" {
					rng = (rng*1103515245 + 12345) & 0x7FFFFFFF
					row["status"] = statuses[int(rng>>9)%len(statuses)]
				}
				after = row
			}

		case "orders":
			rowID := int(rng%5000) + 1
			userID := int(rng>>3)%1000 + 1
			total := float64(int(rng%50000)+100) / 100.0
			st := orderStatuses[int(rng>>6)%len(orderStatuses)]
			row := map[string]interface{}{
				"id": rowID, "user_id": userID, "total": total,
				"status": st, "created_at": int64(nowMs) - int64(rng%86400000),
			}
			if op == "DELETE" || op == "UPDATE" {
				before = row
			}
			if op == "INSERT" || op == "UPDATE" {
				if op == "UPDATE" {
					rng = (rng*1103515245 + 12345) & 0x7FFFFFFF
					row["status"] = orderStatuses[int(rng>>6)%len(orderStatuses)]
				}
				after = row
			}

		case "products":
			rowID := int(rng%2000) + 1
			name := fmt.Sprintf("Product-%d", rowID)
			price := float64(int(rng%100000)+99) / 100.0
			stock := int(rng>>3) % 5000
			cat := categories[int(rng>>8)%len(categories)]
			row := map[string]interface{}{
				"id": rowID, "name": name, "price": price,
				"stock": stock, "category": cat,
			}
			if op == "DELETE" || op == "UPDATE" {
				before = row
			}
			if op == "INSERT" || op == "UPDATE" {
				if op == "UPDATE" {
					rng = (rng*1103515245 + 12345) & 0x7FFFFFFF
					row["stock"] = int(rng>>3) % 5000
					row["price"] = float64(int(rng%100000)+99) / 100.0
				}
				after = row
			}

		case "inventory":
			prodID := int(rng%2000) + 1
			wh := warehouses[int(rng>>5)%len(warehouses)]
			qty := int(rng>>8) % 10000
			reserved := int(rng>>12) % (qty + 1)
			row := map[string]interface{}{
				"product_id": prodID, "warehouse": wh,
				"quantity": qty, "reserved": reserved,
			}
			if op == "DELETE" || op == "UPDATE" {
				before = row
			}
			if op == "INSERT" || op == "UPDATE" {
				if op == "UPDATE" {
					rng = (rng*1103515245 + 12345) & 0x7FFFFFFF
					row["quantity"] = int(rng>>8) % 10000
				}
				after = row
			}
		}

		events = append(events, WALEvent{
			EventID:   eventID,
			LSN:       lsn,
			Table:     table,
			Operation: op,
			Before:    before,
			After:     after,
			Timestamp: int64(nowMs) + int64(i),
		})
	}
	return events
}

func lower(s string) string {
	out := make([]byte, len(s))
	for i := 0; i < len(s); i++ {
		c := s[i]
		if c >= 'A' && c <= 'Z' {
			c += 32
		}
		out[i] = c
	}
	return string(out)
}

// ─── Transformation ─────────────────────────────────────────────────────────

func transformEvent(evt WALEvent) []TransformedEvent {
	data := evt.After
	if data == nil {
		data = evt.Before
	}
	if data == nil {
		data = map[string]interface{}{}
	}

	transformed := map[string]interface{}{}
	for k, v := range data {
		transformed[k] = v
	}

	// Computed fields
	if evt.Table == "users" {
		fn := strVal(data["first_name"])
		ln := strVal(data["last_name"])
		if fn != "" || ln != "" {
			transformed["full_name"] = fn + " " + ln
		}
		if email := strVal(data["email"]); email != "" {
			for i := 0; i < len(email); i++ {
				if email[i] == '@' {
					transformed["email_domain"] = email[i+1:]
					break
				}
			}
		}
	}

	if evt.Table == "inventory" {
		qty := intVal(data["quantity"], 0)
		reserved := intVal(data["reserved"], 0)
		transformed["available"] = qty - reserved
		if qty > 0 {
			transformed["utilization_pct"] = math.Round(float64(reserved)/float64(qty)*10000) / 100
		}
	}

	if evt.Table == "orders" && evt.Operation == "UPDATE" {
		if evt.Before != nil && evt.After != nil {
			transformed["status_changed_from"] = strVal(evt.Before["status"])
			transformed["status_changed_to"] = strVal(evt.After["status"])
		}
	}

	// Field renaming
	if _, ok := transformed["id"]; ok {
		transformed[evt.Table+"_id"] = transformed["id"]
		delete(transformed, "id")
	}

	nowMs := int64(pxHost.NowMs())
	var results []TransformedEvent

	// Fan-out to search index (INSERT/UPDATE only)
	if evt.Operation != "DELETE" {
		results = append(results, TransformedEvent{
			EventID: evt.EventID, LSN: evt.LSN, Table: evt.Table,
			Operation: evt.Operation, Data: transformed,
			SinkType: "search_index", Timestamp: evt.Timestamp, ProcessedAt: nowMs,
		})
	}

	// Fan-out to analytics (all operations)
	analyticsData := map[string]interface{}{
		"table": evt.Table, "operation": evt.Operation,
		"lsn": evt.LSN, "timestamp": evt.Timestamp,
	}
	for k, v := range transformed {
		analyticsData[k] = v
	}
	results = append(results, TransformedEvent{
		EventID: evt.EventID, LSN: evt.LSN, Table: evt.Table,
		Operation: evt.Operation, Data: analyticsData,
		SinkType: "analytics", Timestamp: evt.Timestamp, ProcessedAt: nowMs,
	})

	// Fan-out to cache invalidation (UPDATE/DELETE only)
	if evt.Operation != "INSERT" {
		cacheKey := fmt.Sprintf("%s:%v", evt.Table, transformed[evt.Table+"_id"])
		results = append(results, TransformedEvent{
			EventID: evt.EventID, LSN: evt.LSN, Table: evt.Table,
			Operation: evt.Operation,
			Data:      map[string]interface{}{"cache_key": cacheKey, "action": "invalidate"},
			SinkType:  "cache_invalidation", Timestamp: evt.Timestamp, ProcessedAt: nowMs,
		})
	}

	return results
}

// ─── Actors ──────────────────────────────────────────────────────────────────

type initConfig struct {
	ActorID string            `json:"actor_id"`
	Args    map[string]string `json:"args"`
}

type LeaderActor struct {
	plexspaces.BaseActor
}

type WorkerActor struct {
	plexspaces.BaseActor
	positions  map[string]*CDCPosition
	seenEvents map[string]bool
	sinkStats  SinkStats
	computeMs  int64
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
	w.positions = map[string]*CDCPosition{}
	w.seenEvents = map[string]bool{}
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
	case "process_batch":
		return marshal(w.OnProcessBatch(payload))
	default:
		return marshal(map[string]interface{}{"error": "unknown_op", "op": op})
	}
}

func (l *LeaderActor) OnRun(payload map[string]interface{}) map[string]interface{} {
	eventCount := intVal(payload["event_count"], 10000)
	workerCount := intVal(payload["worker_count"], 8)
	batchSize := intVal(payload["batch_size"], 500)

	coordStart := pxHost.NowMs()
	groupID := fmt.Sprintf("cdc-go-%d", pxHost.NowMs())
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

	events := generateWALEvents(eventCount, int64(pxHost.NowMs()%100000))

	var totalComputeMs, totalCoordMs int64
	totalCoordMs = int64(coordCreate)
	var totalTransformed, totalDeduplicated int
	errorCount := 0
	tableCounts := map[string]int{}
	opCounts := map[string]int{}
	sinkTotals := SinkStats{}

	for i := 0; i < len(events); i += batchSize {
		end := i + batchSize
		if end > len(events) {
			end = len(events)
		}
		batch := events[i:end]

		batchPayload := map[string]interface{}{
			"op": "process_batch", "events": batch,
		}
		sgStart := pxHost.NowMs()
		sgResult, sgErr := pxHost.ScatterGather(plexspaces.ScatterGatherRequest{
			GroupID:   groupID,
			Query:     batchPayload,
			TimeoutMs: 30000,
		})
		sgElapsed := pxHost.NowMs() - sgStart
		totalCoordMs += int64(sgElapsed)
		if sgErr != nil {
			errorCount++
			continue
		}

		for _, resp := range sgResult.ShardResponses {
			result := unwrapPayload(resp)
			if _, hasErr := result["error"]; hasErr {
				errorCount++
				continue
			}
			totalComputeMs += int64(intVal(result["compute_ms"], 0))
			totalTransformed += intVal(result["transformed_count"], 0)
			totalDeduplicated += intVal(result["deduplicated_count"], 0)

			if tc, ok := result["table_counts"].(map[string]interface{}); ok {
				for t, c := range tc {
					tableCounts[t] += intVal(c, 0)
				}
			}
			if oc, ok := result["op_counts"].(map[string]interface{}); ok {
				for o, c := range oc {
					opCounts[o] += intVal(c, 0)
				}
			}
			if ss, ok := result["sink_stats"].(map[string]interface{}); ok {
				sinkTotals.SearchIndex += intVal(ss["search_index"], 0)
				sinkTotals.Analytics += intVal(ss["analytics"], 0)
				sinkTotals.CacheInvalidation += intVal(ss["cache_invalidation"], 0)
			}
		}
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
	eps := int64(0)
	if wallTime > 0 {
		eps = int64(eventCount) * 1000 / wallTime
	}

	pxHost.ApplicationMetricsAdd(l.ApplicationID(), map[string]any{
		"counter_metrics": map[string]any{
			"leader.compute":      totalComputeMs,
			"leader.coordination": totalCoordMs,
		},
	})

	return map[string]interface{}{
		"status": "ok", "event_count": eventCount, "worker_count": workerCount,
		"wall_time_ms": wallTime, "compute_time_ms": totalComputeMs,
		"coordination_time_ms": totalCoordMs, "granularity_ratio": granularity,
		"events_per_sec": eps, "transformed_count": totalTransformed,
		"deduplicated_count": totalDeduplicated,
		"table_counts": tableCounts, "op_counts": opCounts,
		"sink_stats": sinkTotals,
		"node_count": 1, "actor_count": len(shardIDs) + 1,
		"error_count": errorCount,
	}
}

func (l *LeaderActor) OnRunScalingBenchmark(payload map[string]interface{}) map[string]interface{} {
	eventCount := intVal(payload["event_count"], 10000)
	shardCounts := intSlice(payload["shard_counts"], []int{2, 4, 8, 16})
	batchSize := intVal(payload["batch_size"], 500)
	warmupRounds := intVal(payload["warmup_rounds"], 1)
	benchmarkRounds := intVal(payload["benchmark_rounds"], 2)

	var results []map[string]interface{}
	var baselineWall int64

	for _, sc := range shardCounts {
		for w := 0; w < warmupRounds; w++ {
			l.OnRun(map[string]interface{}{"event_count": min(eventCount, 1000), "worker_count": sc, "batch_size": batchSize})
		}
		var tw, tc, tco, teps int64
		var nc, ec int
		for r := 0; r < benchmarkRounds; r++ {
			res := l.OnRun(map[string]interface{}{"event_count": eventCount, "worker_count": sc, "batch_size": batchSize})
			tw += int64(intVal(res["wall_time_ms"], 0))
			tc += int64(intVal(res["compute_time_ms"], 0))
			tco += int64(intVal(res["coordination_time_ms"], 0))
			teps += int64(intVal(res["events_per_sec"], 0))
			nc = intVal(res["node_count"], 0)
			ec += intVal(res["error_count"], 0)
		}
		aw := tw / int64(benchmarkRounds)
		ac := tc / int64(benchmarkRounds)
		aco := tco / int64(benchmarkRounds)
		aeps := teps / int64(benchmarkRounds)
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
			"shards": sc, "events_per_sec": aeps, "wall_time_ms": aw,
			"compute_time_ms": ac, "coordination_time_ms": aco,
			"compute_pct": float64(ac) * 100 / float64(tot), "granularity_ratio": gran,
			"speedup": math.Round(speedup*100) / 100, "efficiency_pct": math.Round(eff*10) / 10,
			"node_count": nc, "error_count": ec,
		})
	}
	return map[string]interface{}{"status": "ok", "event_count": eventCount, "results": results}
}

func (l *LeaderActor) OnRunWeakScaling(payload map[string]interface{}) map[string]interface{} {
	eventsPerShard := intVal(payload["events_per_shard"], 5000)
	shardCounts := intSlice(payload["shard_counts"], []int{2, 4, 8, 16})
	batchSize := intVal(payload["batch_size"], 500)
	warmupRounds := intVal(payload["warmup_rounds"], 1)
	benchmarkRounds := intVal(payload["benchmark_rounds"], 2)

	var results []map[string]interface{}
	var baselineEPS int64

	for _, sc := range shardCounts {
		totalEvents := eventsPerShard * sc
		for w := 0; w < warmupRounds; w++ {
			l.OnRun(map[string]interface{}{"event_count": min(totalEvents, 1000), "worker_count": sc, "batch_size": batchSize})
		}
		var teps, tw, tc, tco int64
		var nc, ec int
		for r := 0; r < benchmarkRounds; r++ {
			res := l.OnRun(map[string]interface{}{"event_count": totalEvents, "worker_count": sc, "batch_size": batchSize})
			teps += int64(intVal(res["events_per_sec"], 0))
			tw += int64(intVal(res["wall_time_ms"], 0))
			tc += int64(intVal(res["compute_time_ms"], 0))
			tco += int64(intVal(res["coordination_time_ms"], 0))
			nc = intVal(res["node_count"], 0)
			ec += intVal(res["error_count"], 0)
		}
		aeps := teps / int64(benchmarkRounds)
		aw := tw / int64(benchmarkRounds)
		ac := tc / int64(benchmarkRounds)
		aco := tco / int64(benchmarkRounds)
		if baselineEPS == 0 {
			baselineEPS = aeps
		}
		eff := 100.0
		if baselineEPS > 0 {
			eff = float64(aeps) / float64(baselineEPS) * 100
		}
		gran := 0.0
		if aco > 0 {
			gran = math.Round(float64(ac)/float64(aco)*10) / 10
		}
		results = append(results, map[string]interface{}{
			"shards": sc, "total_events": totalEvents, "events_per_sec": aeps,
			"wall_time_ms": aw, "compute_time_ms": ac, "coordination_time_ms": aco,
			"granularity_ratio": gran, "efficiency_pct": math.Round(eff*10) / 10,
			"node_count": nc, "error_count": ec,
		})
	}
	return map[string]interface{}{"status": "ok", "events_per_shard": eventsPerShard, "results": results}
}

func (w *WorkerActor) OnProcessBatch(payload map[string]interface{}) map[string]interface{} {
	compStart := pxHost.NowMs()
	rawEvents := sliceVal(payload["events"])

	if w.positions == nil {
		w.positions = map[string]*CDCPosition{}
	}
	if w.seenEvents == nil {
		w.seenEvents = map[string]bool{}
	}

	events := make([]WALEvent, 0, len(rawEvents))
	for _, re := range rawEvents {
		if m, ok := re.(map[string]interface{}); ok {
			events = append(events, walEventFromMap(m))
		}
	}

	var transformed []TransformedEvent
	deduplicated := 0
	tableCounts := map[string]int{}
	opCounts := map[string]int{}
	localSinkStats := SinkStats{}

	for _, evt := range events {
		// Deduplication
		if w.seenEvents[evt.EventID] {
			deduplicated++
			continue
		}
		w.seenEvents[evt.EventID] = true

		// Position tracking
		pos, ok := w.positions[evt.Table]
		if !ok {
			pos = &CDCPosition{Table: evt.Table}
			w.positions[evt.Table] = pos
		}
		if evt.LSN > pos.LastLSN {
			pos.LastLSN = evt.LSN
		}
		pos.EventCnt++

		// Transform and fan-out
		results := transformEvent(evt)
		for _, r := range results {
			switch r.SinkType {
			case "search_index":
				localSinkStats.SearchIndex++
			case "analytics":
				localSinkStats.Analytics++
			case "cache_invalidation":
				localSinkStats.CacheInvalidation++
			}
		}
		transformed = append(transformed, results...)

		tableCounts[evt.Table]++
		opCounts[evt.Operation]++
	}

	computeMs := int64(pxHost.NowMs() - compStart)
	w.computeMs += computeMs
	w.sinkStats.SearchIndex += localSinkStats.SearchIndex
	w.sinkStats.Analytics += localSinkStats.Analytics
	w.sinkStats.CacheInvalidation += localSinkStats.CacheInvalidation

	pxHost.ApplicationMetricsAdd(w.ApplicationID(), map[string]any{
		"counter_metrics": map[string]any{"worker.compute": computeMs},
	})

	return map[string]interface{}{
		"events_in":         len(events),
		"transformed_count": len(transformed),
		"deduplicated_count": deduplicated,
		"table_counts":      tableCounts,
		"op_counts":         opCounts,
		"sink_stats": map[string]interface{}{
			"search_index":       localSinkStats.SearchIndex,
			"analytics":          localSinkStats.Analytics,
			"cache_invalidation": localSinkStats.CacheInvalidation,
		},
		"compute_ms": computeMs,
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

func sliceVal(v interface{}) []interface{} {
	arr, ok := v.([]interface{})
	if !ok {
		return nil
	}
	return arr
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

func marshal(v interface{}) string {
	b, _ := json.Marshal(v)
	return string(b)
}

func unwrapPayload(resp map[string]interface{}) map[string]interface{} {
	if r, ok := resp["result"].(map[string]interface{}); ok {
		return r
	}
	return resp
}

func strVal(v interface{}) string {
	if s, ok := v.(string); ok {
		return s
	}
	return ""
}

func walEventFromMap(m map[string]interface{}) WALEvent {
	var before, after map[string]interface{}
	if b, ok := m["before"].(map[string]interface{}); ok {
		before = b
	}
	if a, ok := m["after"].(map[string]interface{}); ok {
		after = a
	}
	return WALEvent{
		EventID:   strVal(m["event_id"]),
		LSN:       int64(intVal(m["lsn"], 0)),
		Table:     strVal(m["table"]),
		Operation: strVal(m["operation"]),
		Before:    before,
		After:     after,
		Timestamp: int64(intVal(m["timestamp"], 0)),
	}
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
