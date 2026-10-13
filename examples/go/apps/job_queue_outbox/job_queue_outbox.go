// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Job Queue Outbox Actor — Go WASM
//
// Demonstrates high-scale outbox dispatch with admission control:
//   - Leader creates N virtual workers (default 100) via CreateShardGroup
//   - Leader dispatches jobs to all workers via outbox
//   - Upgrade simulation: leader Ask()s workers for upgrade status
//   - Heartbeat OVERWRITE: LWW idempotency with conflict_resolution="overwrite"
//   - Rich metrics: wall_time_ms, ask RTT (min/max/avg), per-worker stats
//
// ## TinyGo WASM JSON Safety Rules
//
//   FORBIDDEN in TinyGo WASM actors (corrupts function table → call_indirect trap):
//     - encoding/json.Marshal  (links fmtsort)
//     - fmt.Sprintf / fmt.Errorf / fmt.Sscanf  (interface dispatch via %v/%d/%w)
//
//   SAFE — use these instead:
//     - plexspaces.SafeMarshal(map[string]any{...})  (type-switch encoding, no fmtsort)
//     - plexspaces.WasmJSONFindObject/ExtractStr/ExtractInt  (zero-alloc JSON read)
//     - plexspaces.WasmJSONFindArray + WasmJSONSplitArray  (string array decoding)
//     - strconv.Itoa / strconv.FormatInt / strconv.ParseInt  (no interface dispatch)
//     - Raw string building via strings.Builder or concatenation

package main

import (
	"strconv"
	"strings"

	"github.com/bhatti/PlexSpaces/sdks/go/plexspaces"
)

const (
	dispatcherID    = "job-outbox-dispatcher"
	defaultWorkers  = 100
	heartbeatIdemKey = "hb-leader"
)

var host = plexspaces.NewHost()

// ============================================================================
// JobQueueLeader — Coordinator Actor
// ============================================================================

type JobQueueLeader struct {
	plexspaces.BaseActor

	// Dispatch tracking
	TotalDispatched int `json:"total_dispatched"`
	TotalAccepted   int `json:"total_accepted"`
	TotalRejected   int `json:"total_rejected"`
	DispatchCount   int `json:"dispatch_count"`
	WallTimeMs      int `json:"wall_time_ms"`

	// Heartbeat tracking
	HeartbeatCount int `json:"heartbeat_count"`

	// Upgrade Ask RTT tracking (milliseconds)
	AskRttMin   int `json:"ask_rtt_min_ms"`
	AskRttMax   int `json:"ask_rtt_max_ms"`
	AskRttSum   int `json:"ask_rtt_sum_ms"`
	AskRttCount int `json:"ask_rtt_count"`

	// Worker shard group
	ShardGroupID string   `json:"shard_group_id"`
	WorkerIDs    []string `json:"worker_ids"`
}

func NewJobQueueLeader() plexspaces.Actor {
	a := &JobQueueLeader{}
	a.SetSelf(a)
	return a
}

func (j *JobQueueLeader) Init(configJSON string) string {
	actorID := plexspaces.WasmJSONExtractStr(configJSON, "actor_id")
	j.SetRuntimeMetadata(actorID)
	host.Info("JobQueueLeader " + j.ActorID() + ": initialized")
	return ""
}

func (j *JobQueueLeader) Handle(fromActor, msgType, payloadJSON string) string {
	switch msgType {
	case "init_workers":
		return j.handleInitWorkers(payloadJSON)
	case "dispatch_jobs":
		return j.handleDispatchJobs(payloadJSON)
	case "upgrade_workers":
		return j.handleUpgradeWorkers(payloadJSON)
	case "send_heartbeat":
		return j.handleSendHeartbeat(payloadJSON)
	case "get_metrics":
		return j.handleGetMetrics()
	case "scaling_benchmark":
		return j.handleScalingBenchmark(payloadJSON)
	// Backward-compat handlers for existing test.sh
	case "dispatch":
		return j.handleDispatch(payloadJSON)
	case "batch_dispatch":
		return j.handleBatchDispatch(payloadJSON)
	case "status":
		return j.handleStatus()
	case "summary":
		return j.handleSummary()
	default:
		return marshal(map[string]any{"error": "unknown operation: " + msgType})
	}
}

