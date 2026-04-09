# Abstractions (Go WASM)

This is a real deployable Go WASM app example, not a local-only SDK contract test.

It exercises the same node-backed lifecycle as the Rust abstractions example:

- durable virtual GenServer actor with checkpointed reactivation
- non-durable virtual GenServer actor that reinitializes from init-config
- workflow actor with `workflow_run`, `workflow_signal:*`, and `workflow_query:*`
- event actor with process-group delivery
- timers, reminders, KV, tuple space, and blob services
- controller actor that stops actors through the normal runtime path

## Build

```bash
./build.sh
```

Output: `<repo-root>/target/examples/go/abstractions/abstractions_actor.wasm`.

## Tests

**SDK / wire (no node):** TupleSpace host payloads are protobuf `WriteRequest` / `ReadRequest` / `ReadResponse` on WASM; the Go SDK tests that encoding against the same layout as the Rust guest:

```bash
cd ../../../../sdks/go && go test ./plexspaces/... -count=1
```

**End-to-end (requires a running node):** `test.sh` deploys the app, drives HTTP `ask` calls (including Step 5: KV, `ts_write` / `ts_read` with pattern `["abstractions","task","*"]`, blob), and tears down. Pass the node HTTP port (default `8092`):

```bash
./build.sh
./test.sh 8092
```

To capture server-side traces while debugging, enable debug logging on the node and keep a log file (for example `plexspaces-node-8091.log`) open; tuple matching issues usually show up as empty reads or decode errors on the WASM host path.
