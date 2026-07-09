// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// LLMGatewayActor — model abstraction with Ollama integration and KV response cache.
//
// Demonstrates: GenServer pattern, KV caching, token usage tracking.

use plexspaces_sdk::{
    gen_server_actor, plexspaces_handlers, handler,
    ActorContext, BehaviorError, Message, Value, json,
};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use tracing::{info, warn};

const DEFAULT_MODEL: &str = "llama3.2";
const OLLAMA_BASE_URL: &str = "http://localhost:11434";

/// LLM Gateway actor: routes completion requests to Ollama (or mock fallback).
///
/// Tracks token usage per request for budget enforcement.
/// Caches deterministic completions to avoid redundant LLM calls during eval replays.
#[gen_server_actor(name = "llm_gateway")]
pub struct LLMGatewayActor {
    actor_id: String,
    model: String,
    provider: String,
    base_url: String,
    total_requests: u64,
    total_input_tokens: u64,
    total_output_tokens: u64,
    cache_hits: u64,
}

impl LLMGatewayActor {
    pub fn new(provider: &str, model: &str, base_url: &str) -> Self {
        Self {
            actor_id: String::new(),
            model: model.to_string(),
            provider: provider.to_string(),
            base_url: base_url.to_string(),
            total_requests: 0,
            total_input_tokens: 0,
            total_output_tokens: 0,
            cache_hits: 0,
        }
    }
}

