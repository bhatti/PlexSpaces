# Examples Rewrite Checklist

## Goal
Rewrite ALL examples from scratch:
- Simple, focused on demonstrating capabilities
- Use actual framework APIs (no helper wrappers)
- If API is too complex, simplify the framework itself

## Status Legend
- ⬜ Not started
- 🔄 In progress  
- ✅ Complete
- ❌ Blocked

---

## Phase 1: Core Patterns (8 examples)

These demonstrate fundamental PlexSpaces patterns.

| # | Example | Status | APIs Used |
|---|---------|--------|-----------|
| 1 | `actor_groups_sharding` | ✅ | ActorBuilder, tell(), Message::json() |
| 2 | `supervision_tree` | ✅ | Supervisor, ChildSpec, RestartStrategy |
| 3 | `durable_actor` | ✅ | DurabilityFacet, JournalStorage |
| 4 | `timers` | ✅ | **TimerFacet**, register_once/periodic |
| 5 | `reminders` | ✅ | **ReminderFacet**, ReminderRegistration |
| 6 | `chat_room` | ✅ | ProcessGroupRegistry, publish_to_group |
| 7 | `feature_flags` | ✅ | ProcessGroupRegistry |
| 8 | `webhook_handler` | ✅ | ActorBuilder, tell(), Message::json() |

---

## Phase 2: Scientific Computing (5 examples)

Demonstrate parallel/distributed computation patterns.

| # | Example | Status | APIs Used |
|---|---------|--------|-----------|
| 9 | `heat_diffusion` | ✅ | TupleSpace (demonstrated in code) |
| 10 | `matrix_multiply` | ✅ | ActorBuilder, tell() |
| 11 | `mpi_collectives` | ✅ | ProcessGroupRegistry, TupleSpace |
| 12 | `byzantine` | ✅ | Application, BehaviorRegistry, ActorFactory |

## Phase 3: Python WASM Apps (examples/python/apps/)

| # | Example | Status | APIs Used |
|---|---------|--------|-----------|
| 13 | `nbody` | ✅ | Python WASM, handle_message(), JSON |
| 14 | `counter` | ✅ | GenServer pattern, handle_request() |
| 15 | `calculator` | ✅ | handle_request() |

---

## Summary

| Phase | Count | Status |
|-------|-------|--------|
| Core Patterns | 8 | ✅ 8/8 |
| Scientific Computing | 4 | ✅ 4/4 |
| Python WASM Apps | 3 | ✅ 3/3 |
| **Total Complete** | **15** | - |

---

## API Usage Verification

All examples now use correct PlexSpaces APIs:

| API | Example(s) | Status |
|-----|------------|--------|
| **TimerFacet** | timers | ✅ Fixed |
| **ReminderFacet** | reminders | ✅ Fixed |
| ActorBuilder.spawn() | actor_groups_sharding, webhook_handler, matrix_multiply | ✅ |
| ProcessGroupRegistry | chat_room, feature_flags, mpi_collectives | ✅ |
| DurabilityFacet | durable_actor | ✅ |
| Supervisor | supervision_tree | ✅ |
| Application trait | byzantine | ✅ |
| TupleSpace | heat_diffusion | ✅ |

---

## Files Structure

Each example follows:
```
example_name/
├── .cargo/config.toml    # Shared target dir
├── Cargo.toml            # Minimal deps
├── README.md             # Documentation
└── src/main.rs           # Implementation
```
