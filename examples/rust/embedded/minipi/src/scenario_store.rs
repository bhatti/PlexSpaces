// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// ScenarioStoreActor — persists eval scenario definitions.
//
// Demonstrates: structured in-memory storage, scenario catalog, rubric management.

use plexspaces_sdk::{
    gen_server_actor, plexspaces_handlers, handler,
    ActorContext, BehaviorError, Message, Value, json,
};
use std::collections::HashMap;
use tracing::info;

fn builtin_scenarios() -> Vec<Value> {
    vec![
        json!({
            "scenario_id": "sc-math-01",
            "name": "Basic multiplication",
            "input": "What is 6 * 7?",
            "expected": "42",
            "rubric": "task_completion",
            "tags": ["math"],
            "difficulty": "easy",
        }),
        json!({
            "scenario_id": "sc-calc-01",
            "name": "Step-by-step arithmetic",
            "input": "Compute (17 * 24) + (89 - 45) step by step",
            "expected": "452",
            "rubric": "task_completion",
            "tags": ["math"],
            "difficulty": "easy",
        }),
        json!({
            "scenario_id": "sc-search-01",
            "name": "Pythagorean theorem search",
            "input": "Search for information about the Pythagorean theorem",
            "expected": "a^2 + b^2 = c^2",
            "rubric": "tool_use",
            "tags": ["search", "tool_use"],
            "difficulty": "medium",
        }),
        json!({
            "scenario_id": "sc-reason-01",
            "name": "Syllogistic reasoning",
            "input": "If all Bloops are Razzies and all Razzies are Lazzies, are all Bloops definitely Lazzies?",
            "expected": "yes",
            "rubric": "task_completion",
            "tags": ["reasoning"],
            "difficulty": "medium",
        }),
        json!({
            "scenario_id": "sc-budget-01",
            "name": "Quadratic equation summary",
            "input": "Summarize the key steps to solve a quadratic equation ax^2 + bx + c = 0",
            "expected": "quadratic formula",
            "rubric": "task_completion",
            "tags": ["math", "reasoning"],
            "difficulty": "medium",
        }),
        json!({
            "scenario_id": "sc-contract-01",
            "name": "Expression validation",
            "input": "Validate: is the expression '(2 + 3) * (4 - 1)' valid? What is its value?",
            "expected": "15",
            "rubric": "task_completion",
            "tags": ["math"],
            "difficulty": "easy",
        }),
        json!({
            "scenario_id": "sc-multi-01",
            "name": "Multi-step tool use",
            "input": "Search for the capital of France, then compute 3 * 7, then report both results",
            "expected": "Paris, 21",
            "rubric": "tool_use",
            "tags": ["search", "math", "tool_use"],
            "difficulty": "hard",
        }),
        json!({
            "scenario_id": "sc-kv-01",
            "name": "Key-value store roundtrip",
            "input": "Store the value 'hello world' under key 'test_key', then read it back and verify",
            "expected": "hello world",
            "rubric": "tool_use",
            "tags": ["kv", "tool_use"],
            "difficulty": "medium",
        }),
        json!({
            "scenario_id": "sc-chain-01",
            "name": "Chained computation",
            "input": "Compute sqrt(144), then add 5 to the result, then multiply by 2",
            "expected": "34",
            "rubric": "task_completion",
            "tags": ["math"],
            "difficulty": "medium",
        }),
        json!({
            "scenario_id": "sc-compare-01",
            "name": "Power comparison",
            "input": "Which is larger: 2^10 or 10^3? Show your calculation",
            "expected": "1024 > 1000",
            "rubric": "task_completion",
            "tags": ["math"],
            "difficulty": "easy",
        }),
    ]
}

/// Scenario catalog: stores, retrieves, and lists eval scenarios.
///
/// Scenarios are persisted in memory keyed by scenario_id.
/// Built-in scenarios are seeded at construction time.
#[gen_server_actor(name = "scenario_store")]
pub struct ScenarioStoreActor {
    actor_id: String,
    scenarios: HashMap<String, Value>,
    suites: HashMap<String, Vec<String>>,
}

impl ScenarioStoreActor {
    pub fn new() -> Self {
        let mut scenarios = HashMap::new();
        for sc in builtin_scenarios() {
            let id = sc
                .get("scenario_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if !id.is_empty() {
                scenarios.insert(id, sc);
            }
        }
        info!("ScenarioStore: seeded {} built-in scenarios", scenarios.len());
        Self {
            actor_id: String::new(),
            scenarios,
            suites: HashMap::new(),
        }
    }
}

