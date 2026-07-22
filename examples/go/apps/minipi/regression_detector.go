// SPDX-License-Identifier: AGPL-3.0-or-later
// RegressionDetectorActor — compare trajectories across eval runs.
//
// Demonstrates: reading from TupleSpace blackboard, diff logic,
// and the eval feedback loop (run → score → compare → diagnose → fix → rerun).
package main

import (
	"encoding/json"
	"fmt"

	"github.com/bhatti/PlexSpaces/sdks/go/plexspaces"
)

// RegressionDetectorActor detects regressions by comparing trajectory scores across eval runs.
//
// The eval feedback loop:
// 1. Run eval (EvalRunnerActor)
// 2. Score trajectories (ScorerActor)
// 3. Detect regressions (this actor) — flag scenarios that got worse
// 4. Diagnose: inspect trajectories to understand WHY
// 5. Fix harness config, policy, or prompt
// 6. Rerun: compare new scores against baseline
type RegressionDetectorActor struct {
	plexspaces.BaseActor
	ActorID           string `json:"actor_id"`
	TotalComparisons  int    `json:"total_comparisons"`
}

func NewRegressionDetectorActor() plexspaces.Actor {
	a := &RegressionDetectorActor{}
	a.SetSelf(a)
	return a
}

func (r *RegressionDetectorActor) Init(configJSON string) string {
	var config struct {
		ActorID string `json:"actor_id"`
	}
	if err := json.Unmarshal([]byte(configJSON), &config); err != nil {
		return "ERROR: " + err.Error()
	}
	r.SetRuntimeMetadata(config.ActorID)
	r.ActorID = config.ActorID
	if err := host.PG().Join("svc:regression"); err != nil {
		host.Warn(fmt.Sprintf("RegressionDetectorActor: failed to join svc:regression: %v", err))
	}
	host.Info(fmt.Sprintf("RegressionDetectorActor Init actor_id=%s", config.ActorID))
	return ""
}

func (r *RegressionDetectorActor) Handle(fromActor, msgType, payloadJSON string) string {
	p := parsePayload(payloadJSON)
	op := stringVal(p, "op", msgType)
	switch op {
	case "compare":
		return r.compare(p)
	case "set_baseline":
		return r.setBaseline(p)
	case "get_baseline":
		return r.getBaseline()
	case "replay_diff":
		return r.replayDiff(p)
	case "get_stats":
		return marshal(map[string]any{"status": "ok", "total_comparisons": r.TotalComparisons})
	default:
		return marshal(map[string]any{"error": "unknown_op", "op": op})
	}
}

func (r *RegressionDetectorActor) compare(p map[string]any) string {
	evalRunID := stringVal(p, "eval_run_id", "")
	if evalRunID == "" {
		return marshal(map[string]any{"error": "eval_run_id is required"})
	}
	scoresRaw := sliceVal(p, "scores")
	if len(scoresRaw) == 0 {
		return marshal(map[string]any{
			"regressions":  []any{},
			"improvements": []any{},
			"unchanged":    []any{},
		})
	}

	scores := make([]map[string]any, 0, len(scoresRaw))
	for _, s := range scoresRaw {
		if m, ok := s.(map[string]any); ok {
			scores = append(scores, m)
		}
	}

	// Load baseline
	baseline := r.loadBaseline()
	if len(baseline) == 0 {
		// No baseline — store current as baseline and return clean
		r.storeBaseline(evalRunID, scores)
		return marshal(map[string]any{
			"regressions":  []any{},
			"improvements": []any{},
			"unchanged":    []any{},
			"message":      fmt.Sprintf("Stored as baseline (eval_run_id=%s)", evalRunID),
		})
	}

	regressions := []any{}
	improvements := []any{}
	unchanged := []any{}

	const threshold = 0.05 // 5% regression threshold

	for _, current := range scores {
		trajID := stringVal(current, "trajectory_id", "")
		currentScore := float64Val(current, "score", 0.0)

		baselineEntry, exists := baseline[trajID]
		if !exists {
			unchanged = append(unchanged, map[string]any{
				"trajectory_id": trajID,
				"current":       currentScore,
				"baseline":      nil,
			})
			continue
		}

		baselineScore := float64Val(baselineEntry, "score", 0.0)
		delta := currentScore - baselineScore

		entry := map[string]any{
			"trajectory_id": trajID,
			"current":       currentScore,
			"baseline":      baselineScore,
			"delta":         roundFloat(delta, 3),
		}

		if delta < -threshold {
			if delta < -0.15 {
				entry["severity"] = "high"
			} else {
				entry["severity"] = "medium"
			}
			regressions = append(regressions, entry)
		} else if delta > threshold {
			improvements = append(improvements, entry)
		} else {
			unchanged = append(unchanged, entry)
		}
	}

	r.TotalComparisons++
	r.IncrCounter(host, "regression_comparisons_total")

	if len(regressions) > 0 {
		host.Warn(fmt.Sprintf("Regressions detected: %d scenarios degraded in eval_run=%s", len(regressions), evalRunID))
	}

	return marshal(map[string]any{
		"regressions":       regressions,
		"improvements":      improvements,
		"unchanged":         unchanged,
		"regression_count":  len(regressions),
		"improvement_count": len(improvements),
		"eval_run_id":       evalRunID,
	})
}

