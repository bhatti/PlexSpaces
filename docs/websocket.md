<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com> -->

# WebSocket Transport

PlexSpaces supports WebSocket as a first-class transport alongside gRPC and REST. WebSocket is designed for **thin clients** — browsers, edge devices, and any client behind a firewall that can make outbound TCP connections but cannot accept inbound gRPC connections.

## Overview

```
┌─────────────────────────────────────────────────────────┐
│  High-level Services (UNCHANGED)                        │
│  ActorServiceImpl, NodeRegistry/SWIM, NodeService, etc. │
└───────────────────────────┬─────────────────────────────┘
                            │ uses trait
┌───────────────────────────▼─────────────────────────────┐
│  Transport Client Traits                                │
│  ActorTransportClient, NodeTransportClient              │
└───────────────┬───────────────────────┬─────────────────┘
                │                       │
    ┌───────────▼──────────┐  ┌────────▼──────────────┐
    │  GrpcTransportClient │  │  WsTransportClient    │
    │  (existing channels) │  │  (via WsRegistry)     │
    └──────────────────────┘  └───────────────────────┘
```

The routing invariant: `WsRegistry` is the single source of truth. `get_sender(node_id) → Some` routes via WebSocket; `None` falls back to gRPC. Full nodes never register in `WsRegistry` so always route via gRPC. Thin nodes register on connect and unregister on disconnect.

## Connection Endpoint

All WebSocket connections use a single endpoint:

```
ws://<node-addr>/ws
wss://<node-addr>/ws   (when TLS is configured)
```

## Authentication

JWT validation happens at HTTP upgrade time, before the WebSocket connection is established. If the JWT is invalid or missing (and auth is enabled), the server returns HTTP 401 and the connection is refused.

When auth is disabled (e.g., development mode with `NodeBuilder::with_auth_disabled()`), the upgrade always succeeds regardless of headers.

The tenant ID is extracted from the JWT at upgrade time and associated with the session for the lifetime of the connection.

## Wire Protocol

All frames use **binary Protobuf** encoding of the `WsFrame` message (defined in `proto/plexspaces/transport/ws/v1/websocket.proto`):

```protobuf
message WsFrame {
  string request_id = 1;   // echoed back in responses for correlation
  oneof payload {
    NodeRegistration     node_register      = 10;
    WsNodeRegisterAck    node_register_ack  = 11;
    SendMessageRequest   tell               = 20;
    AskReplyRequest      ask                = 21;
    AskReplyResponse     ask_response       = 22;
    SendHeartbeatRequest heartbeat          = 30;
    WsHeartbeatAck       heartbeat_ack      = 31;
    WsError              error              = 99;
  }
}
```

`request_id` is always echoed back in the response frame for client-side correlation.

## Handshake Sequence

Every WebSocket connection must complete a registration handshake before any other frames are accepted:

```
Client                              Server
  │                                   │
  │── HTTP Upgrade /ws ──────────────►│
  │◄─ 101 Switching Protocols ────────│
  │                                   │
  │── WsFrame{NodeRegister} ─────────►│  (must arrive within 10s)
  │                                   │  server registers in WsRegistry
  │◄─ WsFrame{NodeRegisterAck} ───────│  client may now send any frame
  │                                   │
  │── WsFrame{Tell/Ask/Heartbeat} ───►│
  │◄─ WsFrame{AskResponse/HbAck} ────│
```

**Key ordering guarantee:** `WsRegistry.register()` is called **before** the `NodeRegisterAck` is sent. A client that has received the ack can immediately query `ServiceLocator.get_ws_registry()` and see its own session — no polling delay needed.

### NodeRegistration frame

```protobuf
// ws_frame.payload = node_register
NodeRegistration {
  node_id:      string  // empty → server assigns a ULID
  node_address: string  // informational; not used for routing
  capabilities: map     // optional
}
```

If `node_id` is empty, the server assigns a random ULID and returns it in the ack.

### NodeRegisterAck frame

```protobuf
// ws_frame.payload = node_register_ack
WsNodeRegisterAck {
  success:          bool
  assigned_node_id: string  // the canonical ID for this session
  error_message:    string  // non-empty on failure
}
```

## Thin Nodes

A node that connects via WebSocket is automatically assigned `NodeRoleThin`. Thin nodes:

- Appear in `list_connected_nodes()` as `NODE_ROLE_THIN`
- Are registered in `NodeRegistry` (cluster membership) for discovery
- Are **excluded** from SWIM indirect-ping intermediary selection (they have no inbound gRPC)
- Are unregistered from both `WsRegistry` and `NodeRegistry` on disconnect

## Tell (fire-and-forget)

```
Client                              Server
  │── WsFrame{tell: SendMessageRequest} ►│
  │                                       │  dispatched to actor
  │   (no response frame)                │
```

Send a `WsFrame` with `payload = tell` (a `SendMessageRequest`). The frame is dispatched to the target actor. No response is sent.

## Ask (request-reply)

