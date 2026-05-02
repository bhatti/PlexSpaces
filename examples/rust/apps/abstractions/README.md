# Abstractions (Rust)

This example now follows the standard Rust WASM app pattern used elsewhere in the repo:

- `./build.sh` builds a deployable component WASM file at [abstractions_actor.wasm](/Users/shahzadbhatti/workspace/myspaces/examples/rust/apps/abstractions/abstractions_actor.wasm)
- `./test.sh [HTTP_PORT]` runs the native contract tests, deploys the app to a node started with [scripts/server.sh](/Users/shahzadbhatti/workspace/myspaces/scripts/server.sh), and verifies the runtime behavior
- [app-config.toml](/Users/shahzadbhatti/workspace/myspaces/examples/rust/apps/abstractions/app-config.toml) defines one module with multiple child specs so the deployed app exposes:
  - `abstractions:{id}` as a GenServer-style virtual durable actor with timer and reminder facets
  - `workflow:{id}` as a workflow-style virtual durable actor
  - `channel:{id}` as a GenEvent-style channel consumer backed by process groups
  - `controller` for stop/spawn control used by the verification flow

The WASM app exercises the same high-level abstractions as the other SDK examples:

- GenServer operations: `increment`, `status`
- Workflow operations: `workflow_run`, `workflow_signal:cancel`, `workflow_query:status`
- Event/channel operations: `publish`, direct send, process-group broadcast
- Services: key/value, tuple space, blob store
- Lifecycle: timer, reminder, stop, durable virtual-actor resume, and non-durable virtual-actor reinitialization from init-config

Run it against a local server:

```bash
./scripts/server.sh
cd examples/rust/apps/abstractions
./test.sh 8091
```

The verifier uses the actor `/ask` endpoint for steps that inspect returned payloads. The plain actor POST endpoint is tell/cast-only and returns an acknowledgement envelope, not the handler response body.

`./scripts/server.sh` uses the checked-in `release.yaml`, which now enables the local blob backend by default so this example can validate blob APIs without requiring MinIO.

The crate still keeps native Rust contract tests for the annotation-based SDK surface so the example validates both:

- native SDK authoring and facet declaration
- deployable WASM behavior through the same server/runtime path used by the other Rust examples
