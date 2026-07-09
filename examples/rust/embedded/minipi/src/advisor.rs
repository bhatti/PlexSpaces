// SPDX-License-Identifier: AGPL-3.0-or-later
// AdvisorActor — two-tier LLM: cheap executor + expensive advisor on low confidence.
//
// Demonstrates the Anthropic Advisor strategy (2026):
// - Executor (cheap model, every turn): llama3.2
// - Advisor (expensive model, on-demand): llama3.3:70b
// Escalation threshold (0.0–1.0) controls when advisor is invoked.
// Tracks: escalation_rate_pct, advisor_token_share_pct for cost/quality analysis.

use plexspaces_sdk::{
    gen_server_actor, plexspaces_handlers, handler,
    ActorContext, BehaviorError, Message, Value, json,
};
use tracing::info;

#[gen_server_actor(name = "advisor")]
pub struct AdvisorActor {
    actor_id: String,
    confidence_threshold: f64,
    fast_model: String,
    expensive_model: String,
    total_requests: u64,
    escalation_count: u64,
    fast_input_tokens: u64,
    fast_output_tokens: u64,
    advisor_input_tokens: u64,
    advisor_output_tokens: u64,
}

impl AdvisorActor {
    pub fn new(confidence_threshold: f64) -> Self {
        Self {
            actor_id: String::new(),
            confidence_threshold,
            fast_model: "llama3.2".to_string(),
            expensive_model: "llama3.3:70b".to_string(),
            total_requests: 0,
            escalation_count: 0,
            fast_input_tokens: 0,
            fast_output_tokens: 0,
            advisor_input_tokens: 0,
            advisor_output_tokens: 0,
        }
    }
}

#[plexspaces_handlers]
impl AdvisorActor {
    #[handler("advise")]
    async fn advise(&mut self, _ctx: &ActorContext, msg: &Message) -> Result<Value, BehaviorError> {
        let payload: Value = serde_json::from_slice(&msg.payload).unwrap_or(Value::Null);
        let prompt = payload.get("prompt").and_then(|x| x.as_str()).unwrap_or("");
        let in_tokens = (prompt.len() / 4 + 1) as u64;
        let out_tokens = 20u64;

        self.total_requests += 1;

        // Simulate confidence: deterministic hash of prompt
        let hash: u64 = prompt.bytes().fold(0u64, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u64));
        let confidence = 0.5 + (hash % 50) as f64 * 0.01;
        let escalated = confidence < self.confidence_threshold;

        self.fast_input_tokens += in_tokens;
        self.fast_output_tokens += out_tokens;

        let (recommendation, model_used) = if escalated {
            self.escalation_count += 1;
            self.advisor_input_tokens += in_tokens * 2;
            self.advisor_output_tokens += out_tokens * 3;
            info!("AdvisorActor: escalating to {} (confidence={:.2} < threshold={:.2})", self.expensive_model, confidence, self.confidence_threshold);
            (format!("(advisor:{}) Detailed analysis: {}", self.expensive_model, &prompt[..prompt.len().min(60)]), self.expensive_model.clone())
        } else {
            (format!("(executor:{}) Proceed with task.", self.fast_model), self.fast_model.clone())
        };

        Ok(json!({
            "status": "ok",
            "recommendation": recommendation,
            "model_used": model_used,
            "confidence": confidence,
            "escalated": escalated,
            "fast_tokens": in_tokens + out_tokens,
        }))
    }

    #[handler("get_stats")]
    async fn get_stats(&mut self, _ctx: &ActorContext, _msg: &Message) -> Result<Value, BehaviorError> {
        let escalation_rate = if self.total_requests > 0 {
            self.escalation_count as f64 / self.total_requests as f64 * 100.0
        } else { 0.0 };
        let total_fast = self.fast_input_tokens + self.fast_output_tokens;
        let total_advisor = self.advisor_input_tokens + self.advisor_output_tokens;
        let advisor_share = if total_fast + total_advisor > 0 {
            total_advisor as f64 / (total_fast + total_advisor) as f64 * 100.0
        } else { 0.0 };
        Ok(json!({
            "status": "ok",
            "confidence_threshold": self.confidence_threshold,
            "fast_model": self.fast_model,
            "expensive_model": self.expensive_model,
            "total_requests": self.total_requests,
            "escalation_count": self.escalation_count,
            "escalation_rate_pct": escalation_rate,
            "advisor_token_share_pct": advisor_share,
            "fast_input_tokens": self.fast_input_tokens,
            "fast_output_tokens": self.fast_output_tokens,
            "advisor_input_tokens": self.advisor_input_tokens,
            "advisor_output_tokens": self.advisor_output_tokens,
        }))
    }

    #[handler("reset_stats")]
    async fn reset_stats(&mut self, _ctx: &ActorContext, _msg: &Message) -> Result<Value, BehaviorError> {
        self.total_requests = 0;
        self.escalation_count = 0;
        self.fast_input_tokens = 0;
        self.fast_output_tokens = 0;
        self.advisor_input_tokens = 0;
        self.advisor_output_tokens = 0;
        Ok(json!({"status": "ok"}))
    }
}
