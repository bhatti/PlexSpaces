// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// AI Pipeline Supervision with Monitor & Link (Go WASM)
//
// Demonstrates fault tolerance in multi-agent AI systems using PlexSpaces
// monitor/link primitives — the distributed systems answer to the FLP problem:
//
//   Problem: Multi-agent AI pipelines fail silently. An LLM inference worker
//   can crash (OOM, context limit, rate limiting) and the orchestrator may not
//   know. Without failure detection, requests queue indefinitely.
//
//   Solution: host.monitor() + host.link() give you:
//   - monitor: Supervisor watches workers (one-way). Gets __DOWN__ when any
//              worker stops for any reason (crash, timeout, budget exceeded).
//              Supervisor stays alive — it observes but does not die with the worker.
//   - link:    Critical workers linked together (bidirectional). If Worker-A
//              dies abnormally, Worker-B gets __EXIT__ and can decide to stop
//              or handle gracefully. Used for fate-sharing in pipeline stages.
//
// Architecture:
//   PipelineSupervisor ──monitor──> InferenceWorker (1..N)
//   PipelineSupervisor ──monitor──> ValidatorAgent
//   InferenceWorker-A  <──link──>   InferenceWorker-B   (paired fate-sharing)
//   ValidatorAgent     <──link──>   InferenceWorker      (linked pipeline stages)
//
// When an InferenceWorker crashes:
//   1. Supervisor receives __DOWN__ → restarts worker (liveness recovery, FLP §1.2)
//   2. Linked validator receives __EXIT__ → logs degraded state, continues running
//   3. Audit log records the failure event for Byzantine analysis
//
// Byzantine fault scenario:
//   An InferenceWorker set to "byzantine" mode sends plausible-looking but
//   incorrect results. Validator detects divergence and signals orchestrator.
//   The FLP bound: ≥ 1/3 Byzantine workers means no correct consensus is possible.
//   This example demonstrates detection — external validation converts Byzantine
//   faults to detectable failures, which the supervisor then restarts.
//
// SDK Features:
//   - host.Monitor(actorID)         → one-way watch, get __DOWN__ notification
//   - host.Link(actorID)            → bidirectional fate-sharing, get __EXIT__
//   - host.Unlink(actorID)          → remove link before graceful shutdown
//   - host.Demonitor(monitorRef)    → cancel monitor
//   - BaseActor + Handle(msgType)   → handles __DOWN__, __EXIT__, and normal msgs

package main

import (
	"encoding/json"
	"fmt"
	"strings"

	"github.com/bhatti/plexspaces/sdks/go/plexspaces"
)

var host = plexspaces.NewHost()

// ============================================================
// Helpers
// ============================================================

func marshal(v map[string]any) string {
	data, _ := json.Marshal(v)
	return string(data)
}

// siblingID returns the canonical actor ID for a sibling in the same supervised application.
// Supervised siblings have deterministic IDs: {child_id}//{actor_type}::{namespace}@{node}
// where child_id == actor_type (from ChildSpec.id == ChildSpec.actor_type).
// If bareID already contains "//" it is canonical and returned unchanged.
// Otherwise uses ParseActorID + WithTypeAndName to build the sibling ID from self's namespace/node.
func siblingID(bareID, selfActorID string) string {
	if strings.Contains(bareID, "//") {
		return bareID
	}
	self, err := plexspaces.ParseActorID(selfActorID)
	if err != nil {
		return bareID
	}
	return self.WithTypeAndName(bareID, bareID).String()
}

func parsePayload(payloadJSON string) map[string]any {
	if payloadJSON == "" {
		return map[string]any{}
	}
	var m map[string]any
	if err := json.Unmarshal([]byte(payloadJSON), &m); err != nil {
		return map[string]any{}
	}
	return m
}

func stringVal(m map[string]any, key, fallback string) string {
	if v, ok := m[key]; ok {
		if s, ok := v.(string); ok && s != "" {
			return s
		}
	}
	return fallback
}

func intVal(m map[string]any, key string, fallback int) int {
	if v, ok := m[key]; ok {
		switch n := v.(type) {
		case float64:
			return int(n)
		case int:
			return n
		}
	}
	return fallback
}

