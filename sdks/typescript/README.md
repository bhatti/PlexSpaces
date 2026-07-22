# PlexSpaces TypeScript SDK

Build PlexSpaces actors with minimal boilerplate using **inheritance** (TypeScript analogue to the Python SDK’s decorators).

## Installation

From the repo (development):

```bash
cd sdks/typescript
npm install
npm run build
```

Proto-shaped SDK types live in **`src/proto.ts`** and keep canonical proto field names even when generated files are not present yet. When you run **`make proto`** or **`make proto-typescript`**, richer generated models can be added on top, but application code can already stay proto-first at the SDK boundary.

The SDK now supports decorator-based authoring metadata in addition to the existing base classes:

- `@actor`
- `@gen_server_actor`
- `@event_actor`
- `@fsm_actor` / `@fsm_actor({ states: [...], initial: "..." })` — FSM actor with optional state list and initial state
- `@workflow_actor`
- `@run_handler`
- `@signal_handler("name")`
- `@query_handler("name")`
- `@handler`

These decorators align the TypeScript surface with the Rust and Python SDKs while keeping `PlexSpacesActor` and `WorkflowActor` as the runtime substrate.

In an example or app, add a dependency:

```json
"dependencies": {
  "@plexspaces/sdk": "file:../../../../sdks/typescript"
}
```

## Tests

From the repo root, **`make test`** runs **`npm test`** here (`node --test`). Locally: `cd sdks/typescript && npm test`.

## Quick start

### 1. Extend `PlexSpacesActor`

Override `getDefaultState()` and add handlers as `on<Op>(payload)` methods. The base class wires WIT `init`, `handle`, `getState`, `setState` and dispatches by `payload.op`.

```ts
import { PlexSpacesActor } from "@plexspaces/sdk";

interface MyState {
  count: number;
}

export class CounterActor extends PlexSpacesActor<MyState> {
  getDefaultState(): MyState {
    return { count: 0 };
  }

  onIncrement(payload: Record<string, unknown>) {
    const amount = Number(payload.amount ?? 1);
    this.state.count += amount;
    return { count: this.state.count };
  }

  onGet() {
    return { count: this.state.count };
  }
}

const instance = new CounterActor();
export const actor = {
  init: (c: string) => instance.init(c),
  handle: (from: string, msg: string, payload: string) => instance.handle(from, msg, payload),
  getState: () => instance.getState(),
  setState: (s: string) => instance.setState(s),
};
```

### 2. Build to WASM

- Compile: `tsc`
- Bundle (e.g. esbuild) your entry into a single ESM file that exports `actor`.
- Build WASM: `jco componentize your-bundle.mjs --wit wit/plexspaces-actor -o actor.wasm --disable all`

See [examples/typescript/apps/bank_account](../../examples/typescript/apps/bank_account) for a full example and E2E test.

## Virtual Actors (Orleans-Style)

Virtual actors are automatically activated on first message and deactivated after idle timeout. Configure via `app-config.toml`:

```toml
[[supervisor.children]]
id = "my-actor"
type = "worker"
facets = [
  { type = "virtual_actor", priority = 100, config = { idle_timeout = "5m", activation_strategy = "lazy" } }
]
```

The TypeScript SDK works seamlessly with virtual actors - no code changes needed. The framework handles activation/deactivation automatically.

## API

- **`PlexSpacesActor<TState>`**  
  Base class. `TState` is your state shape (plain object).

- **`getDefaultState(): TState`**  
  Override to return initial state.

- **`onInit(config: Record<string, unknown>): void`**  
  Optional. Called from `init()` with parsed config JSON.

- **Handlers**  
  Implement `on<Op>(payload)` for each message op (e.g. `onDeposit`, `onBalance`). The runtime calls `handle(from, msgType, payloadJson)` and the base class dispatches by `payload.op` (or `payload`) to `on` + capitalized op name. Return a JSON-serializable value; the base class serializes it (or `"ERROR:..."` on throw).

- **`protected state: TState`**  
  Current state; read/write in handlers.

- **`protected json(obj), error(msg)`**  
  Helpers for returning JSON or error strings.

## WIT Interface

The SDK targets the **plexspaces-actor** WIT world: `init`, `handle`, `get-state`, `set-state`. All data crosses the boundary as strings (JSON). See `wit/plexspaces-actor/world.wit`.

### Host Functions

The SDK uses WIT host functions for runtime capabilities. Host functions are provided by the PlexSpaces runtime when the WASM component is instantiated.

**Virtual Import Pattern**: jco componentize uses virtual imports for WIT host interfaces. The SDK handles these internally - you don't need to import them directly.

The SDK automatically handles logging via `host.log()` for error reporting and observability. In non-WASM environments (e.g., Node.js verification), host functions are undefined and logging gracefully degrades to no-op.

