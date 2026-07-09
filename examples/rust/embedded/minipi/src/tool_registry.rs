// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// ToolRegistryActor — tool catalog with JSON Schema-validated execution.
//
// Demonstrates: GenServer pattern, built-in tool catalog, schema awareness.
// SchemaValidationFacet (priority 95 in app-config.toml) validates inputs
// before this actor sees them.

use plexspaces_sdk::{
    gen_server_actor, plexspaces_handlers, handler,
    ActorContext, BehaviorError, Message, Value, json,
};
use tracing::info;

/// ToolRegistry actor: maintains tool catalog and executes validated tool calls.
///
/// SchemaValidationFacet (priority 95) validates all incoming tool_call messages
/// against registered JSON Schemas before this actor processes them.
#[gen_server_actor(name = "tool_registry")]
pub struct ToolRegistryActor {
    actor_id: String,
    total_executions: u64,
    total_rejections: u64,
}

impl ToolRegistryActor {
    pub fn new() -> Self {
        Self {
            actor_id: String::new(),
            total_executions: 0,
            total_rejections: 0,
        }
    }

    fn builtin_tools() -> Vec<Value> {
        vec![
            json!({
                "name": "web_search",
                "description": "Search the web for information",
                "schema": {
                    "type": "object",
                    "required": ["query"],
                    "properties": {
                        "query": {"type": "string", "minLength": 1, "maxLength": 500},
                        "num_results": {"type": "integer", "minimum": 1, "maximum": 20}
                    }
                }
            }),
            json!({
                "name": "calculator",
                "description": "Evaluate a mathematical expression",
                "schema": {
                    "type": "object",
                    "required": ["expression"],
                    "properties": {
                        "expression": {"type": "string", "minLength": 1}
                    }
                }
            }),
            json!({
                "name": "kv_read",
                "description": "Read a value from key-value store",
                "schema": {
                    "type": "object",
                    "required": ["key"],
                    "properties": {
                        "key": {"type": "string"}
                    }
                }
            }),
            json!({
                "name": "kv_write",
                "description": "Write a value to key-value store",
                "schema": {
                    "type": "object",
                    "required": ["key", "value"],
                    "properties": {
                        "key": {"type": "string"},
                        "value": {}
                    }
                }
            }),
        ]
    }
}

