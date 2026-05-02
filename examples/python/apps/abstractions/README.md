# Abstractions (Python WASM)

This is a deployable Python WASM app example. It uses the SDK's real
multi-actor `ACTOR_ROLES` pattern and runs against a node, covering the same
lifecycle as the Rust and Go abstractions examples:

- durable virtual actor reactivation
- non-durable virtual actor reinitialization from init-config
- workflow run/signal/query
- process-group event delivery
- timers, reminders, KV, tuple space, and blob services

Run it against a running node:

```bash
./build.sh
./test.sh 8091
```