```
Client                              Server
  │── WsFrame{ask: AskReplyRequest, request_id="abc"} ►│
  │                                                      │  dispatched to actor
  │◄─ WsFrame{ask_response: AskReplyResponse, request_id="abc"} │
```

The `request_id` in the outer `WsFrame` envelope is used to correlate the response. The client must use the same `request_id` in the `AskReplyRequest.request_id` field and the outer `WsFrame.request_id`. The server echoes both back.

On the server side, `WsActorTransportClient` registers a `oneshot::Sender` keyed by `request_id` before sending the frame. The WS receive loop resolves it when the `ask_response` arrives.

## Heartbeat

```
Client                              Server
  │── WsFrame{heartbeat: SendHeartbeatRequest} ►│
  │◄─ WsFrame{heartbeat_ack: WsHeartbeatAck} ───│
```

The heartbeat ack echoes back the `request_id` for liveness verification.

## Disconnect

On WS close or error, the server:
1. Unregisters the session from `WsRegistry`
2. Unregisters the thin node from `NodeRegistry`
3. Cancels all in-flight asks for that session via `PendingAsks::cancel_for_node()`

## WsRegistry

`WsRegistry` tracks live WebSocket sessions and is accessible via `ServiceLocator`:

```rust
let ws_reg = node.service_locator().get_ws_registry().await
    .expect("WsRegistry registered after Node::start()");

// Check if a thin node is connected
let connected = ws_reg.is_connected("my-thin-node-id").await;

// List all thin node IDs
let thin_nodes: Vec<String> = ws_reg.list_thin_nodes().await;

// List all connected node IDs (including non-thin)
let all: Vec<String> = ws_reg.list_all_nodes().await;

// Count active sessions
let count: usize = ws_reg.session_count().await;
```

## Client Example (Rust)

```rust
use tokio_tungstenite::connect_async;
use prost::Message as ProstMessage;
use plexspaces_proto::transport::ws::v1::{ws_frame, WsFrame};
use plexspaces_proto::node::v1::NodeRegistration;

let (ws_stream, _) = connect_async("ws://127.0.0.1:8080/ws").await?;
let (mut write, mut read) = ws_stream.split();

// Send registration
let reg = WsFrame {
    request_id: "handshake-1".to_string(),
    payload: Some(ws_frame::Payload::NodeRegister(NodeRegistration {
        node_id: "my-thin-node".to_string(),
        ..Default::default()
    })),
};
write.send(Message::Binary(reg.encode_to_vec().into())).await?;

// Receive ack
let msg = read.next().await.unwrap()?;
let ack = WsFrame::decode(msg.into_data().as_ref())?;
// ack.request_id == "handshake-1"
// ack.payload == Some(NodeRegisterAck { success: true, assigned_node_id: "my-thin-node" })
```

## Node Ping and Resource Hints

Thin nodes can query a server node's live resource state with a `PingRequest`:

```
Client                              Server
  │── WsFrame{node_ping: PingRequest} ─────►│
  │◄─ WsFrame{node_ping_response: PingResponse} │
```

The `PingResponse` (proto field 9) carries a `NodeResourceHints` embedded message:

```protobuf
message NodeResourceHints {
  float  cpu_percent         = 1;  // 0–100; 0 = unavailable
  uint64 memory_available_mb = 2;  // MiB available
  uint32 available_cores     = 3;  // hardware_concurrency
}

message PingResponse {
  string            request_id        = 1;
  string            node_id           = 2;
  // ... other fields ...
  NodeResourceHints resources         = 9;
}
```

The server populates `resources` in real-time via `sysinfo` (CPU usage, available memory) and `std::thread::available_parallelism` (logical CPU count).

Thin nodes that want to advertise their own capabilities at connect time can include them in `NodeRegistration.resource_hints` (field 11):

```protobuf
message NodeRegistration {
  // ...
  NodeRole          node_role         = 10;
  NodeResourceHints resource_hints    = 11;  // startup estimate from thin nodes
}
```

## TypeScript WsThinClient SDK

The `@plexspaces/sdk` package ships a zero-dependency `WsThinClient` for browser and Node.js 22+ environments. All wire encoding is hand-rolled (no protobufjs, no grpc-web dependency).

### Stable Node IDs

By default `WsThinClient` generates a random ULID for `nodeId`. For applications
where virtual actors need to route messages to the same thin node reliably across
page refreshes (e.g. a chat client where messages should reach `alice` no matter
when she refreshes), pass a **deterministic node ID** derived from the user
identity:

```typescript
// Stable convention: "<username>.io"
// Same node_id every session → virtual actor routes to alice's thin node reliably.
const client = new WsThinClient({ wsUrl, nodeId: `${username}.io`, namespace });
```

If the previous session is still alive (TCP close not yet propagated), the server
rejects the duplicate. Wait briefly and retry:

```typescript
try {
  await client.connect();
} catch (e) {
  if ((e as Error).message.includes('already registered')) {
    await new Promise(r => setTimeout(r, 1000));
    client = new WsThinClient(opts);
    await client.connect();
  }
}
```

