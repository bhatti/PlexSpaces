// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Worker Actor for Data-Parallel Processing
// Processes tasks from ShardGroup and demonstrates worker pool pattern

use plexspaces_sdk::{
    gen_server_actor, json, plexspaces_handlers, ActorContext, BehaviorError, Message, Value,
};
use serde_json;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Worker actor that processes data-parallel tasks
/// Each shard in a ShardGroup runs a worker actor instance
#[gen_server_actor]
pub struct WorkerActor {
    /// Worker ID (shard ID)
    worker_id: String,
    /// Local state (key-value store for this shard)
    state: Arc<RwLock<HashMap<String, Value>>>,
    /// Processing statistics
    tasks_processed: u64,
    total_processing_time_ms: u64,
}

impl WorkerActor {
    pub fn new(worker_id: String) -> Self {
        Self {
            worker_id,
            state: Arc::new(RwLock::new(HashMap::new())),
            tasks_processed: 0,
            total_processing_time_ms: 0,
        }
    }
}

#[plexspaces_handlers]
impl WorkerActor {
    #[handler("*")]
    async fn process(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<Value, BehaviorError> {
        let payload: Value =
            match serde_json::from_slice::<Value>(&msg.payload) {
                Ok(p) => {
                    // Debug log for message received (guarded)
                    if tracing::enabled!(tracing::Level::DEBUG) {
                        let action_opt =
                            p.get("action").and_then(|v: &serde_json::Value| v.as_str());
                        tracing::debug!(
                        "[WORKER_ACTOR] Message received: message_id={}, action={:?}, worker_id={}",
                        msg.id, action_opt, self.worker_id
                    );
                    }
                    p
                }
                Err(e) => {
                    tracing::warn!(
                    "[WORKER_ACTOR] Error parsing payload: message_id={}, error={}, worker_id={}",
                    msg.id, e, self.worker_id
                );
                    return Err(BehaviorError::ProcessingError(format!(
                        "Failed to parse payload: {}",
                        e
                    )));
                }
            };

        let action = payload["action"].as_str().unwrap_or("unknown");

        let result = match action {
            "increment" => {
                let key = payload["key"].as_str().unwrap_or("default");
                let mut state = self.state.write().await;
                let current = state.get(key).and_then(|v| v.as_u64()).unwrap_or(0);
                state.insert(key.to_string(), json!(current + 1));
                self.tasks_processed += 1;
                let elapsed = std::time::Instant::now().elapsed().as_millis() as u64;
                self.total_processing_time_ms += elapsed;
                Ok(json!({
                    "action": "increment",
                    "key": key,
                    "value": current + 1,
                    "worker_id": self.worker_id,
                    "processing_time_ms": elapsed
                }))
            }
            "set" => {
                let key = payload["key"].as_str().unwrap_or("default");
                let value = payload["value"].clone();
                let mut state = self.state.write().await;
                state.insert(key.to_string(), value.clone());
                self.tasks_processed += 1;
                let elapsed = std::time::Instant::now().elapsed().as_millis() as u64;
                self.total_processing_time_ms += elapsed;
                Ok(json!({
                    "action": "set",
                    "key": key,
                    "value": value,
                    "worker_id": self.worker_id,
                    "processing_time_ms": elapsed
                }))
            }
            "get" => {
                let key = payload["key"].as_str().unwrap_or("default");
                let state = self.state.read().await;
                let value = state.get(key).cloned().unwrap_or(json!(null));
                Ok(json!({
                    "action": "get",
                    "key": key,
                    "value": value,
                    "worker_id": self.worker_id
                }))
            }
            "get_all_keys" => {
                let state = self.state.read().await;
                let keys: Vec<String> = state.keys().cloned().collect();
                Ok(json!({
                    "action": "get_all_keys",
                    "keys": keys,
                    "count": keys.len(),
                    "worker_id": self.worker_id
                }))
            }
            "get_total_count" => {
                let state = self.state.read().await;
                let total: u64 = state.values().filter_map(|v| v.as_u64()).sum();
                Ok(json!({
                    "action": "get_total_count",
                    "total": total,
                    "worker_id": self.worker_id,
                    "keys_processed": state.len()
                }))
            }
            "stats" => {
                let avg_time = if self.tasks_processed > 0 {
                    self.total_processing_time_ms / self.tasks_processed
                } else {
                    0
                };
                let state = self.state.read().await;
                Ok(json!({
                    "action": "stats",
                    "worker_id": self.worker_id,
                    "tasks_processed": self.tasks_processed,
                    "avg_processing_time_ms": avg_time,
                    "keys_in_state": state.len()
                }))
            }
            _ => {
                let err = BehaviorError::ProcessingError(format!("Unknown action: {}", action));
                tracing::warn!(
                    "[WORKER_ACTOR] Unknown action: message_id={}, action={}, worker_id={}",
                    msg.id,
                    action,
                    self.worker_id
                );
                Err(err)
            }
        };

        // Debug log for message processing result (guarded)
        if tracing::enabled!(tracing::Level::DEBUG) {
            match &result {
                Ok(_) => {
                    tracing::debug!(
                        "[WORKER_ACTOR] Message processed: message_id={}, action={}, worker_id={}",
                        msg.id,
                        action,
                        self.worker_id
                    );
                }
                Err(_) => {
                    // Errors are logged at warn level below
                }
            }
        }

        // Always log errors at warn level
        if let Err(e) = &result {
            tracing::warn!(
                "[WORKER_ACTOR] Error processing: message_id={}, action={}, error={}, worker_id={}",
                msg.id,
                action,
                e,
                self.worker_id
            );
        }

        result
    }
}
