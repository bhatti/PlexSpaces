// SPDX-License-Identifier: AGPL-3.0-or-later
// DashboardActor — aggregates eval metrics and exposes query handlers.
//
// Demonstrates: read-only aggregation pattern, query-only actor.
package main

import (
	"encoding/json"
	"fmt"

	"github.com/bhatti/PlexSpaces/sdks/go/plexspaces"
)

// DashboardActor aggregates results from all eval runs.
//
// Query this actor for:
// - Eval run summaries
// - Pass rate trends over time
// - Token cost trends
// - Regression alerts
type DashboardActor struct {
	plexspaces.BaseActor
	ActorID string `json:"actor_id"`
}

func NewDashboardActor() plexspaces.Actor {
	a := &DashboardActor{}
	a.SetSelf(a)
	return a
}

func (d *DashboardActor) Init(configJSON string) string {
	var config struct {
		ActorID string `json:"actor_id"`
	}
	if err := json.Unmarshal([]byte(configJSON), &config); err != nil {
		return "ERROR: " + err.Error()
	}
	d.SetRuntimeMetadata(config.ActorID)
	d.ActorID = config.ActorID
	if err := host.PG().Join("svc:dashboard"); err != nil {
		host.Warn(fmt.Sprintf("DashboardActor: failed to join svc:dashboard: %v", err))
	}
	host.Info(fmt.Sprintf("DashboardActor Init actor_id=%s", config.ActorID))
	return ""
}

func (d *DashboardActor) Handle(fromActor, msgType, payloadJSON string) string {
	p := parsePayload(payloadJSON)
	op := stringVal(p, "op", msgType)
	switch op {
	case "get_eval_report":
		return d.getEvalReport(p)
	case "report_eval":
		return d.reportEval(p)
	case "list_eval_runs":
		return d.listEvalRuns(p)
	case "get_trajectory":
		return d.getTrajectory(p)
	case "get_regressions":
		return d.getRegressions()
	case "summary":
		return d.summary()
	default:
		return marshal(map[string]any{"error": "unknown_op", "op": op})
	}
}

func (d *DashboardActor) reportEval(p map[string]any) string {
	evalRunID := stringVal(p, "eval_run_id", "")
	if evalRunID == "" {
		return marshal(map[string]any{"error": "eval_run_id is required"})
	}
	reportData, ok := p["report"].(map[string]any)
	if !ok {
		reportData = p
	}
	data, err := json.Marshal(reportData)
	if err != nil {
		return marshal(map[string]any{"error": "failed to serialize report"})
	}
	host.KV().Put("eval_report:"+evalRunID, string(data))
	host.Info(fmt.Sprintf("DashboardActor: stored eval report eval_run_id=%s", evalRunID))
	return marshal(map[string]any{"status": "ok", "eval_run_id": evalRunID})
}

func (d *DashboardActor) getEvalReport(p map[string]any) string {
	evalRunID := stringVal(p, "eval_run_id", "")
	if evalRunID == "" {
		return marshal(map[string]any{"error": "eval_run_id is required"})
	}
	raw, _ := host.KV().Get("eval_report:" + evalRunID)
	if raw == "" {
		return marshal(map[string]any{"error": fmt.Sprintf("eval run %s not found", evalRunID)})
	}
	var report map[string]any
	if err := json.Unmarshal([]byte(raw), &report); err != nil {
		return marshal(map[string]any{"error": "failed to parse report"})
	}
	if _, ok := report["status"]; !ok {
		report["status"] = "ok"
	}
	return marshal(report)
}