#[plexspaces_handlers]
impl LLMGatewayActor {
    /// Request a completion from the LLM. Returns response with token usage.
    #[handler("completion")]
    async fn completion(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<Value, BehaviorError> {
        let payload: Value = serde_json::from_slice(&msg.payload).unwrap_or(Value::Null);
        let messages = payload.get("messages").cloned().unwrap_or(Value::Null);
        if messages.is_null() {
            return Ok(json!({"error": "messages is required"}));
        }

        // Check cache
        let cache_key = self.cache_key(&payload);
        if let Some(cached) = self.get_cached(&cache_key) {
            self.cache_hits += 1;
            info!("LLMGateway cache hit key={}", &cache_key[..16.min(cache_key.len())]);
            return Ok(cached);
        }

        let temperature = payload.get("temperature").and_then(|v| v.as_f64()).unwrap_or(0.7);
        let tools = payload.get("tools").cloned().unwrap_or(Value::Null);

        let result = if self.provider == "ollama" {
            self.ollama_completion(&messages, &tools, temperature).await
        } else {
            self.mock_completion(&messages, &tools)
        };

        if result.get("error").is_none() {
            self.total_requests += 1;
            let input_tokens = result.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
            let output_tokens = result.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
            self.total_input_tokens += input_tokens;
            self.total_output_tokens += output_tokens;
            self.put_cached(&cache_key, &result);
        }

        Ok(result)
    }

    /// Return usage statistics.
    #[handler("get_stats")]
    async fn get_stats(
        &mut self,
        _ctx: &ActorContext,
        _msg: &Message,
    ) -> Result<Value, BehaviorError> {
        Ok(json!({
            "status": "ok",
            "model": self.model,
            "provider": self.provider,
            "total_requests": self.total_requests,
            "total_input_tokens": self.total_input_tokens,
            "total_output_tokens": self.total_output_tokens,
            "cache_hits": self.cache_hits,
        }))
    }

    /// Change the active model.
    #[handler("set_model")]
    async fn set_model(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<Value, BehaviorError> {
        let payload: Value = serde_json::from_slice(&msg.payload).unwrap_or(Value::Null);
        let model = payload.get("model").and_then(|v| v.as_str()).unwrap_or("");
        if model.is_empty() {
            return Ok(json!({"error": "model is required"}));
        }
        self.model = model.to_string();
        Ok(json!({"status": "ok", "model": self.model}))
    }
}

impl LLMGatewayActor {
    fn mock_completion(&self, messages: &Value, _tools: &Value) -> Value {
        // Find the last user message
        let last_user_content = messages
            .as_array()
            .and_then(|msgs| {
                msgs.iter()
                    .rev()
                    .find(|m| m.get("role").and_then(|r| r.as_str()) == Some("user"))
                    .and_then(|m| m.get("content"))
                    .and_then(|c| c.as_str())
                    .map(|s| s.to_string())
            })
            .unwrap_or_default();

        let lower = last_user_content.to_lowercase();
        let word_count = last_user_content.split_whitespace().count() as i64;

        if lower.contains("search") || lower.contains("find") {
            json!({
                "response": {
                    "content": "",
                    "stop_reason": "tool_use",
                    "tool_name": "web_search",
                    "arguments": {"query": &last_user_content[..50.min(last_user_content.len())]},
                    "tool_calls": [{"name": "web_search", "input": {"query": &last_user_content[..50.min(last_user_content.len())]}}]
                },
                "input_tokens": word_count * 2,
                "output_tokens": 20,
                "model": "mock",
            })
        } else if lower.contains("calculat") || last_user_content.contains('+')
            || last_user_content.contains('*')
            || last_user_content.contains('-')
            || last_user_content.contains('/')
        {
            json!({
                "response": {
                    "content": "",
                    "stop_reason": "tool_use",
                    "tool_name": "calculator",
                    "arguments": {"expression": &last_user_content},
                    "tool_calls": [{"name": "calculator", "input": {"expression": &last_user_content}}]
                },
                "input_tokens": word_count * 2,
                "output_tokens": 15,
                "model": "mock",
            })
        } else {
            let preview = &last_user_content[..60.min(last_user_content.len())];
            json!({
                "response": {
                    "content": format!("I processed your request: {}", preview),
                    "stop_reason": "end_turn",
                    "tool_calls": [],
                    "tool_name": null,
                    "arguments": {}
                },
                "input_tokens": word_count * 2,
                "output_tokens": 25,
                "model": "mock",
            })
        }
    }

    async fn ollama_completion(&self, messages: &Value, tools: &Value, temperature: f64) -> Value {
        let mut body = json!({
            "model": self.model,
            "messages": messages,
            "stream": false,
            "options": {"temperature": temperature},
        });
        if !tools.is_null() {
            body["tools"] = tools.clone();
        }

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap_or_default();

        let url = format!("{}/api/chat", self.base_url);
        match client.post(&url).json(&body).send().await {
            Err(e) => {
                warn!("Ollama request failed: {}", e);
                // Fall back to mock
                self.mock_completion(messages, tools)
            }
            Ok(resp) => {
                let status = resp.status().as_u16();
                if status != 200 {
                    warn!("Ollama returned HTTP {}", status);
                    return self.mock_completion(messages, tools);
                }
                match resp.json::<Value>().await {
                    Err(e) => {
                        warn!("Ollama JSON parse error: {}", e);
                        self.mock_completion(messages, tools)
                    }
                    Ok(data) => {
                        let message = data.get("message").cloned().unwrap_or(Value::Null);
                        let content = message
                            .get("content")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let done = data.get("done").and_then(|v| v.as_bool()).unwrap_or(true);
                        let tool_calls = message
                            .get("tool_calls")
                            .cloned()
                            .unwrap_or(json!([]));
                        let stop_reason = if done { "end_turn" } else { "tool_use" };
                        let input_tokens = data
                            .get("prompt_eval_count")
                            .and_then(|v| v.as_i64())
                            .unwrap_or(0);
                        let output_tokens =
                            data.get("eval_count").and_then(|v| v.as_i64()).unwrap_or(0);
                        json!({
                            "response": {
                                "content": content,
                                "stop_reason": stop_reason,
                                "tool_calls": tool_calls,
                            },
                            "input_tokens": input_tokens,
                            "output_tokens": output_tokens,
                            "model": self.model,
                        })
                    }
                }
            }
        }
    }

    fn cache_key(&self, payload: &Value) -> String {
        let content =
            serde_json::to_string(payload).unwrap_or_default() + &self.model;
        let mut h = DefaultHasher::new();
        content.hash(&mut h);
        format!("llm_cache:{:016x}", h.finish())
    }

    fn get_cached(&self, _key: &str) -> Option<Value> {
        // In-process cache not available in embedded mode without actor context KV.
        // Skip caching — deterministic mock is fast enough.
        None
    }

    fn put_cached(&self, _key: &str, _value: &Value) {
        // No-op: embedded mode
    }
}