#[plexspaces_handlers]
impl ToolRegistryActor {
    /// Execute a tool call. SchemaValidationFacet has already validated arguments.
    #[handler("execute")]
    async fn execute(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<Value, BehaviorError> {
        let payload: Value = serde_json::from_slice(&msg.payload).unwrap_or(Value::Null);
        let name = payload.get("name").and_then(|v| v.as_str()).unwrap_or("");
        if name.is_empty() {
            return Ok(json!({"error": "tool name is required"}));
        }
        let input = payload.get("input").cloned().unwrap_or(json!({}));

        self.total_executions += 1;

        let result = match name {
            "web_search" => {
                let query = input.get("query").and_then(|v| v.as_str()).unwrap_or("");
                if query.is_empty() {
                    json!({"error": "schema validation error: query minLength:1 violated"})
                } else {
                    let num_results = input
                        .get("num_results")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(3) as usize;
                    let results: Vec<Value> = (0..num_results.min(3))
                        .map(|i| {
                            json!({
                                "title": format!("Result {} for: {}", i+1, &query[..40.min(query.len())]),
                                "url": format!("https://example.com/result-{}", i+1),
                                "snippet": format!("Relevant snippet about {} from result {}.", &query[..30.min(query.len())], i+1),
                            })
                        })
                        .collect();
                    json!({"status": "ok", "query": query, "results": results})
                }
            }
            "calculator" => {
                let expr = input
                    .get("expression")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                self.calculator(expr)
            }
            "kv_read" => {
                let key = input.get("key").and_then(|v| v.as_str()).unwrap_or("");
                json!({"status": "ok", "key": key, "value": null})
            }
            "kv_write" => {
                let key = input.get("key").and_then(|v| v.as_str()).unwrap_or("");
                json!({"status": "ok", "key": key})
            }
            _ => {
                json!({"error": format!("Unknown tool: {}", name)})
            }
        };

        info!("ToolRegistry: executed tool={} total={}", name, self.total_executions);
        Ok(result)
    }

    /// List all available tools with descriptions and schemas.
    #[handler("list_tools")]
    async fn list_tools(
        &mut self,
        _ctx: &ActorContext,
        _msg: &Message,
    ) -> Result<Value, BehaviorError> {
        let tools = Self::builtin_tools();
        let count = tools.len();
        Ok(json!({"status": "ok", "tools": tools, "count": count}))
    }

    /// Register a custom tool.
    #[handler("register_tool")]
    async fn register_tool(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<Value, BehaviorError> {
        let payload: Value = serde_json::from_slice(&msg.payload).unwrap_or(Value::Null);
        let name = payload.get("name").and_then(|v| v.as_str()).unwrap_or("");
        if name.is_empty() {
            return Ok(json!({"error": "tool name is required"}));
        }
        Ok(json!({"status": "ok", "tool": name}))
    }

    /// Return execution statistics.
    #[handler("get_stats")]
    async fn get_stats(
        &mut self,
        _ctx: &ActorContext,
        _msg: &Message,
    ) -> Result<Value, BehaviorError> {
        Ok(json!({
            "status": "ok",
            "total_executions": self.total_executions,
            "total_rejections": self.total_rejections,
        }))
    }
}

impl ToolRegistryActor {
    fn calculator(&self, expression: &str) -> Value {
        // Safe arithmetic-only evaluator
        let allowed: bool = expression
            .chars()
            .all(|c| c.is_ascii_digit() || "+-*/()., ".contains(c));
        if !allowed {
            return json!({"error": "Invalid expression: contains unsafe characters"});
        }

        // Parse and evaluate simple arithmetic
        match self.eval_expr(expression.trim()) {
            Ok(result) => json!({
                "status": "ok",
                "expression": expression,
                "result": result
            }),
            Err(e) => json!({"error": format!("Calculation failed: {}", e)}),
        }
    }

    fn eval_expr(&self, expr: &str) -> Result<f64, String> {
        // Simple recursive descent parser for arithmetic
        let tokens: Vec<&str> = expr.split_whitespace().collect();
        if tokens.is_empty() {
            return Err("empty expression".to_string());
        }

        // Try to evaluate with Python-style precedence via a simple approach:
        // Remove spaces and parse
        let clean: String = expr.chars().filter(|c| !c.is_whitespace()).collect();
        self.parse_additive(&clean, &mut 0)
    }

    fn parse_additive(&self, s: &str, pos: &mut usize) -> Result<f64, String> {
        let mut left = self.parse_multiplicative(s, pos)?;
        while *pos < s.len() {
            let ch = s.chars().nth(*pos).unwrap();
            if ch == '+' || ch == '-' {
                *pos += 1;
                let right = self.parse_multiplicative(s, pos)?;
                if ch == '+' {
                    left += right;
                } else {
                    left -= right;
                }
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_multiplicative(&self, s: &str, pos: &mut usize) -> Result<f64, String> {
        let mut left = self.parse_primary(s, pos)?;
        while *pos < s.len() {
            let ch = s.chars().nth(*pos).unwrap();
            if ch == '*' || ch == '/' {
                *pos += 1;
                let right = self.parse_primary(s, pos)?;
                if ch == '*' {
                    left *= right;
                } else {
                    if right == 0.0 {
                        return Err("division by zero".to_string());
                    }
                    left /= right;
                }
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_primary(&self, s: &str, pos: &mut usize) -> Result<f64, String> {
        if *pos >= s.len() {
            return Err("unexpected end of expression".to_string());
        }
        let ch = s.chars().nth(*pos).unwrap();
        if ch == '(' {
            *pos += 1;
            let val = self.parse_additive(s, pos)?;
            if *pos < s.len() && s.chars().nth(*pos) == Some(')') {
                *pos += 1;
            }
            Ok(val)
        } else if ch == '-' {
            *pos += 1;
            Ok(-self.parse_primary(s, pos)?)
        } else {
            // Parse number
            let start = *pos;
            while *pos < s.len() {
                let c = s.chars().nth(*pos).unwrap();
                if c.is_ascii_digit() || c == '.' {
                    *pos += 1;
                } else {
                    break;
                }
            }
            let num_str = &s[start..*pos];
            num_str
                .parse::<f64>()
                .map_err(|_| format!("invalid number: {}", num_str))
        }
    }
}