// ============================================================
// InferenceWorker — LLM inference actor (one per pipeline slot)
//
// Simulates an LLM inference backend. Can be set to "byzantine" mode
// to return plausible-but-wrong results (demonstrating FLP §1.2 detection).
// ============================================================

type InferenceWorker struct {
	plexspaces.BaseActor
	ActorID         string  `json:"actor_id"`
	WorkerID        string  `json:"worker_id"`
	RequestsHandled int     `json:"requests_handled"`
	FailureCount    int     `json:"failure_count"`
	Mode            string  `json:"mode"` // "normal" | "byzantine" | "degraded"
	LinkedWorkerID  string  `json:"linked_worker_id"` // peer this worker is linked to
}

func (w *InferenceWorker) Init(configJSON string) string {
	cfg := parsePayload(configJSON)
	w.ActorID = stringVal(cfg, "actor_id", "")
	args, _ := cfg["args"].(map[string]any)
	if args != nil {
		w.WorkerID = stringVal(args, "worker_id", "worker-unknown")
	} else {
		w.WorkerID = stringVal(cfg, "worker_id", "worker-unknown")
	}
	w.Mode = stringVal(cfg, "mode", "normal")

	// Write worker info to TupleSpace so supervisor can discover it
	host.TS().Write([]any{"worker", w.WorkerID, "status", "ready"})
	host.TS().Write([]any{"worker", w.WorkerID, "mode", w.Mode})
	return ""
}

func (w *InferenceWorker) Handle(fromActor, msgType, payloadJSON string) string {
	p := parsePayload(payloadJSON)

	switch msgType {
	case "__EXIT__":
		// Linked peer died abnormally — keep running.
		exitingID := stringVal(p, "exit_from", "unknown")
		reason := stringVal(p, "exit_reason", "unknown")
		w.FailureCount++
		if w.LinkedWorkerID == exitingID {
			w.LinkedWorkerID = ""
		}
		return marshal(map[string]any{
			"status":      "degraded",
			"worker_id":   w.WorkerID,
			"exit_from":   exitingID,
			"exit_reason": reason,
		})

	case "infer":
		prompt := stringVal(p, "prompt", "")
		requestID := stringVal(p, "request_id", fmt.Sprintf("req-%d", w.RequestsHandled))
		w.RequestsHandled++

		var result string
		switch w.Mode {
		case "byzantine":
			result = "The answer is 42. " + prompt + " [BYZANTINE: wrong but convincing]"
		case "degraded":
			end := len(prompt)
			if end > 50 {
				end = 50
			}
			result = "[DEGRADED] " + strings.ToUpper(prompt[:end])
		default:
			result = "Inference result for: " + prompt + ". [Worker " + w.WorkerID + ", req " + requestID + "]"
		}

		return marshal(map[string]any{
			"status":     "ok",
			"request_id": requestID,
			"worker_id":  w.WorkerID,
			"result":     result,
			"mode":       w.Mode,
		})

	case "set_mode":
		newMode := stringVal(p, "mode", "normal")
		oldMode := w.Mode
		w.Mode = newMode
		return marshal(map[string]any{
			"status":   "ok",
			"worker_id": w.WorkerID,
			"old_mode": oldMode,
			"new_mode": newMode,
		})

	case "link_with":
		// Links are bidirectional: if either dies abnormally, the other gets __EXIT__.
		peerID := siblingID(stringVal(p, "peer_id", ""), w.ActorID)
		if peerID == "" {
			return marshal(map[string]any{"error": "peer_id required"})
		}
		if err := host.Link(peerID); err != nil {
			return marshal(map[string]any{"error": "link failed: " + err.Error()})
		}
		w.LinkedWorkerID = peerID
		return marshal(map[string]any{
			"status":    "ok",
			"worker_id": w.WorkerID,
			"linked_to": peerID,
		})

	case "unlink_from":
		// Remove link before graceful shutdown (prevents cascade).
		peerID := siblingID(stringVal(p, "peer_id", w.LinkedWorkerID), w.ActorID)
		if peerID == "" {
			return marshal(map[string]any{"status": "ok", "note": "no peer linked"})
		}
		if err := host.Unlink(peerID); err != nil {
			return marshal(map[string]any{"error": "unlink failed: " + err.Error()})
		}
		w.LinkedWorkerID = ""
		return marshal(map[string]any{
			"status":    "ok",
			"worker_id": w.WorkerID,
			"unlinked":  peerID,
		})

	case "status":
		return marshal(map[string]any{
			"status":           "ok",
			"worker_id":        w.WorkerID,
			"mode":             w.Mode,
			"requests_handled": w.RequestsHandled,
			"failure_count":    w.FailureCount,
			"linked_to":        w.LinkedWorkerID,
		})

	default:
		return marshal(map[string]any{
			"error":     "unknown message type",
			"msg_type":  msgType,
			"worker_id": w.WorkerID,
		})
	}
}