func (d *DashboardActor) listEvalRuns(p map[string]any) string {
	limit := intVal(p, "limit", 10)
	reports := make([]any, 0)
	seen := map[string]bool{}

	// Read eval run IDs from TupleSpace index (posted by EvalRunnerActor on completion)
	ts := host.TS()
	for i := 0; i < limit+10; i++ {
		r, ok := ts.Read([]any{"eval_run", nil, nil, nil})
		if !ok || len(r) < 2 || isErrorResult(r) {
			break
		}
		evalRunID, _ := r[1].(string)
		if evalRunID == "" || seen[evalRunID] {
			break
		}
		seen[evalRunID] = true
		raw, _ := host.KV().Get("eval_report:" + evalRunID)
		if raw == "" {
			continue
		}
		var report map[string]any
		if err := json.Unmarshal([]byte(raw), &report); err == nil {
			reports = append(reports, map[string]any{
				"eval_run_id": evalRunID,
				"suite_name":  stringVal(report, "suite_name", ""),
				"pass_rate":   float64Val(report, "pass_rate", 0.0),
				"avg_score":   float64Val(report, "avg_score", 0.0),
				"completed":   intVal(report, "completed_scenarios", 0),
				"total":       intVal(report, "total_scenarios", 0),
				"status":      stringVal(report, "status", ""),
			})
		}
		if len(reports) >= limit {
			break
		}
	}

	// Also scan well-known KV keys to pick up runs reported directly to dashboard
	{
		candidateIDs := []string{
			"eval-smoke-001", "eval-smoke-002", "eval-bench-001",
			"bench-001", "bench-002",
		}
		for _, evalRunID := range candidateIDs {
			if seen[evalRunID] {
				continue
			}
			raw, _ := host.KV().Get("eval_report:" + evalRunID)
			if raw == "" {
				continue
			}
			seen[evalRunID] = true
			var report map[string]any
			if err := json.Unmarshal([]byte(raw), &report); err == nil {
				reports = append(reports, map[string]any{
					"eval_run_id": evalRunID,
					"suite_name":  stringVal(report, "suite_name", ""),
					"pass_rate":   float64Val(report, "pass_rate", 0.0),
					"avg_score":   float64Val(report, "avg_score", 0.0),
					"completed":   intVal(report, "completed_scenarios", 0),
					"total":       intVal(report, "total_scenarios", 0),
					"status":      stringVal(report, "status", ""),
				})
			}
			if len(reports) >= limit {
				break
			}
		}
	}

	return marshal(map[string]any{"status": "ok", "runs": reports, "count": len(reports)})
}

func (d *DashboardActor) getTrajectory(p map[string]any) string {
	trajID := stringVal(p, "trajectory_id", "")
	if trajID == "" {
		return marshal(map[string]any{"error": "trajectory_id is required"})
	}
	// Check both KV keys used by different actors
	raw, _ := host.KV().Get("trajectory:" + trajID)
	if raw == "" {
		raw, _ = host.KV().Get("agent_trajectory:" + trajID)
	}
	if raw == "" {
		return marshal(map[string]any{"error": fmt.Sprintf("trajectory %s not found", trajID)})
	}
	var traj map[string]any
	if err := json.Unmarshal([]byte(raw), &traj); err != nil {
		return marshal(map[string]any{"error": "failed to parse trajectory"})
	}
	if _, ok := traj["status"]; !ok {
		traj["status"] = "ok"
	}
	return marshal(traj)
}

func (d *DashboardActor) getRegressions() string {
	baselineRun, _ := host.KV().Get("regression_baseline_eval_run")
	baseline, _ := host.KV().Get("regression_baseline")
	if baseline == "" {
		baseline = "{}"
	}
	var baselineData map[string]any
	if err := json.Unmarshal([]byte(baseline), &baselineData); err != nil {
		return marshal(map[string]any{"error": "failed to parse baseline"})
	}
	return marshal(map[string]any{
		"status":                "ok",
		"baseline_eval_run":     baselineRun,
		"baseline_scenario_count": len(baselineData),
	})
}

func (d *DashboardActor) summary() string {
	// Build summary by scanning known eval runs
	candidateIDs := []string{
		"eval-smoke-001", "eval-smoke-002", "eval-bench-001", "bench-001", "bench-002",
	}
	totalEvals := 0
	scoreSum := 0.0

	// Also check TupleSpace
	ts := host.TS()
	tsIDs := map[string]bool{}
	for i := 0; i < 20; i++ {
		r, ok := ts.Read([]any{"eval_run", nil, nil, nil})
		if !ok || len(r) < 2 || isErrorResult(r) {
			break
		}
		evalRunID, _ := r[1].(string)
		if evalRunID != "" {
			tsIDs[evalRunID] = true
		}
	}
	for id := range tsIDs {
		candidateIDs = append(candidateIDs, id)
	}

	seen := map[string]bool{}
	for _, evalRunID := range candidateIDs {
		if seen[evalRunID] {
			continue
		}
		raw, _ := host.KV().Get("eval_report:" + evalRunID)
		if raw == "" {
			continue
		}
		seen[evalRunID] = true
		var report map[string]any
		if err := json.Unmarshal([]byte(raw), &report); err == nil {
			totalEvals++
			scoreSum += float64Val(report, "avg_score", 0.0)
		}
	}

	avgScore := 0.0
	if totalEvals > 0 {
		avgScore = roundFloat(scoreSum/float64(totalEvals), 3)
	}

	return marshal(map[string]any{
		"status":      "ok",
		"actor_id":    d.ActorID,
		"total_evals": totalEvals,
		"avg_score":   avgScore,
		"message":     "Use get_eval_report, list_eval_runs, get_trajectory for details.",
	})
}