// handleInitWorkers creates N virtual workers via CreateShardGroup and stores their IDs.
func (j *JobQueueLeader) handleInitWorkers(payloadJSON string) string {
	count := plexspaces.WasmJSONExtractInt(payloadJSON, "count")
	if count <= 0 {
		count = defaultWorkers
	}

	suffix := plexspaces.WasmJSONExtractStr(payloadJSON, "group_suffix")
	groupID := "go-job-workers"
	if suffix != "" {
		groupID = "go-job-workers-" + suffix
	}
	resp, err := host.CreateShardGroup(plexspaces.CreateShardGroupRequest{
		GroupID:    groupID,
		ActorType:  "JobQueueWorker",
		ShardCount: count,
		Placement:  plexspaces.NodePlacement{Strategy: "from_registry"},
	})
	if err != nil {
		return marshal(map[string]any{"error": "create_shard_group failed: " + err.Error()})
	}

	j.ShardGroupID = resp.GroupID
	j.WorkerIDs = resp.ShardActorIDs

	host.Info("init_workers: created " + strconv.Itoa(len(j.WorkerIDs)) + " workers in group " + groupID)
	return marshal(map[string]any{
		"success":      true,
		"group_id":     j.ShardGroupID,
		"worker_count": len(j.WorkerIDs),
		"worker_ids":   j.WorkerIDs,
	})
}

// handleDispatchJobs dispatches one job per worker via outbox, tracking wall time.
func (j *JobQueueLeader) handleDispatchJobs(payloadJSON string) string {
	if len(j.WorkerIDs) == 0 {
		return marshal(map[string]any{"error": "call init_workers first"})
	}

	rawPayload := plexspaces.WasmJSONFindObject(payloadJSON, "payload")
	if rawPayload == "" {
		rawPayload = `{"task":"process","priority":"normal"}`
	}

	startMs := int(host.NowMs())
	ctx := `{"tenant":"default","namespace":"default"}`
	result, err := host.Outbox().DispatchWithTargets(ctx, dispatcherID, j.WorkerIDs, rawPayload, "cast", "", "")
	endMs := int(host.NowMs())

	accepted, rejected := 0, 0
	if err == nil && result != nil {
		accepted = int(result.Accepted)
		rejected = int(result.Rejected)
	}
	wallMs := endMs - startMs

	j.TotalDispatched += len(j.WorkerIDs)
	j.TotalAccepted += accepted
	j.TotalRejected += rejected
	j.DispatchCount++
	j.WallTimeMs = wallMs

	host.Info("dispatch_jobs: " + strconv.Itoa(len(j.WorkerIDs)) + " workers accepted=" +
		strconv.Itoa(accepted) + " wall_ms=" + strconv.Itoa(wallMs))

	return marshal(map[string]any{
		"success":          true,
		"accepted":         accepted,
		"rejected":         rejected,
		"total_dispatched": j.TotalDispatched,
		"wall_time_ms":     wallMs,
	})
}

