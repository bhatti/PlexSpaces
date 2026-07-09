// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// ScorerActor — trajectory scoring for eval pipelines.
//
// Demonstrates: LLM-as-judge pattern, heuristic scoring, rubric evaluation.

use plexspaces_sdk::{
    gen_server_actor, plexspaces_handlers, handler,
    ActorContext, BehaviorError, Message, Value, json,
};
use tracing::info;

/// Scores agent trajectories against rubrics.
///
/// Supports four scoring modes:
/// - task_completion: outcome + keyword matching
/// - tool_use: correct tool invocation
/// - efficiency: token efficiency
/// - llm_judge: LLM-as-judge (falls back to heuristic)
#[gen_server_actor(name = "scorer")]
pub struct ScorerActor {
    actor_id: String,
    total_scored: u64,
}

impl ScorerActor {
    pub fn new() -> Self {
        Self {
            actor_id: String::new(),
            total_scored: 0,
        }
    }
}

#[plexspaces_handlers]
impl ScorerActor {
    /// Score a trajectory against a rubric. Returns score 0.0–1.0.
    #[handler("score")]
    async fn score(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<Value, BehaviorError> {
        let payload: Value = serde_json::from_slice(&msg.payload).unwrap_or(Value::Null);
        let trajectory = match payload.get("trajectory") {
            Some(t) => t.clone(),
            None => return Ok(json!({"error": "trajectory is required", "score": 0.0})),
        };

        // rubric can be a string shorthand or a dict
        let rubric = payload.get("rubric").cloned().unwrap_or(json!({"type": "task_completion"}));
        let rubric_obj = if rubric.is_string() {
            json!({"type": rubric})
        } else {
            rubric
        };

        let rubric_type = rubric_obj
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("task_completion");

        let (score, detail) = match rubric_type {
            "tool_use" => self.score_tool_use(&trajectory, &rubric_obj),
            "efficiency" => self.score_efficiency(&trajectory, &rubric_obj),
            "llm_judge" => self.score_task_completion(&trajectory, &rubric_obj), // fallback
            _ => self.score_task_completion(&trajectory, &rubric_obj),
        };

        self.total_scored += 1;
        let traj_id = trajectory
            .get("trajectory_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        info!("Scorer: scored trajectory_id={} score={:.3} rubric={}", traj_id, score, rubric_type);

        Ok(json!({
            "status": "ok",
            "trajectory_id": traj_id,
            "score": (score * 1000.0).round() / 1000.0,
            "rubric_type": rubric_type,
            "detail": detail,
        }))
    }

    /// Score multiple trajectories against the same rubric.
    #[handler("batch_score")]
    async fn batch_score(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<Value, BehaviorError> {
        let payload: Value = serde_json::from_slice(&msg.payload).unwrap_or(Value::Null);
        let trajectories = match payload.get("trajectories").and_then(|v| v.as_array()) {
            Some(t) => t.clone(),
            None => return Ok(json!({"error": "trajectories is required", "scores": []})),
        };

        let rubric = payload
            .get("rubric")
            .cloned()
            .unwrap_or(json!({"type": "task_completion"}));
        let rubric_obj = if rubric.is_string() {
            json!({"type": rubric})
        } else {
            rubric
        };
        let rubric_type = rubric_obj
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("task_completion");

        let mut results = Vec::new();
        let mut score_sum = 0.0f64;

        for traj in &trajectories {
            let (score, detail) = match rubric_type {
                "tool_use" => self.score_tool_use(traj, &rubric_obj),
                "efficiency" => self.score_efficiency(traj, &rubric_obj),
                _ => self.score_task_completion(traj, &rubric_obj),
            };
            let traj_id = traj
                .get("trajectory_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            score_sum += score;
            results.push(json!({
                "trajectory_id": traj_id,
                "score": (score * 1000.0).round() / 1000.0,
                "detail": detail,
            }));
            self.total_scored += 1;
        }

        let count = trajectories.len().max(1) as f64;
        let mean = score_sum / count;
        let pass_rate = results
            .iter()
            .filter(|r| r.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0) >= 0.8)
            .count() as f64
            / count;

        Ok(json!({
            "status": "ok",
            "scores": results,
            "mean_score": (mean * 1000.0).round() / 1000.0,
            "pass_rate": (pass_rate * 1000.0).round() / 1000.0,
        }))
    }

    /// Return statistics.
    #[handler("get_stats")]
    async fn get_stats(
        &mut self,
        _ctx: &ActorContext,
        _msg: &Message,
    ) -> Result<Value, BehaviorError> {
        Ok(json!({"status": "ok", "total_scored": self.total_scored}))
    }
}

impl ScorerActor {
    fn score_task_completion(&self, traj: &Value, rubric: &Value) -> (f64, String) {
        let outcome = traj.get("outcome").and_then(|v| v.as_str()).unwrap_or("");
        let steps = traj.get("steps").and_then(|v| v.as_array()).cloned().unwrap_or_default();
        let expected_keywords: Vec<String> = rubric
            .get("expected_keywords")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
            .unwrap_or_default();

        let base_score = match outcome {
            "success" | "completed" => 0.7,
            "budget_exceeded" => 0.3,
            "suspended" => 0.5,
            _ => 0.1,
        };

        let max_steps = rubric
            .get("max_steps")
            .and_then(|v| v.as_u64())
            .unwrap_or(20) as usize;
        let step_count = steps.len();
        let mut score = base_score;
        if step_count <= max_steps / 2 {
            score = (score + 0.15_f64).min(1.0_f64);
        }

        let all_outputs = serde_json::to_string(&steps).unwrap_or_default().to_lowercase();
        let keyword_matches = expected_keywords
            .iter()
            .filter(|kw| all_outputs.contains(kw.to_lowercase().as_str()))
            .count();
        if !expected_keywords.is_empty() {
            let bonus = 0.15_f64 * (keyword_matches as f64 / expected_keywords.len() as f64);
            score = (score + bonus).min(1.0_f64);
        }

        let detail = format!(
            "outcome={} steps={} keywords_matched={}/{}",
            outcome,
            step_count,
            keyword_matches,
            expected_keywords.len()
        );
        (score, detail)
    }

    fn score_tool_use(&self, traj: &Value, rubric: &Value) -> (f64, String) {
        let steps = traj.get("steps").and_then(|v| v.as_array()).cloned().unwrap_or_default();
        let tool_calls: Vec<&Value> = steps
            .iter()
            .filter(|s| s.get("kind").and_then(|v| v.as_str()) == Some("tool_call"))
            .collect();

        let expected_tools: Vec<String> = rubric
            .get("expected_tools")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
            .unwrap_or_default();

        let used_tools: std::collections::HashSet<String> = tool_calls
            .iter()
            .filter_map(|s| {
                s.get("tool_name")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            })
            .collect();

        let score = if expected_tools.is_empty() {
            if tool_calls.is_empty() { 0.4 } else { 0.8 }
        } else {
            let matches = expected_tools
                .iter()
                .filter(|t| used_tools.contains(t.as_str()))
                .count();
            matches as f64 / expected_tools.len() as f64
        };

        let detail = format!(
            "tool_calls={} used_tools={:?} expected={:?}",
            tool_calls.len(),
            used_tools,
            expected_tools
        );
        (score, detail)
    }

    fn score_efficiency(&self, traj: &Value, rubric: &Value) -> (f64, String) {
        let input_tokens = traj
            .get("total_input_tokens")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let output_tokens = traj
            .get("total_output_tokens")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let total = input_tokens + output_tokens;
        let budget = rubric.get("token_budget").and_then(|v| v.as_i64()).unwrap_or(4096);

        if total == 0 {
            return (0.5, "no token data".to_string());
        }

        let outcome = traj.get("outcome").and_then(|v| v.as_str()).unwrap_or("");
        let mut efficiency = (1.0 - (total as f64 / budget as f64)).max(0.0);
        if outcome != "success" && outcome != "completed" {
            efficiency *= 0.5;
        }

        let detail = format!("tokens={} budget={} outcome={}", total, budget, outcome);
        ((efficiency * 1000.0).round() / 1000.0, detail)
    }
}