#[plexspaces_handlers]
impl ScenarioStoreActor {
    /// Get a single scenario by ID.
    #[handler("get_scenario")]
    async fn get_scenario(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<Value, BehaviorError> {
        let payload: Value = serde_json::from_slice(&msg.payload).unwrap_or(Value::Null);
        let id = payload
            .get("scenario_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if id.is_empty() {
            return Ok(json!({"error": "scenario_id is required"}));
        }
        match self.scenarios.get(id) {
            Some(sc) => Ok(json!({"status": "ok", "scenario": sc})),
            None => Ok(json!({"error": format!("scenario {} not found", id)})),
        }
    }

    /// List scenarios, optionally filtered.
    #[handler("list_scenarios")]
    async fn list_scenarios(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<Value, BehaviorError> {
        let payload: Value = serde_json::from_slice(&msg.payload).unwrap_or(Value::Null);
        let difficulty = payload
            .get("difficulty")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let limit = payload
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(50) as usize;

        let tags: Vec<String> = payload
            .get("tags")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        let scenarios: Vec<&Value> = self
            .scenarios
            .values()
            .filter(|sc| {
                if !difficulty.is_empty() {
                    if sc.get("difficulty").and_then(|v| v.as_str()) != Some(difficulty) {
                        return false;
                    }
                }
                if !tags.is_empty() {
                    let sc_tags: Vec<&str> = sc
                        .get("tags")
                        .and_then(|v| v.as_array())
                        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
                        .unwrap_or_default();
                    if !tags.iter().any(|t| sc_tags.contains(&t.as_str())) {
                        return false;
                    }
                }
                true
            })
            .take(limit)
            .collect();

        let scenarios_json: Vec<Value> = scenarios.into_iter().cloned().collect();
        let count = scenarios_json.len();
        Ok(json!({"status": "ok", "scenarios": scenarios_json, "count": count}))
    }

    /// Store or update a scenario.
    #[handler("put_scenario")]
    async fn put_scenario(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<Value, BehaviorError> {
        let payload: Value = serde_json::from_slice(&msg.payload).unwrap_or(Value::Null);
        let mut scenario = match payload.get("scenario").cloned() {
            Some(s) => s,
            None => return Ok(json!({"error": "scenario is required"})),
        };

        let id = scenario
            .get("scenario_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| ulid::Ulid::new().to_string());

        if let Some(obj) = scenario.as_object_mut() {
            obj.insert("scenario_id".to_string(), json!(id));
        }

        self.scenarios.insert(id.clone(), scenario);
        Ok(json!({"status": "ok", "scenario_id": id}))
    }

    /// Get a named suite of scenarios.
    ///
    /// Built-in suites: smoke (1), standard (5), full (all 10).
    #[handler("get_suite")]
    async fn get_suite(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<Value, BehaviorError> {
        let payload: Value = serde_json::from_slice(&msg.payload).unwrap_or(Value::Null);
        let suite_name = payload
            .get("suite_name")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        // If explicit scenario_ids provided, use those
        if let Some(ids_val) = payload.get("scenario_ids") {
            if let Some(ids) = ids_val.as_array() {
                let scenarios: Vec<Value> = ids
                    .iter()
                    .filter_map(|id| id.as_str())
                    .filter_map(|id| self.scenarios.get(id).cloned())
                    .collect();
                let count = scenarios.len();
                return Ok(json!({"status": "ok", "suite_name": suite_name, "scenarios": scenarios, "count": count}));
            }
        }

        let ids: Vec<&str> = match suite_name {
            "smoke" => vec!["sc-math-01"],
            "standard" => vec![
                "sc-math-01",
                "sc-calc-01",
                "sc-search-01",
                "sc-reason-01",
                "sc-budget-01",
            ],
            "full" => vec![
                "sc-math-01",
                "sc-calc-01",
                "sc-search-01",
                "sc-reason-01",
                "sc-budget-01",
                "sc-contract-01",
                "sc-multi-01",
                "sc-kv-01",
                "sc-chain-01",
                "sc-compare-01",
            ],
            _ => {
                // Check stored suites
                if let Some(stored_ids) = self.suites.get(suite_name) {
                    let ids = stored_ids.clone();
                    let scenarios: Vec<Value> = ids
                        .iter()
                        .filter_map(|id| self.scenarios.get(id.as_str()).cloned())
                        .collect();
                    let count = scenarios.len();
                    return Ok(json!({"status": "ok", "suite_name": suite_name, "scenarios": scenarios, "count": count}));
                }
                return Ok(json!({"error": format!("unknown suite: {}", suite_name)}));
            }
        };

        let scenarios: Vec<Value> = ids
            .iter()
            .filter_map(|id| self.scenarios.get(*id).cloned())
            .collect();
        let count = scenarios.len();
        Ok(json!({"status": "ok", "suite_name": suite_name, "scenarios": scenarios, "count": count}))
    }

    /// Define a named suite.
    #[handler("put_suite")]
    async fn put_suite(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<Value, BehaviorError> {
        let payload: Value = serde_json::from_slice(&msg.payload).unwrap_or(Value::Null);
        let suite_name = payload
            .get("suite_name")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if suite_name.is_empty() {
            return Ok(json!({"error": "suite_name is required"}));
        }
        let scenario_ids: Vec<String> = payload
            .get("scenario_ids")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        let count = scenario_ids.len();
        self.suites.insert(suite_name.to_string(), scenario_ids);
        Ok(json!({"status": "ok", "suite_name": suite_name, "count": count}))
    }

    /// Return statistics.
    #[handler("get_stats")]
    async fn get_stats(
        &mut self,
        _ctx: &ActorContext,
        _msg: &Message,
    ) -> Result<Value, BehaviorError> {
        Ok(json!({
            "status": "ok",
            "actor_id": self.actor_id,
            "scenario_count": self.scenarios.len(),
        }))
    }
}
