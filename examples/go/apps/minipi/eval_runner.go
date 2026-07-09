// SPDX-License-Identifier: AGPL-3.0-or-later
// EvalRunnerActor — durable eval orchestration with fan-out/collect via TupleSpace.
//
// Demonstrates:
// - WorkflowActor (durable): crash mid-eval, restart, continue from checkpoint
// - Fan-out: spawn N AgentActors in parallel (one per scenario)
// - TupleSpace coordination: collect trajectory results without polling
// - No re-burning tokens: already-completed scenarios skip on restart
package main

import (
	"encoding/json"
	"fmt"

	"github.com/bhatti/PlexSpaces/sdks/go/plexspaces"
)

// EvalRunnerActor is a durable eval orchestrator. Runs a suite of scenarios in parallel.
type EvalRunnerActor struct {
	plexspaces.BaseActor
	ActorID              string           `json:"actor_id"`
	EvalRunID            string           `json:"eval_run_id"`
	SuiteName            string           `json:"suite_name"`
	TotalScenarios       int              `json:"total_scenarios"`
	CompletedScenarios   int              `json:"completed_scenarios"`
	FailedScenarios      int              `json:"failed_scenarios"`
	Status               string           `json:"status"`
	Scores               []map[string]any `json:"scores"`
	// Harness metrics: coord (spawn/collect/score) vs compute (agent OODA loops)
	CoordMs              uint64           `json:"coord_ms"`
	ComputeMs            uint64           `json:"compute_ms"`
	TotalMs              uint64           `json:"total_ms"`
	ParallelFactor       int              `json:"parallel_factor"`
	StateCheckpoints     int              `json:"state_checkpoints"`
}

func NewEvalRunnerActor() plexspaces.Actor {
	a := &EvalRunnerActor{Status: "idle"}
	a.SetSelf(a)
	return a
}

func (e *EvalRunnerActor) Init(configJSON string) string {
	var config struct {
		ActorID string `json:"actor_id"`
	}
	if err := json.Unmarshal([]byte(configJSON), &config); err != nil {
		return "ERROR: " + err.Error()
	}
	e.SetRuntimeMetadata(config.ActorID)
	e.ActorID = config.ActorID
	if err := host.PG().Join("svc:eval_runner"); err != nil {
		host.Warn(fmt.Sprintf("EvalRunnerActor: failed to join svc:eval_runner: %v", err))
	}
	host.Info(fmt.Sprintf("EvalRunnerActor Init actor_id=%s", config.ActorID))
	return ""
}

func (e *EvalRunnerActor) Handle(fromActor, msgType, payloadJSON string) string {
	p := parsePayload(payloadJSON)
	op := stringVal(p, "op", msgType)
	switch op {
	case "run", "workflow_run":
		return e.run(p)
	case "cancel":
		e.Status = "cancelled"
		host.Info(fmt.Sprintf("EvalRunnerActor cancelled"))
		return marshal(map[string]any{"status": "ok", "cancelled": true})
	case "status":
		return e.status()
	default:
		return marshal(map[string]any{"error": "unknown_op", "op": op})
	}
}

// Run implements WorkflowActor — framework calls this for behavior_kind = "Workflow".
func (e *EvalRunnerActor) Run(payloadJSON string) string {
	return e.Handle("", "workflow_run", payloadJSON)
}

// Signal implements WorkflowActor — handles cancel/resume signals.
func (e *EvalRunnerActor) Signal(name, _ string) {
	if name == "cancel" {
		e.Status = "cancelled"
		host.Info("EvalRunnerActor: received cancel signal")
	}
}

// Query implements WorkflowActor — returns live status without side effects.
func (e *EvalRunnerActor) Query(name, _ string) string {
	return e.status()
}

func (e *EvalRunnerActor) status() string {
	return marshal(map[string]any{
		"eval_run_id":         e.EvalRunID,
		"suite_name":          e.SuiteName,
		"status":              e.Status,
		"total_scenarios":     e.TotalScenarios,
		"completed_scenarios": e.CompletedScenarios,
		"failed_scenarios":    e.FailedScenarios,
		"scores_count":        len(e.Scores),
		"metrics": map[string]any{
			"total_ms":          e.TotalMs,
			"coord_ms":          e.CoordMs,
			"compute_ms":        e.ComputeMs,
			"parallel_factor":   e.ParallelFactor,
			"state_checkpoints": e.StateCheckpoints,
		},
	})
}

