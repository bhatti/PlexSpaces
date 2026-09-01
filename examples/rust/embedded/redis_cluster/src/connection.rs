// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! ConnectionActor — Orleans-style virtual actor for per-client state.
//!
//! Each client gets its own ConnectionActor that auto-activates on first
//! message and auto-deactivates on idle timeout. Transaction state
//! (MULTI/EXEC command queue) lives entirely in this actor — no locks,
//! no shared mutable state across clients.
//!
//! Framework primitives demonstrated:
//! - `#[gen_server_actor(facets = ["virtual_actor"])]` — auto-lifecycle management
//! - Actor-local transaction queue — safe because actor processes one message at a time

use plexspaces_sdk::{
    gen_server_actor, plexspaces_handlers,
    ActorContext, BehaviorError, Message,
    json, Value,
};
use serde::Deserialize;

// =============================================================================
// ConnectionActor — Virtual Actor per Client
// =============================================================================

/// Per-client connection actor with transaction queue.
///
/// ## Virtual Actor Pattern
/// - One actor per client_id (spawned on first command)
/// - Auto-deactivates after idle timeout (no explicit lifecycle management needed)
/// - Transaction state dies with deactivation → matches Redis behaviour on disconnect
///
/// ## Concurrency Safety
/// Because actors process exactly one message at a time, the transaction queue
/// (`command_queue`) never needs a mutex — the actor model IS the lock.
#[gen_server_actor(facets = ["virtual_actor"])]
pub struct ConnectionActor {
    pub client_id: String,
    pub in_transaction: bool,
    /// Queued commands: (command_name, args) pairs accumulated between MULTI and EXEC.
    pub command_queue: Vec<(String, Vec<String>)>,
}

impl ConnectionActor {
    pub fn new(client_id: impl Into<String>) -> Self {
        Self {
            client_id: client_id.into(),
            in_transaction: false,
            command_queue: Vec::new(),
        }
    }
}

// =============================================================================
// Transaction Command Handler
// =============================================================================

#[plexspaces_handlers(gen_server)]
impl ConnectionActor {
    /// Execute a Redis command through this connection.
    ///
    /// Handles MULTI / EXEC / DISCARD directly.  When `in_transaction` is true,
    /// all other commands are queued and returned "QUEUED".  Outside a transaction,
    /// the command is described as pending work for the cluster layer (returned
    /// to the caller so that `RedisCluster::execute` can dispatch it to the
    /// correct shard group via `bulk_update` / `map`).
    #[handler("execute")]
    async fn handle_execute(&mut self, _ctx: &ActorContext, msg: &Message) -> Result<Value, BehaviorError> {
        #[derive(Deserialize)]
        struct ExecutePayload {
            command: String,
            #[serde(default)] args: Vec<String>,
        }
        let p: ExecutePayload = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("bad execute payload: {}", e)))?;

        let cmd = p.command.to_uppercase();

        match cmd.as_str() {
            "MULTI" => {
                if self.in_transaction {
                    return Ok(json!({ "result": null, "error": "ERR MULTI calls can not be nested" }));
                }
                self.in_transaction = true;
                self.command_queue.clear();
                Ok(json!({ "result": "OK", "error": null }))
            }

            "DISCARD" => {
                if !self.in_transaction {
                    return Ok(json!({ "result": null, "error": "ERR DISCARD without MULTI" }));
                }
                self.in_transaction = false;
                self.command_queue.clear();
                Ok(json!({ "result": "OK", "error": null }))
            }

            "EXEC" => {
                if !self.in_transaction {
                    return Ok(json!({ "result": null, "error": "ERR EXEC without MULTI" }));
                }
                let queued = std::mem::take(&mut self.command_queue);
                self.in_transaction = false;
                // Return the queued commands to the caller (RedisCluster) for dispatch.
                // The cluster layer will execute each command against the shard group
                // and return results as an array.
                Ok(json!({
                    "result": "EXEC",
                    "queued": queued.iter().map(|(cmd, args)| {
                        json!({ "command": cmd, "args": args })
                    }).collect::<Vec<_>>(),
                    "error": null
                }))
            }

            _ => {
                if self.in_transaction {
                    // Queue the command and return QUEUED
                    self.command_queue.push((cmd.to_string(), p.args));
                    Ok(json!({ "result": "QUEUED", "error": null }))
                } else {
                    // Pass-through — caller (RedisCluster) dispatches against shard group
                    Ok(json!({
                        "result": "PASSTHROUGH",
                        "command": cmd,
                        "args": p.args,
                        "error": null
                    }))
                }
            }
        }
    }
}
