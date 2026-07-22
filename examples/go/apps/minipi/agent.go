// SPDX-License-Identifier: AGPL-3.0-or-later
// AgentActor — OODA-loop agent with durable execution trace capture.
//
// Demonstrates the harness layer: loop control, tool calling, token budget,
// crash recovery via DurabilityFacet, and execution trace export via
// ExecutionTraceFacet.
//
// Harness = agent - model (everything except the LLM).
package main

import (
	"encoding/json"
	"fmt"

	"github.com/bhatti/PlexSpaces/sdks/go/plexspaces"
)

const (
	agentMaxIter    = 10
	agentTokenBudget = 4096
)

// AgentActor runs an OODA loop: Observe → Orient → Decide → Act.
//
// Durable: DurabilityFacet journals every step. Crash at step 7?
// Restart brings back all prior steps from journal — no re-burned tokens.
//
// Execution Trace: ExecutionTraceFacet captures ordered step sequence for eval,
// writes trace:{id} and trace_index:{actor_id} to KV on completion.
type AgentActor struct {
	plexspaces.BaseActor
	ActorID         string `json:"actor_id"`
	Task            string `json:"task"`
	IterationsDone  int    `json:"iterations_done"`
	TotalToolCalls  int    `json:"total_tool_calls"`
	EvalRunID       string `json:"eval_run_id"`
	ScenarioID      string `json:"scenario_id"`
}

func NewAgentActor() plexspaces.Actor {
	a := &AgentActor{}
	a.SetSelf(a)
	return a
}

func (a *AgentActor) Init(configJSON string) string {
	var config struct {
		ActorID string            `json:"actor_id"`
		Args    map[string]string `json:"args"`
	}
	if err := json.Unmarshal([]byte(configJSON), &config); err != nil {
		return "ERROR: " + err.Error()
	}
	a.SetRuntimeMetadata(config.ActorID)
	a.ActorID = config.ActorID
	if v, ok := config.Args["eval_run_id"]; ok {
		a.EvalRunID = v
	}
	if v, ok := config.Args["scenario_id"]; ok {
		a.ScenarioID = v
	}
	if err := host.PG().Join("svc:agents"); err != nil {
		host.Warn(fmt.Sprintf("AgentActor: failed to join svc:agents: %v", err))
	}
	host.Info(fmt.Sprintf("AgentActor Init actor_id=%s eval_run=%s", config.ActorID, a.EvalRunID))
	return ""
}

func (a *AgentActor) Handle(fromActor, msgType, payloadJSON string) string {
	p := parsePayload(payloadJSON)
	op := stringVal(p, "op", msgType)
	switch op {
	case "run", "workflow_run":
		return a.run(p)
	case "resume":
		host.Info(fmt.Sprintf("AgentActor resumed: %v", p))
		return marshal(map[string]any{"status": "ok", "resumed": true})
	case "execution_trace":
		return a.queryExecutionTrace()
	case "status":
		return a.queryStatus()
	default:
		return marshal(map[string]any{"error": "unknown_op", "op": op})
	}
}

// Run implements WorkflowActor — framework calls this for behavior_kind = "Workflow".
func (a *AgentActor) Run(payloadJSON string) string {
	return a.Handle("", "workflow_run", payloadJSON)
}

// Signal implements WorkflowActor — handles resume/cancel signals.
func (a *AgentActor) Signal(name, _ string) {
	host.Info(fmt.Sprintf("AgentActor signal: %s", name))
}

// Query implements WorkflowActor — returns trace or status without side effects.
func (a *AgentActor) Query(name, _ string) string {
	switch name {
	case "execution_trace":
		return a.queryExecutionTrace()
	default:
		return a.queryStatus()
	}
}

