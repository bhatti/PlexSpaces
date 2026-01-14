# Message Routing Design

## Overview

This document describes the message routing design for the PlexSpaces actor system, covering `tell()` and `ask()` patterns for local and remote actors, as well as the temporary actor pattern for non-actor callers.

## Design Principles

1. **Location Transparency**: Actors can communicate without knowing if the target is local or remote
2. **Request-Reply Pattern**: `ask()` provides synchronous request-reply semantics over async messaging
3. **Simplified Design**: Always create temporary sender ActorRef for `ask()` calls (actor or non-actor) - simplifies code and ensures consistent behavior
4. **Single Routing Rule**: If receiver is temporary sender → REPLY → route to ReplyWaiter; otherwise → send to mailbox

## Core Concepts

### ActorRef

An `ActorRef` is a location-transparent handle to an actor. It can represent:
- **Local Actor**: Actor on the same node (has mailbox)
- **Remote Actor**: Actor on a different node (uses gRPC)

### Message Types

- **Request**: Message sent TO an actor (receiver = target actor, sender = caller/temporary actor)
- **Reply**: Message sent back FROM an actor (receiver = caller/temporary actor, sender = target actor)

### ReplyWaiter vs ReplyWaiterRegistry

- **ReplyWaiter**: Async condition variable for waiting on a reply (NOT for routing)
- **ReplyWaiterRegistry**: Global registry storing ReplyWaiters by correlation_id (for routing replies)

## tell() Pattern

### Local Actor (Same Node)

```
Caller → ActorRef::tell() → ActorRegistry lookup → Mailbox → Actor
```

**Flow:**
1. `ActorRef::tell(message)` is called
2. If `ActorRef` is Local: Get mailbox from `ActorRefInner::Local`
3. If `ActorRef` is Remote pointing to local: Lookup actor in `ActorRegistry`, get mailbox
4. Send message to mailbox
5. Actor processes message from mailbox

**Key Points:**
- Messages always go to mailbox (no bypass)
- No correlation_id handling in `tell()`
- Simple fire-and-forget semantics

### Remote Actor (Different Node)

```
Caller → ActorRef::tell() → ServiceLocator::get_node_client() → gRPC → Remote Node → ActorRegistry → Mailbox → Actor
```

**Flow:**
1. `ActorRef::tell(message)` is called
2. Extract node_id from `ActorRef` (Remote variant)
3. Get gRPC client for remote node via `ServiceLocator`
4. Convert message to proto format
5. Send via gRPC `send_message` RPC
6. Remote node receives, looks up actor in `ActorRegistry`
7. Sends to actor's mailbox
8. Actor processes message

**Key Points:**
- Uses gRPC for network transport
- Message serialization/deserialization handled automatically
- Location-transparent (caller doesn't know it's remote)

## ask() Pattern

### Overview

**Simplified Design**: We always create a temporary sender ActorRef for `ask()` calls, whether called from an actor or non-actor context. This simplifies the code and ensures consistent behavior.

`ask()` provides request-reply semantics:
1. `ask()` always creates temporary sender ActorRef and `ReplyWaiter`
2. Temporary sender ID format: `"ask-{correlation_id}@{node_id}"`
3. Temporary sender is always local (created on node where `ask()` is called)
4. Receiver can be local or remote (extracted from `message.receiver`)
5. `ask()` sends request message with `correlation_id` and temporary sender ID
6. Target actor processes request and sends reply with same `correlation_id`
7. Reply is routed to temporary sender ActorRef
8. `tell()` detects receiver is temporary sender → routes to `ReplyWaiter` (bypasses mailbox)
9. `ReplyWaiter` wakes up sender with reply message
10. Temporary sender is cleaned up after `ask()` completes

### Actor Calling ask() on Another Actor (Local)

```
Actor A → ActorRef::ask() → Create Temporary Sender → Register in ActorRegistry → ReplyWaiterRegistry::register() → ActorRef::tell(request) → Actor B Mailbox
                                                                                                                                              ↓
Actor B → handle_request() → ctx.send_reply() → ActorService::send_reply() → ActorRegistry lookup → Temporary Sender ActorRef::tell(reply)
                                                                                                                                              ↓
Temporary Sender ActorRef::tell() → ReplyWaiterRegistry::notify() → ReplyWaiter::notify() → Actor A ask() wakes up → Cleanup Temporary Sender
```

