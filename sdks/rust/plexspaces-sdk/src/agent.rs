// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Runtime structs for OODA-loop agent actors.
//
// Provides step recording, trajectory capture, token budget enforcement,
// iteration limits, suspend/resume for human-in-the-loop patterns, and
// tool-call tracking.  All types are plain Rust — no proc macros required.
//
// ## Usage
// ```rust,ignore
// use plexspaces_sdk::{AgentLoop, AgentConfig};
// use serde_json::json;
//
// let config = AgentConfig {
//     max_iterations: 5,
//     token_budget: 1000,
//     ..Default::default()
// };
// let mut agent = AgentLoop::new("my-actor-id", config);
// let obs  = agent.observe(json!({"task": "summarise"}));
// let plan = agent.orient(obs);
// let act  = agent.decide(plan);
// let result = agent.act(act, 50, 80, "claude-3-5-sonnet");
// let traj = agent.finalize_trajectory("success", "done");
// ```

use std::collections::HashMap;
use std::time::SystemTime;

// ============================================================================
// Step-kind constants
// ============================================================================

/// Step kind: gather information from environment / memory / context.
pub const STEP_KIND_OBSERVE: &str = "observe";

/// Step kind: process observations into a plan.
pub const STEP_KIND_ORIENT: &str = "orient";

/// Step kind: select next action from available options.
pub const STEP_KIND_DECIDE: &str = "decide";

/// Step kind: execute the chosen action.
pub const STEP_KIND_ACT: &str = "act";

/// Step kind: validated tool invocation.
pub const STEP_KIND_TOOL_CALL: &str = "tool_call";

/// Step kind: agent suspended awaiting external signal.
pub const STEP_KIND_SUSPEND: &str = "suspend";

// ============================================================================
// AgentStep
// ============================================================================

/// A single step in an agent's OODA-loop trajectory.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AgentStep {
    /// Unique identifier for this step (ULID).
    pub step_id: String,
    /// One of the `STEP_KIND_*` constants.
    pub kind: String,
    /// Method or function name associated with this step.
    pub method: String,
    /// Input data passed to the step.
    pub input: serde_json::Value,
    /// Output data produced by the step.
    pub output: serde_json::Value,
    /// Wall-clock start time in milliseconds since Unix epoch.
    pub started_at_ms: i64,
    /// Wall-clock completion time in milliseconds since Unix epoch.
    pub completed_at_ms: i64,
    /// Duration in milliseconds (`completed_at_ms - started_at_ms`).
    pub duration_ms: i64,
    /// Whether the step completed successfully.
    pub success: bool,
    /// Error message if the step failed.
    pub error: Option<String>,
    /// Tool name for `tool_call` steps.
    pub tool_name: Option<String>,
    /// Number of input tokens consumed by this step.
    pub input_tokens: i32,
    /// Number of output tokens produced by this step.
    pub output_tokens: i32,
    /// Model identifier used for this step (empty if not an LLM step).
    pub model: String,
    /// Arbitrary key-value metadata.
    pub metadata: HashMap<String, String>,
}

// ============================================================================
// AgentTrajectory
// ============================================================================

/// Complete execution trajectory for one agent run.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AgentTrajectory {
    /// Unique identifier for this trajectory (ULID).
    pub trajectory_id: String,
    /// Actor ID of the agent that produced this trajectory.
    pub agent_actor_id: String,
    /// Eval-run identifier for batch evaluation pipelines.
    pub eval_run_id: String,
    /// Scenario identifier for eval pipelines.
    pub scenario_id: String,
    /// Ordered list of steps recorded during the run.
    pub steps: Vec<AgentStep>,
    /// Final outcome: `"success"`, `"failure"`, `"suspended"`, `"budget_exceeded"`, `"running"`, etc.
    pub outcome: String,
    /// Human-readable explanation of the outcome.
    pub outcome_detail: String,
    /// Cumulative input tokens across all steps.
    pub total_input_tokens: i32,
    /// Cumulative output tokens across all steps.
    pub total_output_tokens: i32,
    /// Wall-clock start time in milliseconds since Unix epoch.
    pub started_at_ms: i64,
    /// Wall-clock completion time in milliseconds since Unix epoch (`0` while running).
    pub completed_at_ms: i64,
    /// Total duration in milliseconds (`completed_at_ms - started_at_ms`).
    pub duration_ms: i64,
    /// Optional numeric score (e.g. from an evaluator); `0.0` by default.
    pub score: f64,
    /// Arbitrary key-value metadata.
    pub metadata: HashMap<String, String>,
}

// ============================================================================
// AgentConfig
// ============================================================================

