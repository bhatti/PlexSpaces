// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! StorageActor — one shard of the distributed key-value store.
//!
//! Each StorageActor owns a slice of the hash-partitioned keyspace.
//! Masters handle writes and propagate to replicas via the framework's
//! `broadcast` primitive. Replicas apply writes via the `replicate` cast handler.
//!
//! Framework primitives demonstrated here:
//! - `scatter_gather` queries route to `keys` and `get_ack` handlers
//! - `reduce(SUM)` queries route to the `dbsize` handler
//! - `broadcast` replication routes to the `replicate` cast handler
//! - `barrier` coordination routes to the `info` handler (framework sends "info" op)
//! - `map` snapshot queries route to the `snapshot` handler

use crate::StoredEntry;
use plexspaces_sdk::{
    gen_server_actor, plexspaces_handlers,
    ActorContext, BehaviorError, Message,
    json, Value,
};
use serde::Deserialize;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

// =============================================================================
// StorageActor — Shard Group Member
// =============================================================================

/// A single shard in the Redis cluster's data tier.
///
/// Instances are created by the PlexSpaces framework via `BehaviorRegistry`
/// when `create_shard_group` is called. The `shard_id` field is assigned
/// at construction time via an atomic counter shared across the factory closure.
#[gen_server_actor]
pub struct StorageActor {
    pub shard_id: usize,
    pub data: HashMap<String, StoredEntry>,
    pub role: String,          // "master" or "replica"
    pub replication_offset: i64,
}

impl StorageActor {
    /// Zero-arg constructor for default factory registration.
    pub fn new() -> Self {
        Self::new_with_id(0, "master")
    }

    /// Constructor used in BehaviorRegistry factory closures.
    pub fn new_with_id(shard_id: usize, role: &str) -> Self {
        Self {
            shard_id,
            data: HashMap::new(),
            role: role.to_string(),
            replication_offset: 0,
        }
    }
}

// =============================================================================
// Utility
// =============================================================================

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn is_expired(entry: &StoredEntry) -> bool {
    if let Some(exp) = entry.expires_at_ms {
        now_ms() > exp
    } else {
        false
    }
}

// =============================================================================
// Handlers
// =============================================================================

#[plexspaces_handlers(gen_server)]
impl StorageActor {
    // -------------------------------------------------------------------------
    // Basic commands (Ch5–6)
    // -------------------------------------------------------------------------

    /// PING → "PONG"
    #[handler("ping")]
    async fn handle_ping(&mut self, _ctx: &ActorContext, _msg: &Message) -> Result<Value, BehaviorError> {
        Ok(json!({ "result": "PONG" }))
    }

