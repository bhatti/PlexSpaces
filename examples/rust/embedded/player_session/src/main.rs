// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Player Session Manager - Orleans-style Virtual Actor with Annotations
//
// Real-world use case: Game server managing millions of player sessions
// with automatic activation/deactivation and state persistence.
//
// ## Key Demonstration: Annotation-Based Facets
// This example shows how to use SDK annotations to declare facets:
// - `#[gen_server_actor(facets = ["virtual_actor"])]` - Non-durable virtual actor
// - `#[gen_server_actor(facets = ["virtual_actor", "durability"])]` - Durable virtual actor
//
// ## Virtual Actor Pattern (Orleans)
// - One actor per player (millions of actors possible)
// - Auto-activate on first message
// - Auto-deactivate after idle timeout
// - State persisted on deactivation, restored on activation (with durability facet)
//
// ## SDK Features Demonstrated
// - `#[gen_server_actor(facets = [...])]` - Declare facets via annotation
// - `#[handler("op")]` - Message handlers
// - `create_facets()` / `create_facets_with_storage()` - Create facets from FACETS const
// - Tenant isolation via RequestContext

use plexspaces_sdk::{
    gen_server_actor, plexspaces_handlers,
    ActorContext, BehaviorError, RequestContext, Message,
    NodeBuilder, ActorRef,
    // Simplified spawn functions - facets come from annotations!
    spawn, spawn_with_storage,
    call_message,
    // Storage for durable actors
    JournalStorage,
    json, Value,
};
use plexspaces_journaling::SqliteJournalStorage;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tracing::Level;
use chrono::{DateTime, Utc};

// Required for macro-generated code
extern crate plexspaces_core;
extern crate plexspaces_behavior;

// =============================================================================
// Domain Types - Player Session
// =============================================================================

/// Player position in game world
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Position {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub world: String,
}

/// Item in player inventory
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryItem {
    pub item_id: String,
    pub name: String,
    pub quantity: u32,
    pub rarity: String,
}

/// Player statistics
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlayerStats {
    pub level: u32,
    pub experience: u64,
    pub health: u32,
    pub max_health: u32,
    pub gold: u64,
    pub kills: u32,
    pub deaths: u32,
    pub playtime_minutes: u64,
}

/// Complete player state (persisted)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlayerState {
    pub player_id: String,
    pub username: String,
    pub position: Position,
    pub stats: PlayerStats,
    pub inventory: Vec<InventoryItem>,
    pub achievements: Vec<String>,
    pub last_login: Option<DateTime<Utc>>,
    pub last_activity: Option<DateTime<Utc>>,
    pub session_start: Option<DateTime<Utc>>,
    pub is_online: bool,
}

// =============================================================================
// ACTOR 1: VirtualPlayerSession (Non-Durable)
// =============================================================================
// 
// Uses annotation: #[gen_server_actor(facets = ["virtual_actor"])]
// 
// This actor demonstrates:
// - Orleans-style virtual actor lifecycle (auto-activate/deactivate)
// - State is IN-MEMORY ONLY - lost when actor terminates
// - Good for: Caching, temporary sessions, real-time data
// =============================================================================

/// Non-durable virtual player session.
/// 
/// ## Facets (via annotation)
/// - `virtual_actor`: Orleans-style activation/deactivation
/// 
/// ## State Persistence
/// - State is IN-MEMORY ONLY
/// - State is LOST when actor terminates
/// - Use for temporary/cacheable data
#[gen_server_actor(facets = ["virtual_actor"])]
struct VirtualPlayerSession {
    state: PlayerState,
    activation_count: u32,
}

impl VirtualPlayerSession {
    fn new(player_id: &str) -> Self {
        Self {
            state: PlayerState {
                player_id: player_id.to_string(),
                stats: PlayerStats {
                    level: 1,
                    health: 100,
                    max_health: 100,
                    ..Default::default()
                },
                ..Default::default()
            },
            activation_count: 0,
        }
    }
    
    fn update_activity(&mut self) {
        self.state.last_activity = Some(Utc::now());
    }
}

