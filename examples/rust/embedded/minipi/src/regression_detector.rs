// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// RegressionDetectorActor — compare trajectories across eval runs.
//
// Demonstrates: diff logic, baseline management, eval feedback loop.

use plexspaces_sdk::{
    gen_server_actor, plexspaces_handlers, handler,
    ActorContext, BehaviorError, Message, Value, json,
};
use std::collections::HashMap;
use tracing::{info, warn};

/// Detects regressions by comparing trajectory scores across eval runs.
///
/// The eval feedback loop:
/// 1. Run eval → score → detect regressions (this actor) → diagnose → fix → rerun
/// 5% regression threshold flags scenarios that got worse.
#[gen_server_actor(name = "regression_detector")]
pub struct RegressionDetectorActor {
    actor_id: String,
    /// baseline: trajectory_id -> { score, eval_run_id }
    baseline: HashMap<String, Value>,
    baseline_eval_run: String,
    total_comparisons: u64,
}

impl RegressionDetectorActor {
    pub fn new() -> Self {
        Self {
            actor_id: String::new(),
            baseline: HashMap::new(),
            baseline_eval_run: String::new(),
            total_comparisons: 0,
        }
    }
}

#[plexspaces_handlers]
impl RegressionDetectorActor {
    /// Compare current eval scores against baseline.
    #[handler("compare")]
    async fn compare(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<Value, BehaviorError> {
        let payload: Value = serde_json::from_slice(&msg.payload).unwrap_or(Value::Null);
        let eval_run_id = payload
            .get("eval_run_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if eval_run_id.is_empty() {
            return Ok(json!({"error": "eval_run_id is required"}));
        }

        let scores = match payload.get("scores").and_then(|v| v.as_array()) {
            Some(s) => s.clone(),
            None => {
                return Ok(json!({
                    "regressions": [],
                    "improvements": [],
                    "unchanged": [],
                }))
            }
        };

        if self.baseline.is_empty() {
            // No baseline — store current and return clean
            self.store_baseline(eval_run_id, &scores);
            return Ok(json!({
                "regressions": [],
                "improvements": [],
                "unchanged": [],
                "message": format!("Stored as baseline (eval_run_id={})", eval_run_id),
            }));
        }

        let mut regressions = Vec::new();
        let mut improvements = Vec::new();
        let mut unchanged = Vec::new();
        const THRESHOLD: f64 = 0.05;

        for current in &scores {
            let traj_id = current
                .get("trajectory_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let current_score = current.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0);

            let baseline_entry = self.baseline.get(&traj_id);
            let baseline_score = baseline_entry
                .and_then(|e| e.get("score"))
                .and_then(|v| v.as_f64());

            match baseline_score {
                None => unchanged.push(json!({
                    "trajectory_id": traj_id,
                    "current": current_score,
                    "baseline": null,
                })),
                Some(bs) => {
                    let delta = current_score - bs;
                    let entry = json!({
                        "trajectory_id": traj_id,
                        "current": current_score,
                        "baseline": bs,
                        "delta": (delta * 1000.0).round() / 1000.0,
                    });
                    if delta < -THRESHOLD {
                        let severity = if delta < -0.15 { "high" } else { "medium" };
                        let mut e = entry.clone();
                        e["severity"] = json!(severity);
                        regressions.push(e);
                    } else if delta > THRESHOLD {
                        improvements.push(entry);
                    } else {
                        unchanged.push(entry);
                    }
                }
            }
        }

        self.total_comparisons += 1;

        if !regressions.is_empty() {
            warn!(
                "Regressions detected: {} scenarios degraded in eval_run={}",
                regressions.len(),
                eval_run_id
            );
        }

        Ok(json!({
            "regressions": regressions,
            "improvements": improvements,
            "unchanged": unchanged,
            "regression_count": regressions.len(),
            "improvement_count": improvements.len(),
            "eval_run_id": eval_run_id,
        }))
    }

    /// Explicitly set a baseline from an eval run.
    #[handler("set_baseline")]
    async fn set_baseline(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<Value, BehaviorError> {
        let payload: Value = serde_json::from_slice(&msg.payload).unwrap_or(Value::Null);
        let eval_run_id = payload
            .get("eval_run_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let scores = match payload.get("scores").and_then(|v| v.as_array()) {
            Some(s) => s.clone(),
            None => return Ok(json!({"error": "scores is required"})),
        };

        let count = scores.len();
        self.store_baseline(&eval_run_id, &scores);
        info!("RegressionDetector: baseline set from eval_run={}", eval_run_id);

        Ok(json!({
            "status": "ok",
            "baseline_eval_run_id": eval_run_id,
            "scenarios": count,
        }))
    }

    /// Get the current baseline.
    #[handler("get_baseline")]
    async fn get_baseline(
        &mut self,
        _ctx: &ActorContext,
        _msg: &Message,
    ) -> Result<Value, BehaviorError> {
        let count = self.baseline.len();
        let baseline: HashMap<String, Value> = self.baseline.clone();
        Ok(json!({"status": "ok", "baseline": baseline, "count": count}))
    }

    /// Compare two trajectories step-by-step to diagnose score divergence.
    #[handler("replay_diff")]
    async fn replay_diff(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<Value, BehaviorError> {
        let payload: Value = serde_json::from_slice(&msg.payload).unwrap_or(Value::Null);
        let _traj_id_a = payload
            .get("traj_id_a")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let _traj_id_b = payload
            .get("traj_id_b")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        // In embedded mode, trajectories are stored in TrajectoryStoreActor (separate actor).
        // Return a placeholder — full diff requires actor-to-actor communication.
        Ok(json!({
            "status": "ok",
            "message": "Use TrajectoryStoreActor.get() to fetch trajectories, then compare steps client-side.",
        }))
    }

    /// Return statistics.
    #[handler("get_stats")]
    async fn get_stats(
        &mut self,
        _ctx: &ActorContext,
        _msg: &Message,
    ) -> Result<Value, BehaviorError> {
        Ok(json!({"status": "ok", "total_comparisons": self.total_comparisons}))
    }
}

impl RegressionDetectorActor {
    fn store_baseline(&mut self, eval_run_id: &str, scores: &[Value]) {
        self.baseline.clear();
        for s in scores {
            let traj_id = s
                .get("trajectory_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let score = s.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0);
            self.baseline.insert(
                traj_id,
                json!({"score": score, "eval_run_id": eval_run_id}),
            );
        }
        self.baseline_eval_run = eval_run_id.to_string();
    }
}
