// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Unit tests for AgentLoop — OODA-loop agent harness.

package plexspaces

import (
	"testing"
)

// TestAgentLoopObserveOrientDecideAct verifies that running all four OODA steps
// produces exactly four steps in the trajectory, one per phase.
func TestAgentLoopObserveOrientDecideAct(t *testing.T) {
	cfg := DefaultAgentConfig()
	loop := NewAgentLoop("test-actor", cfg)

	loop.Observe("raw environment data")
	loop.Orient("contextualised observation")
	loop.Decide("selected action plan")
	loop.Act("execute plan", 50, 30, "test-model")

	traj := loop.GetTrajectory()
	if len(traj.Steps) != 4 {
		t.Fatalf("expected 4 steps, got %d", len(traj.Steps))
	}

	expectedKinds := []AgentStepKind{
		StepKindObserve,
		StepKindOrient,
		StepKindDecide,
		StepKindAct,
	}
	for i, step := range traj.Steps {
		if step.Kind != expectedKinds[i] {
			t.Errorf("step[%d]: expected kind %q, got %q", i, expectedKinds[i], step.Kind)
		}
		if !step.Success {
			t.Errorf("step[%d]: expected success=true", i)
		}
		if step.StepID == "" {
			t.Errorf("step[%d]: StepID must not be empty", i)
		}
	}

	// ACT step should carry token counts
	actStep := traj.Steps[3]
	if actStep.InputTokens != 50 {
		t.Errorf("act step: expected InputTokens=50, got %d", actStep.InputTokens)
	}
	if actStep.OutputTokens != 30 {
		t.Errorf("act step: expected OutputTokens=30, got %d", actStep.OutputTokens)
	}
	if actStep.Model != "test-model" {
		t.Errorf("act step: expected Model=%q, got %q", "test-model", actStep.Model)
	}
}

// TestAgentLoopBudgetExceeded verifies that BudgetExceeded returns false until cumulative
// tokens reach the configured budget, and true at or above that threshold.
func TestAgentLoopBudgetExceeded(t *testing.T) {
	cfg := DefaultAgentConfig()
	cfg.TokenBudget = 100
	loop := NewAgentLoop("test-actor", cfg)

	if loop.BudgetExceeded() {
		t.Fatal("budget should not be exceeded before any steps")
	}

	// 60 tokens — still under budget
	loop.Act("first action", 40, 20, "model-a")
	if loop.BudgetExceeded() {
		t.Fatal("budget should not be exceeded at 60 tokens (budget=100)")
	}

	// 41 more tokens — total 101, exceeds budget
	loop.Act("second action", 30, 11, "model-a")
	if !loop.BudgetExceeded() {
		t.Fatal("budget should be exceeded at 101 tokens (budget=100)")
	}

	traj := loop.GetTrajectory()
	if traj.TotalInputTokens != 70 {
		t.Errorf("expected TotalInputTokens=70, got %d", traj.TotalInputTokens)
	}
	if traj.TotalOutputTokens != 31 {
		t.Errorf("expected TotalOutputTokens=31, got %d", traj.TotalOutputTokens)
	}
}

// TestAgentLoopIterationLimit verifies that IterationLimitReached returns false until
// IncrementIteration has been called MaxIterations times.
func TestAgentLoopIterationLimit(t *testing.T) {
	cfg := DefaultAgentConfig()
	cfg.MaxIterations = 2
	loop := NewAgentLoop("test-actor", cfg)

	if loop.IterationLimitReached() {
		t.Fatal("iteration limit should not be reached at iteration 0")
	}

	loop.IncrementIteration() // iteration count = 1
	if loop.IterationLimitReached() {
		t.Fatal("iteration limit should not be reached at iteration 1 (max=2)")
	}

	loop.IncrementIteration() // iteration count = 2
	if !loop.IterationLimitReached() {
		t.Fatal("iteration limit should be reached at iteration 2 (max=2)")
	}

	loop.IncrementIteration() // iteration count = 3 — still exceeded
	if !loop.IterationLimitReached() {
		t.Fatal("iteration limit should remain exceeded at iteration 3 (max=2)")
	}
}

// TestAgentLoopSuspend verifies that Suspend records a StepKindSuspend step,
// stores the reason as the step input, and sets IsSuspended to true.
func TestAgentLoopSuspend(t *testing.T) {
	cfg := DefaultAgentConfig()
	loop := NewAgentLoop("test-actor", cfg)

	if loop.IsSuspended() {
		t.Fatal("loop should not be suspended before Suspend is called")
	}

	const reason = "awaiting human approval"
	loop.Suspend(reason)

	if !loop.IsSuspended() {
		t.Fatal("IsSuspended should return true after Suspend")
	}

	traj := loop.GetTrajectory()
	if len(traj.Steps) != 1 {
		t.Fatalf("expected 1 step after Suspend, got %d", len(traj.Steps))
	}

	step := traj.Steps[0]
	if step.Kind != StepKindSuspend {
		t.Errorf("expected step kind %q, got %q", StepKindSuspend, step.Kind)
	}
	if step.Input != reason {
		t.Errorf("expected step input %q, got %v", reason, step.Input)
	}
	if !step.Success {
		t.Errorf("suspend step should have Success=true")
	}
}