#[plexspaces_handlers(gen_server)]
impl VirtualPlayerSession {
    #[handler("login")]
    async fn handle_login(&mut self, _ctx: &ActorContext, msg: &Message) -> Result<Value, BehaviorError> {
        #[derive(Deserialize)]
        struct LoginRequest {
            username: String,
            #[serde(default)]
            position: Option<Position>,
        }
        
        let req: LoginRequest = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("Invalid request: {}", e)))?;
        
        self.activation_count += 1;
        self.state.session_start = Some(Utc::now());
        self.state.username = req.username.clone();
        self.state.is_online = true;
        self.state.last_login = Some(Utc::now());
        
        if let Some(pos) = req.position {
            self.state.position = pos;
        }
        
        self.update_activity();
        
        println!("  ✓ [Virtual] Player {} logged in (activation #{})", req.username, self.activation_count);
        
        Ok(json!({
            "status": "logged_in",
            "player_id": self.state.player_id,
            "username": self.state.username,
            "activation_count": self.activation_count
        }))
    }
    
    #[handler("add_item")]
    async fn handle_add_item(&mut self, _ctx: &ActorContext, msg: &Message) -> Result<Value, BehaviorError> {
        let item: InventoryItem = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("Invalid item: {}", e)))?;
        
        if let Some(existing) = self.state.inventory.iter_mut().find(|i| i.item_id == item.item_id) {
            existing.quantity += item.quantity;
            println!("  + [Virtual] Stacked {} x{}", item.name, item.quantity);
        } else {
            println!("  + [Virtual] Added {} x{}", item.name, item.quantity);
            self.state.inventory.push(item.clone());
        }
        
        self.update_activity();
        
        Ok(json!({
            "status": "item_added",
            "item": item,
            "inventory_size": self.state.inventory.len()
        }))
    }
    
    #[handler("update_stats")]
    async fn handle_update_stats(&mut self, _ctx: &ActorContext, msg: &Message) -> Result<Value, BehaviorError> {
        #[derive(Deserialize)]
        struct StatsUpdate {
            #[serde(default)]
            add_xp: Option<u64>,
            #[serde(default)]
            add_gold: Option<u64>,
        }
        
        let update: StatsUpdate = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("Invalid update: {}", e)))?;
        
        if let Some(xp) = update.add_xp {
            self.state.stats.experience += xp;
            println!("  📊 [Virtual] +{} XP", xp);
        }
        
        if let Some(gold) = update.add_gold {
            self.state.stats.gold += gold;
            println!("  📊 [Virtual] +{} gold", gold);
        }
        
        self.update_activity();
        
        Ok(json!({
            "status": "stats_updated",
            "stats": self.state.stats
        }))
    }
    
    #[handler("get_state")]
    async fn handle_get_state(&mut self, _ctx: &ActorContext, _msg: &Message) -> Result<Value, BehaviorError> {
        self.update_activity();
        
        Ok(json!({
            "player": self.state,
            "activation_count": self.activation_count
        }))
    }
}

// =============================================================================
// ACTOR 2: DurablePlayerSession (With Durability)
// =============================================================================
// 
// Uses annotation: #[gen_server_actor(facets = ["virtual_actor", "durability"])]
// 
// This actor demonstrates:
// - Orleans-style virtual actor lifecycle (auto-activate/deactivate)
// - State is PERSISTED via DurabilityFacet journaling
// - State survives actor termination and is restored on reactivation
// - Good for: Player data, shopping carts, bank accounts
// =============================================================================

/// Durable virtual player session with state persistence.
/// 
/// ## Facets (via annotation)
/// - `virtual_actor`: Orleans-style activation/deactivation
/// - `durability`: State persistence via journaling
/// 
/// ## State Persistence
/// - State is JOURNALED on every change
/// - State SURVIVES actor termination
/// - State is RESTORED on reactivation
/// - Use for critical/persistent data
#[gen_server_actor(facets = ["virtual_actor", "durability"])]
struct DurablePlayerSession {
    state: PlayerState,
    activation_count: u32,
}

impl DurablePlayerSession {
    fn new(player_id: &str) -> Self {
        Self {
            state: PlayerState {
                player_id: player_id.to_string(),
                stats: PlayerStats {
                    level: 1,
                    health: 100,
                    max_health: 100,
                    ..Default::default()
                },
                ..Default::default()
            },
            activation_count: 0,
        }
    }
    
    fn update_activity(&mut self) {
        self.state.last_activity = Some(Utc::now());
    }
}

