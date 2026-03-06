// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Merlin → PlexSpaces: Scientific simulation manager (parameter sweep).
//
// Worker pool (elastic size): elastic pool API (checkout/checkin) + tuple space (work queue).
// Coordinator (Run): scatter tasks → checkout workers from pool, send work_available → gather → checkin.
// If pool is not configured, falls back to process group broadcast.
// Workers (work_available): take tasks from tuple space, run simulation, write results.
// Convention: sweep:coord-1 = coordinator; sweep:worker-0, worker-1, ... = pool workers.

package main

import (
	"encoding/json"
	"strconv"

	"github.com/plexobject/plexspaces/sdks/go/plexspaces"
)

var host = plexspaces.NewHost()

const (
	simulationMs       = 22
	tuplePrefix        = "merlin"
	workerGroup        = "merlin-workers"
	poolName           = "merlin-workers"
	maxCheckoutWorkers = 10
	gatherPollMax      = 200
	checkoutTimeoutMs  = 5000
)

// SweepActor implements parameter sweep: coordinator (Run) and workers (Handle work_available).
type SweepActor struct {
	plexspaces.BaseActor

	SweepID          string  `json:"sweep_id"`
	NumParams        int     `json:"num_params"`
	NumCompleted     int     `json:"num_completed"`
	Status           string  `json:"status"`
	TotalComputeMs   float64 `json:"total_compute_ms"`
	TotalCoordMs     float64 `json:"total_coord_ms"`
	CreatedAtMs      uint64  `json:"created_at_ms"`
	UpdatedAtMs      uint64  `json:"updated_at_ms"`
	CancelRequested  bool    `json:"cancel_requested"`
	WorkerJoined     bool    `json:"worker_joined"`
}

// NewSweepActor creates a new SweepActor.
func NewSweepActor() *SweepActor {
	a := &SweepActor{Status: "idle"}
	a.SetSelf(a)
	return a
}

func (s *SweepActor) Init(configJSON string) string {
	return ""
}

func (s *SweepActor) Run(payloadJSON string) string {
	t0 := host.NowMs()
	if s.CreatedAtMs == 0 {
		s.CreatedAtMs = t0
	}
	var payload map[string]any
	_ = json.Unmarshal([]byte(payloadJSON), &payload)
	s.UpdatedAtMs = host.NowMs()

	if s.CancelRequested {
		s.Status = "cancelled"
		return s.finish(t0, 0, "cancelled")
	}
	if s.Status == "completed" {
		return s.finish(t0, 0, "completed")
	}

	numParams := 10
	if n, ok := payload["num_params"].(float64); ok && n > 0 {
		numParams = int(n)
	} else if n, ok := payload["num_params"].(int); ok && n > 0 {
		numParams = n
	}
	if id, ok := payload["sweep_id"].(string); ok && id != "" {
		s.SweepID = id
	} else if id, ok := payload["job_id"].(string); ok && id != "" {
		s.SweepID = id
	}
	if numParams <= 0 {
		return s.finish(t0, 0, "no_params")
	}

	s.NumParams = numParams
	s.NumCompleted = 0
	s.Status = "scattering"
	computeMs := 0.0

	paramBase := 0
	if p, ok := payload["param_base"].(float64); ok {
		paramBase = int(p)
	} else if p, ok := payload["param_base"].(int); ok {
		paramBase = p
	}

	for i := 0; i < numParams; i++ {
		paramVal := paramBase + i
		out := host.TS().Write([]any{tuplePrefix, s.SweepID, "task", "p" + strconv.Itoa(i), map[string]any{"param": paramVal}})
		if out != "" && len(out) >= 5 && out[:5] == "ERROR" {
			host.Log("warn", "ts write failed: "+out)
		}
		computeMs += 2
	}
	s.UpdatedAtMs = host.NowMs()
	s.Status = "running"

	checkoutHandles := []map[string]any{}
	usePool := true
	for i := 0; i < maxCheckoutWorkers && i < numParams; i++ {
		handle := host.PoolCheckout(poolName, checkoutTimeoutMs)
		if handle == nil {
			break
		}
		checkoutHandles = append(checkoutHandles, handle)
		actorID, _ := handle["actor_id"].(string)
		if actorID == "" {
			usePool = false
			break
		}
		out := host.Send(actorID, "work_available", map[string]any{
			"sweep_id":   s.SweepID,
			"num_params": numParams,
		})
		if out != "" && len(out) >= 5 && out[:5] == "ERROR" {
			host.Log("warn", "send to worker failed: "+out)
		}
	}
	if !usePool || len(checkoutHandles) == 0 {
		if err := host.PG().Broadcast(workerGroup, "work_available", map[string]any{
			"sweep_id":   s.SweepID,
			"num_params": numParams,
		}); err != nil {
			host.Log("warn", "pg_broadcast failed: "+err.Error())
		}
	}
	s.TotalCoordMs += float64(host.NowMs() - t0)

	pattern := []any{tuplePrefix, s.SweepID, "result", nil, nil}
	for i := 0; i < gatherPollMax; i++ {
		if s.CancelRequested {
			s.Status = "cancelled"
			return s.finish(t0, computeMs, "cancelled")
		}
		results := host.TS().ReadAll(pattern)
		s.NumCompleted = len(results)
		if s.NumCompleted >= numParams {
			break
		}
		s.UpdatedAtMs = host.NowMs()
	}

	for _, handle := range checkoutHandles {
		actorID, _ := handle["actor_id"].(string)
		checkoutID, _ := handle["checkout_id"].(string)
		if actorID != "" && checkoutID != "" {
			if err := host.PoolCheckin(poolName, actorID, checkoutID, true); err != nil {
				host.Log("warn", "pool_checkin failed: "+err.Error())
			}
		}
	}

	s.Status = "completed"
	s.UpdatedAtMs = host.NowMs()
	return s.finish(t0, computeMs, "completed")
}

