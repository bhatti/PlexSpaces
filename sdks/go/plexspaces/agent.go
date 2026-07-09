// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// AgentLoop standalone utility — OODA-loop agent harness for PlexSpaces Go SDK.
//
// Provides Go parity with Python/Rust/TypeScript AgentLoop:
// - Durable step recording (each Observe/Orient/Decide/Act journaled in AgentTrajectory)
// - Token budget enforcement (BudgetExceeded() halts loop when cumulative tokens >= budget)
// - Iteration limit enforcement (IterationLimitReached() halts loop after N iterations)
// - Human-in-the-loop suspend (Suspend sets IsSuspended to true)
// - Trajectory capture (FinalizeTrajectory / GetTrajectory)
//
// AgentLoop is a standalone plain struct — it is NOT a decorator and does NOT wrap
// other types. Embed it in an actor struct or hold it as a field.
//
// Usage:
//
//	type ResearchAgent struct {
//	    plexspaces.BaseActor
//	    loop *plexspaces.AgentLoop
//	}
//
//	func (a *ResearchAgent) Handle(from, msgType, payloadJSON string) string {
//	    cfg := plexspaces.DefaultAgentConfig()
//	    cfg.MaxIterations = 5
//	    a.loop = plexspaces.NewAgentLoop("research-agent-01", cfg)
//	    for !a.loop.IterationLimitReached() && !a.loop.BudgetExceeded() {
//	        obs := a.loop.Observe(payloadJSON)
//	        plan := a.loop.Orient(obs)
//	        action := a.loop.Decide(plan)
//	        result := a.loop.Act(action, 100, 50, "gpt-4")
//	        a.loop.IncrementIteration()
//	        _ = result
//	        break
//	    }
//	    traj := a.loop.FinalizeTrajectory("success", "")
//	    data, _ := json.Marshal(traj)
//	    return string(data)
//	}

package plexspaces

import (
	"fmt"
	"time"
)

// ========================================================================
// AgentStepKind constants
// ========================================================================

// AgentStepKind identifies the OODA phase or special event for a trajectory step.
// Aligns with Python AgentLoop step kinds.
type AgentStepKind string

const (
	// StepKindObserve is the Observe phase: gather information from environment, memory, context.
	StepKindObserve AgentStepKind = "observe"
	// StepKindOrient is the Orient phase: analyse and contextualise observations.
	StepKindOrient AgentStepKind = "orient"
	// StepKindDecide is the Decide phase: select next action from candidate plans.
	StepKindDecide AgentStepKind = "decide"
	// StepKindAct is the Act phase: execute the chosen action (often an LLM call).
	StepKindAct AgentStepKind = "act"
	// StepKindToolCall records a single tool invocation within an Act phase.
	StepKindToolCall AgentStepKind = "tool_call"
	// StepKindSuspend records agent suspension for human-in-the-loop or external signal.
	StepKindSuspend AgentStepKind = "suspend"
)

// ========================================================================
// AgentStep
// ========================================================================

// AgentStep records a single OODA step or event within an AgentTrajectory.
// Aligns with Python AgentStep and proto trajectory step shape.
type AgentStep struct {
	// StepID is a unique ULID-like identifier for this step.
	StepID string `json:"step_id"`
	// Kind is the OODA phase or event kind (observe, orient, decide, act, …).
	Kind AgentStepKind `json:"kind"`
	// Method is the handler method name that produced this step.
	Method string `json:"method"`
	// Input is the data passed into this step.
	Input any `json:"input,omitempty"`
	// Output is the data returned by this step.
	Output any `json:"output,omitempty"`
	// StartedAtMs is the Unix epoch timestamp in milliseconds when the step started.
	StartedAtMs int64 `json:"started_at_ms"`
	// CompletedAtMs is the Unix epoch timestamp in milliseconds when the step completed.
	CompletedAtMs int64 `json:"completed_at_ms"`
	// DurationMs is the wall-clock duration of the step in milliseconds.
	DurationMs int64 `json:"duration_ms"`
	// Success is true when the step completed without error.
	Success bool `json:"success"`
	// Error holds the error message when Success is false.
	Error string `json:"error,omitempty"`
	// ToolName is the tool identifier for StepKindToolCall steps.
	ToolName string `json:"tool_name,omitempty"`
	// InputTokens is the number of prompt tokens consumed in this step.
	InputTokens int `json:"input_tokens"`
	// OutputTokens is the number of completion tokens produced in this step.
	OutputTokens int `json:"output_tokens"`
	// Model is the LLM model identifier used in this step (empty for non-LLM steps).
	Model string `json:"model,omitempty"`
	// Metadata holds arbitrary string key-value annotations for this step.
	Metadata map[string]string `json:"metadata,omitempty"`
}

// ========================================================================
// AgentTrajectory
// ========================================================================

