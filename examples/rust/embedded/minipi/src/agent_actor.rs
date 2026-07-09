// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// AgentActor — OODA-loop agent with AgentLoop harness.
//
// Demonstrates: OODA loop (Observe→Orient→Decide→Act), AgentLoop,
// token budget enforcement, trajectory export, human-in-the-loop suspend.

use plexspaces_sdk::{
    gen_server_actor, plexspaces_handlers,
    ActorContext, BehaviorError, Message, Value, json,
    AgentLoop, AgentConfig,
};
use tracing::{info, warn};

const MAX_ITER: usize = 10;
const TOKEN_BUDGET: i32 = 4096;

/// OODA-loop agent: Observe → Orient → Decide → Act.
///
/// Uses AgentLoop for step recording, token budget enforcement,
/// iteration limits, and trajectory finalization.
/// DurabilityFacet journals every step for crash recovery.
/// ExecutionTraceFacet captures ordered steps for eval.
#[gen_server_actor(name = "agent_runner")]
pub struct AgentActor {
    actor_id: String,
    task: String,
    iterations_done: usize,
    total_tool_calls: usize,
    eval_run_id: String,
    scenario_id: String,
    last_trajectory: Value,
    // Reference to the LLMGatewayActor for completions (set externally)
    llm_completions: Vec<Value>,
    // Tool results store (populated externally from ToolRegistryActor)
    tool_results: Vec<Value>,
}

impl AgentActor {
    pub fn new(actor_id: &str, eval_run_id: &str, scenario_id: &str) -> Self {
        Self {
            actor_id: actor_id.to_string(),
            task: String::new(),
            iterations_done: 0,
            total_tool_calls: 0,
            eval_run_id: eval_run_id.to_string(),
            scenario_id: scenario_id.to_string(),
            last_trajectory: Value::Null,
            llm_completions: Vec::new(),
            tool_results: Vec::new(),
        }
    }

    /// Register a mock LLM completion response (used when LLMGateway is unavailable).
    pub fn push_llm_completion(&mut self, resp: Value) {
        self.llm_completions.push(resp);
    }
}

