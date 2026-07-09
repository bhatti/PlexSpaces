// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// BenchmarkActor — fan-out N eval runs with different configs, measure throughput.
//
// Demonstrates: parallel eval fan-out, config comparison, performance measurement.

use plexspaces_sdk::{
    gen_server_actor, plexspaces_handlers, handler,
    ActorContext, BehaviorError, Message, Value, json,
};
use std::collections::HashMap;
use tracing::info;

/// Benchmark runner: same scenario, N different harness configs.
///
/// Measures: pass rate, completed scenarios per config.
/// Output: comparison table showing which harness config wins.
#[gen_server_actor(name = "benchmark")]
pub struct BenchmarkActor {
    actor_id: String,
    benchmark_id: String,
    status: String,
    results: Vec<Value>,
    /// KV store for eval reports (keyed by eval_run_id)
    eval_reports: HashMap<String, Value>,
}

impl BenchmarkActor {
    pub fn new() -> Self {
        Self {
            actor_id: String::new(),
            benchmark_id: String::new(),
            status: "idle".to_string(),
            results: Vec::new(),
            eval_reports: HashMap::new(),
        }
    }
}

#[plexspaces_handlers]
impl BenchmarkActor {
    /// Run the same scenarios with N different harness configs and produce a comparison table.
    #[handler("run")]
    async fn run(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<Value, BehaviorError> {
        let payload: Value = serde_json::from_slice(&msg.payload).unwrap_or(Value::Null);
        let scenarios = match payload.get("scenarios").and_then(|v| v.as_array()) {
            Some(s) => s.clone(),
            None => return Ok(json!({"error": "scenarios is required"})),
        };
        let configs: Vec<Value> = payload
            .get("configs")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_else(|| {
                vec![json!({
                    "name": "default",
                    "max_iterations": 10,
                    "token_budget": 4096
                })]
            });

        let benchmark_id = payload
            .get("benchmark_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| ulid::Ulid::new().to_string());

        self.benchmark_id = benchmark_id.clone();
        self.status = "running".to_string();

        info!(
            "BenchmarkActor starting: benchmark_id={} configs={} scenarios={}",
            benchmark_id,
            configs.len(),
            scenarios.len()
        );

        // In embedded mode: simulate eval run results without spawning sub-actors.
        // Each config gets a synthetic result based on its max_iterations/token_budget.
        let mut results = Vec::new();
        for (i, cfg) in configs.iter().enumerate() {
            let config_name = cfg
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or(&format!("config-{}", i))
                .to_string();
            let eval_run_id = format!("bench-{}-config-{}", benchmark_id, i);
            let max_iter = cfg.get("max_iterations").and_then(|v| v.as_u64()).unwrap_or(10);
            let token_budget = cfg.get("token_budget").and_then(|v| v.as_u64()).unwrap_or(4096);

            // Simulate: higher budget/iterations → better pass rate
            let pass_rate = if max_iter >= 10 && token_budget >= 4096 {
                0.9
            } else if max_iter >= 5 || token_budget >= 2048 {
                0.7
            } else {
                0.5
            };

            let report = json!({
                "status": "completed",
                "eval_run_id": eval_run_id,
                "suite_name": format!("benchmark-{}", config_name),
                "total_scenarios": scenarios.len(),
                "completed_scenarios": scenarios.len(),
                "pass_rate": pass_rate,
            });
            self.eval_reports.insert(eval_run_id.clone(), report.clone());

            results.push(json!({
                "config_name": config_name,
                "config": cfg,
                "eval_run_id": eval_run_id,
                "pass_rate": pass_rate,
                "completed_scenarios": scenarios.len(),
                "total_scenarios": scenarios.len(),
            }));
        }

        // Sort by pass rate (best first)
        results.sort_by(|a, b| {
            let pa = a.get("pass_rate").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let pb = b.get("pass_rate").and_then(|v| v.as_f64()).unwrap_or(0.0);
            pb.partial_cmp(&pa).unwrap_or(std::cmp::Ordering::Equal)
        });

        self.results = results.clone();
        self.status = "completed".to_string();

        let comparison_table: Vec<Value> = results
            .iter()
            .map(|r| {
                json!({
                    "config": r.get("config_name").cloned().unwrap_or(json!("")),
                    "pass_rate": format!("{:.1}%", r.get("pass_rate").and_then(|v| v.as_f64()).unwrap_or(0.0) * 100.0),
                    "completed": format!("{}/{}", r.get("completed_scenarios").and_then(|v| v.as_u64()).unwrap_or(0), r.get("total_scenarios").and_then(|v| v.as_u64()).unwrap_or(0)),
                    "max_iterations": r.get("config").and_then(|c| c.get("max_iterations")).cloned().unwrap_or(json!("?")),
                    "token_budget": r.get("config").and_then(|c| c.get("token_budget")).cloned().unwrap_or(json!("?")),
                })
            })
            .collect();

        let winner = results
            .first()
            .and_then(|r| r.get("config_name"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        info!(
            "BenchmarkActor completed: benchmark_id={} configs={} winner={}",
            benchmark_id,
            results.len(),
            winner
        );

        Ok(json!({
            "status": "completed",
            "benchmark_id": benchmark_id,
            "configs_tested": results.len(),
            "scenarios": scenarios.len(),
            "results": results,
            "comparison_table": comparison_table,
            "winner": winner,
        }))
    }

    /// Return current benchmark status.
    #[handler("get_status")]
    async fn get_status(
        &mut self,
        _ctx: &ActorContext,
        _msg: &Message,
    ) -> Result<Value, BehaviorError> {
        Ok(json!({
            "benchmark_id": self.benchmark_id,
            "status": self.status,
            "results_count": self.results.len(),
        }))
    }
}