func (a *AgentActor) run(p map[string]any) string {
	task := stringVal(p, "task", "")
	if evalRunID := stringVal(p, "eval_run_id", ""); evalRunID != "" {
		a.EvalRunID = evalRunID
	}
	if scenarioID := stringVal(p, "scenario_id", ""); scenarioID != "" {
		a.ScenarioID = scenarioID
	}
	if task == "" {
		return marshal(map[string]any{"error": "task is required"})
	}
	a.Task = task

	host.Info(fmt.Sprintf("AgentActor starting task: %.80s", task))

	cfg := plexspaces.AgentConfig{
		MaxIterations: agentMaxIter,
		TokenBudget:   agentTokenBudget,
		EvalRunID:     a.EvalRunID,
		ScenarioID:    a.ScenarioID,
	}
	actorID := a.ActorID
	if actorID == "" {
		actorID = "agent-unknown"
	}
	loop := plexspaces.NewAgentLoop(actorID, cfg)

	for !loop.IterationLimitReached() {
		if loop.BudgetExceeded() {
			a.IncrCounter(host, "agent_budget_exceeded")
			traj := loop.FinalizeTrajectory("budget_exceeded", fmt.Sprintf("Token budget %d exceeded", agentTokenBudget))
			a.exportTrajectory(traj)
			trajJSON, _ := json.Marshal(traj)
			return marshal(map[string]any{
				"status":      "budget_exceeded",
				"trajectory":  json.RawMessage(trajJSON),
				"trajectory_id": traj.TrajectoryID,
			})
		}

		if loop.IsSuspended() {
			traj := loop.GetTrajectory()
			trajJSON, _ := json.Marshal(traj)
			return marshal(map[string]any{
				"status":     "suspended",
				"trajectory": json.RawMessage(trajJSON),
				"trajectory_id": traj.TrajectoryID,
			})
		}

		// OBSERVE: fetch context from memory and environment
		observations := a.doObserve(loop, task)

		// ORIENT: analyze observations with LLM
		plan := a.doOrient(loop, observations)

		// DECIDE: pick next action
		action := a.doDecide(loop, plan)

		if boolVal(action.(map[string]any), "done") {
			break
		}

		// Check for approval-required actions
		if boolVal(action.(map[string]any), "needs_approval") {
			toolName := stringVal(action.(map[string]any), "tool_name", "unknown")
			loop.Suspend("action_needs_approval:" + toolName)
			traj := loop.GetTrajectory()
			trajJSON, _ := json.Marshal(traj)
			return marshal(map[string]any{
				"status":     "suspended",
				"trajectory": json.RawMessage(trajJSON),
				"trajectory_id": traj.TrajectoryID,
			})
		}

		// ACT: execute the chosen tool
		a.doAct(loop, action.(map[string]any))
		a.TotalToolCalls++
		a.IterationsDone++

		a.IncrCounter(host, "agent_iterations")
		loop.IncrementIteration()
	}

	traj := loop.FinalizeTrajectory("completed", fmt.Sprintf("Completed %d iterations", a.IterationsDone))
	a.exportTrajectory(traj)
	a.IncrCounter(host, "agent_runs_completed")
	trajJSON, _ := json.Marshal(traj)
	return marshal(map[string]any{
		"status":      "success",
		"task":        task,
		"iterations":  a.IterationsDone,
		"trajectory":  json.RawMessage(trajJSON),
		"trajectory_id": traj.TrajectoryID,
		"step_count":  len(traj.Steps),
		"outcome":     traj.Outcome,
	})
}

func (a *AgentActor) doObserve(loop *plexspaces.AgentLoop, task string) any {
	memoryKey := "agent_memory:" + a.ActorID
	priorContext := map[string]any{}
	if raw, _ := host.KV().Get(memoryKey); raw != "" {
		var m map[string]any
		if err := json.Unmarshal([]byte(raw), &m); err == nil {
			priorContext = m
		}
	}
	observations := map[string]any{
		"task":          task,
		"prior_context": priorContext,
		"iteration":     a.IterationsDone,
	}
	return loop.Observe(observations)
}

func (a *AgentActor) doOrient(loop *plexspaces.AgentLoop, observations any) any {
	obs, _ := observations.(map[string]any)
	if obs == nil {
		obs = map[string]any{}
	}
	llmID, err := registryFirst("llm_gateway", "svc:llm_gateway", "completion")

	var plan map[string]any
	if err != nil || llmID == "" {
		plan = map[string]any{
			"analysis":  fmt.Sprintf("Processing task: %v", obs["task"]),
			"next_tool": "calculator",
			"arguments": map[string]any{"expression": "1+1"},
			"done":      false,
		}
	} else {
		messages := []any{
			map[string]any{"role": "system", "content": "You are a helpful agent. Analyze the task and decide what to do next."},
			map[string]any{"role": "user", "content": fmt.Sprintf("Task: %v\nIteration: %v", obs["task"], obs["iteration"])},
		}
		resp, askErr := askActor(llmID, "completion", map[string]any{
			"op":       "completion",
			"messages": messages,
		}, 10000)
		if askErr != nil || len(resp) == 0 {
			plan = map[string]any{"done": true, "result": "LLM unavailable"}
		} else {
			response, _ := resp["response"].(map[string]any)
			if response == nil {
				response = map[string]any{}
			}
			stopReason := stringVal(response, "stop_reason", "end_turn")
			toolCalls := sliceVal(response, "tool_calls")
			plan = map[string]any{
				"analysis":      stringVal(response, "content", ""),
				"next_tool":     "calculator",
				"arguments":     map[string]any{},
				"input_tokens":  intVal(resp, "input_tokens", 0),
				"output_tokens": intVal(resp, "output_tokens", 0),
				"model":         stringVal(resp, "model", ""),
				"done":          stopReason == "end_turn" && len(toolCalls) == 0,
			}
		}
	}
	return loop.Orient(plan)
}