/// Configuration for an [`AgentLoop`].
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AgentConfig {
    /// Maximum number of OODA iterations before the loop is forcibly stopped.
    pub max_iterations: usize,
    /// Cumulative token budget (input + output).  `0` means unlimited.
    pub token_budget: i32,
    /// Eval-run identifier propagated into the trajectory.
    pub eval_run_id: String,
    /// Scenario identifier propagated into the trajectory.
    pub scenario_id: String,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_iterations: 10,
            token_budget: 0,
            eval_run_id: String::new(),
            scenario_id: String::new(),
        }
    }
}

// ============================================================================
// AgentLoop
// ============================================================================

/// Runtime harness that drives an OODA-loop agent.
///
/// `AgentLoop` tracks every step, enforces token budgets and iteration limits,
/// supports suspend/resume for human-in-the-loop scenarios, and produces a
/// final [`AgentTrajectory`] via [`finalize_trajectory`](AgentLoop::finalize_trajectory).
pub struct AgentLoop {
    config: AgentConfig,
    trajectory: AgentTrajectory,
    iteration_count: usize,
    is_suspended: bool,
}

impl AgentLoop {
    // -------------------------------------------------------------------------
    // Construction
    // -------------------------------------------------------------------------

    /// Create a new `AgentLoop` with the given actor ID and configuration.
    ///
    /// `actor_id` is stored in [`AgentTrajectory::agent_actor_id`] to link
    /// the trajectory to the producing actor.
    pub fn new(actor_id: &str, config: AgentConfig) -> Self {
        let trajectory = AgentTrajectory {
            trajectory_id: Self::new_step_id(),
            agent_actor_id: actor_id.to_string(),
            eval_run_id: config.eval_run_id.clone(),
            scenario_id: config.scenario_id.clone(),
            steps: Vec::new(),
            outcome: "running".to_string(),
            outcome_detail: String::new(),
            total_input_tokens: 0,
            total_output_tokens: 0,
            started_at_ms: Self::now_ms(),
            completed_at_ms: 0,
            duration_ms: 0,
            score: 0.0,
            metadata: HashMap::new(),
        };
        Self {
            config,
            trajectory,
            iteration_count: 0,
            is_suspended: false,
        }
    }

    // -------------------------------------------------------------------------
    // OODA step helpers
    // -------------------------------------------------------------------------

    /// Record an OBSERVE step.  Returns the `output` field of the recorded step.
    pub fn observe(&mut self, input: serde_json::Value) -> serde_json::Value {
        self.record_step(STEP_KIND_OBSERVE, "observe", input.clone(), input, 0, 0, "")
    }

    /// Record an ORIENT step.  Returns the `output` field of the recorded step.
    pub fn orient(&mut self, obs: serde_json::Value) -> serde_json::Value {
        self.record_step(STEP_KIND_ORIENT, "orient", obs.clone(), obs, 0, 0, "")
    }

    /// Record a DECIDE step.  Returns the `output` field of the recorded step.
    pub fn decide(&mut self, plan: serde_json::Value) -> serde_json::Value {
        self.record_step(STEP_KIND_DECIDE, "decide", plan.clone(), plan, 0, 0, "")
    }

    /// Record an ACT step with optional token accounting.
    pub fn act(
        &mut self,
        action: serde_json::Value,
        input_tokens: i32,
        output_tokens: i32,
        model: &str,
    ) -> serde_json::Value {
        self.record_step(
            STEP_KIND_ACT,
            "act",
            action.clone(),
            action,
            input_tokens,
            output_tokens,
            model,
        )
    }

    /// Record a TOOL_CALL step.
    ///
    /// `arguments` is the JSON payload sent to the tool; `result` is its response.
    pub fn tool_call(
        &mut self,
        tool_name: &str,
        arguments: serde_json::Value,
        result: serde_json::Value,
        input_tokens: i32,
        output_tokens: i32,
        model: &str,
    ) -> serde_json::Value {
        let started_at_ms = Self::now_ms();
        let completed_at_ms = Self::now_ms();
        let step = AgentStep {
            step_id: Self::new_step_id(),
            kind: STEP_KIND_TOOL_CALL.to_string(),
            method: format!("tool:{}", tool_name),
            input: arguments,
            output: result.clone(),
            started_at_ms,
            completed_at_ms,
            duration_ms: completed_at_ms - started_at_ms,
            success: true,
            error: None,
            tool_name: Some(tool_name.to_string()),
            input_tokens,
            output_tokens,
            model: model.to_string(),
            metadata: HashMap::new(),
        };
        self.trajectory.total_input_tokens += input_tokens;
        self.trajectory.total_output_tokens += output_tokens;
        self.trajectory.steps.push(step);
        result
    }

    // -------------------------------------------------------------------------
    // Suspend / Resume
    // -------------------------------------------------------------------------