func (r *RegressionDetectorActor) setBaseline(p map[string]any) string {
	evalRunID := stringVal(p, "eval_run_id", "")
	scoresRaw := sliceVal(p, "scores")
	if len(scoresRaw) == 0 {
		return marshal(map[string]any{"error": "scores is required"})
	}
	scores := make([]map[string]any, 0, len(scoresRaw))
	for _, s := range scoresRaw {
		if m, ok := s.(map[string]any); ok {
			scores = append(scores, m)
		}
	}
	r.storeBaseline(evalRunID, scores)
	return marshal(map[string]any{
		"status":               "ok",
		"baseline_eval_run_id": evalRunID,
		"scenarios":            len(scores),
	})
}

func (r *RegressionDetectorActor) getBaseline() string {
	baseline := r.loadBaseline()
	return marshal(map[string]any{
		"status":   "ok",
		"baseline": baseline,
		"count":    len(baseline),
	})
}

func (r *RegressionDetectorActor) replayDiff(p map[string]any) string {
	trajIDA := stringVal(p, "traj_id_a", "")
	trajIDB := stringVal(p, "traj_id_b", "")

	trajA := r.loadTrajectory(trajIDA)
	trajB := r.loadTrajectory(trajIDB)

	if len(trajA) == 0 || len(trajB) == 0 {
		return marshal(map[string]any{"error": "one or both trajectories not found"})
	}

	stepsA := sliceVal(trajA, "steps")
	stepsB := sliceVal(trajB, "steps")

	diffs := []any{}
	maxSteps := len(stepsA)
	if len(stepsB) > maxSteps {
		maxSteps = len(stepsB)
	}
	if maxSteps > 20 {
		maxSteps = 20
	}

	for i := 0; i < maxSteps; i++ {
		if i >= len(stepsA) {
			stepB, _ := stepsB[i].(map[string]any)
			diffs = append(diffs, map[string]any{"step": i, "type": "added", "b": stepB})
		} else if i >= len(stepsB) {
			stepA, _ := stepsA[i].(map[string]any)
			diffs = append(diffs, map[string]any{"step": i, "type": "removed", "a": stepA})
		} else {
			stepA, _ := stepsA[i].(map[string]any)
			stepB, _ := stepsB[i].(map[string]any)
			if stepA == nil {
				stepA = map[string]any{}
			}
			if stepB == nil {
				stepB = map[string]any{}
			}
			kindA := stringVal(stepA, "kind", "")
			kindB := stringVal(stepB, "kind", "")
			successA := boolVal(stepA, "success")
			successB := boolVal(stepB, "success")
			if kindA != kindB || successA != successB {
				diffs = append(diffs, map[string]any{
					"step":      i,
					"type":      "changed",
					"a_kind":    kindA,
					"b_kind":    kindB,
					"a_success": successA,
					"b_success": successB,
				})
			}
		}
	}

	return marshal(map[string]any{
		"trajectory_id_a": trajIDA,
		"trajectory_id_b": trajIDB,
		"steps_a":         len(stepsA),
		"steps_b":         len(stepsB),
		"score_a":         float64Val(trajA, "score", 0),
		"score_b":         float64Val(trajB, "score", 0),
		"diff_count":      len(diffs),
		"diffs":           diffs,
	})
}

func (r *RegressionDetectorActor) loadBaseline() map[string]map[string]any {
	raw, _ := host.KV().Get("regression_baseline")
	if raw == "" {
		return map[string]map[string]any{}
	}
	var baseline map[string]map[string]any
	if err := json.Unmarshal([]byte(raw), &baseline); err != nil {
		return map[string]map[string]any{}
	}
	return baseline
}

func (r *RegressionDetectorActor) storeBaseline(evalRunID string, scores []map[string]any) {
	baseline := map[string]map[string]any{}
	for _, s := range scores {
		trajID := stringVal(s, "trajectory_id", "")
		if trajID != "" {
			baseline[trajID] = map[string]any{
				"score":       float64Val(s, "score", 0.0),
				"eval_run_id": evalRunID,
			}
		}
	}
	baselineJSON, _ := json.Marshal(baseline)
	host.KV().Put("regression_baseline", string(baselineJSON))
	host.KV().Put("regression_baseline_eval_run", evalRunID)
}

func (r *RegressionDetectorActor) loadTrajectory(trajID string) map[string]any {
	if trajID == "" {
		return map[string]any{}
	}
	raw, _ := host.KV().Get("trajectory:" + trajID)
	if raw == "" {
		// Try agent_trajectory key
		raw, _ = host.KV().Get("agent_trajectory:" + trajID)
	}
	if raw == "" {
		return map[string]any{}
	}
	var traj map[string]any
	if err := json.Unmarshal([]byte(raw), &traj); err != nil {
		return map[string]any{}
	}
	return traj
}