func (w *InferenceWorker) GetState() string {
	data, _ := json.Marshal(w)
	return string(data)
}

func (w *InferenceWorker) SetState(stateJSON string) string {
	return "" // BaseActor handles JSON deserialization via SetSelf
}

// ============================================================
// ValidatorAgent — checks inference results for Byzantine behavior
//
// Linked to inference workers. When a worker dies, ValidatorAgent gets
// __EXIT__ and marks that slot as unavailable for result aggregation.
// Detects Byzantine results by checking confidence divergence.
// ============================================================

type ValidatorAgent struct {
	plexspaces.BaseActor
	ActorID         string            `json:"actor_id"`
	ValidatorID     string            `json:"validator_id"`
	ValidationCount int               `json:"validation_count"`
	ByzantineCount  int               `json:"byzantine_count"`
	FailedWorkers   []string          `json:"failed_workers"`
	WorkerMonitors  map[string]string `json:"worker_monitors"` // canonical workerID → monitorRef
}

func (v *ValidatorAgent) Init(configJSON string) string {
	cfg := parsePayload(configJSON)
	v.ActorID = stringVal(cfg, "actor_id", "")
	v.ValidatorID = stringVal(cfg, "actor_type", "validator_agent")
	v.WorkerMonitors = make(map[string]string)
	v.FailedWorkers = []string{}
	return ""
}

func (v *ValidatorAgent) Handle(fromActor, msgType, payloadJSON string) string {
	p := parsePayload(payloadJSON)

	switch msgType {
	case "__DOWN__":
		// A monitored worker stopped (one-way: validator observes but does not die).
		// The supervisor also gets this — two independent observers of the same event.
		workerID := stringVal(p, "down_from", "unknown")
		monitorRef := stringVal(p, "monitor_ref", "")
		reason := stringVal(p, "down_reason", "unknown")
		v.FailedWorkers = append(v.FailedWorkers, workerID)
		for wid, ref := range v.WorkerMonitors {
			if ref == monitorRef || wid == workerID {
				delete(v.WorkerMonitors, wid)
				break
			}
		}
		return marshal(map[string]any{
			"status":      "observed_down",
			"down_from":   workerID,
			"monitor_ref": monitorRef,
			"down_reason": reason,
		})

	case "__EXIT__":
		// Linked worker died abnormally — validator continues.
		workerID := stringVal(p, "exit_from", "unknown")
		reason := stringVal(p, "exit_reason", "unknown")
		v.FailedWorkers = append(v.FailedWorkers, workerID)
		return marshal(map[string]any{
			"status":      "linked_peer_died",
			"exit_from":   workerID,
			"exit_reason": reason,
		})

	case "monitor_worker":
		canonical := siblingID(stringVal(p, "worker_id", ""), v.ActorID)
		if canonical == "" {
			return marshal(map[string]any{"error": "worker_id required"})
		}
		monitorRef, err := host.Monitor(canonical)
		if err != nil {
			return marshal(map[string]any{"error": "monitor failed: " + err.Error()})
		}
		v.WorkerMonitors[canonical] = monitorRef
		return marshal(map[string]any{
			"status":      "ok",
			"worker_id":   canonical,
			"monitor_ref": monitorRef,
		})

	case "validate":
		workerID := stringVal(p, "worker_id", "")
		result := stringVal(p, "result", "")
		v.ValidationCount++

		isByzantine := strings.Contains(result, "[BYZANTINE") ||
			strings.Contains(strings.ToLower(result), "42 is the answer") ||
			strings.Contains(strings.ToLower(result), "sky is green") ||
			result == "null" ||
			strings.Contains(strings.ToLower(result), "checkpoint corrupted") ||
			len(strings.TrimSpace(result)) < 10

		if isByzantine {
			v.ByzantineCount++
		}

		byzantineRatio := float64(0)
		if v.ValidationCount > 0 {
			byzantineRatio = float64(v.ByzantineCount) / float64(v.ValidationCount)
		}
		flpThresholdExceeded := byzantineRatio >= (1.0 / 3.0)

		return marshal(map[string]any{
			"status":                "ok",
			"valid":                 !isByzantine,
			"byzantine_suspected":   isByzantine,
			"worker_id":             workerID,
			"flp_ratio":             byzantineRatio,
			"flp_threshold_exceeded": flpThresholdExceeded,
		})

	case "status":
		flpRatio := float64(0)
		if v.ValidationCount > 0 {
			flpRatio = float64(v.ByzantineCount) / float64(v.ValidationCount)
		}
		return marshal(map[string]any{
			"status":              "ok",
			"total_validations":   v.ValidationCount,
			"byzantine_count":     v.ByzantineCount,
			"flp_ratio":           flpRatio,
			"flp_threshold":       1.0 / 3.0,
			"failed_workers":      v.FailedWorkers,
			"monitor_count":       len(v.WorkerMonitors),
			"down_events_received": len(v.FailedWorkers),
		})

	default:
		return marshal(map[string]any{
			"error":        "unknown message type",
			"msg_type":     msgType,
			"validator_id": v.ValidatorID,
		})
	}
}