    /// ECHO arg → arg
    #[handler("echo")]
    async fn handle_echo(&mut self, _ctx: &ActorContext, msg: &Message) -> Result<Value, BehaviorError> {
        let payload: Value = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("bad payload: {}", e)))?;
        let arg = payload.get("arg").and_then(|v| v.as_str()).unwrap_or("").to_string();
        Ok(json!({ "result": arg }))
    }

    /// GET key — with passive expiry check (Ch4).
    #[handler("get")]
    async fn handle_get(&mut self, _ctx: &ActorContext, msg: &Message) -> Result<Value, BehaviorError> {
        let payload: Value = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("bad payload: {}", e)))?;
        let key = payload.get("key").and_then(|v| v.as_str()).unwrap_or("");
        match self.data.get(key) {
            Some(entry) if is_expired(entry) => {
                self.data.remove(key);
                Ok(json!({ "result": null, "found": false }))
            }
            Some(entry) => Ok(json!({ "result": entry.value, "found": true })),
            None => Ok(json!({ "result": null, "found": false })),
        }
    }

    /// SET key value [NX|XX] [EX seconds | PX millis] (Ch4).
    #[handler("set")]
    async fn handle_set(&mut self, _ctx: &ActorContext, msg: &Message) -> Result<Value, BehaviorError> {
        #[derive(Deserialize)]
        struct SetPayload {
            key: String,
            value: String,
            #[serde(default)] nx: bool,
            #[serde(default)] xx: bool,
            #[serde(default)] ex: Option<u64>,
            #[serde(default)] px: Option<u64>,
        }
        let p: SetPayload = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("bad payload: {}", e)))?;

        // NX: only if not exists
        if p.nx && self.data.contains_key(&p.key) {
            return Ok(json!({ "result": null, "ok": false }));
        }
        // XX: only if exists
        if p.xx && !self.data.contains_key(&p.key) {
            return Ok(json!({ "result": null, "ok": false }));
        }

        let expires_at_ms = if let Some(ex) = p.ex {
            Some(now_ms() + ex * 1000)
        } else if let Some(px) = p.px {
            Some(now_ms() + px)
        } else {
            None
        };

        self.data.insert(p.key, StoredEntry { value: p.value, expires_at_ms });
        self.replication_offset += 1;
        Ok(json!({ "result": "OK", "ok": true }))
    }

    /// INCR key — create with 1 if missing; error if value is not an integer (Ch9).
    #[handler("incr")]
    async fn handle_incr(&mut self, _ctx: &ActorContext, msg: &Message) -> Result<Value, BehaviorError> {
        let payload: Value = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("bad payload: {}", e)))?;
        let key = payload.get("key").and_then(|v| v.as_str()).unwrap_or("").to_string();

        // Passive expiry
        if self.data.get(&key).map(is_expired).unwrap_or(false) {
            self.data.remove(&key);
        }

        let new_val = match self.data.get(&key) {
            None => 1i64,
            Some(entry) => {
                match entry.value.parse::<i64>() {
                    Ok(n) => n + 1,
                    Err(_) => return Ok(json!({
                        "result": null,
                        "error": "ERR value is not an integer or out of range"
                    })),
                }
            }
        };

        self.data.insert(key, StoredEntry { value: new_val.to_string(), expires_at_ms: None });
        self.replication_offset += 1;
        Ok(json!({ "result": new_val, "error": null }))
    }

    /// DEL key — remove key, return count deleted.
    #[handler("del")]
    async fn handle_del(&mut self, _ctx: &ActorContext, msg: &Message) -> Result<Value, BehaviorError> {
        let payload: Value = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("bad payload: {}", e)))?;
        let key = payload.get("key").and_then(|v| v.as_str()).unwrap_or("");
        let deleted = if self.data.remove(key).is_some() {
            self.replication_offset += 1;
            1
        } else {
            0
        };
        Ok(json!({ "result": deleted }))
    }

    // -------------------------------------------------------------------------
    // Cross-shard query handlers — used by scatter_gather / reduce / map
    // -------------------------------------------------------------------------

    /// DBSIZE — key count for reduce(SUM). Returns {"count": N}.
    #[handler("dbsize")]
    async fn handle_dbsize(&mut self, _ctx: &ActorContext, _msg: &Message) -> Result<Value, BehaviorError> {
        let count = self.data.values().filter(|e| !is_expired(e)).count() as i64;
        Ok(json!({ "count": count, "shard_id": self.shard_id }))
    }

    /// KEYS — return all non-expired keys for scatter_gather aggregation.
    #[handler("keys")]
    async fn handle_keys(&mut self, _ctx: &ActorContext, _msg: &Message) -> Result<Value, BehaviorError> {
        let keys: Vec<String> = self.data
            .iter()
            .filter(|(_, e)| !is_expired(e))
            .map(|(k, _)| k.clone())
            .collect();
        Ok(json!({ "keys": keys, "shard_id": self.shard_id }))
    }

    /// INFO — node role/offset/shard metadata. Also handles framework `barrier` ops
    /// (which send message_type="info" internally).
    #[handler("info")]
    async fn handle_info(&mut self, _ctx: &ActorContext, _msg: &Message) -> Result<Value, BehaviorError> {
        let key_count = self.data.values().filter(|e| !is_expired(e)).count();
        Ok(json!({
            "role": self.role,
            "shard_id": self.shard_id,
            "replication_offset": self.replication_offset,
            "keys": key_count,
        }))
    }

    // -------------------------------------------------------------------------
    // Replication handlers (Ch7-8)
    // -------------------------------------------------------------------------

    /// REPLICATE — apply a write event from the master.
    /// Used by broadcast_shard_group for replication propagation.
    #[handler("replicate")]
    async fn handle_replicate(&mut self, _ctx: &ActorContext, msg: &Message) -> Result<Value, BehaviorError> {
        #[derive(Deserialize)]
        struct ReplicatePayload {
            command: String,
            #[serde(default)] key: String,
            #[serde(default)] value: String,
            #[serde(default)] expires_at_ms: Option<u64>,
            offset: i64,
        }
        let p: ReplicatePayload = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("bad replicate payload: {}", e)))?;

        match p.command.to_uppercase().as_str() {
            "SET" => {
                self.data.insert(p.key, StoredEntry { value: p.value, expires_at_ms: p.expires_at_ms });
            }
            "DEL" => { self.data.remove(&p.key); }
            "INCR" => {
                let new_val: i64 = self.data.get(&p.key)
                    .and_then(|e| e.value.parse().ok())
                    .unwrap_or(0) + 1;
                self.data.insert(p.key, StoredEntry { value: new_val.to_string(), expires_at_ms: None });
            }
            _ => {}
        }
        self.replication_offset = p.offset;
        Ok(json!({ "result": "OK", "offset": self.replication_offset }))
    }

    /// GET_ACK — return current replication offset for WAIT scatter_gather (Ch8).
    #[handler("get_ack")]
    async fn handle_get_ack(&mut self, _ctx: &ActorContext, _msg: &Message) -> Result<Value, BehaviorError> {
        Ok(json!({ "offset": self.replication_offset, "shard_id": self.shard_id }))
    }

    /// HANDSHAKE — replication handshake protocol: PING → REPLCONF → PSYNC (Ch7).
    #[handler("handshake")]
    async fn handle_handshake(&mut self, _ctx: &ActorContext, msg: &Message) -> Result<Value, BehaviorError> {
        #[derive(Deserialize)]
        struct HandshakePayload {
            step: String,
            #[serde(default)] args: Vec<String>,
        }
        let p: HandshakePayload = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("bad handshake payload: {}", e)))?;

        let response = match p.step.to_lowercase().as_str() {
            "ping" => "PONG".to_string(),
            "replconf" => "OK".to_string(),
            "psync" => format!("FULLRESYNC {} 0", self.shard_id),
            _ => "ERR unknown handshake step".to_string(),
        };
        let _ = p.args; // acknowledged
        Ok(json!({ "status": response }))
    }

    /// BULK_SYNC — receive full state transfer from master (RDB equivalent, Ch7).
    #[handler("bulk_sync")]
    async fn handle_bulk_sync(&mut self, _ctx: &ActorContext, msg: &Message) -> Result<Value, BehaviorError> {
        #[derive(Deserialize)]
        struct BulkSyncPayload {
            data: HashMap<String, StoredEntry>,
            offset: i64,
        }
        let p: BulkSyncPayload = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("bad bulk_sync payload: {}", e)))?;

        self.data = p.data;
        self.replication_offset = p.offset;
        self.role = "replica".to_string();
        Ok(json!({ "status": "OK", "keys_synced": self.data.len() }))
    }

    // -------------------------------------------------------------------------
    // Parallel primitive handlers
    // -------------------------------------------------------------------------

    /// EXPIRE_SWEEP — active expiry: scan and remove expired keys (triggered by broadcast).
    #[handler("expire_sweep")]
    async fn handle_expire_sweep(&mut self, _ctx: &ActorContext, _msg: &Message) -> Result<Value, BehaviorError> {
        let expired: Vec<String> = self.data
            .iter()
            .filter(|(_, e)| is_expired(e))
            .map(|(k, _)| k.clone())
            .collect();
        let count = expired.len();
        for key in &expired {
            self.data.remove(key);
        }
        Ok(json!({ "result": "OK", "swept": count }))
    }

    /// SNAPSHOT — return full data state for barrier-coordinated cluster snapshot.
    #[handler("snapshot")]
    async fn handle_snapshot(&mut self, _ctx: &ActorContext, _msg: &Message) -> Result<Value, BehaviorError> {
        let live: HashMap<&String, &StoredEntry> = self.data
            .iter()
            .filter(|(_, e)| !is_expired(e))
            .collect();
        Ok(json!({
            "shard_id": self.shard_id,
            "role": self.role,
            "replication_offset": self.replication_offset,
            "data": live,
        }))
    }
}