**Flow:**
1. Actor A calls `actor_ref.ask(request_message, timeout)`
2. `ask()` generates `correlation_id` (ULID)
3. `ask()` creates `ReplyWaiter` and registers in `ReplyWaiterRegistry` with `correlation_id`
4. `ask()` always creates temporary sender ActorRef with ID `ask-{correlation_id}@{node_id}`
5. Temporary sender ActorRef is registered in `ActorRegistry`
6. `ask()` sets `message.sender = temporary_sender_id`
7. `ask()` sets `message.correlation_id = correlation_id`
8. `ask()` calls `target_actor_ref.tell(request_message)`
9. Actor B receives request in mailbox, processes it
10. Actor B calls `ctx.send_reply(correlation_id, current_actor_id, temporary_sender_id, reply_message)`
11. `ActorContext::send_reply()` uses `ActorService::send()` to route the reply
12. `ActorService::send()` routes to temporary sender ActorRef (temporary sender behaves like normal actor)
13. `temporary_sender_ref.tell(reply_message)` is called with `correlation_id`
13. `tell()` detects receiver is temporary sender → routes to `ReplyWaiterRegistry`
14. `ReplyWaiterRegistry::notify()` finds waiting `ReplyWaiter`, routes reply
15. `ReplyWaiter::notify()` wakes up waiting `ask()` caller
16. `ask()` cleans up temporary sender ActorRef (unregisters from `ActorRegistry`)
17. `ask()` returns reply message to Actor A

**Key Points:**
- Always creates temporary sender ActorRef (simplifies code, consistent behavior)
- Temporary sender ActorRef is registered in `ActorRegistry` (so it can be looked up)
- Temporary sender ActorRef's `tell()` routes messages directly to `ReplyWaiter` (bypasses mailbox)
- One temporary sender ActorRef per `ask()` call
- Temporary sender ActorRef is cleaned up after `ask()` completes

### Actor Calling ask() on Another Actor (Remote)

```
Actor A (Node1) → ActorRef::ask() → Create Temporary Sender (Node1) → Register in ActorRegistry → ReplyWaiterRegistry::register() → gRPC → Node2 → Actor B Mailbox
                                                                                                                                                        ↓
Actor B → handle_request() → ctx.send_reply() → ActorService::send_reply() → gRPC → Node1
                                                                                        ↓
Node1 → ActorRegistry lookup → Temporary Sender ActorRef::tell(reply) → ReplyWaiterRegistry::notify() → ReplyWaiter::notify() → Actor A ask() wakes up → Cleanup Temporary Sender
```

**Flow:**
1. Actor A (Node1) calls `actor_ref.ask(request_message, timeout)` where `actor_ref` points to remote node
2. `ask()` always creates temporary sender ActorRef on Node1 (local node, where ask() is called)
3. `ask()` registers temporary sender ActorRef in Node1's `ActorRegistry`
4. `ask()` registers `ReplyWaiter` in `ReplyWaiterRegistry`
5. `ask()` sets `message.sender = temporary_sender_id@node1`
6. `ask()` extracts target node_id from `message.receiver` (e.g., `server@node2`)
7. `ask()` gets gRPC client for Node2
8. `ask()` sends request via gRPC `send_message` RPC with `wait_for_response=false`
9. Node2 receives request, looks up Actor B in `ActorRegistry`, sends to mailbox
10. Actor B processes request, calls `ctx.send_reply()`
11. `ActorContext::send_reply()` uses `ActorService::send()` to route the reply
12. `ActorService::send()` detects remote target (`temporary_sender_id@node1`)
13. `ActorService::send()` gets gRPC client for Node1 and sends reply via gRPC
14. Node1 receives reply, looks up temporary sender ActorRef in `ActorRegistry`
15. Node1 calls `temporary_sender_ref.tell(reply_message)` with `correlation_id`
16. `tell()` detects receiver is temporary sender → routes to `ReplyWaiterRegistry`
17. `ReplyWaiterRegistry::notify()` finds waiting `ReplyWaiter`, routes reply
18. `ask()` wakes up with reply
19. `ask()` cleans up temporary sender ActorRef