// ============================================================
// PipelineSupervisor — orchestrates workers, handles DOWN events
//
// Uses host.monitor() to watch all workers. When a worker dies,
// supervisor receives __DOWN__ and can restart it (liveness recovery).
// This is the core FLP response: detect failures and recover.
//
// Uses host.link() between validator and workers for pipeline fate-sharing:
// if the validator dies (e.g., OOM), workers get __EXIT__ and know to stop
// sending results (no point validating if validator is gone).
// ============================================================

type PipelineSupervisor struct {
	plexspaces.BaseActor
	ActorID         string            `json:"actor_id"`
	WorkerMonitors  map[string]string `json:"worker_monitors"` // canonical workerID → monitorRef
	DownEvents      int               `json:"down_events"`
	TotalDispatched int               `json:"total_dispatched"`
}

func (s *PipelineSupervisor) Init(configJSON string) string {
	cfg := parsePayload(configJSON)
	s.ActorID = stringVal(cfg, "actor_id", "")
	s.WorkerMonitors = make(map[string]string)
	return ""
}

func (s *PipelineSupervisor) Handle(fromActor, msgType, payloadJSON string) string {
	p := parsePayload(payloadJSON)

	switch msgType {
	case "__DOWN__":
		workerID := stringVal(p, "down_from", "unknown")
		monitorRef := stringVal(p, "monitor_ref", "")
		reason := stringVal(p, "down_reason", "unknown")
		for wid, ref := range s.WorkerMonitors {
			if ref == monitorRef || wid == workerID {
				delete(s.WorkerMonitors, wid)
				break
			}
		}
		s.DownEvents++
		return marshal(map[string]any{
			"status":      "worker_down_detected",
			"down_from":   workerID,
			"monitor_ref": monitorRef,
			"down_reason": reason,
		})

	case "monitor_worker":
		canonical := siblingID(stringVal(p, "worker_id", ""), s.ActorID)
		if canonical == "" {
			return marshal(map[string]any{"error": "worker_id required"})
		}
		monitorRef, err := host.Monitor(canonical)
		if err != nil {
			return marshal(map[string]any{"error": "monitor failed: " + err.Error()})
		}
		s.WorkerMonitors[canonical] = monitorRef
		return marshal(map[string]any{
			"status":      "ok",
			"worker_id":   canonical,
			"monitor_ref": monitorRef,
		})

	case "demonitor_worker":
		canonical := siblingID(stringVal(p, "worker_id", ""), s.ActorID)
		monitorRef, ok := s.WorkerMonitors[canonical]
		if !ok {
			return marshal(map[string]any{"status": "ok"})
		}
		if err := host.Demonitor(monitorRef); err != nil {
			return marshal(map[string]any{"error": "demonitor failed: " + err.Error()})
		}
		delete(s.WorkerMonitors, canonical)
		return marshal(map[string]any{"status": "ok", "worker_id": canonical})

	case "dispatch":
		prompt := stringVal(p, "prompt", "")
		if prompt == "" {
			return marshal(map[string]any{"error": "prompt required"})
		}
		if len(s.WorkerMonitors) == 0 {
			return marshal(map[string]any{"status": "error", "reason": "no_workers_available"})
		}
		// Pick the first active worker (simple round-robin would need a list)
		var workerID string
		for wid := range s.WorkerMonitors {
			workerID = wid
			break
		}
		s.TotalDispatched++
		requestID := fmt.Sprintf("task-%d", s.TotalDispatched)

		result, err := host.Ask(workerID, "infer", map[string]any{
			"prompt":     prompt,
			"request_id": requestID,
		}, 30000)
		if err != nil {
			return marshal(map[string]any{
				"error":      "dispatch failed: " + err.Error(),
				"worker_id":  workerID,
				"request_id": requestID,
			})
		}
		resultMap, _ := result.(map[string]any)
		byzantineDetected := false
		if resultMap != nil {
			byzantineDetected = resultMap["mode"].(string) == "byzantine"
		}
		return marshal(map[string]any{
			"status":            "ok",
			"request_id":        requestID,
			"worker_used":       workerID,
			"result":            result,
			"byzantine_detected": byzantineDetected,
		})

	case "status":
		return marshal(map[string]any{
			"status":               "ok",
			"monitor_count":        len(s.WorkerMonitors),
			"down_events_received": s.DownEvents,
			"total_dispatched":     s.TotalDispatched,
		})

	default:
		return marshal(map[string]any{
			"error":    "unknown message type",
			"msg_type": msgType,
		})
	}
}

