// SPDX-License-Identifier: AGPL-3.0-or-later
// TrajectoryStoreActor — persists and indexes agent trajectory records.
//
// Demonstrates: BlobStorage for large payloads, TupleSpace-based discovery index,
// read-all pattern for eval collection, and trajectory lifecycle management.
package main

import (
	"encoding/json"
	"fmt"

	"github.com/bhatti/PlexSpaces/sdks/go/plexspaces"
)

// TrajectoryStoreActor stores and indexes agent trajectory records.
//
// Two storage tiers:
// - KV: trajectory metadata + full record (for fast retrieval)
// - TupleSpace index: discovery entries (eval_run_id, outcome) for batch collection
type TrajectoryStoreActor struct {
	plexspaces.BaseActor
	ActorID      string `json:"actor_id"`
	StoredCount  int    `json:"stored_count"`
	FailedCount  int    `json:"failed_count"`
}

func NewTrajectoryStoreActor() plexspaces.Actor {
	a := &TrajectoryStoreActor{}
	a.SetSelf(a)
	return a
}

func (t *TrajectoryStoreActor) Init(configJSON string) string {
	var config struct {
		ActorID string `json:"actor_id"`
	}
	if err := json.Unmarshal([]byte(configJSON), &config); err != nil {
		return "ERROR: " + err.Error()
	}
	t.SetRuntimeMetadata(config.ActorID)
	t.ActorID = config.ActorID
	if err := host.PG().Join("svc:trajectory_store"); err != nil {
		host.Warn(fmt.Sprintf("TrajectoryStoreActor: failed to join svc:trajectory_store: %v", err))
	}
	host.Info(fmt.Sprintf("TrajectoryStoreActor Init actor_id=%s", config.ActorID))
	return ""
}

func (t *TrajectoryStoreActor) Handle(fromActor, msgType, payloadJSON string) string {
	p := parsePayload(payloadJSON)
	op := stringVal(p, "op", msgType)
	switch op {
	case "put":
		return t.put(p)
	case "get":
		return t.get(p)
	case "list_for_eval_run":
		return t.listForEvalRun(p)
	case "delete":
		return t.deleteTraj(p)
	case "delete_eval_run":
		return t.deleteEvalRun(p)
	case "get_stats":
		return marshal(map[string]any{
			"status":       "ok",
			"actor_id":     t.ActorID,
			"stored_count": t.StoredCount,
			"failed_count": t.FailedCount,
		})
	default:
		return marshal(map[string]any{"error": "unknown_op", "op": op})
	}
}

func (t *TrajectoryStoreActor) put(p map[string]any) string {
	traj, _ := p["trajectory"].(map[string]any)
	if traj == nil {
		return marshal(map[string]any{"error": "trajectory is required"})
	}

	trajID := stringVal(traj, "trajectory_id", "")
	if trajID == "" {
		trajID = fmt.Sprintf("traj-%d", host.NowMs())
		traj["trajectory_id"] = trajID
	}

	evalRunID := stringVal(traj, "eval_run_id", "")
	outcome := stringVal(traj, "outcome", "unknown")
	agentActorID := stringVal(traj, "agent_actor_id", "")

	// Store full trajectory in KV
	trajJSON, err := json.Marshal(traj)
	if err != nil || len(trajJSON) == 0 {
		t.FailedCount++
		return marshal(map[string]any{"error": "failed to marshal trajectory"})
	}
	host.KVPut("trajectory:"+trajID, string(trajJSON))

	// Store metadata for listing
	steps := sliceVal(traj, "steps")
	meta := map[string]any{
		"trajectory_id":       trajID,
		"eval_run_id":         evalRunID,
		"agent_actor_id":      agentActorID,
		"outcome":             outcome,
		"score":               float64Val(traj, "score", 0.0),
		"total_input_tokens":  intVal(traj, "total_input_tokens", 0),
		"total_output_tokens": intVal(traj, "total_output_tokens", 0),
		"step_count":          len(steps),
		"stored_at_ms":        host.NowMs(),
	}
	metaJSON, _ := json.Marshal(meta)
	host.KVPut("traj_meta:"+trajID, string(metaJSON))

	// Index by eval_run_id for batch collection
	if evalRunID != "" {
		indexKey := "traj_index:" + evalRunID
		var index []string
		if raw := host.KVGet(indexKey); raw != "" {
			_ = json.Unmarshal([]byte(raw), &index)
		}
		// Deduplicate
		found := false
		for _, id := range index {
			if id == trajID {
				found = true
				break
			}
		}
		if !found {
			index = append(index, trajID)
			indexJSON, _ := json.Marshal(index)
			host.KVPut(indexKey, string(indexJSON))
		}
	}

	t.StoredCount++
	t.IncrCounter(host, "trajectories_stored_total")
	host.Info(fmt.Sprintf("TrajectoryStore: stored traj_id=%s eval_run=%s outcome=%s", trajID, evalRunID, outcome))
	return marshal(map[string]any{"status": "ok", "trajectory_id": trajID})
}