**Key Points:**
- Always creates temporary sender ActorRef (simplifies code, consistent behavior)
- Temporary sender ActorRef is ALWAYS created on local node (where ask() is called)
- Target can be local or remote (extracted from `message.receiver`)
- Reply routing crosses network boundary back to temporary sender ActorRef
- Temporary sender ActorRef's `tell()` routes replies directly to `ReplyWaiter` (bypasses mailbox)

## Simplified Design: Always Create Temporary Sender

### Design Overview

**Key Simplification**: We always create a temporary sender ActorRef for `ask()` calls, whether called from an actor or non-actor context. This removes conditional logic and ensures consistent behavior.

The temporary sender ActorRef:
1. Is always created on the local node (where `ask()` is called)
2. Has ID format: `"ask-{correlation_id}@{node_id}"` (never matches actor IDs)
3. Is registered in `ActorRegistry` as an actual ActorRef (so it can be looked up)
4. When `tell()` is called on it, routes messages directly to `ReplyWaiter` (bypasses mailbox)
5. Is cleaned up after `ask()` completes (success or timeout)

This follows Akka's pattern where temporary actors are created for ask() calls. The temporary ActorRef doesn't process messages in a mailbox - it routes replies directly to the waiting `ask()` caller.

**Benefits of Always Creating Temporary Sender:**
- Simpler code: No conditional logic for actor vs non-actor
- Consistent behavior: All `ask()` calls work the same way
- Easier to maintain: Single path for temporary sender creation and cleanup
- Self-messaging prevention: Temporary sender IDs never match actor IDs

### Non-Actor ask() - Local Target

```
Non-Actor → ActorRef::ask() → Create Temporary ActorRef → Register in ActorRegistry → ReplyWaiterRegistry::register() → ActorRef::tell(request) → Target Actor Mailbox
                                                                                                                                                      ↓
Target Actor → handle_request() → ctx.send_reply() → ActorService::send_reply() → ActorRegistry lookup → Temporary ActorRef::tell(reply)
                                                                                                                                    ↓
Temporary ActorRef::tell() → ReplyWaiterRegistry::notify() → ReplyWaiter::notify() → Non-Actor ask() wakes up → Cleanup Temporary ActorRef
```

**Flow:**
1. Non-actor code calls `actor_ref.ask(request_message, timeout)`
2. `ask()` generates `correlation_id` (ULID)
3. `ask()` creates `ReplyWaiter` and registers in `ReplyWaiterRegistry` with `correlation_id`
4. `ask()` creates temporary ActorRef with ID `ask-{correlation_id}@{node_id}`
5. Temporary ActorRef is registered in `ActorRegistry` (so it can be looked up)
6. `ask()` sets `message.sender = temporary_actor_id`
7. `ask()` sets `message.correlation_id = correlation_id`
8. `ask()` calls `target_actor_ref.tell(request_message)`
9. Target actor receives request in mailbox, processes it
10. Target actor calls `ctx.send_reply(correlation_id, current_actor_id, temporary_actor_id, reply_message)`
11. `ActorContext::send_reply()` uses `ActorService::send()` to route the reply
12. `ActorService::send()` routes to temporary ActorRef (temporary sender behaves like normal actor)
13. `temporary_actor_ref.tell(reply_message)` is called with `correlation_id`
13. Temporary ActorRef's `tell()` detects it's a temporary sender and routes to `ReplyWaiterRegistry`
14. `ReplyWaiterRegistry::notify()` finds waiting `ReplyWaiter`, routes reply
15. `ReplyWaiter::notify()` wakes up waiting `ask()` caller
16. `ask()` cleans up temporary ActorRef (unregisters from `ActorRegistry`)
17. `ask()` returns reply message to non-actor caller

**Key Points:**
- Temporary ActorRef is a REAL ActorRef registered in `ActorRegistry` (not just an ID)
- Temporary ActorRef's `tell()` routes messages directly to `ReplyWaiter` (bypasses mailbox)
- One temporary ActorRef per `ask()` call
- Temporary ActorRef is cleaned up after `ask()` completes

### Non-Actor ask() - Remote Target

```
Non-Actor (Node1) → ActorRef::ask() → Create Temporary Actor (Node1) → Register in ActorRegistry → ReplyWaiterRegistry::register() → gRPC → Node2 → Target Actor Mailbox
                                                                                                                                                        ↓
Target Actor (Node2) → handle_request() → ctx.send_reply() → ActorService::send_reply() → gRPC → Node1
                                                                                                                                  ↓
Node1 → ActorRegistry lookup → Temporary Actor Mailbox → handle_message() → ReplyWaiterRegistry::notify() → ReplyWaiter::notify() → Non-Actor ask() wakes up → Cleanup Temporary Actor
```