    /// Suspend the agent.  Sets the suspended flag and records a SUSPEND step.
    pub fn suspend(&mut self, reason: &str) {
        self.is_suspended = true;
        let started_at_ms = Self::now_ms();
        let step = AgentStep {
            step_id: Self::new_step_id(),
            kind: STEP_KIND_SUSPEND.to_string(),
            method: "suspend".to_string(),
            input: serde_json::Value::String(reason.to_string()),
            output: serde_json::Value::Null,
            started_at_ms,
            completed_at_ms: started_at_ms,
            duration_ms: 0,
            success: true,
            error: None,
            tool_name: None,
            input_tokens: 0,
            output_tokens: 0,
            model: String::new(),
            metadata: HashMap::new(),
        };
        self.trajectory.steps.push(step);
    }

    /// Whether the agent is currently suspended.
    pub fn is_suspended(&self) -> bool {
        self.is_suspended
    }

    // -------------------------------------------------------------------------
    // Budget / Iteration checks
    // -------------------------------------------------------------------------

    /// Returns `true` if the cumulative token usage meets or exceeds the budget.
    ///
    /// A `token_budget` of `0` in [`AgentConfig`] means unlimited (`false` always).
    pub fn budget_exceeded(&self) -> bool {
        if self.config.token_budget <= 0 {
            return false;
        }
        let used = self.trajectory.total_input_tokens + self.trajectory.total_output_tokens;
        used >= self.config.token_budget
    }

    /// Returns `true` if `iteration_count` has reached `max_iterations`.
    ///
    /// A `max_iterations` of `0` means unlimited — always returns `false`.
    pub fn iteration_limit_reached(&self) -> bool {
        if self.config.max_iterations == 0 {
            return false;
        }
        self.iteration_count >= self.config.max_iterations
    }

    /// Increment the iteration counter by one.
    pub fn increment_iteration(&mut self) {
        self.iteration_count += 1;
    }

    // -------------------------------------------------------------------------
    // Finalization
    // -------------------------------------------------------------------------

    /// Complete the trajectory, set the outcome, and return a clone.
    pub fn finalize_trajectory(&mut self, outcome: &str, detail: &str) -> AgentTrajectory {
        let now = Self::now_ms();
        self.trajectory.outcome = outcome.to_string();
        self.trajectory.outcome_detail = detail.to_string();
        self.trajectory.completed_at_ms = now;
        self.trajectory.duration_ms = now - self.trajectory.started_at_ms;
        self.trajectory.clone()
    }

    /// Return a snapshot of the current (possibly incomplete) trajectory.
    pub fn get_trajectory(&self) -> AgentTrajectory {
        self.trajectory.clone()
    }

    // -------------------------------------------------------------------------
    // Private helpers
    // -------------------------------------------------------------------------

    /// Record a generic step and return its output value.
    fn record_step(
        &mut self,
        kind: &str,
        method: &str,
        input: serde_json::Value,
        output: serde_json::Value,
        input_tokens: i32,
        output_tokens: i32,
        model: &str,
    ) -> serde_json::Value {
        let started_at_ms = Self::now_ms();
        let completed_at_ms = Self::now_ms();
        let step = AgentStep {
            step_id: Self::new_step_id(),
            kind: kind.to_string(),
            method: method.to_string(),
            input,
            output: output.clone(),
            started_at_ms,
            completed_at_ms,
            duration_ms: completed_at_ms - started_at_ms,
            success: true,
            error: None,
            tool_name: None,
            input_tokens,
            output_tokens,
            model: model.to_string(),
            metadata: HashMap::new(),
        };
        self.trajectory.total_input_tokens += input_tokens;
        self.trajectory.total_output_tokens += output_tokens;
        self.trajectory.steps.push(step);
        output
    }

    /// Current time in milliseconds since the Unix epoch.
    fn now_ms() -> i64 {
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    }

