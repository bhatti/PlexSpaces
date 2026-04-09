// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Comprehensive tests for PlexSpaces SDK macros.
//
// These tests verify that all SDK annotations generate correct code
// and integrate properly with the behavior system.

use plexspaces_core::{Actor, BehaviorType};
use plexspaces_sdk::{
    actor, event_actor, fsm_actor, gen_server_actor, handler, init_handler, json,
    plexspaces_handlers, query_handler, run_handler, signal_handler,
    simple_actor::ActorWorldHandlers, workflow_actor, ActorContext, BehaviorError, Message, Value,
};

// ============================================================================
// Test: #[gen_server_actor] generates correct impl Actor
// ============================================================================

#[gen_server_actor]
struct TestGenServerActor {
    counter: i32,
}

impl TestGenServerActor {
    fn new() -> Self {
        Self { counter: 0 }
    }
}

#[plexspaces_handlers]
impl TestGenServerActor {
    #[handler("increment")]
    async fn increment(
        &mut self,
        _ctx: &ActorContext,
        _msg: &Message,
    ) -> Result<Value, BehaviorError> {
        self.counter += 1;
        Ok(json!({ "counter": self.counter }))
    }

    #[handler("get")]
    async fn get(&mut self, _ctx: &ActorContext, _msg: &Message) -> Result<Value, BehaviorError> {
        Ok(json!({ "counter": self.counter }))
    }
}

#[test]
fn test_gen_server_actor_behavior_type() {
    let actor = TestGenServerActor::new();
    assert_eq!(actor.behavior_type(), BehaviorType::GenServer);
}

#[test]
fn test_gen_server_actor_facets() {
    assert_eq!(TestGenServerActor::FACETS, &[] as &[&str]);
}

// ============================================================================
// Test: #[gen_server_actor(facets = [...])] generates FACETS const
// ============================================================================

#[gen_server_actor(facets = ["timer", "durability"])]
struct TestGenServerWithFacets {
    data: String,
}

#[plexspaces_handlers]
impl TestGenServerWithFacets {
    #[handler("get")]
    async fn get(&mut self, _ctx: &ActorContext, _msg: &Message) -> Result<Value, BehaviorError> {
        Ok(json!({ "data": self.data }))
    }
}

#[test]
fn test_gen_server_with_facets() {
    assert_eq!(TestGenServerWithFacets::FACETS, &["timer", "durability"]);
}

// ============================================================================
// Test: #[gen_server_actor(name = "custom")] generates Custom behavior type
// ============================================================================

#[gen_server_actor(name = "custom_webhook")]
struct TestCustomNamedActor {
    value: i32,
}

#[plexspaces_handlers]
impl TestCustomNamedActor {
    #[handler("get")]
    async fn get(&mut self, _ctx: &ActorContext, _msg: &Message) -> Result<Value, BehaviorError> {
        Ok(json!({ "value": self.value }))
    }
}

#[test]
fn test_custom_named_actor_behavior_type() {
    let actor = TestCustomNamedActor { value: 0 };
    match actor.behavior_type() {
        BehaviorType::Custom(name) => assert_eq!(name, "custom_webhook"),
        _ => panic!("Expected Custom behavior type"),
    }
}

// ============================================================================
// Test: #[actor] generates FACETS const only
// ============================================================================

#[actor]
struct TestBasicActor {
    state: String,
}

#[test]
fn test_basic_actor_facets() {
    assert_eq!(TestBasicActor::FACETS, &[] as &[&str]);
}

#[actor(facets = ["registry", "lock"])]
struct TestActorWithFacets {
    state: String,
}

#[test]
fn test_actor_with_facets() {
    assert_eq!(TestActorWithFacets::FACETS, &["registry", "lock"]);
}

// ============================================================================
// Test: #[event_actor] generates GenEvent behavior type
// ============================================================================

#[event_actor]
struct TestEventActor {
    events: Vec<String>,
}

#[plexspaces_handlers(event)]
impl TestEventActor {
    #[handler("log", cast)]
    async fn log(&mut self, _ctx: &ActorContext, msg: &Message) -> Result<(), BehaviorError> {
        let payload = String::from_utf8_lossy(&msg.payload);
        self.events.push(payload.to_string());
        Ok(())
    }
}