func (a *AgentActor) doDecide(loop *plexspaces.AgentLoop, plan any) any {
	planMap, _ := plan.(map[string]any)
	if planMap == nil {
		planMap = map[string]any{}
	}
	toolName := stringVal(planMap, "next_tool", "calculator")
	args, _ := planMap["arguments"].(map[string]any)
	if args == nil {
		args = map[string]any{}
	}
	action := map[string]any{
		"tool_name":      toolName,
		"arguments":      args,
		"done":           boolVal(planMap, "done"),
		"needs_approval": boolVal(planMap, "needs_approval"),
	}
	return loop.Decide(action)
}

func (a *AgentActor) doAct(loop *plexspaces.AgentLoop, action map[string]any) any {
	toolName := stringVal(action, "tool_name", "")
	arguments, _ := action["arguments"].(map[string]any)
	if arguments == nil {
		arguments = map[string]any{}
	}

	toolID, toolErr := registryFirst("tool_registry", "svc:tools")
	var result map[string]any
	if toolErr == nil && toolID != "" {
		// Use "execute" as msgType so SchemaValidationFacet (keyed by method name)
		// does not apply tool-specific schemas to the wrapper envelope.
		resp, askErr := askActor(toolID, "execute", map[string]any{
			"op":    "execute",
			"name":  toolName,
			"input": arguments,
		}, 5000)
		if askErr == nil {
			result = resp
		}
	}
	if result == nil {
		result = map[string]any{"error": "tool_registry unavailable", "tool": toolName}
	}

	inputTokens := intVal(result, "input_tokens", 0)
	outputTokens := intVal(result, "output_tokens", 0)
	return loop.ToolCall(toolName, arguments, result, inputTokens, outputTokens, "")
}

func (a *AgentActor) exportTrajectory(traj plexspaces.AgentTrajectory) {
	key := "agent_trajectory:" + traj.TrajectoryID
	trajJSON, err := json.Marshal(traj)
	if err != nil {
		host.Warn(fmt.Sprintf("AgentActor: failed to marshal trajectory: %v", err))
		return
	}
	host.KV().Put(key, string(trajJSON))

	// Update per-agent index
	indexKey := "agent_trajectory_index:" + a.ActorID
	existing := func() []string { v, _ := host.KV().Get(indexKey); return parseStringSlice(v) }()
	existing = append(existing, traj.TrajectoryID)
	indexJSON, _ := json.Marshal(existing)
	host.KV().Put(indexKey, string(indexJSON))

	// Write trajectory tuple to TupleSpace for eval collection
	_ = host.TS().Write([]any{"trajectory", traj.EvalRunID, traj.TrajectoryID, traj.Outcome})

	// Store in trajectory store if available
	tsID, err := registryFirst("trajectory_store", "svc:trajectory_store")
	if err == nil && tsID != "" {
		trajMap := map[string]any{}
		if err2 := json.Unmarshal(trajJSON, &trajMap); err2 == nil {
			_, _ = askActor(tsID, "put", map[string]any{
				"op":         "put",
				"trajectory": trajMap,
			}, 3000)
		}
	}
}

func (a *AgentActor) queryExecutionTrace() string {
	indexKey := "trace_index:" + a.ActorID
	indexRaw, _ := host.KV().Get(indexKey)
	if indexRaw != "" {
		var traceIDs []string
		if err := json.Unmarshal([]byte(indexRaw), &traceIDs); err == nil && len(traceIDs) > 0 {
			raw, _ := host.KV().Get("trace:" + traceIDs[len(traceIDs)-1])
			if raw != "" {
				var trace map[string]any
				if err2 := json.Unmarshal([]byte(raw), &trace); err2 == nil {
					return marshal(trace)
				}
			}
		}
	}
	return marshal(map[string]any{"actor_id": a.ActorID, "steps": []any{}, "outcome": "running"})
}

func (a *AgentActor) queryStatus() string {
	task := a.Task
	if len(task) > 80 {
		task = task[:80]
	}
	return marshal(map[string]any{
		"actor_id":        a.ActorID,
		"task":            task,
		"iterations_done": a.IterationsDone,
		"total_tool_calls": a.TotalToolCalls,
	})
}