#[plexspaces_handlers(gen_server)]
impl DurablePlayerSession {
    #[handler("login")]
    async fn handle_login(&mut self, _ctx: &ActorContext, msg: &Message) -> Result<Value, BehaviorError> {
        #[derive(Deserialize)]
        struct LoginRequest {
            username: String,
            #[serde(default)]
            position: Option<Position>,
        }
        
        let req: LoginRequest = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("Invalid request: {}", e)))?;
        
        self.activation_count += 1;
        self.state.session_start = Some(Utc::now());
        self.state.username = req.username.clone();
        self.state.is_online = true;
        self.state.last_login = Some(Utc::now());
        
        if let Some(pos) = req.position {
            self.state.position = pos;
        }
        
        self.update_activity();
        
        println!("  ✓ [Durable] Player {} logged in (activation #{})", req.username, self.activation_count);
        
        Ok(json!({
            "status": "logged_in",
            "player_id": self.state.player_id,
            "username": self.state.username,
            "activation_count": self.activation_count
        }))
    }
    
    #[handler("add_item")]
    async fn handle_add_item(&mut self, _ctx: &ActorContext, msg: &Message) -> Result<Value, BehaviorError> {
        let item: InventoryItem = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("Invalid item: {}", e)))?;
        
        if let Some(existing) = self.state.inventory.iter_mut().find(|i| i.item_id == item.item_id) {
            existing.quantity += item.quantity;
            println!("  + [Durable] Stacked {} x{}", item.name, item.quantity);
        } else {
            println!("  + [Durable] Added {} x{}", item.name, item.quantity);
            self.state.inventory.push(item.clone());
        }
        
        self.update_activity();
        
        Ok(json!({
            "status": "item_added",
            "item": item,
            "inventory_size": self.state.inventory.len()
        }))
    }
    
    #[handler("update_stats")]
    async fn handle_update_stats(&mut self, _ctx: &ActorContext, msg: &Message) -> Result<Value, BehaviorError> {
        #[derive(Deserialize)]
        struct StatsUpdate {
            #[serde(default)]
            add_xp: Option<u64>,
            #[serde(default)]
            add_gold: Option<u64>,
        }
        
        let update: StatsUpdate = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("Invalid update: {}", e)))?;
        
        if let Some(xp) = update.add_xp {
            self.state.stats.experience += xp;
            println!("  📊 [Durable] +{} XP (JOURNALED)", xp);
        }
        
        if let Some(gold) = update.add_gold {
            self.state.stats.gold += gold;
            println!("  📊 [Durable] +{} gold (JOURNALED)", gold);
        }
        
        self.update_activity();
        
        Ok(json!({
            "status": "stats_updated",
            "stats": self.state.stats
        }))
    }
    
    #[handler("get_state")]
    async fn handle_get_state(&mut self, _ctx: &ActorContext, _msg: &Message) -> Result<Value, BehaviorError> {
        self.update_activity();
        
        Ok(json!({
            "player": self.state,
            "activation_count": self.activation_count
        }))
    }
}

// =============================================================================
// Helper function to send messages
// =============================================================================

async fn send_call(actor_ref: &ActorRef, op: &str, payload: Value) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
    let mut merged = payload.as_object().cloned().unwrap_or_default();
    merged.insert("op".to_string(), json!(op));
    
    let msg = call_message(serde_json::Value::Object(merged));
    
    let response = actor_ref.ask(msg, Duration::from_secs(5)).await?;
    let result: Value = serde_json::from_slice(&response.payload)?;
    Ok(result)
}