func (s *SweepActor) finish(t0 uint64, computeMs float64, status string) string {
	elapsed := float64(host.NowMs() - t0)
	if elapsed < computeMs {
		elapsed = computeMs
	}
	coordMs := elapsed - computeMs
	if coordMs < 0 {
		coordMs = 0
	}
	s.TotalComputeMs += computeMs
	s.TotalCoordMs += coordMs
	approxBytes := (s.NumParams * 100) + (s.NumCompleted * 80)
	return marshal(map[string]any{
		"status":             status,
		"sweep_id":            s.SweepID,
		"num_params":          s.NumParams,
		"num_completed":       s.NumCompleted,
		"data_size_bytes":     approxBytes,
		"total_compute_ms":    s.TotalComputeMs,
		"total_coord_ms":      s.TotalCoordMs,
	})
}

func (s *SweepActor) Signal(name, _ string) {
	if name == "cancel" {
		s.CancelRequested = true
		s.UpdatedAtMs = host.NowMs()
	}
}

func (s *SweepActor) Query(name, _ string) string {
	if name == "status" {
		return marshal(map[string]any{
			"sweep_id":          s.SweepID,
			"status":            s.Status,
			"num_params":        s.NumParams,
			"num_completed":     s.NumCompleted,
			"cancel_requested":  s.CancelRequested,
			"total_compute_ms":  s.TotalComputeMs,
			"total_coord_ms":    s.TotalCoordMs,
			"created_at_ms":     s.CreatedAtMs,
			"updated_at_ms":     s.UpdatedAtMs,
		})
	}
	return marshal(map[string]any{"error": "unknown_query", "name": name})
}

func (s *SweepActor) Handle(from, msgType, payloadJSON string) string {
	if msgType != "work_available" {
		return marshal(map[string]any{"error": "unknown_op", "op": msgType})
	}
	if !s.WorkerJoined {
		if err := host.PG().Join(workerGroup); err != nil {
			host.Log("warn", "pg_join failed: "+err.Error())
		} else {
			s.WorkerJoined = true
			host.Log("info", "Joined worker pool "+workerGroup)
		}
	}
	var payload map[string]any
	_ = json.Unmarshal([]byte(payloadJSON), &payload)
	sweepID, _ := payload["sweep_id"].(string)
	numParams := 0
	if n, ok := payload["num_params"].(float64); ok {
		numParams = int(n)
	} else if n, ok := payload["num_params"].(int); ok {
		numParams = n
	}
	if sweepID == "" || numParams <= 0 {
		return marshal(map[string]any{"ok": true, "message": "warmup"})
	}

	t0 := host.NowMs()
	pattern := []any{tuplePrefix, sweepID, "task", nil, nil}
	processed := 0
	computeMs := 0.0
	for {
		tuple, ok := host.TS().Take(pattern)
		if !ok {
			break
		}
		paramID := ""
		if len(tuple) > 3 {
			switch v := tuple[3].(type) {
			case string:
				paramID = v
			case float64:
				paramID = strconv.Itoa(int(v))
			}
		}
		computeMs += simulationMs
		host.TS().Write([]any{tuplePrefix, sweepID, "result", paramID, map[string]any{"ok": true, "ms": simulationMs}})
		processed++
	}
	elapsed := float64(host.NowMs() - t0)
	s.TotalComputeMs += computeMs
	if coord := elapsed - computeMs; coord > 0 {
		s.TotalCoordMs += coord
	}
	return marshal(map[string]any{"ok": true, "processed": processed, "sweep_id": sweepID})
}

func marshal(v map[string]any) string {
	b, _ := json.Marshal(v)
	return string(b)
}

func init() {
	plexspaces.Register(NewSweepActor())
}

func main() {}