#[test]
fn test_event_actor_behavior_type() {
    let actor = TestEventActor { events: vec![] };
    assert_eq!(actor.behavior_type(), BehaviorType::GenEvent);
}

#[event_actor(facets = ["logging"])]
struct TestEventActorWithFacets {
    events: Vec<String>,
}

#[plexspaces_handlers(event)]
impl TestEventActorWithFacets {
    #[handler("log", cast)]
    async fn log(&mut self, _ctx: &ActorContext, msg: &Message) -> Result<(), BehaviorError> {
        let payload = String::from_utf8_lossy(&msg.payload);
        self.events.push(payload.to_string());
        Ok(())
    }
}

#[test]
fn test_event_actor_with_facets() {
    assert_eq!(TestEventActorWithFacets::FACETS, &["logging"]);
}

// ============================================================================
// Test: #[fsm_actor] generates GenStateMachine behavior type
// ============================================================================

#[fsm_actor]
struct TestFsmActor {
    state: String,
}

#[plexspaces_handlers(fsm)]
impl TestFsmActor {
    #[handler("transition")]
    async fn transition(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<Value, BehaviorError> {
        let payload: Value = serde_json::from_slice(&msg.payload).unwrap_or(json!({}));
        if let Some(new_state) = payload["state"].as_str() {
            self.state = new_state.to_string();
        }
        Ok(json!({ "state": self.state }))
    }
}

#[test]
fn test_fsm_actor_behavior_type() {
    let actor = TestFsmActor {
        state: "idle".to_string(),
    };
    assert_eq!(actor.behavior_type(), BehaviorType::GenStateMachine);
}

#[fsm_actor(facets = ["durability"])]
struct TestFsmActorWithFacets {
    state: String,
}

#[plexspaces_handlers(fsm)]
impl TestFsmActorWithFacets {
    #[handler("transition")]
    async fn transition(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<Value, BehaviorError> {
        let payload: Value = serde_json::from_slice(&msg.payload).unwrap_or(json!({}));
        if let Some(new_state) = payload["state"].as_str() {
            self.state = new_state.to_string();
        }
        Ok(json!({ "state": self.state }))
    }
}

#[test]
fn test_fsm_actor_with_facets() {
    assert_eq!(TestFsmActorWithFacets::FACETS, &["durability"]);
}

// ============================================================================
// Test: #[workflow_actor] generates Workflow behavior type
// ============================================================================

#[workflow_actor]
struct TestWorkflowActor {
    status: String,
}

#[plexspaces_handlers(workflow)]
impl TestWorkflowActor {
    #[run_handler]
    async fn run(&mut self, _ctx: &ActorContext, input: Message) -> Result<Message, BehaviorError> {
        let payload: Value = serde_json::from_slice(&input.payload).unwrap_or(json!({}));
        self.status = payload["status"].as_str().unwrap_or("running").to_string();
        Ok(Message {
            payload: serde_json::to_vec(&json!({ "status": self.status })).unwrap_or_default(),
            ..Default::default()
        })
    }

    #[signal_handler("cancel")]
    async fn on_cancel(
        &mut self,
        _ctx: &ActorContext,
        _data: Message,
    ) -> Result<(), BehaviorError> {
        self.status = "cancelled".to_string();
        Ok(())
    }

    #[query_handler("status")]
    async fn get_status(
        &self,
        _ctx: &ActorContext,
        _params: Message,
    ) -> Result<Message, BehaviorError> {
        Ok(Message {
            payload: serde_json::to_vec(&json!({ "status": self.status })).unwrap_or_default(),
            ..Default::default()
        })
    }
}

#[test]
fn test_workflow_actor_behavior_type() {
    let actor = TestWorkflowActor {
        status: "pending".to_string(),
    };
    assert_eq!(actor.behavior_type(), BehaviorType::Workflow);
}

#[workflow_actor(facets = ["durability", "event_sourcing"])]
struct TestWorkflowActorWithFacets {
    status: String,
}

#[plexspaces_handlers(workflow)]
impl TestWorkflowActorWithFacets {
    #[run_handler]
    async fn run(
        &mut self,
        _ctx: &ActorContext,
        _input: Message,
    ) -> Result<Message, BehaviorError> {
        self.status = "running".to_string();
        Ok(Message::default())
    }