// handleUpgradeWorkers asks N random workers for their upgrade status.
func (j *JobQueueLeader) handleUpgradeWorkers(payloadJSON string) string {
	askCount := plexspaces.WasmJSONExtractInt(payloadJSON, "count")
	if askCount <= 0 {
		askCount = 10
	}

	workers := j.WorkerIDs
	// Fall back to static workers from app-config.toml (actor_type=OutboxWorker)
	if len(workers) == 0 {
		workers = []string{
			"OutboxWorker:worker-0",
			"OutboxWorker:worker-1",
			"OutboxWorker:worker-2",
			"OutboxWorker:worker-3",
		}
	}
	if askCount > len(workers) {
		askCount = len(workers)
	}

	// Simple deterministic selection (no math/rand in TinyGo WASM)
	step := len(workers) / askCount
	if step < 1 {
		step = 1
	}

	var replies []any
	var totalRtt int
	minRtt := -1
	maxRtt := 0

	for i := 0; i < askCount; i++ {
		idx := (i * step) % len(workers)
		workerID := workers[idx]

		startMs := int(host.NowMs())
		reply, err := host.Ask(workerID, "upgrade", map[string]any{"requester": j.ActorID()}, 5000)
		endMs := int(host.NowMs())
		rtt := endMs - startMs
		totalRtt += rtt
		if minRtt < 0 || rtt < minRtt {
			minRtt = rtt
		}
		if rtt > maxRtt {
			maxRtt = rtt
		}

		if err != nil {
			replies = append(replies, map[string]any{"worker_id": workerID, "error": err.Error()})
		} else {
			replyStr, _ := reply.(string)
			status := plexspaces.WasmJSONExtractStr(replyStr, "status")
			version := plexspaces.WasmJSONExtractStr(replyStr, "version")
			replies = append(replies, map[string]any{
				"worker_id": workerID,
				"status":    status,
				"version":   version,
				"rtt_ms":    rtt,
			})
		}
	}

	// Update running RTT stats
	j.AskRttSum += totalRtt
	j.AskRttCount += askCount
	if minRtt >= 0 {
		if j.AskRttMin == 0 || minRtt < j.AskRttMin {
			j.AskRttMin = minRtt
		}
	}
	if maxRtt > j.AskRttMax {
		j.AskRttMax = maxRtt
	}

	avgRtt := 0
	if askCount > 0 {
		avgRtt = totalRtt / askCount
	}

	return marshal(map[string]any{
		"success":        true,
		"workers_asked":  askCount,
		"replies":        replies,
		"ask_rtt_min_ms": minRtt,
		"ask_rtt_max_ms": maxRtt,
		"ask_rtt_avg_ms": avgRtt,
	})
}

// handleSendHeartbeat demonstrates OVERWRITE idempotency: same key, latest message wins.
func (j *JobQueueLeader) handleSendHeartbeat(payloadJSON string) string {
	seq := plexspaces.WasmJSONExtractInt(payloadJSON, "seq")
	if seq <= 0 {
		j.HeartbeatCount++
		seq = j.HeartbeatCount
	} else {
		j.HeartbeatCount = seq
	}

	workers := j.WorkerIDs
	if len(workers) == 0 {
		workers = []string{
			"OutboxWorker:worker-0",
			"OutboxWorker:worker-1",
			"OutboxWorker:worker-2",
			"OutboxWorker:worker-3",
		}
	}

	// All heartbeats use the same idempotency key with OVERWRITE so only the latest
	// heartbeat is queued if the worker hasn't processed the previous one yet.
	hbPayload := `{"type":"heartbeat","seq":` + strconv.Itoa(seq) + `}`
	ctx := `{"tenant":"default","namespace":"default"}`
	result, err := host.Outbox().DispatchWithTargets(ctx, dispatcherID, workers, hbPayload, "cast", heartbeatIdemKey, "overwrite")

	accepted, rejected := 0, 0
	if err == nil && result != nil {
		accepted = int(result.Accepted)
		rejected = int(result.Rejected)
	}

	host.Info("heartbeat seq=" + strconv.Itoa(seq) + " accepted=" + strconv.Itoa(accepted))

	return marshal(map[string]any{
		"success":          true,
		"seq":              seq,
		"accepted":         accepted,
		"rejected":         rejected,
		"heartbeat_count":  j.HeartbeatCount,
	})
}

// handleGetMetrics returns all leader-side metrics.
func (j *JobQueueLeader) handleGetMetrics() string {
	avgRtt := 0
	if j.AskRttCount > 0 {
		avgRtt = j.AskRttSum / j.AskRttCount
	}
	throughput := 0
	if j.WallTimeMs > 0 && j.TotalDispatched > 0 {
		throughput = j.TotalDispatched * 1000 / j.WallTimeMs
	}
	return marshal(map[string]any{
		"success":          true,
		"total_dispatched": j.TotalDispatched,
		"total_accepted":   j.TotalAccepted,
		"total_rejected":   j.TotalRejected,
		"dispatch_count":   j.DispatchCount,
		"wall_time_ms":     j.WallTimeMs,
		"throughput_per_s": throughput,
		"heartbeat_count":  j.HeartbeatCount,
		"ask_rtt_min_ms":   j.AskRttMin,
		"ask_rtt_max_ms":   j.AskRttMax,
		"ask_rtt_avg_ms":   avgRtt,
		"worker_count":     len(j.WorkerIDs),
		"shard_group_id":   j.ShardGroupID,
	})
}