// ============================================================
// AuditLogActor — fire-and-forget event log for compliance
//
// Records all DOWN/EXIT events, validation failures, and Byzantine
// detections. Uses GenEvent behavior (cast-only, no reply needed).
// ============================================================

type AuditLogActor struct {
	plexspaces.BaseActor
	Events     []string `json:"events"`
	EventCount int      `json:"event_count"`
}

func (a *AuditLogActor) Init(configJSON string) string {
	a.Events = []string{}
	return ""
}

func (a *AuditLogActor) Handle(fromActor, msgType, payloadJSON string) string {
	p := parsePayload(payloadJSON)

	switch msgType {
	case "log_event":
		eventType := stringVal(p, "event_type", "unknown")
		details := stringVal(p, "details", "")
		actorID := stringVal(p, "actor_id", "")
		entry := fmt.Sprintf("%s actor=%s %s", eventType, actorID, details)
		a.Events = append(a.Events, entry)
		a.EventCount++
		if len(a.Events) > 100 {
			a.Events = a.Events[len(a.Events)-100:]
		}
		return marshal(map[string]any{"status": "ok", "events_received": a.EventCount})

	case "get_stats":
		lastEvent := ""
		if len(a.Events) > 0 {
			lastEvent = a.Events[len(a.Events)-1]
		}
		return marshal(map[string]any{
			"status":          "ok",
			"events_received": a.EventCount,
			"last_event":      lastEvent,
		})

	default:
		// GenEvent: silently absorb unknown events
		a.EventCount++
		return marshal(map[string]any{"status": "ok"})
	}
}

// ============================================================
// init — register all actor roles/classes via ActorRouter.
// The framework now dispatches Go multi-actor WASM modules by ChildSpec.role
// (sent as role in init config), with actor_type only as fallback.
// ============================================================

func init() {
	router := plexspaces.NewActorRouter()
	router.Route("inference_worker", func() plexspaces.Actor {
		a := &InferenceWorker{}
		a.SetSelf(a)
		return a
	})
	router.Route("validator_agent", func() plexspaces.Actor {
		a := &ValidatorAgent{}
		a.SetSelf(a)
		return a
	})
	router.Route("pipeline_supervisor", func() plexspaces.Actor {
		a := &PipelineSupervisor{}
		a.SetSelf(a)
		return a
	})
	router.Route("audit_log", func() plexspaces.Actor {
		a := &AuditLogActor{}
		a.SetSelf(a)
		return a
	})
	plexspaces.Register(router)
}

func main() {}
