# Session Manager App

**Real-world timer example**: user session with **idle timeout** (one-shot) and **heartbeat** (periodic) using PlexSpaces `TimerFacet`.

- **Location**: `examples/rust/apps/session_manager/` (same layout as TypeScript apps under `examples/typescript/apps/`).
- **Build**: Debug by default for tests/examples (`cargo build`, `cargo run`).
- **Future**: Deploy as WASM app (when Rust SDK + WASM target is ready), analogous to TypeScript `bank_account`.

## Quick start

```bash
cd examples/rust/apps/session_manager
cargo build
cargo run --bin session_manager
```

Or run the test script (debug build + run with timeout):

```bash
./scripts/test.sh
```

## What it demonstrates

1. **TimerFacet** attached to a session actor.
2. **idle_timeout**: one-shot timer (e.g. 5s) – session expires if no activity.
3. **heartbeat**: periodic timer (e.g. 2s) – keep-alive / health ping.
4. **Registration from main**: after spawn, `node.get_facets(actor_id)` + downcast to `TimerFacet` to call `register_once` / `register_periodic`.

## PlexSpaces APIs (SDK style)

- **SDK spawn**: `plexspaces_sdk::spawn_actor(ctx, service_locator, actor_id, namespace, SessionActor::new(...), facets)` — facets passed at spawn (like Python `@actor(facets=[...])`).
- **Handler dispatch**: `plexspaces_impl_handlers!(SessionActor, BehaviorType::Custom("SessionActor".into()), timer_fired => handle_timer_fired, activity => handle_activity)` — message_type → method (like Python `@handler("timer_fired")`).
- `NodeBuilder`, `RequestContext`, `TimerFacet::new()`, `register_once()`, `register_periodic()`
- `node.get_facets()`, facet downcast via `as_any().downcast_ref::<TimerFacet>()`

## Convention

Examples and tests use **debug** builds; use `cargo build` / `cargo run` (no `--release`) unless you need optimized binaries.