### Quick Start

```typescript
import { WsThinClient, ThinClientOptions } from '@plexspaces/sdk';

const client = new WsThinClient({
  wsUrl: 'ws://localhost:8091/ws',
  jwtToken: 'eyJ...',     // optional
  nodeId: 'alice.io',     // stable: same node_id across page refreshes
  tenant: 'my-tenant',
  namespace: 'default',
});

// Connect and register as a thin node
const nodeId = await client.connect();
console.log('assigned node_id:', nodeId);  // echoes back the nodeId you sent

// Fire-and-forget tell
await client.tell('MyActor//MyActorType::default@<leaderNodeId>', 'my_op', { key: 'value' });

// Request-reply ask (5s timeout)
const reply = await client.ask('MyActor//MyActorType::default@<leaderNodeId>', 'get_state', {});

// Subscribe to incoming messages (server → thin node)
client.onMessage((actorId, msgType, payload) => {
  console.log(`${msgType} from ${actorId}:`, payload);
});

// Query node resource stats
const ping = await client.pingNode(nodeId);
console.log(`CPU: ${ping.cpuPercent}%, RAM: ${ping.memoryAvailableMb}MB, cores: ${ping.availableCores}`);

// Build a canonical actor ID scoped to this thin node
const myId = client.localActorId('alice', 'ChatClient', 'chat');

await client.disconnect();
```

### Interface

```typescript
interface ThinClientOptions {
  wsUrl: string;           // "ws://host:port/ws"
  jwtToken?: string;       // appended as ?token=<jwt> if present
  nodeId?: string;         // ULID preferred; server assigns one if omitted
  tenant?: string;
  namespace?: string;
}

interface ThinNodePingResult {
  success: boolean;
  nodeId: string;
  cpuPercent: number;
  memoryAvailableMb: number;
  availableCores: number;
}

class WsThinClient {
  static newUlid(): string;                               // 26-char Crockford ULID
  constructor(opts: ThinClientOptions);
  connect(): Promise<string>;                             // returns assigned node_id
  tell(actorId: string, msgType: string, payload: unknown): Promise<void>;
  ask(actorId: string, msgType: string, payload: unknown, timeoutMs?: number): Promise<unknown>;
  onMessage(handler: (actorId: string, msgType: string, payload: unknown) => void): void;
  pingNode(targetNodeId: string): Promise<ThinNodePingResult>;
  heartbeat(): Promise<void>;
  localActorId(name: string, type: string, namespace?: string): string;
  get nodeId(): string;
  disconnect(): Promise<void>;
}
```

### Wire Protocol Details

| Direction | Frame field | WsFrame oneof | Payload type |
|-----------|-------------|----------------|--------------|
| client→server | `node_register` | field 20 | `NodeRegistration` |
| server→client | `node_register_ack` | field 21 | `WsNodeRegisterAck` |
| client→server | `tell` | field 10 | `SendMessageRequest` |
| server→client | `tell_response` | field 11 | `SendMessageResponse` |
| client→server | `ask` | field 12 | `AskReplyRequest` |
| server→client | `ask_response` | field 13 | `AskReplyResponse` |
| client→server | `heartbeat` | field 24 | `SendHeartbeatRequest` |
| server→client | `heartbeat_ack` | field 25 | `WsHeartbeatAck` |
| client→server | `node_ping` | field 22 | `PingRequest` |
| server→client | `node_ping_response` | field 23 | `PingResponse` |
| server→client | `error` | field 30 | `WsError` |

The `WsThinClient` auto-heartbeats every 25 seconds to keep the session alive.

### Skeleton SDKs

Stub implementations (interface-only, pending full wire encoding) exist for:
- **Rust**: `sdks/rust/plexspaces-sdk/src/ws_thin_client.rs` — `WsThinClient`, `WsThinClientOptions`, `ThinNodePingResult`
- **Go**: `sdks/go/plexspaces/ws_thin_client.go` — `WsThinClient`, `WsThinClientOptions`, `ThinNodePingResult`
- **Python**: `sdks/python/plexspaces/ws_thin_client.py` — `WsThinClient`, `WsThinClientOptions`, `ThinNodePingResult`

## Examples

| Example | What it demonstrates |
|---------|----------------------|
| [`examples/typescript/apps/ws_chat_room`](../examples/typescript/apps/ws_chat_room/) | Multi-tab browser chat; browser tabs as thin nodes; ChatRoomActor fan-out via `host.send()` |
| [`examples/typescript/apps/mersenne_prime`](../examples/typescript/apps/mersenne_prime/) | SETI@home-style computation; browsers as workers; Lucas-Lehmer BigInt via Web Worker |

## References

- [Architecture](architecture.md) — transport layer overview
- [Detailed Design](detailed-design.md#websocket-transport) — WsRegistry, WsTransportClient internals
- [Getting Started](getting-started.md) — quick start guide
- [Security](security.md) — JWT auth and tenant isolation