    #[signal_handler("stop")]
    async fn on_stop(&mut self, _ctx: &ActorContext, _data: Message) -> Result<(), BehaviorError> {
        self.status = "stopped".to_string();
        Ok(())
    }

    #[query_handler("status")]
    async fn get_status(
        &self,
        _ctx: &ActorContext,
        _params: Message,
    ) -> Result<Message, BehaviorError> {
        Ok(Message::default())
    }
}

#[test]
fn test_workflow_actor_with_facets() {
    assert_eq!(
        TestWorkflowActorWithFacets::FACETS,
        &["durability", "event_sourcing"]
    );
}

// ============================================================================
// Test: Handler with cast semantics
// ============================================================================

#[actor]
struct TestCastActor {
    received: Vec<String>,
}

#[plexspaces_handlers(custom)]
impl TestCastActor {
    #[handler("log", cast)]
    async fn log(&mut self, _ctx: &ActorContext, msg: &Message) -> Result<(), BehaviorError> {
        let payload = String::from_utf8_lossy(&msg.payload);
        self.received.push(payload.to_string());
        Ok(())
    }
}

#[test]
fn test_cast_actor_behavior_type() {
    let actor = TestCastActor { received: vec![] };
    // Custom actor generates Custom behavior type
    match actor.behavior_type() {
        BehaviorType::Custom(name) => assert_eq!(name, "TestCastActor"),
        _ => panic!("Expected Custom behavior type"),
    }
}

// ============================================================================
// Test: Multiple handlers in one impl block
// ============================================================================

#[gen_server_actor]
struct TestMultiHandlerActor {
    balance: i64,
}

#[plexspaces_handlers]
impl TestMultiHandlerActor {
    #[handler("deposit")]
    async fn deposit(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<Value, BehaviorError> {
        let payload: Value = serde_json::from_slice(&msg.payload).unwrap_or(json!({}));
        let amount = payload["amount"].as_i64().unwrap_or(0);
        self.balance += amount;
        Ok(json!({ "balance": self.balance }))
    }

    #[handler("withdraw")]
    async fn withdraw(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<Value, BehaviorError> {
        let payload: Value = serde_json::from_slice(&msg.payload).unwrap_or(json!({}));
        let amount = payload["amount"].as_i64().unwrap_or(0);
        if amount > self.balance {
            return Err(BehaviorError::ProcessingError(
                "Insufficient funds".to_string(),
            ));
        }
        self.balance -= amount;
        Ok(json!({ "balance": self.balance }))
    }

    #[handler("balance")]
    async fn get_balance(
        &mut self,
        _ctx: &ActorContext,
        _msg: &Message,
    ) -> Result<Value, BehaviorError> {
        Ok(json!({ "balance": self.balance }))
    }
}

#[test]
fn test_multi_handler_actor() {
    let actor = TestMultiHandlerActor { balance: 0 };
    assert_eq!(actor.behavior_type(), BehaviorType::GenServer);
}

// ============================================================================
// Test: Named event actor
// ============================================================================

#[event_actor(name = "audit_logger")]
struct TestNamedEventActor {
    logs: Vec<String>,
}

#[plexspaces_handlers(event)]
impl TestNamedEventActor {
    #[handler("log", cast)]
    async fn log(&mut self, _ctx: &ActorContext, msg: &Message) -> Result<(), BehaviorError> {
        let payload = String::from_utf8_lossy(&msg.payload);
        self.logs.push(payload.to_string());
        Ok(())
    }
}

#[test]
fn test_named_event_actor() {
    let actor = TestNamedEventActor { logs: vec![] };
    match actor.behavior_type() {
        BehaviorType::Custom(name) => assert_eq!(name, "audit_logger"),
        _ => panic!("Expected Custom behavior type with name 'audit_logger'"),
    }
}

// ============================================================================
// Test: Named FSM actor
// ============================================================================

#[fsm_actor(name = "order_workflow")]
struct TestNamedFsmActor {
    state: String,
}

#[plexspaces_handlers(fsm)]
impl TestNamedFsmActor {
    #[handler("transition")]
    async fn transition(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<Value, BehaviorError> {
        let payload: Value = serde_json::from_slice(&msg.payload).unwrap_or(json!({}));
        if let Some(new_state) = payload["state"].as_str() {
            self.state = new_state.to_string();
        }
        Ok(json!({ "state": self.state }))
    }
}

#[test]
fn test_named_fsm_actor() {
    let actor = TestNamedFsmActor {
        state: "idle".to_string(),
    };
    match actor.behavior_type() {
        BehaviorType::Custom(name) => assert_eq!(name, "order_workflow"),
        _ => panic!("Expected Custom behavior type with name 'order_workflow'"),
    }
}

// ============================================================================
// Test: Named workflow actor
// ============================================================================

#[workflow_actor(name = "payment_pipeline")]
struct TestNamedWorkflowActor {
    status: String,
}

#[plexspaces_handlers(workflow)]
impl TestNamedWorkflowActor {
    #[run_handler]
    async fn run(
        &mut self,
        _ctx: &ActorContext,
        _input: Message,
    ) -> Result<Message, BehaviorError> {
        self.status = "running".to_string();
        Ok(Message::default())
    }

    #[signal_handler("cancel")]
    async fn on_cancel(
        &mut self,
        _ctx: &ActorContext,
        _data: Message,
    ) -> Result<(), BehaviorError> {
        self.status = "cancelled".to_string();
        Ok(())
    }

    #[query_handler("status")]
    async fn get_status(
        &self,
        _ctx: &ActorContext,
        _params: Message,
    ) -> Result<Message, BehaviorError> {
        Ok(Message::default())
    }
}

#[test]
fn test_named_workflow_actor() {
    let actor = TestNamedWorkflowActor {
        status: "pending".to_string(),
    };
    match actor.behavior_type() {
        BehaviorType::Custom(name) => assert_eq!(name, "payment_pipeline"),
        _ => panic!("Expected Custom behavior type with name 'payment_pipeline'"),
    }
}

// ============================================================================
// Test: All facet types are valid strings
// ============================================================================

#[gen_server_actor(facets = [
    "timer",
    "reminder", 
    "durability",
    "event_sourcing",
    "virtual_actor",
    "key_value",
    "http_client",
    "lock",
    "registry",
    "process_group",
    "logging",
    "caching",
    "metrics",
    "event_emitter"
])]
struct TestAllFacetsActor {
    data: String,
}

#[plexspaces_handlers]
impl TestAllFacetsActor {
    #[handler("get")]
    async fn get(&mut self, _ctx: &ActorContext, _msg: &Message) -> Result<Value, BehaviorError> {
        Ok(json!({ "data": self.data }))
    }
}

#[test]
fn test_all_facets() {
    assert_eq!(TestAllFacetsActor::FACETS.len(), 14);
    assert!(TestAllFacetsActor::FACETS.contains(&"timer"));
    assert!(TestAllFacetsActor::FACETS.contains(&"reminder"));
    assert!(TestAllFacetsActor::FACETS.contains(&"durability"));
    assert!(TestAllFacetsActor::FACETS.contains(&"event_sourcing"));
    assert!(TestAllFacetsActor::FACETS.contains(&"virtual_actor"));
    assert!(TestAllFacetsActor::FACETS.contains(&"key_value"));
    assert!(TestAllFacetsActor::FACETS.contains(&"http_client"));
    assert!(TestAllFacetsActor::FACETS.contains(&"lock"));
    assert!(TestAllFacetsActor::FACETS.contains(&"registry"));
    assert!(TestAllFacetsActor::FACETS.contains(&"process_group"));
    assert!(TestAllFacetsActor::FACETS.contains(&"logging"));
    assert!(TestAllFacetsActor::FACETS.contains(&"caching"));
    assert!(TestAllFacetsActor::FACETS.contains(&"metrics"));
    assert!(TestAllFacetsActor::FACETS.contains(&"event_emitter"));
}

// ============================================================================
// Test: Message creation helpers (call_message, cast_message, new_message)
// ============================================================================

use plexspaces_sdk::{call_message, cast_message, new_message};

#[test]
fn test_call_message_sets_message_type() {
    let msg = call_message(json!({ "action": "deposit", "amount": 100 }));

    // Verify message_type is set to "call" for request-reply semantics
    assert_eq!(msg.message_type, "call");

    // Verify payload is serialized correctly
    let payload: Value = serde_json::from_slice(&msg.payload).unwrap();
    assert_eq!(payload["action"], "deposit");
    assert_eq!(payload["amount"], 100);

    // Verify message has a unique ID (ULID format)
    assert!(!msg.id.is_empty());
    assert!(msg.id.len() >= 26); // ULID is 26 characters
}

#[test]
fn test_cast_message_sets_message_type() {
    let msg = cast_message(json!({ "event": "user_login", "user_id": "user-123" }));

    // Verify message_type is set to "cast" for fire-and-forget semantics
    assert_eq!(msg.message_type, "cast");

    // Verify payload is serialized correctly
    let payload: Value = serde_json::from_slice(&msg.payload).unwrap();
    assert_eq!(payload["event"], "user_login");
    assert_eq!(payload["user_id"], "user-123");

    // Verify message has a unique ID
    assert!(!msg.id.is_empty());
}

#[test]
fn test_new_message_with_custom_invocation() {
    // Test "call" pattern
    let call_msg = new_message("call", json!({ "op": "get" }));
    assert_eq!(call_msg.message_type, "call");

    // Test "cast" pattern
    let cast_msg = new_message("cast", json!({ "op": "notify" }));
    assert_eq!(cast_msg.message_type, "cast");

    // Test custom message type (for extensibility)
    let info_msg = new_message("info", json!({ "op": "status" }));
    assert_eq!(info_msg.message_type, "info");
}

#[test]
fn test_message_ids_are_unique() {
    let msg1 = call_message(json!({}));
    let msg2 = call_message(json!({}));
    let msg3 = cast_message(json!({}));

    // Each message should have a unique ID
    assert_ne!(msg1.id, msg2.id);
    assert_ne!(msg2.id, msg3.id);
    assert_ne!(msg1.id, msg3.id);
}

#[test]
fn test_message_payload_serialization() {
    // Test complex nested payload
    let msg = call_message(json!({
        "user": {
            "name": "John",
            "age": 30,
            "roles": ["admin", "user"]
        },
        "metadata": {
            "timestamp": 1234567890,
            "source": "test"
        }
    }));

    let payload: Value = serde_json::from_slice(&msg.payload).unwrap();
    assert_eq!(payload["user"]["name"], "John");
    assert_eq!(payload["user"]["age"], 30);
    assert_eq!(payload["user"]["roles"][0], "admin");
    assert_eq!(payload["metadata"]["timestamp"], 1234567890);
}

#[gen_server_actor(wasm)]
struct TestSimpleWasmActor {
    started: bool,
}

#[plexspaces_handlers(wasm)]
impl TestSimpleWasmActor {
    #[init_handler]
    fn initialize(&mut self, config_json: &[u8]) -> Result<(), String> {
        let s = std::str::from_utf8(config_json).map_err(|e| e.to_string())?;
        self.started = s.contains("start");
        Ok(())
    }

