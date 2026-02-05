# PlexSpaces TypeScript SDK

Build PlexSpaces actors with minimal boilerplate using **inheritance** (TypeScript analogue to the Python SDK’s decorators).

## Installation

From the repo (development):

```bash
cd sdks/typescript
npm install
npm run build
```

In an example or app, add a dependency:

```json
"dependencies": {
  "@plexspaces/sdk": "file:../../../../sdks/typescript"
}
```

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
- Build WASM: `jco componentize your-bundle.mjs --wit wit/plexspaces-simple-actor -o actor.wasm --disable all`

See [examples/typescript/apps/bank_account](../../examples/typescript/apps/bank_account) for a full example and E2E test.

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

## WIT

The SDK targets the **plexspaces-simple-actor** WIT world: `init`, `handle`, `get-state`, `set-state`. All data crosses the boundary as strings (JSON). See `wit/plexspaces-simple-actor/world.wit`.

## License

LGPL-2.1-or-later
