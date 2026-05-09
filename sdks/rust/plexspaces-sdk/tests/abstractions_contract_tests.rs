// SPDX-License-Identifier: AGPL-3.0-or-later

use plexspaces_actor::TestServiceLocatorStub;
use plexspaces_actor::ServiceLocator;
use plexspaces_journaling::SqliteJournalStorage;
use plexspaces_sdk::{
    create_facets_with_storage, event_actor, gen_server_actor, handler, json, new_message,
    plexspaces_handlers, query_handler, run_handler, signal_handler, workflow_actor, Actor,
    ActorContext, BehaviorError, DeclaredFacets, Message, Value,
};
use std::sync::Arc;

#[gen_server_actor(facets = ["virtual_actor", "durability", "timer", "reminder"])]
struct AbstractionsActor {
    actor_id: String,
    count: i64,
}

#[gen_server_actor(facets = ["virtual_actor"])]
struct EphemeralActor {
    count: i64,
}

#[plexspaces_handlers]
impl AbstractionsActor {
    #[handler("increment")]
    async fn increment(
        &mut self,
        _ctx: &ActorContext,
        _msg: &Message,
    ) -> Result<Value, BehaviorError> {
        self.count += 1;
        Ok(json!({ "count": self.count }))
    }
}

#[plexspaces_handlers]
impl EphemeralActor {
    #[handler("increment")]
    async fn increment(
        &mut self,
        _ctx: &ActorContext,
        _msg: &Message,
    ) -> Result<Value, BehaviorError> {
        self.count += 1;
        Ok(json!({ "count": self.count }))
    }
}

#[workflow_actor(facets = ["virtual_actor", "durability"])]
struct AbstractionsWorkflow {
    status: String,
}

#[event_actor(facets = ["process_group"])]
struct AbstractionsChannel {
    received: Vec<String>,
}

#[plexspaces_handlers(workflow)]
impl AbstractionsWorkflow {
    #[run_handler]
    async fn run(
        &mut self,
        _ctx: &ActorContext,
        _input: Message,
    ) -> Result<Message, BehaviorError> {
        self.status = "running:o-1".to_string();
        Ok(Message {
            payload: serde_json::to_vec(&json!({ "status": self.status })).unwrap(),
            ..Default::default()
        })
    }

    #[signal_handler("cancel")]
    async fn cancel(&mut self, _ctx: &ActorContext, _input: Message) -> Result<(), BehaviorError> {
        self.status = "cancelled".to_string();
        Ok(())
    }

    #[query_handler("status")]
    async fn status(&self, _ctx: &ActorContext, _input: Message) -> Result<Message, BehaviorError> {
        Ok(Message {
            payload: serde_json::to_vec(&json!({ "status": self.status })).unwrap(),
            ..Default::default()
        })
    }
}

#[plexspaces_handlers(event)]
impl AbstractionsChannel {
    #[handler("publish", cast)]
    async fn publish(&mut self, _ctx: &ActorContext, msg: &Message) -> Result<(), BehaviorError> {
        let payload: serde_json::Value =
            serde_json::from_slice(&msg.payload).expect("channel payload");
        let channel = payload
            .get("channel")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        let body = payload
            .get("body")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        self.received.push(format!("{channel}:{body}"));
        Ok(())
    }
}

#[test]
fn abstractions_actor_declares_expected_facets() {
    assert_eq!(
        AbstractionsActor::declared_facets(),
        &["virtual_actor", "durability", "timer", "reminder"]
    );
    assert_eq!(EphemeralActor::declared_facets(), &["virtual_actor"]);
}

#[test]
fn abstractions_workflow_declares_expected_facets() {
    assert_eq!(
        AbstractionsWorkflow::declared_facets(),
        &["virtual_actor", "durability"]
    );
    assert_eq!(
        AbstractionsWorkflow {
            status: "pending".to_string(),
        }
        .behavior_type(),
        plexspaces_actor::BehaviorType::Workflow
    );
}

#[test]
fn abstractions_channel_declares_expected_facets_and_behavior() {
    assert_eq!(AbstractionsChannel::declared_facets(), &["process_group"]);
    assert_eq!(
        AbstractionsChannel { received: vec![] }.behavior_type(),
        plexspaces_actor::BehaviorType::GenEvent
    );
}

#[tokio::test]
async fn abstractions_facets_include_timer_reminder_durability_and_virtual_actor() {
    let service_locator: Arc<dyn ServiceLocator> = Arc::new(TestServiceLocatorStub::new());
    let storage = Arc::new(
        SqliteJournalStorage::new(":memory:")
            .await
            .expect("sqlite journal storage"),
    );

    let facets = create_facets_with_storage(
        AbstractionsActor::declared_facets(),
        &json!({
            "timer": { "interval_ms": 500 },
            "reminder": { "tick_interval_ms": 2500 },
            "durability": { "checkpoint_interval": 5 }
        }),
        Some(storage),
        service_locator,
    )
    .expect("facets");

    let facet_types: Vec<&str> = facets.iter().map(|facet| facet.facet_type()).collect();
    assert!(facet_types.contains(&"timer"));
    assert!(facet_types.contains(&"reminder"));
    assert!(facet_types.contains(&"durability"));
    assert!(facet_types.contains(&"virtual_actor"));
}

#[tokio::test]
async fn abstractions_non_durable_virtual_actor_uses_virtual_actor_facet_only() {
    let service_locator: Arc<dyn ServiceLocator> = Arc::new(TestServiceLocatorStub::new());
    let storage = Arc::new(
        SqliteJournalStorage::new(":memory:")
            .await
            .expect("sqlite journal storage"),
    );

    let facets = create_facets_with_storage(
        EphemeralActor::declared_facets(),
        &json!({}),
        Some(storage),
        service_locator,
    )
    .expect("facets");

    let facet_types: Vec<&str> = facets.iter().map(|facet| facet.facet_type()).collect();
    assert_eq!(facet_types, vec!["virtual_actor"]);
}

#[tokio::test]
async fn abstractions_channel_handles_publish_events() {
    let mut channel = AbstractionsChannel { received: vec![] };
    let service_locator: Arc<dyn ServiceLocator> = Arc::new(TestServiceLocatorStub::new());
    let ctx = ActorContext::new(
        "test-node".to_string(),
        String::new(),
        "default".to_string(),
        service_locator,
        None,
    );
    let message = new_message(
        "cast",
        json!({
            "op": "publish",
            "channel": "alerts",
            "body": "hello"
        }),
    );

    channel
        .handle_message(&ctx, message)
        .await
        .expect("publish event");
    assert_eq!(channel.received, vec!["alerts:hello".to_string()]);
}
