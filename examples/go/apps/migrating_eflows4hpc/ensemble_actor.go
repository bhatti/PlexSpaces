// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// EFlows4HPC → PlexSpaces: HPC Ensemble (Go WASM)
//
// Single actor: coordinator (Run) and workers (Handle with msgType "tasks_ready").
// Uses host.TS() (tuple space) and host.PG() (process group join/broadcast).

package main

import (
	"encoding/json"
	"strconv"

	"github.com/plexobject/plexspaces/sdks/go/plexspaces"
)

var host = plexspaces.NewHost()

const (
	taskMs        = 18
	resultPrefix  = "ensemble"
	workerGroup   = "ensemble-workers"
	gatherPollMax = 200
)

// EnsembleActor implements WorkflowActor for HPC ensemble (scatter → broadcast → gather).
type EnsembleActor struct {
	plexspaces.BaseActor

	EnsembleID       string  `json:"ensemble_id"`
	NumTasks         int     `json:"num_tasks"`
	NumCompleted     int     `json:"num_completed"`
	Status           string  `json:"status"`
	TotalComputeMs   float64 `json:"total_compute_ms"`
	TotalCoordMs     float64 `json:"total_coord_ms"`
	CreatedAtMs      uint64  `json:"created_at_ms"`
	UpdatedAtMs      uint64  `json:"updated_at_ms"`
	CancelRequested  bool    `json:"cancel_requested"`
	WorkerJoined     bool    `json:"worker_joined"`
}

func NewEnsembleActor() *EnsembleActor {
	a := &EnsembleActor{Status: "idle"}
	a.SetSelf(a)
	return a
}

func (e *EnsembleActor) Init(configJSON string) string {
	return ""
}

func (e *EnsembleActor) Run(payloadJSON string) string {
	t0 := host.NowMs()
	if e.CreatedAtMs == 0 {
		e.CreatedAtMs = t0
	}
	var payload map[string]any
	_ = json.Unmarshal([]byte(payloadJSON), &payload)
	e.UpdatedAtMs = host.NowMs()

	if e.CancelRequested {
		e.Status = "cancelled"
		return e.finish(t0, 0, "cancelled")
	}
	if e.Status == "completed" {
		return e.finish(t0, 0, "completed")
	}

	numTasks := 10
	if n, ok := payload["num_tasks"].(float64); ok && n > 0 {
		numTasks = int(n)
	} else if n, ok := payload["num_tasks"].(int); ok && n > 0 {
		numTasks = n
	}
	if id, ok := payload["ensemble_id"].(string); ok && id != "" {
		e.EnsembleID = id
	} else if id, ok := payload["job_id"].(string); ok && id != "" {
		e.EnsembleID = id
	}
	if numTasks <= 0 {
		return e.finish(t0, 0, "no_tasks")
	}

	e.NumTasks = numTasks
	e.NumCompleted = 0
	e.Status = "scattering"
	computeMs := 0.0

	for i := 0; i < numTasks; i++ {
		taskID := "t" + strconv.Itoa(i)
		out := host.TS().Write([]any{resultPrefix, e.EnsembleID, "task", taskID, i})
		if out != "" && len(out) >= 5 && out[:5] == "ERROR" {
			host.Log("warn", "ts write failed: "+out)
		}
		computeMs += 2
	}
	e.UpdatedAtMs = host.NowMs()
	e.Status = "running"

	if err := host.PG().Broadcast(workerGroup, "tasks_ready", map[string]any{
		"ensemble_id": e.EnsembleID,
		"num_tasks":   numTasks,
	}); err != nil {
		host.Log("warn", "pg_broadcast failed: "+err.Error())
	}
	e.TotalCoordMs += float64(host.NowMs() - t0)

	pattern := []any{resultPrefix, e.EnsembleID, "result", nil, nil}
	for i := 0; i < gatherPollMax; i++ {
		if e.CancelRequested {
			e.Status = "cancelled"
			return e.finish(t0, computeMs, "cancelled")
		}
		results := host.TS().ReadAll(pattern)
		e.NumCompleted = len(results)
		if e.NumCompleted >= numTasks {
			break
		}
		e.UpdatedAtMs = host.NowMs()
	}

	e.Status = "completed"
	e.UpdatedAtMs = host.NowMs()
	return e.finish(t0, computeMs, "completed")
}