**Flow:**
1. Non-actor code (Node1) calls `actor_ref.ask(request_message, timeout)` where target is on Node2
2. `ask()` creates temporary ActorRef on Node1 (local node, where ask() is called)
3. `ask()` registers temporary ActorRef in Node1's `ActorRegistry`
4. `ask()` registers `ReplyWaiter` in `ReplyWaiterRegistry`
5. `ask()` sets `message.sender = temporary_actor_id@node1`
6. `ask()` extracts target node_id from `message.receiver` (Node2)
7. `ask()` gets gRPC client for Node2
8. `ask()` sends request via gRPC `send_message` RPC
9. Node2 receives request, looks up target actor, sends to mailbox
10. Target actor processes request, calls `ctx.send_reply()`
11. `ActorContext::send_reply()` uses `ActorService::send()` to route the reply
12. `ActorService::send()` detects remote target (`temporary_actor_id@node1`)
13. `ActorService::send()` gets gRPC client for Node1 and sends reply via gRPC
14. Node1 receives reply, looks up temporary ActorRef in `ActorRegistry`
15. Node1 calls `temporary_actor_ref.tell(reply_message)` with `correlation_id`
16. Temporary ActorRef's `tell()` routes directly to `ReplyWaiterRegistry`
17. `ReplyWaiterRegistry::notify()` finds waiting `ReplyWaiter`, routes reply
18. `ask()` wakes up with reply
19. `ask()` cleans up temporary ActorRef (unregisters from `ActorRegistry`)

**Key Points:**
- Temporary ActorRef is ALWAYS created on local node (where ask() is called)
- Target can be local or remote (extracted from `message.receiver`)
- Reply routing crosses network boundary back to temporary ActorRef
- Temporary ActorRef's `tell()` routes replies directly to `ReplyWaiter` (bypasses mailbox)

## Request vs Reply Routing

### Simplified Routing Logic

**Key Simplification**: Since we always create temporary sender for `ask()`, routing is simple:
- **If receiver is temporary sender** → REPLY → route to ReplyWaiter (bypass mailbox)
- **Otherwise** → REQUEST or normal message → send to mailbox

**Request:**
- `receiver = target_actor_id` (message TO target actor)
- `sender = temporary_sender_id` (always temporary sender for ask())
- Has `correlation_id` (for ask() pattern)
- Goes to target actor's mailbox

