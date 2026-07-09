// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// DashboardActor — aggregates eval metrics and exposes query handlers.
//
// Demonstrates: read-only aggregation pattern, query-only actor.

use plexspaces_sdk::{
    gen_server_actor, plexspaces_handlers, handler,
    ActorContext, BehaviorError, Message, Value, json,
};
use std::collections::HashMap;
use tracing::info;

/// Eval dashboard: aggregates results from all eval runs.
///
/// Query this actor for eval run summaries, pass rate trends,
/// token cost trends, and regression alerts.
#[gen_server_actor(name = "dashboard")]
pub struct DashboardActor {
    actor_id: String,
    /// Eval reports keyed by eval_run_id
    eval_reports: HashMap<String, Value>,
    /// Trajectory records keyed by trajectory_id
    trajectories: HashMap<String, Value>,
    /// Regression baseline metadata
    regression_baseline_run: String,
    regression_baseline: HashMap<String, Value>,
}

impl DashboardActor {
    pub fn new() -> Self {
        Self {
            actor_id: String::new(),
            eval_reports: HashMap::new(),
            trajectories: HashMap::new(),
            regression_baseline_run: String::new(),
            regression_baseline: HashMap::new(),
        }
    }

    /// Register an eval report for dashboard queries.
    pub fn register_eval_report(&mut self, eval_run_id: &str, report: Value) {
        self.eval_reports.insert(eval_run_id.to_string(), report);
    }

    /// Register a trajectory for dashboard queries.
    pub fn register_trajectory(&mut self, trajectory_id: &str, traj: Value) {
        self.trajectories.insert(trajectory_id.to_string(), traj);
    }
}

#[plexspaces_handlers]
impl DashboardActor {
    /// Get the full report for an eval run.
    #[handler("get_eval_report")]
    async fn get_eval_report(
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
        match self.eval_reports.get(eval_run_id) {
            Some(report) => Ok(report.clone()),
            None => Ok(json!({"error": format!("eval run {} not found", eval_run_id)})),
        }
    }

    /// List recent eval runs.
    #[handler("list_eval_runs")]
    async fn list_eval_runs(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<Value, BehaviorError> {
        let payload: Value = serde_json::from_slice(&msg.payload).unwrap_or(Value::Null);
        let limit = payload
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(10) as usize;

        let reports: Vec<Value> = self
            .eval_reports
            .iter()
            .take(limit)
            .map(|(run_id, report)| {
                json!({
                    "eval_run_id": run_id,
                    "suite_name": report.get("suite_name").cloned().unwrap_or(json!("")),
                    "pass_rate": report.get("pass_rate").cloned().unwrap_or(json!(0.0)),
                    "completed": report.get("completed_scenarios").cloned().unwrap_or(json!(0)),
                    "total": report.get("total_scenarios").cloned().unwrap_or(json!(0)),
                    "status": report.get("status").cloned().unwrap_or(json!("")),
                })
            })
            .collect();

        let count = reports.len();
        Ok(json!({"status": "ok", "runs": reports, "count": count}))
    }

    /// Get a specific trajectory by ID.
    #[handler("get_trajectory")]
    async fn get_trajectory(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<Value, BehaviorError> {
        let payload: Value = serde_json::from_slice(&msg.payload).unwrap_or(Value::Null);
        let id = payload
            .get("trajectory_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if id.is_empty() {
            return Ok(json!({"error": "trajectory_id is required"}));
        }
        match self.trajectories.get(id) {
            Some(traj) => Ok(traj.clone()),
            None => Ok(json!({"error": format!("trajectory {} not found", id)})),
        }
    }

    /// Get regression baseline info.
    #[handler("get_regressions")]
    async fn get_regressions(
        &mut self,
        _ctx: &ActorContext,
        _msg: &Message,
    ) -> Result<Value, BehaviorError> {
        Ok(json!({
            "status": "ok",
            "baseline_eval_run": self.regression_baseline_run,
            "baseline_scenario_count": self.regression_baseline.len(),
        }))
    }

    /// High-level system summary.
    #[handler("summary")]
    async fn summary(
        &mut self,
        _ctx: &ActorContext,
        _msg: &Message,
    ) -> Result<Value, BehaviorError> {
        Ok(json!({
            "status": "ok",
            "actor_id": self.actor_id,
            "message": "Use get_eval_report, list_eval_runs, get_trajectory for details.",
        }))
    }
}
