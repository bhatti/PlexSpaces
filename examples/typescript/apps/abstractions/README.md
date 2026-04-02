# Abstractions (TypeScript WASM)

This is a deployable TypeScript WASM app example. It uses the SDK's real
`ActorRouter` + `PlexSpacesActor` entrypoint pattern and exercises the same
node-backed lifecycle as the Rust, Go, and Python abstractions examples:

- durable virtual actor reactivation
- non-durable virtual actor reinitialization from init-config
- workflow run/signal/query
- process-group event delivery
- timers, reminders, KV, tuple space, and blob services

Run it against a running node:

```bash
./build.sh
./test.sh 8092
```