// =============================================================================
// Main - Demonstrates annotation-based facets
// =============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .with_env_filter("player_session=info,plexspaces=warn")
        .init();

    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║   Player Session - Annotation-Based Virtual Actor Facets       ║");
    println!("╚════════════════════════════════════════════════════════════════╝");
    println!();
    println!("This example demonstrates SDK annotations for facets:");
    println!("  • VirtualPlayerSession:  #[gen_server_actor(facets = [\"virtual_actor\"])]");
    println!("  • DurablePlayerSession:  #[gen_server_actor(facets = [\"virtual_actor\", \"durability\"])]");
    println!();

    // =========================================================================
    // Step 1: Create Node
    // =========================================================================
    println!("Step 1: Create PlexSpaces Node");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let node = NodeBuilder::new("game-server")
        .with_clustering_enabled(false)
        .build().await;
    let node = Arc::new(node);
    
    let node_for_start = node.clone();
    tokio::spawn(async move {
        let _ = node_for_start.start().await;
    });
    tokio::time::sleep(Duration::from_millis(200)).await;
    
    println!("  ✓ Node 'game-server' created");
    println!();

    // Create RequestContext for tenant isolation
    let ctx = RequestContext::new_without_auth(
        "game-world".to_string(),  // tenant_id (from JWT in production)
        "players".to_string(),     // namespace
    );

    // =========================================================================
    // PART A: Non-Durable Virtual Actor (using annotation-declared facets)
    // =========================================================================
    println!("═══════════════════════════════════════════════════════════════════");
    println!("PART A: VirtualPlayerSession (facets = [\"virtual_actor\"])");
    println!("═══════════════════════════════════════════════════════════════════");
    println!();
    println!("Actor declares: #[gen_server_actor(facets = [\"virtual_actor\"])]");
    println!("This generates: VirtualPlayerSession::FACETS = &[\"virtual_actor\"]");
    println!();
    
    // Show the FACETS constant generated by the macro
    println!("Step A.1: Check FACETS constant (generated by macro)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  VirtualPlayerSession::FACETS = {:?}", VirtualPlayerSession::FACETS);
    println!();
    
    // Spawn actor - facets are automatically created from annotation!
    println!("Step A.2: Spawn VirtualPlayerSession (facets from annotation)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    let virtual_player: ActorRef = spawn(
        &ctx,
        node.service_locator(),
        "player-virtual@game-server",
        "players",
        VirtualPlayerSession::new("player-virtual"),
    ).await?;
    println!("  ✓ Spawned: {}", virtual_player.id());
    println!("  Note: State is IN-MEMORY ONLY (no durability facet)");
    println!();
    
    // Perform actions
    println!("Step A.3: Player actions (login, add items, update stats)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    send_call(&virtual_player, "login", json!({
        "username": "VirtualPlayer",
        "position": { "x": 100.0, "y": 50.0, "z": 100.0, "world": "main" }
    })).await?;
    
    send_call(&virtual_player, "add_item", json!({
        "item_id": "sword-001",
        "name": "Iron Sword",
        "quantity": 1,
        "rarity": "common"
    })).await?;
    
    send_call(&virtual_player, "update_stats", json!({
        "add_xp": 500,
        "add_gold": 100
    })).await?;
    
    let state_before = send_call(&virtual_player, "get_state", json!({})).await?;
    println!();
    println!("  State BEFORE termination:");
    println!("    Gold: {}", state_before["player"]["stats"]["gold"]);
    println!("    Inventory: {} items", state_before["player"]["inventory"].as_array().map(|a| a.len()).unwrap_or(0));
    println!();
    
    // Terminate actor
    println!("Step A.4: Terminate actor (state will be LOST)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    let actor_factory = node.service_locator().get_actor_factory().await
        .ok_or("Actor factory not available")?;
    actor_factory.stop_actor(&ctx, &"player-virtual@game-server".to_string()).await?;
    println!("  ✓ Actor terminated");
    println!("  ⚠️  State is LOST (no durability facet)");
    println!();
    
    tokio::time::sleep(Duration::from_millis(100)).await;
    
    // Re-spawn and verify state is lost
    println!("Step A.5: Re-spawn actor (verify state is LOST)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    let virtual_player2: ActorRef = spawn(
        &ctx,
        node.service_locator(),
        "player-virtual@game-server",
        "players",
        VirtualPlayerSession::new("player-virtual"),
    ).await?;
    
    send_call(&virtual_player2, "login", json!({
        "username": "VirtualPlayer"
    })).await?;
    
    let state_after = send_call(&virtual_player2, "get_state", json!({})).await?;
    let gold_after = state_after["player"]["stats"]["gold"].as_u64().unwrap_or(0);
    let inventory_after = state_after["player"]["inventory"].as_array().map(|a| a.len()).unwrap_or(0);
    
    println!();
    println!("  State AFTER re-spawn:");
    println!("    Gold: {} (was 100)", gold_after);
    println!("    Inventory: {} items (was 1)", inventory_after);
    
    if gold_after == 0 && inventory_after == 0 {
        println!();
        println!("  ✅ VERIFIED: State was LOST (as expected for non-durable actor)");
    }
    
    actor_factory.stop_actor(&ctx, &"player-virtual@game-server".to_string()).await?;
    println!();

    // =========================================================================
    // PART B: Durable Virtual Actor (using annotation-declared facets)
    // =========================================================================
    println!("═══════════════════════════════════════════════════════════════════");
    println!("PART B: DurablePlayerSession (facets = [\"virtual_actor\", \"durability\"])");
    println!("═══════════════════════════════════════════════════════════════════");
    println!();
    println!("Actor declares: #[gen_server_actor(facets = [\"virtual_actor\", \"durability\"])]");
    println!("This generates: DurablePlayerSession::FACETS = &[\"virtual_actor\", \"durability\"]");
    println!();
    
    // Show the FACETS constant
    println!("Step B.1: Check FACETS constant (generated by macro)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  DurablePlayerSession::FACETS = {:?}", DurablePlayerSession::FACETS);
    println!();
    
    // Create storage for durability facet
    println!("Step B.2: Create storage backend for DurabilityFacet");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    let storage: Arc<dyn JournalStorage> = Arc::new(SqliteJournalStorage::new(":memory:").await?);
    println!("  ✓ Created in-memory SQLite journal storage");
    println!();
    
    // Spawn durable actor - facets from annotation, storage provided separately
    println!("Step B.3: Spawn DurablePlayerSession (facets from annotation + storage)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    let durable_player: ActorRef = spawn_with_storage(
        &ctx,
        node.service_locator(),
        "player-durable@game-server",
        "players",
        DurablePlayerSession::new("player-durable"),
        storage.clone(),
    ).await?;
    println!("  ✓ Spawned: {}", durable_player.id());
    println!("  Note: State is JOURNALED (durability facet attached)");
    println!();
    
    // Perform actions
    println!("Step B.4: Player actions (login, add items, update stats)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    send_call(&durable_player, "login", json!({
        "username": "DurablePlayer",
        "position": { "x": 200.0, "y": 100.0, "z": 200.0, "world": "main" }
    })).await?;
    
    send_call(&durable_player, "add_item", json!({
        "item_id": "shield-001",
        "name": "Steel Shield",
        "quantity": 1,
        "rarity": "rare"
    })).await?;
    
    send_call(&durable_player, "update_stats", json!({
        "add_xp": 1000,
        "add_gold": 500
    })).await?;
    
    let durable_state = send_call(&durable_player, "get_state", json!({})).await?;
    println!();
    println!("  State (JOURNALED):");
    println!("    Gold: {}", durable_state["player"]["stats"]["gold"]);
    println!("    Inventory: {} items", durable_state["player"]["inventory"].as_array().map(|a| a.len()).unwrap_or(0));
    println!();
    
    // Terminate actor
    println!("Step B.5: Terminate actor (state is PERSISTED)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    actor_factory.stop_actor(&ctx, &"player-durable@game-server".to_string()).await?;
    println!("  ✓ Actor terminated");
    println!("  ✅ State is PERSISTED (durability facet journals all changes)");
    println!();

    // =========================================================================
    // Summary
    // =========================================================================
    println!("═══════════════════════════════════════════════════════════════════");
    println!("✅ Example Complete!");
    println!("═══════════════════════════════════════════════════════════════════");
    println!();
    println!("Key Takeaways:");
    println!();
    println!("1. ANNOTATION-BASED FACETS (Zero Boilerplate):");
    println!("   • Declare: #[gen_server_actor(facets = [\"virtual_actor\"])]");
    println!("   • Spawn:   spawn(&ctx, svc, id, ns, MyActor::new()).await?");
    println!("   • Facets are automatically created from annotation!");
    println!();
    println!("2. VIRTUAL ACTOR:");
    println!("   • spawn() - facets from annotation, no storage needed");
    println!("   • State is IN-MEMORY ONLY");
    println!();
    println!("3. DURABLE VIRTUAL ACTOR:");
    println!("   • spawn_with_storage() - facets from annotation + storage");
    println!("   • State is JOURNALED and PERSISTED");
    println!();
    println!("4. TENANT ISOLATION:");
    println!("   • RequestContext carries tenant_id (from JWT) and namespace");
    println!("   • ActorFactory enforces tenant/namespace match on all operations");
    println!();

    // Graceful shutdown
    node.shutdown(Duration::from_secs(5)).await?;
    
    Ok(())
}
