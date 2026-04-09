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
- `@fsm_actor`
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
- See `wit/plexspaces-actor/world.wit` for complete interface

**Note**: The SDK uses `host.log()` internally for error logging. For custom logging in your actors, you can import and use host functions directly using the virtual import pattern (future enhancement: SDK may provide helper methods).

### Type Generation

WIT TypeScript types are generated automatically as part of the SDK build process (`npm run build`). These types are optional and only used internally by the SDK for type checking. Client code doesn't need to generate or import these types - the SDK abstracts all WIT details away.

## License

LGPL-2.1-or-later