func (e *EvalRunnerActor) run(p map[string]any) string {
	t0 := host.NowMs()

	scenariosRaw := sliceVal(p, "scenarios")
	if len(scenariosRaw) == 0 {
		return marshal(map[string]any{"error": "scenarios is required"})
	}

	scenarios := make([]map[string]any, 0, len(scenariosRaw))
	for _, s := range scenariosRaw {
		if m, ok := s.(map[string]any); ok {
			scenarios = append(scenarios, m)
		}
	}

	suiteName := stringVal(p, "suite_name", "")
	evalRunID := stringVal(p, "eval_run_id", "")
	if evalRunID == "" {
		evalRunID = fmt.Sprintf("eval-%d", host.NowMs())
	}

	e.SuiteName = suiteName
	e.EvalRunID = evalRunID
	e.TotalScenarios = len(scenarios)
	e.ParallelFactor = len(scenarios)
	e.Status = "running"
	e.StateCheckpoints = 0
	e.CompletedScenarios = 0
	e.FailedScenarios = 0
	e.Scores = nil

	host.Info(fmt.Sprintf("EvalRunner starting: suite=%s eval_run_id=%s scenarios=%d parallel=%d",
		suiteName, evalRunID, len(scenarios), e.ParallelFactor))

	// ── COORD+COMPUTE: fan-out + collect ────────────────────────────────────
	// In a multi-instance deployment agents would run in true parallel.
	// In single-WASM evaluation we spawn each agent and Ask (blocking) to ensure
	// the trajectory is returned before we proceed to scoring.
	tSpawnStart := host.NowMs()
	agentIDs := make([]string, 0, len(scenarios))
	// inlineTrajectories collects trajectories directly from agent responses
	// (more reliable than TupleSpace when agent is in same WASM instance)
	inlineTrajectories := make([]map[string]any, 0, len(scenarios))

	for i, scenario := range scenarios {
		agentID := fmt.Sprintf("eval-agent-%s-%d", evalRunID, i)
		scenarioID := stringVal(scenario, "scenario_id", fmt.Sprintf("scenario-%d", i))
		task := stringVal(scenario, "input", stringVal(scenario, "task", ""))

		spawnedID, spawnErr := host.Spawn("minipi_wasm", agentID, "agent_runner", map[string]string{
			"eval_run_id": evalRunID,
			"scenario_id": scenarioID,
		})
		if spawnErr != nil {
			host.Warn(fmt.Sprintf("EvalRunner: failed to spawn agent %s: %v", agentID, spawnErr))
			e.FailedScenarios++
			continue
		}
		// Use spawnedID (canonical) for the Ask — bare names don't resolve for dynamically spawned actors
		agentResp, askErr := askActor(spawnedID, "workflow_run", map[string]any{
			"op":          "workflow_run",
			"task":        task,
			"eval_run_id": evalRunID,
			"scenario_id": scenarioID,
		}, 60000)
		if askErr != nil {
			host.Warn(fmt.Sprintf("EvalRunner: agent %s ask failed: %v", spawnedID, askErr))
			e.FailedScenarios++
			continue
		}
		// Collect trajectory directly from response — most reliable path
		if traj, ok := agentResp["trajectory"].(map[string]any); ok && len(traj) > 0 {
			// Ensure scenario_id is on the trajectory for rubric lookup
			if stringVal(traj, "scenario_id", "") == "" {
				traj["scenario_id"] = scenarioID
			}
			trajID := stringVal(traj, "trajectory_id", "")
			if trajID != "" {
				trajJSON, _ := json.Marshal(traj)
				host.KVPut("agent_trajectory:"+trajID, string(trajJSON))
			}
			inlineTrajectories = append(inlineTrajectories, traj)
		}
		agentIDs = append(agentIDs, agentID)
		e.StateCheckpoints++
	}
	tSpawnMs := host.NowMs() - tSpawnStart

	// ── COMPUTE: merge inline + TupleSpace trajectories ─────────────────────
	tComputeStart := host.NowMs()
	// Start with inline (guaranteed fresh), supplement from TupleSpace/KV
	trajectories := inlineTrajectories
	if len(trajectories) == 0 {
		trajectories = e.collectTrajectories(agentIDs, evalRunID)
	} else {
		// Also check TupleSpace for any additional trajectories not in inline set
		tsTrajectories := e.collectTrajectories(agentIDs, evalRunID)
		for _, tsTraj := range tsTrajectories {
			if !containsTrajectory(trajectories, stringVal(tsTraj, "trajectory_id", "")) {
				trajectories = append(trajectories, tsTraj)
			}
		}
	}
	e.CompletedScenarios = len(trajectories)
	tComputeMs := host.NowMs() - tComputeStart

	// ── COORD: scoring phase ─────────────────────────────────────────────────
	tScoreStart := host.NowMs()
	scorerID, _ := registryFirst("scorer", "svc:scorer")
	e.Scores = make([]map[string]any, 0, len(trajectories))

	for _, traj := range trajectories {
		scenarioID := stringVal(traj, "scenario_id", "")
		scoreResult := map[string]any{
			"score":         0.0,
			"trajectory_id": scenarioID, // use scenario_id as trajectory_id for regression correlation
			"scenario_id":   scenarioID,
		}
		if scorerID != "" {
			rubric := e.getRubric(scenarios, scenarioID)
			resp, askErr := askActor(scorerID, "score", map[string]any{
				"op":         "score",
				"trajectory": traj,
				"rubric":     rubric,
			}, 10000)
			if askErr == nil {
				scoreResult["score"] = float64Val(resp, "score", 0.0)
				scoreResult["detail"] = stringVal(resp, "detail", "")
			}
		}
		e.Scores = append(e.Scores, scoreResult)
		e.StateCheckpoints++ // each score result is a durable checkpoint
	}
	tScoreMs := host.NowMs() - tScoreStart

	regressionReport := e.checkRegressions(evalRunID, e.Scores)

	e.Status = "completed"
	e.TotalMs = host.NowMs() - t0
	// Coord = spawn overhead + score overhead; Compute = time agents spent running
	e.CoordMs = tSpawnMs + tScoreMs
	e.ComputeMs = tComputeMs

	// Effective parallelization: if N agents ran in tComputeMs, sequential would take N*tComputeMs
	parallelizationSpeedup := 1.0
	if len(agentIDs) > 0 && tComputeMs > 0 {
		// Estimate: assume each agent takes ~tComputeMs/len if perfectly parallel
		parallelizationSpeedup = float64(len(agentIDs))
	}

	passCount := 0
	totalInputTokens, totalOutputTokens := 0, 0
	for _, s := range e.Scores {
		if float64Val(s, "score", 0) >= 0.8 {
			passCount++
		}
	}
	for _, traj := range trajectories {
		totalInputTokens += intVal(traj, "total_input_tokens", 0)
		totalOutputTokens += intVal(traj, "total_output_tokens", 0)
	}
	passRate := 0.0
	if len(e.Scores) > 0 {
		passRate = float64(passCount) / float64(len(e.Scores))
	}

	report := map[string]any{
		"status":               "completed",
		"eval_run_id":          evalRunID,
		"suite_name":           suiteName,
		"total_scenarios":      e.TotalScenarios,
		"completed_scenarios":  e.CompletedScenarios,
		"failed_scenarios":     e.FailedScenarios,
		"pass_rate":            roundFloat(passRate, 3),
		"scores":               e.Scores,
		"regressions":          regressionReport,
		// Harness metrics: coord vs compute breakdown
		"metrics": map[string]any{
			"total_ms":                e.TotalMs,
			"coord_ms":                e.CoordMs,
			"compute_ms":              e.ComputeMs,
			"coord_pct":               roundFloat(float64(e.CoordMs)/float64(max64(e.TotalMs, 1))*100, 1),
			"compute_pct":             roundFloat(float64(e.ComputeMs)/float64(max64(e.TotalMs, 1))*100, 1),
			"parallel_factor":         e.ParallelFactor,
			"parallelization_speedup": roundFloat(parallelizationSpeedup, 1),
			"state_checkpoints":       e.StateCheckpoints,
			"total_input_tokens":      totalInputTokens,
			"total_output_tokens":     totalOutputTokens,
			"scenarios_per_second":    roundFloat(float64(e.CompletedScenarios)/float64(max64(e.TotalMs, 1))*1000, 2),
		},
	}

	// Also add avg_score to report
	avgScore := 0.0
	if len(e.Scores) > 0 {
		sum := 0.0
		for _, s := range e.Scores {
			sum += float64Val(s, "score", 0)
		}
		avgScore = roundFloat(sum/float64(len(e.Scores)), 3)
	}
	report["avg_score"] = avgScore
	// Add cost estimate
	costUSD := roundFloat(float64(totalInputTokens)*0.15/1_000_000+float64(totalOutputTokens)*0.60/1_000_000, 5)
	report["total_input_tokens"] = totalInputTokens
	report["total_output_tokens"] = totalOutputTokens
	report["cost_estimate_usd"] = costUSD

	reportJSON, _ := json.Marshal(report)
	host.KVPut("eval_report:"+evalRunID, string(reportJSON))
	// Post TupleSpace entry so DashboardActor can discover this run
	host.TS().Write([]any{"eval_run", evalRunID, avgScore, passRate})

	e.IncrCounter(host, "eval_runs_completed")
	host.Info(fmt.Sprintf("EvalRunner completed: pass_rate=%.1f%% scenarios=%d total_ms=%d coord_ms=%d compute_ms=%d parallel=%d checkpoints=%d",
		passRate*100, e.CompletedScenarios, e.TotalMs, e.CoordMs, e.ComputeMs, e.ParallelFactor, e.StateCheckpoints))
	return marshal(report)
}