**Available Host Functions** (from `plexspaces:actor/host@0.1.0`):
- `log(level: string, message: string)` - Log a message (used internally by SDK)
- `send(to: string, msgType: string, payloadJson: string)` - Send message to another actor
- `now_ms()` - Get current timestamp in milliseconds
- `kv_get(key: string)`, `kv_put(key: string, value: string)`, etc. - Key-value storage
- **TupleSpace**: Prefer `host.ts` (list-in, list-out): `host.ts.write(tuple)`, `host.ts.take(pattern)` → tuple or null, `host.ts.readAll(pattern)` → tuple[]. Use `null` in patterns for wildcards. Low-level: `ts_write(tupleJson)`, `ts_take(patternJson)`, etc.
- **Process groups**: `host.processGroups.broadcast(group, msgType, payload)` — `msgType` is used by the host for routing; payload can be data-only.
- **Elastic pool**: `host.poolCheckout(poolName, timeoutMs)` → `{ actor_id, pool_name, checkout_id } | null`; `host.poolCheckin(poolName, actorId, checkoutId, healthy)`; `host.poolGetMetrics(poolName)` → metrics object or null. When the pool is not configured, checkout returns null; use process group broadcast as fallback. See [Parameter sweep (migrating_merlin)](../../examples/typescript/apps/migrating_merlin/README.md).
- **Object Registry**: `host.registry.register(reg)`, `host.registry.lookup(id, type)`, `host.registry.lookupByAlias(alias)`, `host.registry.discover(opts)`, `host.registry.heartbeat(id, type)`. All wire encoding is proto-first (see below).
- See `wit/plexspaces-actor/world.wit` for complete interface

**Note**: The SDK uses `host.log()` internally for error logging. For custom logging in your actors, you can import and use host functions directly using the virtual import pattern (future enhancement: SDK may provide helper methods).

### Tier 1 Ergonomics Helpers

Convenience wrappers on the `host` singleton and `EventLog` class.

```typescript
import { host, EventLog } from "@plexspaces/sdk";

// Process Groups — first member
const routerId: string | null = host.processGroups.first("svc:llm_router");
const routerIdStrict: string = host.processGroups.firstOrThrow("svc:llm_router");

// KV JSON helpers
interface Task { seq: number; kind: string; }
host.kvPutJson("task:1", { seq: 1, kind: "summarize" });
const task: Task | null = host.kvGetJson<Task>("task:1");

// Metrics (errors swallowed)
host.incrCounter("my-app", "requests_processed");
host.incrCounters("my-app", { cacheHits: 5, cacheMisses: 2 });

// EventLog — embed in actor state
class MyActor extends PlexSpacesActor<{ auditLog: EventLog }> {
  getDefaultState() { return { auditLog: new EventLog() }; }

  onRecord(payload: unknown) {
    const seq = this.state.auditLog.append(host, "audit:", payload);
    return { seq };
  }

  onPoll(payload: { consumerId: string; limit?: number }) {
    const [events, cursor] = this.state.auditLog.poll(
      host, "audit:", payload.consumerId, payload.limit ?? 50
    );
    return { events, cursor };
  }
}
```

### Object Registry

Service registration and discovery via the host registry surface. The SDK handles protobuf wire encoding automatically; the proto schema is `proto/plexspaces/v1/registry/object_registry.proto`.

```typescript
import { host, RegistryObjectType } from "@plexspaces/sdk";

// Register this actor as a service
host.registry.register({
  objectId: actorId,
  objectType: "actor",
  objectCategory: "worker",
  grpcAddress: "http://node:8080",
  capabilities: ["persistent"],
  labels: ["env=prod"],
});

// Lookup by object ID
const reg = host.registry.lookup(actorId, RegistryObjectType.ACTOR);

// Lookup by alias
const reg2 = host.registry.lookupByAlias("my-service-alias");

// Discover all actors of a type
const regs = host.registry.discover({ objectType: RegistryObjectType.ACTOR });

// Send heartbeat to refresh liveness
host.registry.heartbeat(actorId, RegistryObjectType.ACTOR);
```

**Tenant isolation**: `tenant_id` is injected by the host from the deployment context (JWT for API deploys, or `tenant_id` in `app-config.toml` for file-copy deploys). Any `tenant_id` in the SDK call is silently overridden. `namespace` may be supplied; empty string falls back to the application's default namespace.

### Type Generation

WIT TypeScript types are generated automatically as part of the SDK build process (`npm run build`). These types are optional and only used internally by the SDK for type checking. Client code doesn't need to generate or import these types - the SDK abstracts all WIT details away.

## WsThinClient — Browser & Node.js Thin-Node SDK