    #[handler("ping")]
    fn ping(&mut self, from_actor: &str, payload_json: &[u8]) -> Result<Vec<u8>, String> {
        let payload_str = std::str::from_utf8(payload_json).map_err(|e| e.to_string())?;
        Ok(json!({
            "from_actor": from_actor,
            "payload": payload_str,
            "started": self.started,
        })
        .to_string()
        .into_bytes())
    }
}

#[test]
fn test_wasm_gen_server_handlers_dispatch() {
    let mut actor = TestSimpleWasmActor { started: false };
    actor
        .init(br#"{"start":true}"#)
        .expect("simple actor init should succeed");
    let response = actor
        .handle_operation("leader@test-node", "ping", br#"{"value":1}"#)
        .expect("simple actor dispatch should succeed");
    let parsed: Value = serde_json::from_slice(&response).expect("valid JSON response");
    assert_eq!(parsed["from_actor"], "leader@test-node");
    assert_eq!(parsed["payload"], r#"{"value":1}"#);
    assert_eq!(parsed["started"], true);
}

#[gen_server_actor(wasm, facets = ["timer", "durability"])]
struct TestWasmActorWithFacets;

#[test]
fn test_wasm_gen_server_actor_facets() {
    assert_eq!(TestWasmActorWithFacets::FACETS, &["timer", "durability"]);
}