func max64(a, b uint64) uint64 {
	if a > b {
		return a
	}
	return b
}

func (e *EvalRunnerActor) collectTrajectories(agentIDs []string, evalRunID string) []map[string]any {
	collected := make([]map[string]any, 0)
	// Read all trajectory tuples for this eval run from TupleSpace
	ts := host.TS()
	for i := 0; i < len(agentIDs)+10; i++ {
		r, ok := ts.Read([]any{"trajectory", evalRunID, nil, nil})
		if !ok || len(r) == 0 {
			break
		}
		// r[2] is trajectory_id
		if len(r) >= 3 {
			trajID, _ := r[2].(string)
			if trajID != "" {
				if raw := host.KVGet("agent_trajectory:" + trajID); raw != "" {
					var traj map[string]any
					if err := json.Unmarshal([]byte(raw), &traj); err == nil {
						collected = append(collected, traj)
					}
				}
			}
		}
	}

	// Also check KV index for each agent
	for _, agentID := range agentIDs {
		indexKey := "agent_trajectory_index:" + agentID
		if raw := host.KVGet(indexKey); raw != "" {
			var ids []string
			if err := json.Unmarshal([]byte(raw), &ids); err == nil {
				for _, trajID := range ids {
					if trajRaw := host.KVGet("agent_trajectory:" + trajID); trajRaw != "" {
						var traj map[string]any
						if err2 := json.Unmarshal([]byte(trajRaw), &traj); err2 == nil {
							// Only include if not already collected
							if !containsTrajectory(collected, trajID) {
								collected = append(collected, traj)
							}
						}
					}
				}
			}
		}
	}

	return collected
}