**Reply:**
- `receiver = temporary_sender_id` (message TO temporary sender)
- `sender = target_actor_id` (message FROM target actor)
- Has `correlation_id` (matches request's correlation_id)
- Routed to `ReplyWaiter` (bypasses mailbox) when `tell()` detects receiver is temporary sender

### Routing Logic in tell()

The routing logic is greatly simplified:

```rust
// In ActorRef::tell_impl()
// Get ReplyWaiterRegistry once for all reply routing checks
let waiter_registry: Option<Arc<ReplyWaiterRegistry>> = 
    service_locator.reply_waiter_registry().await;

// SIMPLIFIED ROUTING: Since we always create temporary sender for ask(), routing is simple:
// - If receiver is temporary sender → REPLY → route to ReplyWaiter (bypass mailbox)
// - Otherwise → REQUEST or normal message → send to mailbox
// 
// Note: We only check receiver, not actor_id. When tell() is called on a temporary sender ActorRef,
// the receiver will be that temporary sender ID, so checking receiver covers all cases.
if Self::is_temporary_sender_id(&message.receiver) {
    if let Some(corr_id) = Self::extract_correlation_id_from_temporary_sender(&message.receiver) {
        if let Some(ref waiter_registry) = waiter_registry {
            if waiter_registry.notify(&corr_id, message).await {
                return Ok(()); // Reply routed
            }
        }
    }
}

// REQUEST or normal message → send to mailbox
send_to_mailbox(message)
```

**Key Points:**
- **Single check**: Only check if receiver is temporary sender
- **No complex logic**: No need to distinguish requests from replies using sender/receiver fields
- **Consistent behavior**: All `ask()` calls work the same way
- **Simpler code**: Reduced from 5 checks to 1 check

## ReplyWaiter vs ReplyWaiterRegistry

### ReplyWaiter

- **Purpose**: Async condition variable for waiting on a reply (NOT for routing)
- **Usage**: Created by `ask()`, stored in `ReplyWaiterRegistry`
- **Lifecycle**: 
  - Created when `ask()` is called
  - Registered in `ReplyWaiterRegistry` with `correlation_id`
  - Notified when reply arrives
  - Removed from registry after use or timeout

### ReplyWaiterRegistry

- **Purpose**: Global registry for `ReplyWaiter` instances (for routing replies)
- **Key**: `correlation_id` (String)
- **Value**: `ReplyWaiter` (Arc<ReplyWaiter>)
- **Usage**: 
  - `register(correlation_id, waiter)` - Register waiter when `ask()` is called
  - `notify(correlation_id, message)` - Route reply to waiter (returns true if found)
  - `remove(correlation_id)` - Cleanup after use or timeout

**Why Global Registry?**
- `ask()` might be called on one `ActorRef` instance
- Reply might be routed to a different `ActorRef` instance (cloned, recreated)
- Global registry ensures replies are routed correctly regardless of `ActorRef` instance

## Temporary ActorRef Implementation

### Temporary ActorRef Routing

When `tell()` is called on a temporary ActorRef, it routes messages directly to `ReplyWaiter`:

```rust
// In ActorRef::tell_impl()
// Get ReplyWaiterRegistry once for all reply routing checks
let waiter_registry = service_locator.reply_waiter_registry().await;

// If this ActorRef is a temporary sender, route directly to ReplyWaiter
if Self::is_temporary_sender_id(&actor_id) {
    // Extract correlation_id from temporary sender ID
    if let Some(corr_id) = Self::extract_correlation_id_from_temporary_sender(&actor_id) {
        // Route to ReplyWaiter via ReplyWaiterRegistry
        if let Some(ref waiter_registry) = waiter_registry {
            if waiter_registry.notify(&corr_id, message).await {
                return Ok(()); // Reply routed successfully
            }
        }
    }
}
```

### Temporary ActorRef Creation

```rust
// In ActorRef::ask() for non-actor callers
let temporary_actor_id = format!("ask-{}@{}", correlation_id, node_id);

// Create temporary ActorRef (with dummy mailbox - never used)
let dummy_mailbox = Arc::new(Mailbox::new(MailboxConfig::default(), temporary_actor_id.clone()).await?);
let temporary_actor_ref: Arc<dyn MessageSender> = Arc::new(ActorRef::local(
    temporary_actor_id.clone(),
    dummy_mailbox,
    service_locator.clone(),
));

// Register in ActorRegistry (so it can be looked up)
registry.register_temporary_sender(
    &ctx,
    temporary_actor_id.clone(),
    temporary_actor_ref,
    correlation_id.clone(),
    expires_at,
).await;

// Now use temporary_actor_id as message.sender
message.sender = Some(temporary_actor_id.clone());
```

**Key Points:**
- Temporary ActorRef is registered in `ActorRegistry` via `register_temporary_sender()`
- `register_temporary_sender()` calls `register_actor()` internally (consistent with regular actors)
- `remove_temporary_sender()` calls `unregister_with_cleanup()` internally (consistent cleanup)
- Temporary ActorRef's `tell()` routes directly to `ReplyWaiter` (bypasses mailbox)

## Message Flow Diagrams

### Local Actor ask() (Actor → Actor)

```
┌─────────┐                    ┌──────────────┐
│ Actor A │                    │ ReplyWaiter   │
│         │                    │   Registry    │
└────┬────┘                    └──────┬───────┘
     │                                 │
     │ 1. ask(request)                │
     ├─────────────────────────────────┤
     │ 2. register(corr_id, waiter)    │
     │                                 │
     │ 3. tell(request)                │
     │    └───────────────────────────┼──┐
     │                                 │  │
┌────▼────┐                           │  │
│ Actor B │                           │  │
│         │                           │  │
└────┬────┘                           │  │
     │                                 │  │
     │ 4. handle_request()             │  │
     │ 5. send_reply()                 │  │
     │    └───────────────────────────┼──┘
     │                                 │
     │ 6. tell(reply)                  │
     ├─────────────────────────────────┤
     │ 7. notify(corr_id, reply)       │
     │                                 │
     │ 8. ReplyWaiter wakes up         │
     │                                 │
     │ 9. ask() returns reply          │
```

### Non-Actor ask() (Test/Main → Actor)

```
┌─────────────┐              ┌──────────────┐              ┌──────────────┐
│ Non-Actor   │              │  Temporary   │              │ ReplyWaiter  │
│  (Test)     │              │  ActorRef    │              │   Registry   │
└──────┬──────┘              └──────┬───────┘              └──────┬───────┘
       │                            │                             │
       │ 1. ask(request)             │                             │
       ├─────────────────────────────┤                             │
       │ 2. Create temporary ActorRef│                             │
       │ 3. Register in ActorRegistry │                             │
       │ 4. register(corr_id, waiter)  │                             │
       │                            │                             │
       │ 5. tell(request)            │                             │
       │    └───────────────────────┼─────────────────────────────┼──┐
       │                            │                             │  │
┌──────▼──────┐                    │                             │  │
│ Target Actor │                    │                             │  │
│              │                    │                             │  │
└──────┬───────┘                    │                             │  │
       │                            │                             │  │
       │ 6. handle_request()        │                             │  │
       │ 7. send_reply()             │                             │  │
       │    └───────────────────────┼─────────────────────────────┼──┘
       │                            │                             │
       │ 8. tell(reply)              │                             │
       ├────────────────────────────┤                             │
       │ 9. tell() routes to        │                             │
       │    ReplyWaiterRegistry     │                             │
       │ 10. notify(corr_id, reply) │                             │
       │                            │                             │
       │ 11. ReplyWaiter wakes up   │                             │
       │                            │                             │
       │ 12. ask() returns reply    │                             │
       │                            │                             │
       │ 13. Cleanup temporary      │                             │
       │     ActorRef               │                             │
```

### Remote Actor ask() (Node1 → Node2)

```
Node1                          Node2
┌─────────┐                   ┌──────────────┐
│ Actor A │                   │  Actor B     │
│         │                   │              │
└────┬────┘                   └──────┬───────┘
     │                              │
     │ 1. ask(request)              │
     │ 2. register(corr_id, waiter) │
     │ 3. gRPC send_message()       │
     ├──────────────────────────────►│
     │                              │
     │                              │ 4. handle_request()
     │                              │ 5. send_reply()
     │                              │ 6. gRPC send_message()
     │◄─────────────────────────────┤
     │                              │
     │ 7. tell(reply)                │
     │ 8. notify(corr_id, reply)    │
     │ 9. ReplyWaiter wakes up       │
     │ 10. ask() returns reply       │
```

## Implementation Notes

### Temporary ActorRef Cleanup

Temporary ActorRefs are cleaned up:
1. After `ask()` completes successfully (reply received)
2. On timeout (if reply doesn't arrive)
3. On error (if message send fails)

Cleanup involves:
- Calling `ActorRegistry::remove_temporary_sender()` which:
  - Unregisters from `ActorRegistry` (via `unregister_with_cleanup()`)
  - Removes from `temporary_senders` map
- Clearing the `Option<String>` in ActorRef (if tracking locally)

**Design Note**: `ActorRegistry::register_temporary_sender()` calls `register_actor()`, and `remove_temporary_sender()` calls `unregister_with_cleanup()`. This ensures consistency - temporary ActorRefs are treated like regular actors for registration/unregistration.

### Error Handling

- **Timeout**: If reply doesn't arrive within timeout, `ReplyWaiter` times out, temporary actor cleans up
- **No ReplyWaiter**: If `ReplyWaiter` not found (already consumed/timeout), message goes to mailbox (temporary actor logs warning)
- **Actor Not Found**: If target actor doesn't exist, `ask()` returns error immediately (before waiting)

### Observability

All message routing should include:
- Debug logs for each step (request sent, reply received, etc.)
- Metrics for ask() success/failure rates
- Metrics for temporary actor creation/cleanup
- Correlation IDs in all logs for tracing

## References

- [Akka Ask Pattern](https://doc.akka.io/docs/akka/current/typed/interaction-patterns.html#request-response)
- [Erlang gen_server:call](https://www.erlang.org/doc/man/gen_server.html#call-2)
- [Orleans Request-Response](https://dotnet.github.io/orleans/docs/grains/communication/index.html)

