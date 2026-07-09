// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// TrajectoryStoreActor — persists and indexes agent trajectory records.
//
// Demonstrates: in-memory storage, trajectory lifecycle management, eval collection.

use plexspaces_sdk::{
    gen_server_actor, plexspaces_handlers, handler,
    ActorContext, BehaviorError, Message, Value, json,
};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{info, warn};

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Trajectory storage and index: persists full AgentTrajectory records and
/// exposes query patterns for eval collection.
#[gen_server_actor(name = "trajectory_store")]
pub struct TrajectoryStoreActor {
    actor_id: String,
    /// Full trajectory records keyed by trajectory_id
    trajectories: HashMap<String, Value>,
    /// Metadata keyed by trajectory_id
    metadata: HashMap<String, Value>,
    /// Index: eval_run_id -> [trajectory_id, ...]
    eval_index: HashMap<String, Vec<String>>,
    stored_count: u64,
    failed_count: u64,
}

impl TrajectoryStoreActor {
    pub fn new() -> Self {
        Self {
            actor_id: String::new(),
            trajectories: HashMap::new(),
            metadata: HashMap::new(),
            eval_index: HashMap::new(),
            stored_count: 0,
            failed_count: 0,
        }
    }
}

#[plexspaces_handlers]
impl TrajectoryStoreActor {
    /// Store a trajectory record.
    #[handler("put")]
    async fn put(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<Value, BehaviorError> {
        let payload: Value = serde_json::from_slice(&msg.payload).unwrap_or(Value::Null);
        let mut trajectory = match payload.get("trajectory").cloned() {
            Some(t) => t,
            None => return Ok(json!({"error": "trajectory is required"})),
        };

        let traj_id = trajectory
            .get("trajectory_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| ulid::Ulid::new().to_string());

        if let Some(obj) = trajectory.as_object_mut() {
            obj.insert("trajectory_id".to_string(), json!(traj_id));
        }

        let eval_run_id = trajectory
            .get("eval_run_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let outcome = trajectory
            .get("outcome")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        let agent_actor_id = trajectory
            .get("agent_actor_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        // Store full trajectory
        self.trajectories.insert(traj_id.clone(), trajectory.clone());

        // Store metadata
        let step_count = trajectory
            .get("steps")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        let meta = json!({
            "trajectory_id": traj_id,
            "eval_run_id": eval_run_id,
            "agent_actor_id": agent_actor_id,
            "outcome": outcome,
            "score": trajectory.get("score").cloned().unwrap_or(json!(0.0)),
            "total_input_tokens": trajectory.get("total_input_tokens").cloned().unwrap_or(json!(0)),
            "total_output_tokens": trajectory.get("total_output_tokens").cloned().unwrap_or(json!(0)),
            "step_count": step_count,
            "stored_at_ms": now_ms(),
        });
        self.metadata.insert(traj_id.clone(), meta);

        // Update eval_run_id index
        if !eval_run_id.is_empty() {
            self.eval_index
                .entry(eval_run_id.clone())
                .or_default()
                .push(traj_id.clone());
        }

        self.stored_count += 1;
        info!(
            "TrajectoryStore: stored traj_id={} eval_run={} outcome={}",
            traj_id, eval_run_id, outcome
        );

        Ok(json!({"status": "ok", "trajectory_id": traj_id}))
    }

    /// Get a full trajectory by ID.
    #[handler("get")]
    async fn get(
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
            Some(traj) => Ok(json!({"status": "ok", "trajectory": traj})),
            None => Ok(json!({"error": format!("trajectory {} not found", id)})),
        }
    }

    /// List all trajectories for an eval run.
    #[handler("list_for_eval_run")]
    async fn list_for_eval_run(
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
        let include_full = payload
            .get("include_full")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let ids = self
            .eval_index
            .get(eval_run_id)
            .cloned()
            .unwrap_or_default();

        let trajectories: Vec<Value> = ids
            .iter()
            .filter_map(|id| {
                if include_full {
                    self.trajectories.get(id).cloned()
                } else {
                    self.metadata.get(id).cloned()
                }
            })
            .collect();

        let count = trajectories.len();
        Ok(json!({
            "status": "ok",
            "eval_run_id": eval_run_id,
            "trajectories": trajectories,
            "count": count,
        }))
    }

    /// Delete a trajectory and its metadata.
    #[handler("delete")]
    async fn delete(
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
        self.trajectories.remove(id);
        self.metadata.remove(id);
        Ok(json!({"status": "ok", "trajectory_id": id}))
    }

    /// Delete all trajectories for an eval run.
    #[handler("delete_eval_run")]
    async fn delete_eval_run(
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
        let ids = self
            .eval_index
            .remove(eval_run_id)
            .unwrap_or_default();
        let deleted = ids.len();
        for id in &ids {
            self.trajectories.remove(id);
            self.metadata.remove(id);
        }
        Ok(json!({"status": "ok", "eval_run_id": eval_run_id, "deleted": deleted}))
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
            "stored_count": self.stored_count,
            "failed_count": self.failed_count,
        }))
    }
}
