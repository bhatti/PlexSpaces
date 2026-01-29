# Webhook Handler Example (FaaS Pattern)

**Purpose**: Demonstrate actors as HTTP/webhook handlers.

**Use Case**: Process webhooks from GitHub, Stripe, Slack.

## Quick Start

```bash
cd examples/rust/embedded/webhook_handler

# Build
cargo build

# Run
cargo run
```

## What It Demonstrates

1. **Actors as HTTP Handlers**: Actor processes webhook requests
2. **Event Routing**: Route by event type (pattern matching)
3. **Stateless Processing**: Each request handled independently
4. **Fire-and-Forget**: Async webhook processing with `tell()`

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│ External Services                                               │
│   GitHub  →  POST /webhooks/github   ─┐                        │
│   Stripe  →  POST /webhooks/stripe   ─┼→  [HTTP Server]        │
│   Slack   →  POST /webhooks/slack    ─┘         │              │
└─────────────────────────────────────────────────────────────────┘
                                                  │
                                                  ▼
                                    ┌─────────────────────────┐
                                    │   WebhookActor          │
                                    │   handle_message()      │
                                    │   ├─ GitHubPush         │
                                    │   ├─ StripePayment      │
                                    │   └─ SlackCommand       │
                                    └─────────────────────────┘
```

## Key Code Patterns

### Define Webhook Event Types

```rust
#[derive(Serialize, Deserialize)]
#[serde(tag = "type")]
enum WebhookEvent {
    #[serde(rename = "github.push")]
    GitHubPush { repository: String, branch: String, commits: usize },
    
    #[serde(rename = "stripe.payment")]
    StripePayment { customer_id: String, amount_cents: u64, currency: String },
    
    #[serde(rename = "slack.command")]
    SlackCommand { user: String, channel: String, command: String },
}
```

### Create Handler Actor

```rust
let handler = ActorBuilder::new(Box::new(WebhookActor::new()))
    .with_id("webhook-handler")
    .with_namespace("webhooks")
    .spawn(&ctx, service_locator.clone())
    .await?;
```

### Send Webhook Event

```rust
let event = WebhookEvent::GitHubPush {
    repository: "acme/backend".to_string(),
    branch: "main".to_string(),
    commits: 3,
};

let msg = Message::json(&event)?.with_message_type("webhook");
handler.tell(msg).await?;  // Fire-and-forget
```

### Handle Events in Actor

```rust
async fn handle_message(&mut self, _ctx: &ActorContext, message: Message) -> Result<(), BehaviorError> {
    let event: WebhookEvent = serde_json::from_slice(&message.payload)?;
    
    match event {
        WebhookEvent::GitHubPush { repository, branch, commits } => {
            // Trigger CI pipeline
        }
        WebhookEvent::StripePayment { customer_id, amount_cents, currency } => {
            // Update subscription
        }
        WebhookEvent::SlackCommand { user, channel, command } => {
            // Execute command
        }
    }
    Ok(())
}
```

## Expected Output

```
Step 1: Create webhook handler actor
  Actor: webhook-handler@webhook-node
  Ready to process webhooks

Step 2: GitHub push webhook
  [GitHub] Push to acme/backend/main: 3 commits
    → Triggering CI pipeline...

Step 3: Stripe payment webhook
  [Stripe] Payment from cus_abc123: 99.99 USD
    → Updating subscription status...

Step 4: Slack command webhook
  [Slack] @alice in #engineering: /deploy staging
    → Executing command...
```

## Use Cases

- **GitHub Webhooks**: Push events, PR events, issue comments
- **Stripe Webhooks**: Payment succeeded, subscription updated
- **Slack Commands**: ChatOps, bot commands
- **Twilio Webhooks**: Incoming SMS, voice calls
- **Custom APIs**: Any HTTP endpoint backed by actor

## tell() vs ask()

| Method | Pattern | Use Case |
|--------|---------|----------|
| `tell()` | Fire-and-forget | Async processing, webhook acknowledgment |
| `ask()` | Request-response | Sync API responses, queries |

```rust
// Async webhook (respond 200 immediately)
handler.tell(msg).await?;

// Sync API (wait for response)
let response = handler.ask(msg, Duration::from_secs(5)).await?;
```

## Production Integration

Add HTTP server (axum example):

```rust
use axum::{routing::post, Router, Json};

async fn webhook_handler(
    Json(event): Json<WebhookEvent>
) -> StatusCode {
    let msg = Message::json(&event)?;
    handler.tell(msg).await?;
    StatusCode::OK  // Acknowledge immediately
}

let app = Router::new()
    .route("/webhooks", post(webhook_handler));
```

## See Also

- [Actor Groups (Sharding)](../actor_groups_sharding/) - For scaling handlers
- [Supervision Tree](../supervision_tree/) - For fault tolerance
- [Architecture Docs](../../../../docs/architecture.md)