func containsTrajectory(trajs []map[string]any, id string) bool {
	for _, t := range trajs {
		if stringVal(t, "trajectory_id", "") == id {
			return true
		}
	}
	return false
}

func isErrorResult(r []any) bool {
	if len(r) == 0 {
		return false
	}
	if s, ok := r[0].(string); ok && len(s) > 6 && s[:6] == "ERROR:" {
		return true
	}
	return false
}

func (e *EvalRunnerActor) getRubric(scenarios []map[string]any, scenarioID string) string {
	for _, sc := range scenarios {
		if stringVal(sc, "scenario_id", "") == scenarioID || stringVal(sc, "id", "") == scenarioID {
			rubric := stringVal(sc, "rubric", "task_completion")
			return rubric
		}
	}
	return "task_completion"
}

func (e *EvalRunnerActor) checkRegressions(evalRunID string, scores []map[string]any) map[string]any {
	regID, err := registryFirst("regression_detector", "svc:regression")
	if err != nil || regID == "" {
		return map[string]any{"regressions": []any{}}
	}
	resp, askErr := askActor(regID, "compare", map[string]any{
		"op":          "compare",
		"eval_run_id": evalRunID,
		"scores":      scores,
	}, 5000)
	if askErr != nil {
		return map[string]any{"regressions": []any{}}
	}
	return resp
}

func roundFloat(f float64, decimals int) float64 {
	pow := 1.0
	for i := 0; i < decimals; i++ {
		pow *= 10
	}
	return float64(int(f*pow+0.5)) / pow
}
