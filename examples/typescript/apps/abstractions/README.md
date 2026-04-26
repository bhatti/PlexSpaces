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
cd /Users/shahzadbhatti/workspace/myspaces
make build
./scripts/server.sh 8091
```

Then in a second shell:

```bash
cd /Users/shahzadbhatti/workspace/myspaces/examples/typescript/apps/abstractions
./build.sh
./test.sh 8092
```

`test.sh` refuses to run against a node that was not started by the current repo's
`./scripts/server.sh` flow. That prevents stale-node failures where the TypeScript guest was
rebuilt but the Rust backend reactivation logic was still coming from an older process.