func (t *TrajectoryStoreActor) get(p map[string]any) string {
	trajID := stringVal(p, "trajectory_id", "")
	if trajID == "" {
		return marshal(map[string]any{"error": "trajectory_id is required"})
	}
	raw := host.KVGet("trajectory:" + trajID)
	if raw == "" {
		return marshal(map[string]any{"error": fmt.Sprintf("trajectory %s not found", trajID)})
	}
	var traj map[string]any
	if err := json.Unmarshal([]byte(raw), &traj); err != nil {
		return marshal(map[string]any{"error": "failed to parse trajectory"})
	}
	return marshal(map[string]any{"status": "ok", "trajectory": traj})
}

func (t *TrajectoryStoreActor) listForEvalRun(p map[string]any) string {
	evalRunID := stringVal(p, "eval_run_id", "")
	if evalRunID == "" {
		return marshal(map[string]any{"error": "eval_run_id is required"})
	}
	includeFull := boolVal(p, "include_full")

	// Load from KV index
	var trajIDs []string
	if raw := host.KVGet("traj_index:" + evalRunID); raw != "" {
		_ = json.Unmarshal([]byte(raw), &trajIDs)
	}

	// Also check TupleSpace for any trajectories written there
	tsResults, tsOK := host.TS().Read([]any{"trajectory", evalRunID, nil, nil})
	if tsOK && len(tsResults) >= 3 && !isErrorResult(tsResults) {
		if tsID, ok := tsResults[2].(string); ok && tsID != "" {
			found := false
			for _, id := range trajIDs {
				if id == tsID {
					found = true
					break
				}
			}
			if !found {
				trajIDs = append(trajIDs, tsID)
			}
		}
	}

	trajectories := make([]any, 0, len(trajIDs))
	for _, trajID := range trajIDs {
		if includeFull {
			raw := host.KVGet("trajectory:" + trajID)
			if raw != "" {
				var traj map[string]any
				if err := json.Unmarshal([]byte(raw), &traj); err == nil {
					trajectories = append(trajectories, traj)
				}
			}
		} else {
			raw := host.KVGet("traj_meta:" + trajID)
			if raw != "" {
				var meta map[string]any
				if err := json.Unmarshal([]byte(raw), &meta); err == nil {
					trajectories = append(trajectories, meta)
				}
			}
		}
	}

	return marshal(map[string]any{
		"status":       "ok",
		"eval_run_id":  evalRunID,
		"trajectories": trajectories,
		"count":        len(trajectories),
	})
}

func (t *TrajectoryStoreActor) deleteTraj(p map[string]any) string {
	trajID := stringVal(p, "trajectory_id", "")
	if trajID == "" {
		return marshal(map[string]any{"error": "trajectory_id is required"})
	}
	host.KVDelete("trajectory:" + trajID)
	host.KVDelete("traj_meta:" + trajID)
	t.IncrCounter(host, "trajectories_deleted_total")
	return marshal(map[string]any{"status": "ok", "trajectory_id": trajID})
}

func (t *TrajectoryStoreActor) deleteEvalRun(p map[string]any) string {
	evalRunID := stringVal(p, "eval_run_id", "")
	if evalRunID == "" {
		return marshal(map[string]any{"error": "eval_run_id is required"})
	}
	indexKey := "traj_index:" + evalRunID
	var trajIDs []string
	if raw := host.KVGet(indexKey); raw != "" {
		_ = json.Unmarshal([]byte(raw), &trajIDs)
	}
	deleted := 0
	for _, trajID := range trajIDs {
		host.KVDelete("trajectory:" + trajID)
		host.KVDelete("traj_meta:" + trajID)
		deleted++
	}
	host.KVDelete(indexKey)
	return marshal(map[string]any{"status": "ok", "eval_run_id": evalRunID, "deleted": deleted})
}
