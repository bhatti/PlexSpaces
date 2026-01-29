# Feature Flags Example (Config Updates)

**Purpose**: Demonstrate distributed configuration updates via process groups.

**Use Case**: Feature flag propagation across microservices.

## Quick Start

```bash
cd examples/rust/embedded/feature_flags

# Build
cargo build

# Run
cargo run
```

## What It Demonstrates

1. **Config as Pub/Sub**: Services subscribe to config group
2. **Instant Propagation**: Changes broadcast immediately
3. **Selective Updates**: Only changed values trigger broadcasts
4. **Dynamic Subscribers**: Services can subscribe/unsubscribe

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│ FeatureFlagStore                                                │
│   flags: { dark_mode: true, new_checkout: false }              │
│   group: "feature-flags"                                        │
└─────────────────────────────────────────────────────────────────┘
                              │
                    [Admin: set("dark_mode", true)]
                              │
                              ▼
                      publish_to_group()
                              │
        ┌─────────────────────┼─────────────────────┐
        ▼                     ▼                     ▼
  api-gateway          user-service         billing-service
   (receives)           (receives)            (receives)
```

## Key Code Patterns

### Subscribe to Config Updates

```rust
use plexspaces_process_groups::ProcessGroupRegistry;
use plexspaces_core::ActorId;

let registry = ProcessGroupRegistry::new("config-server", kv_store);
registry.create_group("feature-flags", "acme-corp", "config").await?;

// Service subscribes
let service = ActorId::from("api-gateway@node-1");
registry.join_group("feature-flags", "acme-corp", "config", &service, vec![]).await?;
```

### Broadcast Config Change

```rust
use plexspaces_core::RequestContext;

let ctx = RequestContext::new_without_auth("acme-corp".into(), "config".into());
let message = format!("dark_mode=true").into_bytes();

// Broadcast to all subscribers
let recipients = registry.publish_to_group(&ctx, "feature-flags", None, message).await?;
println!("Updated {} services", recipients.len());
```

### Unsubscribe from Updates

```rust
registry.leave_group(&ctx, "feature-flags", &service).await?;
```

## Expected Output

```
Step 2: Services subscribe for flag updates
  api-gateway@node-1 subscribed
  user-service@node-2 subscribed
  billing-service@node-2 subscribed
  notification-service@node-3 subscribed

Step 3: Admin enables 'dark_mode' feature
  Flag: dark_mode = true
  Broadcasted to 4 services:
    -> api-gateway@node-1
    -> billing-service@node-2
    -> notification-service@node-3
    -> user-service@node-2

Step 7: No broadcast when value unchanged
  Flag: dark_mode = true (same value)
  Recipients: 0 (no broadcast needed)
```

## Use Cases

- **Feature Flags**: Toggle features across all services instantly
- **A/B Testing**: Roll out experiments to specific service groups
- **Gradual Rollouts**: Canary deploys with percentage-based enablement
- **Kill Switches**: Emergency disable of problematic features
- **Rate Limit Updates**: Adjust throttling across services
- **Maintenance Mode**: Enable/disable service availability

## Feature Flags vs Chat Room

| Feature | Feature Flags | Chat Room |
|---------|--------------|-----------|
| **Message Type** | Config state changes | User messages |
| **Persistence** | Flag values stored | Messages ephemeral |
| **Broadcast Logic** | Only on value change | Every message |
| **Subscribers** | Services/actors | Users |

## See Also

- [Chat Room (Process Groups)](../chat_room/) - Basic pub/sub pattern
- [Architecture Docs](../../../../docs/architecture.md)