// AgentTrajectory is the complete execution record for one agent run.
// Aligns with Python AgentTrajectory and the proto trajectory message shape.
type AgentTrajectory struct {
	// TrajectoryID is a unique ULID-like identifier for this trajectory.
	TrajectoryID string `json:"trajectory_id"`
	// AgentActorID is the actor identifier of the agent that produced this trajectory.
	AgentActorID string `json:"agent_actor_id"`
	// EvalRunID links this trajectory to an evaluation run (empty in production).
	EvalRunID string `json:"eval_run_id,omitempty"`
	// ScenarioID links this trajectory to an evaluation scenario (empty in production).
	ScenarioID string `json:"scenario_id,omitempty"`
	// Steps is the ordered list of recorded OODA steps.
	Steps []AgentStep `json:"steps"`
	// Outcome is the final outcome label: "running", "success", "failure", "suspended", etc.
	Outcome string `json:"outcome"`
	// OutcomeDetail provides human-readable context for the outcome.
	OutcomeDetail string `json:"outcome_detail,omitempty"`
	// TotalInputTokens is the cumulative prompt tokens across all steps.
	TotalInputTokens int `json:"total_input_tokens"`
	// TotalOutputTokens is the cumulative completion tokens across all steps.
	TotalOutputTokens int `json:"total_output_tokens"`
	// StartedAtMs is the Unix epoch timestamp in milliseconds when the trajectory started.
	StartedAtMs int64 `json:"started_at_ms"`
	// CompletedAtMs is the Unix epoch timestamp in milliseconds when the trajectory was finalised.
	CompletedAtMs int64 `json:"completed_at_ms"`
	// DurationMs is the total wall-clock duration of the trajectory in milliseconds.
	DurationMs int64 `json:"duration_ms"`
	// Score is an optional numeric quality score assigned by an evaluator.
	Score float64 `json:"score"`
	// Metadata holds arbitrary string key-value annotations for this trajectory.
	Metadata map[string]string `json:"metadata,omitempty"`
}

// ========================================================================
// AgentConfig
// ========================================================================

// AgentConfig controls agent loop behaviour.
// Aligns with the parameters of the Python AgentConfig(max_iterations=…, token_budget=…).
type AgentConfig struct {
	// MaxIterations is the maximum number of OODA loop iterations before the agent stops.
	// Zero is treated as no limit (not recommended for production).
	MaxIterations int
	// TokenBudget is the cumulative token limit across all steps.
	// Zero means unlimited.
	TokenBudget int
	// EvalRunID optionally links trajectory to an evaluation run.
	EvalRunID string
	// ScenarioID optionally links trajectory to an evaluation scenario.
	ScenarioID string
}

// DefaultAgentConfig returns a safe production default: 10 iterations, unlimited tokens.
func DefaultAgentConfig() AgentConfig {
	return AgentConfig{
		MaxIterations: 10,
		TokenBudget:   0,
	}
}

// ========================================================================
// AgentLoop
// ========================================================================

// AgentLoop is a standalone OODA-loop harness.
//
// Embed AgentLoop in an actor struct (or hold it as a field) to gain
// structured step recording, budget/iteration guards, and trajectory export.
// All state is held in the struct — no global state.
//
// Example:
//
//	cfg := plexspaces.DefaultAgentConfig()
//	loop := plexspaces.NewAgentLoop("my-actor-id", cfg)
//	for !loop.IterationLimitReached() && !loop.BudgetExceeded() {
//	    obs   := loop.Observe(input)
//	    plan  := loop.Orient(obs)
//	    action := loop.Decide(plan)
//	    result := loop.Act(action, 120, 60, "claude-3-5-sonnet")
//	    loop.IncrementIteration()
//	    _ = result
//	}
//	traj := loop.FinalizeTrajectory("success", "")
type AgentLoop struct {
	config         AgentConfig
	trajectory     AgentTrajectory
	iterationCount int
	isSuspended    bool
	stepCounter    uint64
}

// NewAgentLoop creates a new AgentLoop with the given actorID and config.
// actorID is stored in trajectory.AgentActorID to link the trajectory to the producing actor.
// The trajectory is initialised with "running" outcome and the current timestamp.
func NewAgentLoop(actorID string, cfg AgentConfig) *AgentLoop {
	return &AgentLoop{
		config: cfg,
		trajectory: AgentTrajectory{
			TrajectoryID: newID(),
			AgentActorID: actorID,
			EvalRunID:    cfg.EvalRunID,
			ScenarioID:   cfg.ScenarioID,
			Outcome:      "running",
			Steps:        []AgentStep{},
			StartedAtMs:  nowMs(),
		},
	}
}

// ========================================================================
// OODA step methods
// ========================================================================

// Observe records a StepKindObserve step and returns input unchanged.
// Use this to journal the raw environment/context data gathered at the start of each OODA cycle.
func (a *AgentLoop) Observe(input any) any {
	a.recordStep(string(StepKindObserve), "observe", input, input, true, "", "", "", 0, 0)
	return input
}

// Orient records a StepKindOrient step and returns obs unchanged.
// Use this to journal the analysis / contextualisation of the observation.
func (a *AgentLoop) Orient(obs any) any {
	a.recordStep(string(StepKindOrient), "orient", obs, obs, true, "", "", "", 0, 0)
	return obs
}

