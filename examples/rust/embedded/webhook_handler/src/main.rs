// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Webhook Handler Example (FaaS Actor Pattern)
//
// Demonstrates HTTP/webhook handling with actors:
// - Request-response pattern (simulated HTTP)
// - Different webhook event types
// - Stateless processing with actor isolation
//
// Use Case: GitHub webhooks, Stripe events, Slack commands

use async_trait::async_trait;
use plexspaces_actor::ActorBuilder;
use plexspaces_core::{
    Actor as ActorTrait, ActorContext, BehaviorError, BehaviorType, RequestContext,
};
use plexspaces_mailbox::Message;
use plexspaces_node::NodeBuilder;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

// =============================================================================
// Webhook Event Types (JSON payloads from external services)
// =============================================================================

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
enum WebhookEvent {
    #[serde(rename = "github.push")]
    GitHubPush {
        repository: String,
        branch: String,
        commits: usize,
    },
    #[serde(rename = "stripe.payment")]
    StripePayment {
        customer_id: String,
        amount_cents: u64,
        currency: String,
    },
    #[serde(rename = "slack.command")]
    SlackCommand {
        user: String,
        channel: String,
        command: String,
    },
}

// =============================================================================
// Webhook Handler Actor
// =============================================================================

struct WebhookActor {
    processed_count: u64,
}

impl WebhookActor {
    fn new() -> Self {
        Self { processed_count: 0 }
    }
}

#[async_trait]
impl ActorTrait for WebhookActor {
    async fn handle_message(
        &mut self,
        _ctx: &ActorContext,
        message: plexspaces_proto::common::v1::Message,
    ) -> Result<(), BehaviorError> {
        let event: WebhookEvent = serde_json::from_slice(&message.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("Parse error: {}", e)))?;

        self.processed_count += 1;
        
        // Route to handler based on event type
        match event {
            WebhookEvent::GitHubPush { repository, branch, commits } => {
                println!("  [GitHub] Push to {}/{}: {} commits", repository, branch, commits);
                println!("    → Triggering CI pipeline...");
            }
            WebhookEvent::StripePayment { customer_id, amount_cents, currency } => {
                let amount = amount_cents as f64 / 100.0;
                println!("  [Stripe] Payment from {}: {:.2} {}", customer_id, amount, currency);
                println!("    → Updating subscription status...");
            }
            WebhookEvent::SlackCommand { user, channel, command } => {
                println!("  [Slack] @{} in #{}: /{}", user, channel, command);
                println!("    → Executing command...");
            }
        }

        Ok(())
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }
}

// =============================================================================
// Main
// =============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║           Webhook Handler Example (FaaS Pattern)               ║");
    println!("╚════════════════════════════════════════════════════════════════╝");
    println!();
    println!("Use Case: Process webhooks from GitHub, Stripe, Slack");
    println!();

    // =========================================================================
    // Step 1: Create node and handler actor
    // =========================================================================
    println!("Step 1: Create webhook handler actor");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let node = Arc::new(NodeBuilder::new("webhook-node").build().await);
    
    let service_locator = node.service_locator();
    let ctx = RequestContext::new_without_auth("acme-corp".to_string(), "webhooks".to_string());

    let handler = ActorBuilder::new(Box::new(WebhookActor::new()))
        .with_id("webhook-handler")
        .with_namespace("webhooks")
        .spawn(&ctx, service_locator.clone())
        .await
        .map_err(|e| format!("Failed to spawn webhook handler: {}", e))?;
    
    println!("  Actor: {}", handler.id());
    println!("  Ready to process webhooks");
    println!();

    // =========================================================================
    // Step 2: Receive GitHub push event
    // =========================================================================
    println!("Step 2: GitHub push webhook");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let github_event = WebhookEvent::GitHubPush {
        repository: "acme/backend".to_string(),
        branch: "main".to_string(),
        commits: 3,
    };
    
    let msg = Message::json(&github_event)?
        .with_message_type("webhook");
    
    handler.tell(msg).await.map_err(|e| format!("Send error: {}", e))?;
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    println!();

    // =========================================================================
    // Step 3: Receive Stripe payment event
    // =========================================================================
    println!("Step 3: Stripe payment webhook");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let stripe_event = WebhookEvent::StripePayment {
        customer_id: "cus_abc123".to_string(),
        amount_cents: 9999,
        currency: "USD".to_string(),
    };
    
    let msg = Message::json(&stripe_event)?
        .with_message_type("webhook");
    
    handler.tell(msg).await.map_err(|e| format!("Send error: {}", e))?;
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    println!();

    // =========================================================================
    // Step 4: Receive Slack command
    // =========================================================================
    println!("Step 4: Slack command webhook");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let slack_event = WebhookEvent::SlackCommand {
        user: "alice".to_string(),
        channel: "engineering".to_string(),
        command: "deploy staging".to_string(),
    };
    
    let msg = Message::json(&slack_event)?
        .with_message_type("webhook");
    
    handler.tell(msg).await.map_err(|e| format!("Send error: {}", e))?;
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    println!();

    // =========================================================================
    // Step 5: Batch processing (multiple events)
    // =========================================================================
    println!("Step 5: Batch webhook processing");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let events = vec![
        WebhookEvent::GitHubPush {
            repository: "acme/frontend".to_string(),
            branch: "feature/dark-mode".to_string(),
            commits: 1,
        },
        WebhookEvent::StripePayment {
            customer_id: "cus_xyz789".to_string(),
            amount_cents: 4999,
            currency: "EUR".to_string(),
        },
    ];
    
    for event in events {
        let msg = Message::json(&event)?.with_message_type("webhook");
        handler.tell(msg).await.map_err(|e| format!("Send error: {}", e))?;
    }
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    println!();

    // =========================================================================
    // Summary
    // =========================================================================
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Webhook Handler Example Complete");
    println!();
    println!("Key Concepts:");
    println!("  - Actors as HTTP/webhook handlers");
    println!("  - Request routing by event type");
    println!("  - Stateless processing with isolation");
    println!();
    println!("PlexSpaces Integration:");
    println!("  - ActorBuilder: Create handler actor");
    println!("  - Message::json(): Serialize webhook payload");
    println!("  - tell(): Fire-and-forget (async processing)");
    println!("  - ask(): Request-response (for sync HTTP)");
    println!();
    println!("Use Cases:");
    println!("  - GitHub webhooks (CI/CD triggers)");
    println!("  - Stripe webhooks (payment processing)");
    println!("  - Slack commands (ChatOps)");
    println!("  - Twilio webhooks (SMS/voice)");
    println!("  - Custom API endpoints");
    println!();
    println!("Production Notes:");
    println!("  - Add HTTP server (axum/actix) in front");
    println!("  - Verify webhook signatures");
    println!("  - Use ask() for synchronous responses");
    println!();

    Ok(())
}