// handleScalingBenchmark measures dispatch throughput at 10/50/100 worker scales.
// Reuses the existing WorkerIDs pool (sliced) to avoid accumulating stale actors
// on the node — consistent with the Python and TypeScript implementations.
func (j *JobQueueLeader) handleScalingBenchmark(payloadJSON string) string {
	if len(j.WorkerIDs) == 0 {
		return marshal(map[string]any{"error": "no workers — call init_workers first"})
	}

	available := j.WorkerIDs
	scales := []int{10, 50, 100}
	type row struct {
		workers    int
		accepted   int
		wallMs     int
		throughput int
		speedup100 int
	}
	var rows []row
	base := 1

	ctx := "{\"tenant\":\"default\",\"namespace\":\"default\"}"
	benchPayload := "{\"task\":\"benchmark_job\"}"

	for _, n := range scales {
		if n > len(available) {
			n = len(available)
		}
		targets := available[:n]

		t0 := host.NowMs()
		result, err := host.Outbox().DispatchWithTargets(ctx, dispatcherID, targets, benchPayload, "cast", "", "")
		wallMs := int(host.NowMs() - t0)
		if wallMs <= 0 {
			wallMs = 1
		}

		accepted := 0
		if err == nil && result != nil {
			accepted = int(result.Accepted)
		}
		throughput := accepted * 1000 / wallMs

		if len(rows) == 0 {
			base = throughput
			if base <= 0 {
				base = 1
			}
		}
		speedup := throughput * 100 / base

		rows = append(rows, row{
			workers:    n,
			accepted:   accepted,
			wallMs:     wallMs,
			throughput: throughput,
			speedup100: speedup,
		})
	}

	scaleResults := make([]any, 0, len(rows))
	for _, r := range rows {
		scaleResults = append(scaleResults, map[string]any{
			"workers":      r.workers,
			"accepted":     r.accepted,
			"wall_time_ms": r.wallMs,
			"throughput":   r.throughput,
			"speedup_pct":  r.speedup100,
		})
	}
	return marshal(map[string]any{"success": true, "scales": scaleResults})
}

// handleDispatch is the backward-compat handler for existing test.sh.
func (j *JobQueueLeader) handleDispatch(payloadJSON string) string {
	targets := extractStringArray(payloadJSON, "targets")
	messageType := plexspaces.WasmJSONExtractStr(payloadJSON, "message_type")
	rawPayload := plexspaces.WasmJSONFindObject(payloadJSON, "payload")
	if rawPayload == "" {
		rawPayload = `{"task":"process"}`
	}
	return j.doDispatch(targets, rawPayload, messageType)
}

// handleBatchDispatch dispatches batchSize messages round-robin across workers.
func (j *JobQueueLeader) handleBatchDispatch(payloadJSON string) string {
	batchSize := plexspaces.WasmJSONExtractInt(payloadJSON, "batch_size")
	if batchSize <= 0 {
		batchSize = 100
	}

	workers := j.WorkerIDs
	if len(workers) == 0 {
		workers = []string{
			"JobQueueWorker:worker-0",
			"JobQueueWorker:worker-1",
			"JobQueueWorker:worker-2",
			"JobQueueWorker:worker-3",
		}
	}
	targets := make([]string, batchSize)
	for i := 0; i < batchSize; i++ {
		targets[i] = workers[i%len(workers)]
	}
	rawPayload := plexspaces.WasmJSONFindObject(payloadJSON, "payload")
	if rawPayload == "" {
		rawPayload = `{"task":"batch_job"}`
	}
	return j.doDispatch(targets, rawPayload, "cast")
}