// TestAgentLoopFinalizeTrajectory runs a complete OODA loop, finalises the trajectory,
// and verifies outcome, total tokens, step count, and timing fields.
func TestAgentLoopFinalizeTrajectory(t *testing.T) {
	cfg := DefaultAgentConfig()
	cfg.MaxIterations = 3
	loop := NewAgentLoop("test-actor", cfg)

	for !loop.IterationLimitReached() {
		loop.Observe("input data")
		loop.Orient("analysis")
		loop.Decide("plan")
		loop.Act("execute", 100, 50, "model-b")
		loop.IncrementIteration()
	}

	const wantOutcome = "success"
	const wantDetail = "completed 3 iterations"
	traj := loop.FinalizeTrajectory(wantOutcome, wantDetail)

	if traj.Outcome != wantOutcome {
		t.Errorf("expected outcome %q, got %q", wantOutcome, traj.Outcome)
	}
	if traj.OutcomeDetail != wantDetail {
		t.Errorf("expected outcome_detail %q, got %q", wantDetail, traj.OutcomeDetail)
	}

	// 3 iterations × 4 OODA steps = 12 steps
	wantSteps := 12
	if len(traj.Steps) != wantSteps {
		t.Errorf("expected %d steps, got %d", wantSteps, len(traj.Steps))
	}

	// 3 Act steps × 100 input tokens = 300 total input tokens
	wantInputTokens := 300
	if traj.TotalInputTokens != wantInputTokens {
		t.Errorf("expected TotalInputTokens=%d, got %d", wantInputTokens, traj.TotalInputTokens)
	}

	// 3 Act steps × 50 output tokens = 150 total output tokens
	wantOutputTokens := 150
	if traj.TotalOutputTokens != wantOutputTokens {
		t.Errorf("expected TotalOutputTokens=%d, got %d", wantOutputTokens, traj.TotalOutputTokens)
	}

	if traj.TrajectoryID == "" {
		t.Error("TrajectoryID must not be empty")
	}
	if traj.CompletedAtMs == 0 {
		t.Error("CompletedAtMs must be set after FinalizeTrajectory")
	}
	if traj.StartedAtMs == 0 {
		t.Error("StartedAtMs must be set")
	}
	if traj.DurationMs < 0 {
		t.Errorf("DurationMs must be non-negative, got %d", traj.DurationMs)
	}
}

// TestAgentLoopToolCall verifies that ToolCall records a StepKindToolCall step,
// sets the method and tool name, and updates the cumulative token totals.
func TestAgentLoopToolCall(t *testing.T) {
	cfg := DefaultAgentConfig()
	loop := NewAgentLoop("test-actor", cfg)

	args := map[string]string{"expr": "2+2"}
	result := "4"
	got := loop.ToolCall("calc", args, result, 10, 5, "llm")

	if got != result {
		t.Errorf("ToolCall should return result unchanged, got %v", got)
	}

	traj := loop.GetTrajectory()
	if len(traj.Steps) != 1 {
		t.Fatalf("expected 1 step after ToolCall, got %d", len(traj.Steps))
	}

	step := traj.Steps[0]
	if step.Kind != StepKindToolCall {
		t.Errorf("expected step kind %q, got %q", StepKindToolCall, step.Kind)
	}
	if step.Method != "tool_call" {
		t.Errorf("expected step method %q, got %q", "tool_call", step.Method)
	}
	if step.ToolName != "calc" {
		t.Errorf("expected ToolName %q, got %q", "calc", step.ToolName)
	}
	if step.InputTokens != 10 {
		t.Errorf("expected InputTokens=10, got %d", step.InputTokens)
	}
	if step.OutputTokens != 5 {
		t.Errorf("expected OutputTokens=5, got %d", step.OutputTokens)
	}
	if step.Model != "llm" {
		t.Errorf("expected Model=%q, got %q", "llm", step.Model)
	}
	if traj.TotalInputTokens != 10 {
		t.Errorf("expected TotalInputTokens=10, got %d", traj.TotalInputTokens)
	}
	if traj.TotalOutputTokens != 5 {
		t.Errorf("expected TotalOutputTokens=5, got %d", traj.TotalOutputTokens)
	}
}

// TestAgentLoopUnlimitedIterations verifies that MaxIterations=0 means unlimited —
// IterationLimitReached must always return false regardless of how many times
// IncrementIteration is called.
func TestAgentLoopUnlimitedIterations(t *testing.T) {
	cfg := DefaultAgentConfig()
	cfg.MaxIterations = 0
	loop := NewAgentLoop("test-actor", cfg)

	for i := 0; i < 1000; i++ {
		loop.IncrementIteration()
		if loop.IterationLimitReached() {
			t.Fatalf("IterationLimitReached returned true at iteration %d with MaxIterations=0", i+1)
		}
	}
}

// TestAgentLoopActorIDInTrajectory verifies that NewAgentLoop stores the provided
// actorID in trajectory.AgentActorID.
func TestAgentLoopActorIDInTrajectory(t *testing.T) {
	cfg := DefaultAgentConfig()
	const wantID = "my-actor"
	loop := NewAgentLoop(wantID, cfg)

	traj := loop.GetTrajectory()
	if traj.AgentActorID != wantID {
		t.Errorf("expected AgentActorID=%q, got %q", wantID, traj.AgentActorID)
	}
}
