// SPDX-License-Identifier: AGPL-3.0-or-later
// BenchmarkActor — fan-out N eval runs with different configs, measure throughput.
//
// Demonstrates: parallel eval fan-out, config comparison, performance measurement.
// The key insight: harness changes (cheap) often beat model changes (expensive).
package main

import (
	"encoding/json"
	"fmt"

	"github.com/bhatti/PlexSpaces/sdks/go/plexspaces"
)

// BenchmarkActor runs the same scenarios with N different harness configs.
//
// Measures:
// - Latency (total_ms, coord_ms, compute_ms per config)
// - Token cost (input + output tokens per scenario)
// - Quality score (from ScorerActor)
// - Pass rate (scenarios scoring >= 0.8)
// - Parallelization factor and speedup vs sequential
// - State management checkpoints (durability overhead)
//
// Output: comparison table showing which harness config wins and why.
type BenchmarkActor struct {
	plexspaces.BaseActor
	ActorID     string           `json:"actor_id"`
	BenchmarkID string           `json:"benchmark_id"`
	Status      string           `json:"status"`
	Results     []map[string]any `json:"results"`
	TotalMs     uint64           `json:"total_ms"`
}

func NewBenchmarkActor() plexspaces.Actor {
	a := &BenchmarkActor{Status: "idle"}
	a.SetSelf(a)
	return a
}

func (b *BenchmarkActor) Init(configJSON string) string {
	var config struct {
		ActorID string `json:"actor_id"`
	}
	if err := json.Unmarshal([]byte(configJSON), &config); err != nil {
		return "ERROR: " + err.Error()
	}
	b.SetRuntimeMetadata(config.ActorID)
	b.ActorID = config.ActorID
	if err := host.PG().Join("svc:benchmark"); err != nil {
		host.Warn(fmt.Sprintf("BenchmarkActor: failed to join svc:benchmark: %v", err))
	}
	host.Info(fmt.Sprintf("BenchmarkActor Init actor_id=%s", config.ActorID))
	return ""
}

func (b *BenchmarkActor) Handle(fromActor, msgType, payloadJSON string) string {
	p := parsePayload(payloadJSON)
	op := stringVal(p, "op", msgType)
	switch op {
	case "run", "workflow_run":
		return b.run(p)
	case "status":
		return b.status()
	default:
		return marshal(map[string]any{"error": "unknown_op", "op": op})
	}
}

// Run implements WorkflowActor — framework calls this for behavior_kind = "Workflow".
func (b *BenchmarkActor) Run(payloadJSON string) string {
	return b.Handle("", "workflow_run", payloadJSON)
}

// Signal implements WorkflowActor.
func (b *BenchmarkActor) Signal(name, _ string) {
	if name == "cancel" {
		b.Status = "cancelled"
		host.Info("BenchmarkActor: received cancel signal")
	}
}

// Query implements WorkflowActor.
func (b *BenchmarkActor) Query(name, _ string) string {
	return b.status()
}

func (b *BenchmarkActor) status() string {
	return marshal(map[string]any{
		"benchmark_id":  b.BenchmarkID,
		"status":        b.Status,
		"results_count": len(b.Results),
	})
}