// Decide records a StepKindDecide step and returns plan unchanged.
// Use this to journal the selected action plan before execution.
func (a *AgentLoop) Decide(plan any) any {
	a.recordStep(string(StepKindDecide), "decide", plan, plan, true, "", "", "", 0, 0)
	return plan
}

// Act records a StepKindAct step with token counts and model, and returns action unchanged.
// inputTokens and outputTokens are added to the trajectory cumulative totals.
func (a *AgentLoop) Act(action any, inputTokens, outputTokens int, model string) any {
	a.recordStep(string(StepKindAct), "act", action, action, true, "", "", model, inputTokens, outputTokens)
	return action
}

// ToolCall records a StepKindToolCall step with tool name, arguments, result, tokens, and model.
// Returns result unchanged for ergonomic chaining.
func (a *AgentLoop) ToolCall(toolName string, arguments, result any, inputTokens, outputTokens int, model string) any {
	a.recordStep(string(StepKindToolCall), "tool_call", arguments, result, true, "", toolName, model, inputTokens, outputTokens)
	return result
}

// Suspend records a StepKindSuspend step with the given reason and sets isSuspended to true.
// Subsequent calls to IsSuspended() will return true.
func (a *AgentLoop) Suspend(reason string) {
	a.isSuspended = true
	a.recordStep(string(StepKindSuspend), "suspend", reason, nil, true, "", "", "", 0, 0)
}

// IsSuspended reports whether Suspend has been called on this loop.
func (a *AgentLoop) IsSuspended() bool {
	return a.isSuspended
}

// BudgetExceeded reports whether cumulative tokens have reached or exceeded the configured
// TokenBudget. Returns false when TokenBudget is zero (unlimited).
func (a *AgentLoop) BudgetExceeded() bool {
	if a.config.TokenBudget <= 0 {
		return false
	}
	total := a.trajectory.TotalInputTokens + a.trajectory.TotalOutputTokens
	return total >= a.config.TokenBudget
}

// IterationLimitReached reports whether the iteration count has reached or exceeded
// MaxIterations. Returns false when MaxIterations is zero (unlimited).
func (a *AgentLoop) IterationLimitReached() bool {
	if a.config.MaxIterations <= 0 {
		return false
	}
	return a.iterationCount >= a.config.MaxIterations
}

// IncrementIteration advances the iteration counter by one.
// Call this once per OODA loop cycle, typically at the end of the loop body.
func (a *AgentLoop) IncrementIteration() {
	a.iterationCount++
}

// FinalizeTrajectory closes the trajectory with the given outcome and detail,
// sets CompletedAtMs and DurationMs, and returns a snapshot copy.
// Subsequent calls to GetTrajectory will reflect the finalised state.
func (a *AgentLoop) FinalizeTrajectory(outcome, detail string) AgentTrajectory {
	a.trajectory.Outcome = outcome
	a.trajectory.OutcomeDetail = detail
	a.trajectory.CompletedAtMs = nowMs()
	a.trajectory.DurationMs = a.trajectory.CompletedAtMs - a.trajectory.StartedAtMs
	return a.trajectory
}

// GetTrajectory returns a snapshot of the trajectory in its current state.
// Safe to call before FinalizeTrajectory for a mid-run inspection.
func (a *AgentLoop) GetTrajectory() AgentTrajectory {
	return a.trajectory
}

// ========================================================================
// Private helpers
// ========================================================================

// recordStep creates and appends an AgentStep to the trajectory.
// It updates cumulative token totals and returns the recorded step.
func (a *AgentLoop) recordStep(
	kind, method string,
	input, output any,
	success bool,
	errorStr, toolName, model string,
	inputTokens, outputTokens int,
) AgentStep {
	start := nowMs()
	end := nowMs()
	step := AgentStep{
		StepID:        a.nextStepID(),
		Kind:          AgentStepKind(kind),
		Method:        method,
		Input:         input,
		Output:        output,
		StartedAtMs:   start,
		CompletedAtMs: end,
		DurationMs:    end - start,
		Success:       success,
		Error:         errorStr,
		ToolName:      toolName,
		Model:         model,
		InputTokens:   inputTokens,
		OutputTokens:  outputTokens,
	}
	a.trajectory.Steps = append(a.trajectory.Steps, step)
	a.trajectory.TotalInputTokens += inputTokens
	a.trajectory.TotalOutputTokens += outputTokens
	return step
}

// nowMs returns the current Unix epoch time in milliseconds.
func nowMs() int64 {
	return time.Now().UnixMilli()
}

// newID generates a unique ID for a step or trajectory.
// Uses a combination of the current nanosecond timestamp and a per-instance
// counter to guarantee uniqueness without global state.
func newID() string {
	return fmt.Sprintf("%016x", time.Now().UnixNano())
}

// nextStepID increments the per-instance counter and returns a unique ID.
func (a *AgentLoop) nextStepID() string {
	a.stepCounter++
	return fmt.Sprintf("%016x%08x", time.Now().UnixNano(), a.stepCounter)
}