#[plexspaces_handlers]
impl AgentActor {
    /// Main OODA loop — run a task and return trajectory.
    #[handler("run")]
    async fn run(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<Value, BehaviorError> {
        let payload: Value = serde_json::from_slice(&msg.payload).unwrap_or(Value::Null);
        let task = payload.get("task").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if task.is_empty() {
            return Ok(json!({"error": "task is required"}));
        }

        if let Some(eval_run) = payload.get("eval_run_id").and_then(|v| v.as_str()) {
            self.eval_run_id = eval_run.to_string();
        }
        if let Some(sc_id) = payload.get("scenario_id").and_then(|v| v.as_str()) {
            self.scenario_id = sc_id.to_string();
        }

        self.task = task.clone();
        let actor_id = if self.actor_id.is_empty() {
            ulid::Ulid::new().to_string()
        } else {
            self.actor_id.clone()
        };

        info!("AgentActor starting task: {}", &task[..80.min(task.len())]);

        let config = AgentConfig {
            max_iterations: MAX_ITER,
            token_budget: TOKEN_BUDGET,
            eval_run_id: self.eval_run_id.clone(),
            scenario_id: self.scenario_id.clone(),
        };

        let mut loop_ = AgentLoop::new(&actor_id, config);
        let mut completed_iterations = 0usize;

        // OODA loop — runs until done, budget exceeded, or suspended
        while !loop_.iteration_limit_reached() {
            if loop_.budget_exceeded() {
                warn!("AgentActor budget exceeded after {} iterations", completed_iterations);
                let traj = loop_.finalize_trajectory(
                    "budget_exceeded",
                    &format!("Token budget {} exceeded", TOKEN_BUDGET),
                );
                let traj_json = serde_json::to_value(&traj).unwrap_or(Value::Null);
                self.last_trajectory = traj_json.clone();
                return Ok(json!({
                    "status": "budget_exceeded",
                    "trajectory": traj_json,
                    "trajectory_id": traj.trajectory_id,
                }));
            }

            if loop_.is_suspended() {
                let traj = loop_.get_trajectory();
                let traj_json = serde_json::to_value(&traj).unwrap_or(Value::Null);
                self.last_trajectory = traj_json.clone();
                return Ok(json!({
                    "status": "suspended",
                    "trajectory": traj_json,
                    "trajectory_id": traj.trajectory_id,
                }));
            }

            // OBSERVE: gather context, memory, environment
            let observations = do_observe(&mut loop_, &task, completed_iterations);

            // ORIENT: analyze observations (simulated LLM call)
            let plan = do_orient(&mut loop_, &observations, &task);

            // DECIDE: select next action
            let action = do_decide(&mut loop_, &plan);

            if action.get("done").and_then(|v| v.as_bool()).unwrap_or(false) {
                break;
            }

            // Check for approval-required actions (human-in-the-loop)
            if action.get("needs_approval").and_then(|v| v.as_bool()).unwrap_or(false) {
                let tool_name = action
                    .get("tool_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                loop_.suspend(&format!("action_needs_approval:{}", tool_name));
                let traj = loop_.get_trajectory();
                let traj_json = serde_json::to_value(&traj).unwrap_or(Value::Null);
                self.last_trajectory = traj_json.clone();
                return Ok(json!({
                    "status": "suspended",
                    "trajectory": traj_json,
                    "trajectory_id": traj.trajectory_id,
                }));
            }

            // ACT: execute the chosen tool
            do_act(&mut loop_, &action);
            self.total_tool_calls += 1;
            completed_iterations += 1;

            loop_.increment_iteration();
        }

        self.iterations_done = completed_iterations;

        let traj = loop_.finalize_trajectory(
            "completed",
            &format!("Completed {} iterations", completed_iterations),
        );
        let traj_json = serde_json::to_value(&traj).unwrap_or(Value::Null);
        self.last_trajectory = traj_json.clone();

        info!(
            "AgentActor completed task iterations={} steps={}",
            completed_iterations,
            traj.steps.len()
        );

        Ok(json!({
            "status": "success",
            "task": task,
            "iterations": completed_iterations,
            "step_count": traj.steps.len(),
            "outcome": traj.outcome,
            "trajectory_id": traj.trajectory_id,
            "trajectory": traj_json,
        }))
    }

    /// Query execution trace (most recent trajectory).
    #[handler("execution_trace")]
    async fn execution_trace(
        &mut self,
        _ctx: &ActorContext,
        _msg: &Message,
    ) -> Result<Value, BehaviorError> {
        if self.last_trajectory.is_null() {
            return Ok(json!({"actor_id": self.actor_id, "steps": [], "outcome": "running"}));
        }
        Ok(self.last_trajectory.clone())
    }

    /// Query actor status.
    #[handler("status")]
    async fn status(
        &mut self,
        _ctx: &ActorContext,
        _msg: &Message,
    ) -> Result<Value, BehaviorError> {
        Ok(json!({
            "actor_id": self.actor_id,
            "task": &self.task[..80.min(self.task.len())],
            "iterations_done": self.iterations_done,
            "total_tool_calls": self.total_tool_calls,
        }))
    }
}

// ---------------------------------------------------------------------------
// OODA phase helpers (free functions)

fn do_observe(loop_: &mut AgentLoop, task: &str, iteration: usize) -> Value {
    let observations = json!({
        "task": task,
        "prior_context": {},
        "iteration": iteration,
    });
    loop_.observe(observations)
}

fn do_orient(loop_: &mut AgentLoop, observations: &Value, task: &str) -> Value {
    // Simulated orientation: decide tool based on task content
    let task_lower = task.to_lowercase();
    let (next_tool, arguments) = if task_lower.contains("search") || task_lower.contains("find") {
        (
            "web_search",
            json!({"query": &task[..50.min(task.len())]}),
        )
    } else if task_lower.contains("calculat")
        || task.contains('*')
        || task.contains('+')
        || task.contains('-')
        || task.contains('/')
    {
        (
            "calculator",
            json!({"expression": task}),
        )
    } else {
        ("calculator", json!({"expression": "1+1"}))
    };

    let plan = json!({
        "analysis": format!("Processing task: {}", &task[..60.min(task.len())]),
        "next_tool": next_tool,
        "arguments": arguments,
        "input_tokens": 30,
        "output_tokens": 20,
        "model": "llama3.2",
        "done": false,
    });
    loop_.orient(plan)
}

fn do_decide(loop_: &mut AgentLoop, plan: &Value) -> Value {
    let action = json!({
        "tool_name": plan.get("next_tool").cloned().unwrap_or(json!("calculator")),
        "arguments": plan.get("arguments").cloned().unwrap_or(json!({})),
        "done": plan.get("done").cloned().unwrap_or(json!(false)),
        "needs_approval": plan.get("needs_approval").cloned().unwrap_or(json!(false)),
    });
    loop_.decide(action)
}

fn do_act(loop_: &mut AgentLoop, action: &Value) -> Value {
    let tool_name = action
        .get("tool_name")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let arguments = action.get("arguments").cloned().unwrap_or(json!({}));

    // Simulate tool execution result
    let result = json!({
        "status": "ok",
        "tool": tool_name,
        "result": format!("Executed {} successfully", tool_name),
    });

    loop_.tool_call(tool_name, arguments, result.clone(), 20, 10, "llama3.2")
}