func (b *BenchmarkActor) run(p map[string]any) string {
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

	configsRaw := sliceVal(p, "configs")
	if len(configsRaw) == 0 {
		configsRaw = []any{
			map[string]any{"name": "default", "max_iterations": 10, "token_budget": 4096},
		}
	}
	configs := make([]map[string]any, 0, len(configsRaw))
	for _, c := range configsRaw {
		if m, ok := c.(map[string]any); ok {
			configs = append(configs, m)
		}
	}

	benchmarkID := stringVal(p, "benchmark_id", "")
	if benchmarkID == "" {
		benchmarkID = fmt.Sprintf("bench-%d", host.NowMs())
	}
	b.BenchmarkID = benchmarkID
	b.Status = "running"

	host.Info(fmt.Sprintf("BenchmarkActor starting: benchmark_id=%s configs=%d scenarios=%d",
		benchmarkID, len(configs), len(scenarios)))

	startMs := host.NowMs()

	// Fan-out: run each config in parallel via separate EvalRunnerActor instances
	type evalRunInfo struct {
		EvalRunID string
		Config    map[string]any
		RunnerID  string
	}
	evalRuns := make([]evalRunInfo, 0, len(configs))

	for i, cfg := range configs {
		evalRunID := fmt.Sprintf("bench-%s-config-%d", benchmarkID, i)
		evalRunnerID := fmt.Sprintf("eval-runner-%s", evalRunID)
		cfgName := stringVal(cfg, "name", fmt.Sprintf("config-%d", i))

		spawnedRunnerID, spawnErr := host.Spawn("minipi_wasm", evalRunnerID, "eval_runner", map[string]string{
			"config_name": cfgName,
		})
		if spawnErr != nil {
			host.Warn(fmt.Sprintf("BenchmarkActor: failed to spawn eval runner for config %s: %v", cfgName, spawnErr))
			continue
		}

		// Ask (blocking) using canonical spawnedRunnerID so resolve_actor_id finds the newly spawned actor
		evalResp, evalErr := askActor(spawnedRunnerID, "workflow_run", map[string]any{
			"op":          "workflow_run",
			"suite_name":  fmt.Sprintf("benchmark-%s", cfgName),
			"scenarios":   scenarios,
			"eval_run_id": evalRunID,
		}, 120000)
		if evalErr != nil {
			host.Warn(fmt.Sprintf("BenchmarkActor: eval runner %s failed: %v", spawnedRunnerID, evalErr))
		} else {
			// Cache the response directly in case KV write raced
			respJSON, _ := json.Marshal(evalResp)
			host.KV().Put("eval_report:"+evalRunID, string(respJSON))
		}
		evalRuns = append(evalRuns, evalRunInfo{
			EvalRunID: evalRunID,
			Config:    cfg,
			RunnerID:  evalRunnerID,
		})
		host.Info(fmt.Sprintf("BenchmarkActor: completed eval run %s with config=%s", evalRunID, cfgName))
	}

	// Collect results from KV (written by EvalRunnerActor on completion)
	b.Results = make([]map[string]any, 0, len(evalRuns))
	for _, run := range evalRuns {
		var report map[string]any
		raw, _ := host.KV().Get("eval_report:" + run.EvalRunID)
		if raw != "" {
			_ = json.Unmarshal([]byte(raw), &report)
		}
		if report == nil {
			report = map[string]any{
				"status":      "not_found",
				"eval_run_id": run.EvalRunID,
			}
		}

		cfgName := stringVal(run.Config, "name", fmt.Sprintf("config-%d", len(b.Results)))
		// Extract harness metrics from eval report
		metrics, _ := report["metrics"].(map[string]any)
		if metrics == nil {
			metrics = map[string]any{}
		}
		b.Results = append(b.Results, map[string]any{
			"config_name":              cfgName,
			"config":                   run.Config,
			"eval_run_id":              run.EvalRunID,
			"pass_rate":                float64Val(report, "pass_rate", 0.0),
			"completed_scenarios":      intVal(report, "completed_scenarios", 0),
			"total_scenarios":          intVal(report, "total_scenarios", len(scenarios)),
			"total_ms":                 intVal(metrics, "total_ms", 0),
			"coord_ms":                 intVal(metrics, "coord_ms", 0),
			"compute_ms":               intVal(metrics, "compute_ms", 0),
			"coord_pct":                float64Val(metrics, "coord_pct", 0),
			"parallelization_speedup":  float64Val(metrics, "parallelization_speedup", 1),
			"state_checkpoints":        intVal(metrics, "state_checkpoints", 0),
			"total_input_tokens":       intVal(metrics, "total_input_tokens", 0),
			"total_output_tokens":      intVal(metrics, "total_output_tokens", 0),
			"scenarios_per_second":     float64Val(metrics, "scenarios_per_second", 0),
		})
	}

	// Sort by pass rate (best first) — simple selection sort for small N
	for i := 0; i < len(b.Results); i++ {
		best := i
		for j := i + 1; j < len(b.Results); j++ {
			if float64Val(b.Results[j], "pass_rate", 0) > float64Val(b.Results[best], "pass_rate", 0) {
				best = j
			}
		}
		if best != i {
			b.Results[i], b.Results[best] = b.Results[best], b.Results[i]
		}
	}

	b.Status = "completed"
	b.TotalMs = host.NowMs() - startMs
	comparisonTable := b.formatComparisonTable()

	winner := ""
	if len(b.Results) > 0 {
		winner = stringVal(b.Results[0], "config_name", "")
	}

	b.IncrCounter(host, "benchmarks_completed")
	host.Info(fmt.Sprintf("BenchmarkActor completed: benchmark_id=%s configs=%d total_ms=%d winner=%s",
		benchmarkID, len(b.Results), b.TotalMs, winner))

	return marshal(map[string]any{
		"status":            "completed",
		"benchmark_id":      benchmarkID,
		"configs_tested":    len(b.Results),
		"scenarios":         len(scenarios),
		"total_duration_ms": b.TotalMs,
		"results":           b.Results,
		"comparison_table":  comparisonTable,
		"winner":            winner,
	})
}

func (b *BenchmarkActor) formatComparisonTable() []any {
	table := make([]any, 0, len(b.Results))
	for _, r := range b.Results {
		cfg, _ := r["config"].(map[string]any)
		if cfg == nil {
			cfg = map[string]any{}
		}
		passRate := float64Val(r, "pass_rate", 0)
		completed := intVal(r, "completed_scenarios", 0)
		total := intVal(r, "total_scenarios", 0)
		coordPct := float64Val(r, "coord_pct", 0)
		speedup := float64Val(r, "parallelization_speedup", 1)
		checkpoints := intVal(r, "state_checkpoints", 0)
		inTok := intVal(r, "total_input_tokens", 0)
		outTok := intVal(r, "total_output_tokens", 0)
		sps := float64Val(r, "scenarios_per_second", 0)
		table = append(table, map[string]any{
			"config":                  stringVal(r, "config_name", ""),
			"pass_rate":               fmt.Sprintf("%.1f%%", passRate*100),
			"completed":               fmt.Sprintf("%d/%d", completed, total),
			"total_ms":                intVal(r, "total_ms", 0),
			"coord_ms":                intVal(r, "coord_ms", 0),
			"compute_ms":              intVal(r, "compute_ms", 0),
			"coord_overhead_pct":      fmt.Sprintf("%.1f%%", coordPct),
			"parallelization_speedup": fmt.Sprintf("%.1fx", speedup),
			"state_checkpoints":       checkpoints,
			"tokens_in_out":           fmt.Sprintf("%d/%d", inTok, outTok),
			"scenarios_per_sec":       fmt.Sprintf("%.2f", sps),
			"max_iterations":          fmt.Sprintf("%v", cfg["max_iterations"]),
			"token_budget":            fmt.Sprintf("%v", cfg["token_budget"]),
		})
	}
	return table
}