func (j *JobQueueLeader) handleStatus() string {
	ctx := `{"tenant":"default","namespace":"default"}`
	relayStatus, err := host.Outbox().Status(ctx, dispatcherID)
	if err != nil {
		relayStatus = map[string]any{"error": err.Error()}
	}
	return marshal(map[string]any{
		"success":          true,
		"dispatcher_id":    dispatcherID,
		"relay_status":     relayStatus,
		"total_dispatched": j.TotalDispatched,
		"total_accepted":   j.TotalAccepted,
		"total_rejected":   j.TotalRejected,
	})
}

func (j *JobQueueLeader) handleSummary() string {
	return marshal(map[string]any{
		"success":          true,
		"total_dispatched": j.TotalDispatched,
		"total_accepted":   j.TotalAccepted,
		"total_rejected":   j.TotalRejected,
		"dispatch_count":   j.DispatchCount,
	})
}

func (j *JobQueueLeader) doDispatch(targets []string, payloadJSON, messageType string) string {
	if len(targets) == 0 {
		return marshal(map[string]any{"success": false, "error": "targets required"})
	}
	if messageType == "" {
		messageType = "cast"
	}
	if payloadJSON == "" || payloadJSON == "null" {
		payloadJSON = `{"task":"process"}`
	}

	ctx := `{"tenant":"default","namespace":"default"}`
	result, err := host.Outbox().DispatchWithTargets(ctx, dispatcherID, targets, payloadJSON, messageType, "", "")
	accepted, rejected := 0, 0
	if err == nil && result != nil {
		accepted = int(result.Accepted)
		rejected = int(result.Rejected)
	}

	j.TotalDispatched += len(targets)
	j.TotalAccepted += accepted
	j.TotalRejected += rejected
	j.DispatchCount++

	return marshal(map[string]any{
		"success":          true,
		"accepted":         accepted,
		"rejected":         rejected,
		"total_dispatched": j.TotalDispatched,
	})
}

// MarshalStateWASM persists leader state across WASM re-instantiation.
func (j *JobQueueLeader) MarshalStateWASM() string {
	return marshal(map[string]any{
		"total_dispatched": j.TotalDispatched,
		"total_accepted":   j.TotalAccepted,
		"total_rejected":   j.TotalRejected,
		"dispatch_count":   j.DispatchCount,
		"wall_time_ms":     j.WallTimeMs,
		"heartbeat_count":  j.HeartbeatCount,
		"ask_rtt_min_ms":   j.AskRttMin,
		"ask_rtt_max_ms":   j.AskRttMax,
		"ask_rtt_sum_ms":   j.AskRttSum,
		"ask_rtt_count":    j.AskRttCount,
		"shard_group_id":   j.ShardGroupID,
		"worker_ids":       j.WorkerIDs,
	})
}

// UnmarshalStateWASM restores leader state from persisted snapshot.
func (j *JobQueueLeader) UnmarshalStateWASM(stateJSON string) bool {
	j.TotalDispatched = plexspaces.WasmJSONExtractInt(stateJSON, "total_dispatched")
	j.TotalAccepted = plexspaces.WasmJSONExtractInt(stateJSON, "total_accepted")
	j.TotalRejected = plexspaces.WasmJSONExtractInt(stateJSON, "total_rejected")
	j.DispatchCount = plexspaces.WasmJSONExtractInt(stateJSON, "dispatch_count")
	j.WallTimeMs = plexspaces.WasmJSONExtractInt(stateJSON, "wall_time_ms")
	j.HeartbeatCount = plexspaces.WasmJSONExtractInt(stateJSON, "heartbeat_count")
	j.AskRttMin = plexspaces.WasmJSONExtractInt(stateJSON, "ask_rtt_min_ms")
	j.AskRttMax = plexspaces.WasmJSONExtractInt(stateJSON, "ask_rtt_max_ms")
	j.AskRttSum = plexspaces.WasmJSONExtractInt(stateJSON, "ask_rtt_sum_ms")
	j.AskRttCount = plexspaces.WasmJSONExtractInt(stateJSON, "ask_rtt_count")
	j.ShardGroupID = plexspaces.WasmJSONExtractStr(stateJSON, "shard_group_id")
	j.WorkerIDs = extractStringArray(stateJSON, "worker_ids")
	return true
}

