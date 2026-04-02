# Abstractions (Go WASM)

This is a real deployable Go WASM app example, not a local-only SDK contract test.

It exercises the same node-backed lifecycle as the Rust abstractions example:

- durable virtual GenServer actor with checkpointed reactivation
- non-durable virtual GenServer actor that reinitializes from init-config
- workflow actor with `workflow_run`, `workflow_signal:*`, and `workflow_query:*`
- event actor with process-group delivery
- timers, reminders, KV, tuple space, and blob services
- controller actor that stops actors through the normal runtime path

Run it against a running node:

```bash
./build.sh
./test.sh 8092
```

`./build.sh` produces the WASM component at `/Users/shahzadbhatti/workspace/myspaces/target/examples/go/abstractions/abstractions_actor.wasm`, and `./test.sh` deploys that component to the node and verifies the end-to-end actor lifecycle.