`WsThinClient` connects a browser tab (or Node.js process) to a PlexSpaces node
as a **thin node** over a binary protobuf WebSocket connection.  Zero runtime
dependencies — all wire encoding is hand-rolled in `src/wire/ws-frame-wire.ts`.

### Thin Node vs Full Node

| | Full Node | Thin Node |
|---|---|---|
| Connection | gRPC or WebSocket | WebSocket only |
| Hosts WASM actors | ✓ | ✗ |
| Receives actor messages | ✓ | ✓ (via `onMessage`) |
| Sends tell/ask | ✓ | ✓ |
| Participates in SWIM | ✓ | ✗ (filtered out of indirect ping targets) |
| Node role | `NODE_ROLE_FULL` | `NODE_ROLE_THIN` |

### Quick Start

```typescript
import { WsThinClient, ActorID } from "@plexspaces/sdk";

const client = new WsThinClient({
  wsUrl: "ws://localhost:8091/ws",
  jwtToken: optionalJwt,           // appended as ?token= query param
  nodeId: WsThinClient.newUlid(),  // ULID preferred; server assigns one if omitted
  namespace: "my-app",
});

// Connect — sends NodeRegistration handshake, receives assigned node_id
const nodeId = await client.connect();

// Build a canonical actor ID on this thin node
const myActorId = client.localActorId("alice", "ChatClient", "my-app");
// → "alice//ChatClient::my-app@<assignedNodeId>"

// Build an ID for a server-side actor
const roomId = new ActorID("lobby", "ChatRoomActor", "my-app", leaderNodeId).toString();

// Fire-and-forget tell
await client.tell(roomId, "send", { sender_actor_id: myActorId, text: "hello" });

// Request-reply ask (5s timeout)
const reply = await client.ask(roomId, "status", {});

// Receive incoming tells addressed to this thin node
client.onMessage((actorId, msgType, payload) => {
  if (msgType === "chat_message") console.log(payload);
});

// Ping another node for resource hints
const ping = await client.pingNode("other-node-id");
// → { success, nodeId, cpuPercent, memoryAvailableMb, availableCores }

await client.disconnect();
```

### Interface

```typescript
export interface ThinClientOptions {
  wsUrl: string;        // "ws://host:port/ws"
  jwtToken?: string;    // optional Bearer token
  nodeId?: string;      // ULID preferred; auto-generated if omitted
  tenant?: string;      // placed in NodeRegistration.capabilities["tenant"]
  namespace?: string;   // placed in NodeRegistration.capabilities["namespace"]
}

// Typed view of PingResponse resource fields (proto fields 9–11).
// Not a proto message — TypeScript interface over decoded response.
export interface ThinNodePingResult {
  success: boolean;
  nodeId: string;
  cpuPercent: number;         // 0–100, from sysinfo on the target node
  memoryAvailableMb: number;  // available RAM in MiB
  availableCores: number;     // logical CPU count
}

export class WsThinClient {
  constructor(opts: ThinClientOptions)

  connect(): Promise<string>         // returns assigned node_id; starts 25s heartbeat
  tell(actorId, msgType, payload): Promise<void>
  ask(actorId, msgType, payload, timeoutMs?): Promise<unknown>
  onMessage(handler: (actorId, msgType, payload) => void): void
  pingNode(targetNodeId, timeoutMs?): Promise<ThinNodePingResult>
  heartbeat(): Promise<void>
  localActorId(name, type, namespace?): string  // "{name}//{type}::{ns}@{assignedNodeId}"
  get nodeId(): string
  static newUlid(): string           // 26-char ULID, Crockford base32, crypto random
  disconnect(): Promise<void>
}
```

### Wire Protocol

Binary protobuf `WsFrame` over WebSocket (`binaryType = 'arraybuffer'`).
Field numbers are defined in `proto/plexspaces/v1/transport/websocket.proto`
and documented in `src/wire/ws-frame-wire.ts`.

| Operation | WsFrame field | Direction |
|---|---|---|
| NodeRegistration handshake | field 20 → ACK field 21 | client → server → client |
| tell (fire-and-forget) | field 10 | client → server |
| tell_response | field 11 | server → client |
| ask (request-reply) | field 12 → response field 13 | client → server → client |
| heartbeat | field 24 → ACK field 25 | client → server → client |
| node_ping | field 22 → response field 23 | client → server → client |
| incoming_tell (server push) | field 10 | server → client |

### Examples

- [`ws_chat_room`](../../examples/typescript/apps/ws_chat_room/README.md) — real-time chat via PG broadcast
- [`mersenne_prime`](../../examples/typescript/apps/mersenne_prime/README.md) — distributed computation with Web Workers

## License

AGPL-3.0-or-later