    /// Generate a new unique step/trajectory ID.
    ///
    /// Uses ULID when the `native` feature (which enables the `ulid` crate) is
    /// active; otherwise falls back to a millisecond timestamp string.
    fn new_step_id() -> String {
        #[cfg(feature = "native")]
        {
            ulid::Ulid::new().to_string()
        }
        #[cfg(not(feature = "native"))]
        {
            // Fallback: timestamp-based unique-ish ID
            format!(
                "step-{}",
                SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0)
            )
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn default_loop() -> AgentLoop {
        AgentLoop::new("test-actor", AgentConfig::default())
    }

    /// 1. Four-step OODA cycle — verify step kinds and count.
    #[test]
    fn test_observe_orient_decide_act() {
        let mut agent = default_loop();

        let obs = agent.observe(json!({"env": "production"}));
        let plan = agent.orient(obs);
        let action = agent.decide(plan);
        let _result = agent.act(action, 10, 20, "claude-3-5-sonnet");

        let traj = agent.get_trajectory();
        assert_eq!(traj.steps.len(), 4);
        assert_eq!(traj.steps[0].kind, STEP_KIND_OBSERVE);
        assert_eq!(traj.steps[1].kind, STEP_KIND_ORIENT);
        assert_eq!(traj.steps[2].kind, STEP_KIND_DECIDE);
        assert_eq!(traj.steps[3].kind, STEP_KIND_ACT);
        assert_eq!(traj.total_input_tokens, 10);
        assert_eq!(traj.total_output_tokens, 20);
    }

    /// 2. Budget enforcement — cumulative tokens >= budget ⇒ exceeded.
    #[test]
    fn test_budget_exceeded() {
        let config = AgentConfig {
            token_budget: 100,
            ..Default::default()
        };
        let mut agent = AgentLoop::new("test-actor", config);

        // Not exceeded yet.
        agent.act(json!({}), 40, 40, "");
        assert!(
            !agent.budget_exceeded(),
            "40+40=80 < 100, should not be exceeded"
        );

        // Push over the limit.
        agent.act(json!({}), 10, 10, "");
        assert!(
            agent.budget_exceeded(),
            "40+40+10+10=100 >= 100, should be exceeded"
        );
    }

    /// 3. Iteration limit — 2 max, increment 3 times.
    #[test]
    fn test_iteration_limit() {
        let config = AgentConfig {
            max_iterations: 2,
            ..Default::default()
        };
        let mut agent = AgentLoop::new("test-actor", config);

        agent.increment_iteration(); // 1
        assert!(!agent.iteration_limit_reached());
        agent.increment_iteration(); // 2
        assert!(agent.iteration_limit_reached(), "2 >= 2");
        agent.increment_iteration(); // 3
        assert!(agent.iteration_limit_reached(), "3 >= 2");
    }

    /// 4. Suspend — flag set, step recorded with SUSPEND kind.
    #[test]
    fn test_suspend() {
        let mut agent = default_loop();
        assert!(!agent.is_suspended());

        agent.suspend("awaiting_approval");
        assert!(agent.is_suspended());

        let traj = agent.get_trajectory();
        assert_eq!(traj.steps.len(), 1);
        assert_eq!(traj.steps[0].kind, STEP_KIND_SUSPEND);
        assert_eq!(
            traj.steps[0].input,
            serde_json::Value::String("awaiting_approval".to_string())
        );
    }

    /// 5. finalize_trajectory — outcome, totals, and non-empty trajectory_id.
    #[test]
    fn test_finalize_trajectory() {
        let mut agent = default_loop();
        agent.act(json!({"cmd": "run"}), 15, 25, "gpt-4");
        let traj = agent.finalize_trajectory("success", "all done");

        assert_eq!(traj.outcome, "success");
        assert_eq!(traj.outcome_detail, "all done");
        assert!(!traj.trajectory_id.is_empty());
        assert_eq!(traj.total_input_tokens, 15);
        assert_eq!(traj.total_output_tokens, 25);
        assert!(traj.completed_at_ms > 0);
        assert!(traj.duration_ms >= 0);
    }

    /// 6. tool_call — step kind and tool_name captured correctly.
    #[test]
    fn test_tool_call_step() {
        let mut agent = default_loop();
        let result = agent.tool_call(
            "web_search",
            json!({"query": "rust actors"}),
            json!({"hits": 42}),
            5,
            10,
            "claude-3-haiku",
        );

        assert_eq!(result, json!({"hits": 42}));
        let traj = agent.get_trajectory();
        assert_eq!(traj.steps.len(), 1);
        assert_eq!(traj.steps[0].kind, STEP_KIND_TOOL_CALL);
        assert_eq!(traj.steps[0].tool_name.as_deref(), Some("web_search"));
        assert_eq!(traj.steps[0].model, "claude-3-haiku");
        assert_eq!(traj.total_input_tokens, 5);
        assert_eq!(traj.total_output_tokens, 10);
    }

    /// 7. trajectory_id is non-empty (indirectly verifies step ID generation).
    #[test]
    fn test_trajectory_id_non_empty() {
        let agent = default_loop();
        let traj = agent.get_trajectory();
        assert!(!traj.trajectory_id.is_empty());
    }

    /// 8. actor_id is stored in trajectory.
    #[test]
    fn test_actor_id_in_trajectory() {
        let agent = AgentLoop::new("my-actor-007", AgentConfig::default());
        assert_eq!(agent.get_trajectory().agent_actor_id, "my-actor-007");
    }

    /// 9. max_iterations=0 means unlimited.
    #[test]
    fn test_unlimited_iterations_when_max_is_zero() {
        let config = AgentConfig {
            max_iterations: 0,
            ..Default::default()
        };
        let mut agent = AgentLoop::new("a", config);
        for _ in 0..1000 {
            agent.increment_iteration();
        }
        assert!(!agent.iteration_limit_reached());
    }
}