// ============================================================================
// JobQueueWorker — Worker Actor
// ============================================================================

type JobQueueWorker struct {
	plexspaces.BaseActor

	JobsReceived     int `json:"jobs_received"`
	LastHeartbeatSeq int `json:"last_heartbeat_seq"`
}

func NewJobQueueWorker() plexspaces.Actor {
	w := &JobQueueWorker{}
	w.SetSelf(w)
	return w
}

func (w *JobQueueWorker) Init(configJSON string) string {
	actorID := plexspaces.WasmJSONExtractStr(configJSON, "actor_id")
	w.SetRuntimeMetadata(actorID)
	host.Info("JobQueueWorker " + w.ActorID() + ": initialized")
	return ""
}

func (w *JobQueueWorker) Handle(fromActor, msgType, payloadJSON string) string {
	switch msgType {
	case "process", "cast":
		w.JobsReceived++
		host.Info("Worker " + w.ActorID() + " processed job #" + strconv.Itoa(w.JobsReceived))
		return marshal(map[string]any{"status": "ok", "jobs_received": w.JobsReceived})
	case "upgrade":
		return marshal(map[string]any{
			"status":    "need_package",
			"version":   "2.1.0",
			"worker_id": w.ActorID(),
		})
	case "heartbeat":
		seq := plexspaces.WasmJSONExtractInt(payloadJSON, "seq")
		if seq > w.LastHeartbeatSeq {
			w.LastHeartbeatSeq = seq
		}
		return marshal(map[string]any{"status": "ok", "last_seq": w.LastHeartbeatSeq})
	case "stats":
		return marshal(map[string]any{
			"success":            true,
			"jobs_received":      w.JobsReceived,
			"last_heartbeat_seq": w.LastHeartbeatSeq,
		})
	default:
		return marshal(map[string]any{"error": "unknown: " + msgType})
	}
}

// MarshalStateWASM persists worker counters across WASM re-instantiation.
func (w *JobQueueWorker) MarshalStateWASM() string {
	return marshal(map[string]any{
		"jobs_received":      w.JobsReceived,
		"last_heartbeat_seq": w.LastHeartbeatSeq,
	})
}

// UnmarshalStateWASM restores worker counters from persisted state.
func (w *JobQueueWorker) UnmarshalStateWASM(stateJSON string) bool {
	w.JobsReceived = plexspaces.WasmJSONExtractInt(stateJSON, "jobs_received")
	w.LastHeartbeatSeq = plexspaces.WasmJSONExtractInt(stateJSON, "last_heartbeat_seq")
	return true
}

// ============================================================================
// Utilities
// ============================================================================

// marshal encodes response values using SafeMarshal — avoids encoding/json.Marshal
// which links fmtsort into the WASM function table, corrupting actor dispatch.
func marshal(v any) string {
	return plexspaces.SafeMarshal(v)
}

// extractStringArray decodes a JSON string array from key in obj without encoding/json.
func extractStringArray(obj, key string) []string {
	arrJSON := plexspaces.WasmJSONFindArray(obj, key)
	if len(arrJSON) < 2 {
		return nil
	}
	inner := arrJSON[1 : len(arrJSON)-1]
	items := plexspaces.WasmJSONSplitArray(inner)
	result := make([]string, 0, len(items))
	for _, item := range items {
		item = strings.TrimSpace(item)
		if len(item) == 0 {
			continue
		}
		if item[0] == '"' {
			str, _ := plexspaces.WasmJSONConsumeParsedString(item)
			result = append(result, str)
		} else if item != "null" {
			result = append(result, item)
		}
	}
	return result
}

// ============================================================================
// Registration
// ============================================================================

func init() {
	router := plexspaces.NewActorRouter()
	// "JobQueueOutboxCoordinator" kept for backward compat with existing test.sh
	router.Route("JobQueueOutboxCoordinator", NewJobQueueLeader)
	router.Route("JobQueueLeader", NewJobQueueLeader)
	router.Route("OutboxWorker", NewJobQueueWorker)
	router.Route("JobQueueWorker", NewJobQueueWorker)
	plexspaces.Register(router)
}

func main() {}