func (e *EnsembleActor) finish(t0 uint64, computeMs float64, status string) string {
	elapsed := float64(host.NowMs() - t0)
	if elapsed < computeMs {
		elapsed = computeMs
	}
	coordMs := elapsed - computeMs
	if coordMs < 0 {
		coordMs = 0
	}
	e.TotalComputeMs += computeMs
	e.TotalCoordMs += coordMs
	return marshal(map[string]any{
		"status":             status,
		"ensemble_id":        e.EnsembleID,
		"num_tasks":          e.NumTasks,
		"num_completed":      e.NumCompleted,
		"total_compute_ms":   e.TotalComputeMs,
		"total_coord_ms":     e.TotalCoordMs,
	})
}

func (e *EnsembleActor) Signal(name, _ string) {
	if name == "cancel" {
		e.CancelRequested = true
		e.UpdatedAtMs = host.NowMs()
	}
}

func (e *EnsembleActor) Query(name, _ string) string {
	if name == "status" {
		return marshal(map[string]any{
			"ensemble_id":        e.EnsembleID,
			"status":             e.Status,
			"num_tasks":          e.NumTasks,
			"num_completed":      e.NumCompleted,
			"cancel_requested":   e.CancelRequested,
			"total_compute_ms":   e.TotalComputeMs,
			"total_coord_ms":     e.TotalCoordMs,
			"created_at_ms":      e.CreatedAtMs,
			"updated_at_ms":      e.UpdatedAtMs,
		})
	}
	return marshal(map[string]any{"error": "unknown_query", "name": name})
}

// Handle processes non-workflow messages; msgType "tasks_ready" = worker logic.
func (e *EnsembleActor) Handle(from, msgType, payloadJSON string) string {
	if msgType != "tasks_ready" {
		return marshal(map[string]any{"error": "unknown_op", "op": msgType})
	}
	// Worker: join group on first use, then take tasks from tuple space
	if !e.WorkerJoined {
		if err := host.PG().Join(workerGroup); err != nil {
			host.Log("warn", "pg_join failed: "+err.Error())
		} else {
			e.WorkerJoined = true
			host.Log("info", "Joined process group "+workerGroup)
		}
	}
	var payload map[string]any
	_ = json.Unmarshal([]byte(payloadJSON), &payload)
	ensembleID, _ := payload["ensemble_id"].(string)
	numTasks := 0
	if n, ok := payload["num_tasks"].(float64); ok {
		numTasks = int(n)
	} else if n, ok := payload["num_tasks"].(int); ok {
		numTasks = n
	}
	if ensembleID == "" || numTasks <= 0 {
		return marshal(map[string]any{"ok": true, "message": "warmup"})
	}

	t0 := host.NowMs()
	pattern := []any{resultPrefix, ensembleID, "task", nil, nil}
	processed := 0
	computeMs := 0.0
	for {
		tuple, ok := host.TS().Take(pattern)
		if !ok {
			break
		}
		taskID := ""
		if len(tuple) > 3 {
			switch v := tuple[3].(type) {
			case string:
				taskID = v
			case float64:
				taskID = strconv.Itoa(int(v))
			}
		}
		computeMs += taskMs
		host.TS().Write([]any{resultPrefix, ensembleID, "result", taskID, map[string]any{"ok": true, "ms": taskMs}})
		processed++
	}
	elapsed := float64(host.NowMs() - t0)
	e.TotalComputeMs += computeMs
	if coord := elapsed - computeMs; coord > 0 {
		e.TotalCoordMs += coord
	}
	return marshal(map[string]any{"ok": true, "processed": processed, "ensemble_id": ensembleID})
}

func marshal(v map[string]any) string {
	b, _ := json.Marshal(v)
	return string(b)
}

func init() {
	plexspaces.Register(NewEnsembleActor())
}

func main() {}
