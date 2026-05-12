# PlexSpaces Go SDK

TinyGo-oriented SDK for PlexSpaces WASM actors: implement the `Actor` interface, embed `BaseActor`, and use generated WIT imports for host calls.

The SDK also exposes a proto-first authoring metadata layer so Go stays aligned with the Rust, Python, and TypeScript SDKs:

- `DefineActor(...)`
- `GenServerActor(...)`
- `EventActor(...)`
- `FSMActor(...)`
- `FSMActorDef(factory, FSMOpts{States: [...], Initial: "...", Facets: [...]})` — FSM with explicit state list and initial state
- `WorkflowActorDefinition(...)`

Use these with `ActorRouter.RouteDefinition(...)` when you want explicit behavior/facet metadata without inventing a separate Go-specific API shape.

## Module layout

- **`plexspaces/`** — Actor API, host wrappers, router
- **`plexspaces/proto/`** — Generated Go types from Protocol Buffers (`make proto` / `make proto-go` at repo root)

Structured errors from the host can be parsed with **`HostError.ParseErrorDetail()`** when the message contains JSON aligned with `ErrorDetail`.

## Tier 1 Ergonomics Helpers

Convenience wrappers over the raw WIT host functions, available without extra imports.

### Process Groups

```go
// First member of a group (error if empty)
memberID, err := host.PG().First("svc:llm_router")
```

### KV JSON Helpers

```go
type Task struct { Seq int `json:"seq"`; Kind string `json:"kind"` }

// Write
if err := host.KVPutJSON("task:1", Task{Seq: 1, Kind: "summarize"}); err != nil { ... }

// Read (ok=false when key is missing)
var t Task
ok, err := host.KVGetJSON("task:1", &t)
```

### Metrics Helpers (on BaseActor)

```go
type MyActor struct { plexspaces.BaseActor }

func (a *MyActor) Handle(from, msgType, payload string) string {
    a.IncrCounter(host, "requests_processed")
    a.IncrCounters(host, map[string]int{"cache_hits": 5, "cache_misses": 2})
    // ...
}
```

### EventLog

Monotonic append-only log backed by KV. Embed in actor state (JSON-serializable). Multiple consumers track independent cursors.

```go
type State struct {
    Log plexspaces.EventLog `json:"log"`
}

// Append
seq, err := state.Log.Append(host, "audit:", map[string]any{"action": "login"})

// Poll (idempotent per consumer)
events, cursor, err := state.Log.Poll(host, "audit:", "consumer-1", 20)
```

### Object Registry

Service registration and discovery via the host registry surface. All wire encoding is handled by the SDK (proto-first, no JSON); the proto schema is `proto/plexspaces/v1/registry/object_registry.proto`.

```go
// Register this actor as a named service
err := host.Registry().Register(plexspaces.ObjectRegistration{
    ObjectID:       cfg.ActorID,
    ObjectType:     "actor",
    ObjectCategory: "worker",
    GRPCAddress:    "http://node:8080",
    Capabilities:   []string{"persistent"},
    Labels:         []string{"env=prod"},
})

// Look up by object ID
reg, err := host.Registry().Lookup("some-actor-id", plexspaces.ObjectType.ACTOR, "", "")

// Look up by alias
reg, err := host.Registry().LookupByAlias("my-service-alias")

// Discover all actors of a given type
regs, err := host.Registry().Discover(plexspaces.DiscoverOptions{
    ObjectType: plexspaces.ObjectType.ACTOR,
})

// Send heartbeat to refresh liveness
err = host.Registry().Heartbeat(cfg.ActorID, plexspaces.ObjectType.ACTOR, "", "")
```

**Tenant isolation**: `tenant_id` is injected by the host from the deployment context (JWT for API
deploys, or `tenant_id` in `app-config.toml` for file-copy deploys). Any `tenant_id` passed by the
actor code is silently overridden. `namespace` may be supplied; empty string falls back to the
application's default namespace.

## Documentation

- [docs/sdk.md](../../docs/sdk.md) — Go SDK section and cross-language notes
- [docs/polyglot.md](../../docs/polyglot.md) — WASM build and WIT

## Tests

From the repository root:

```bash
make test   # includes go test ./... under sdks/go
```

Or locally:

```bash
cd sdks/go && go test ./...
```

## License

AGPL-3.0-or-later
