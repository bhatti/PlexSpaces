# Agent coding principles

Guidelines for AI and human contributors. No backward compatibility, no legacy or dead code. Changes must be complete, match existing design and implementation, and be fully covered by tests.

---

## 1. Quality bar

- **Complete**: Every change is finished; no TODOs, stubs, or “fix later” left behind.
- **Aligned**: Follow existing design and implementation. Do not introduce duplicate or disconnected abstractions; everything must fit the current architecture.
- **Tested**: TDD with high coverage. Target **90%+** (branch/line as appropriate). Tests must pass before any commit; no broken tests.
- **Simple and robust**: No hacks, workarounds, or conditional band-aids. Use traits and proper design to solve problems (including cyclic dependencies). Solve cycles by design, not with tricks.
- **Observable**: Instrument for production observability (metrics, structured logs, tracing where applicable). No temporary debug logs in committed code.
- **Documented**: All public APIs and non-obvious logic commented. Update **docs/*.md** (and crate/example READMEs) whenever behavior or APIs change. Comments explain “why,” not “what”; do not put phrases like “production grade” in comments—the design and implementation should be production-grade by default.

---

## 2. Build and test

- **`make build`** and **`make test`** must succeed with zero errors. No committing with failing or skipped tests.
- Use a **single shared target directory** at the repo base. No separate `target/` for examples or tests. Use **debug** builds for examples/tests unless there is a clear reason for release.
- Tests must be deterministic: no flaky timing; use condition variables or other robust primitives instead of sleeps. No mocking of code we own; test real behavior.

---

## 3. Design and architecture

- **No duplicate or disconnected code**: One clear design and implementation for each concern. No parallel or “alternative” implementations that overlap in responsibility.
- **Traits over `Any`**: Prefer traits and concrete structs. Avoid `any` (or `dyn Any` in Rust) unless there is no other way; such cases must be reviewed and justified.
- **Cyclic dependencies**: Resolve by restructuring (e.g. traits, layering, new crates), not with hacks or backdoors. Flag and fix circular references; do not paper over them.
- **Client use of internals**: Client code must not use mailbox (or other low-level internals) directly except when intentionally overriding or extending default behavior. Prefer public APIs and SDK helpers.

---

## 4. SDK and API design

- **Core in main crates**: Real business logic and core functionality live in the main (Rust) crates. The SDK is a **decorator** that removes boilerplate and simplifies usage; it must not reimplement or duplicate core logic.
- **SDK surface**: Provide helpers, annotations, and wrappers so that typical client code stays thin. WASM support should wrap/expose the same underlying framework (no RPC for in-process integration). gRPC (or other RPC) can be built separately for remote access.
- **Consistency**: SDK and API behavior must be consistent across all supported languages (e.g. Rust, Python, TypeScript, Go). Same semantics and naming where the abstraction maps across languages.

---

## 5. Rust-specific conventions

- **Spawning**: In examples and docs, use **node** (or equivalent high-level spawn API) to spawn actors, not **actor-factory** directly, unless the example is specifically demonstrating low-level usage.
- **Messages**: Use **`new_message`** (or the canonical constructor) instead of **`Message::new`** in examples and docs. Prefer SDK helpers for message construction.
- **Actor definition**: Use **annotations** (e.g. `#[gen_server_actor]`, `#[handler("op")]`) and the standard SDK patterns. Do not document or encourage manual implementation of actor traits where the SDK provides an annotation-based approach. Update **docs/*.md** to use annotations and to replace any references to “ActorTrait” (or similar) with the annotation-based approach and node-based spawning.

---

## 6. No legacy or compatibility debt

- **No backward compatibility** for unreleased or internal surfaces: remove obsolete code paths, deprecated APIs, and unused options rather than carrying them forward.
- **No legacy or dead code**: Delete unused code, commented-out blocks, and obsolete branches. Do not leave “for backward compat” or “legacy” paths “just in case.”
- **No refactoring noise in comments**: Do not add comments like “X moved to Y” or “Z deleted.” Version control and the diff provide that information.

---

## 7. Documentation

- Keep **docs/*.md** and crate/example READMEs in sync with the code. When changing behavior, APIs, or patterns (e.g. spawn style, message construction, annotations), update the relevant docs.
- Ensure any references to old patterns (e.g. ActorTrait, actor-factory spawn, `Message::new`) are replaced with the current approach (annotations, node spawn, `new_message` or SDK helpers).

---

## 8. Checklist before considering work done

- [ ] `make build` and `make test` pass; no failing or skipped tests.
- [ ] No temporary or debug-only logging left in code.
- [ ] No hacks, duplicate abstractions, or unresolved cyclic dependencies.
- [ ] New/changed code has tests and meets the coverage bar (90%+ where applicable).
- [ ] Public APIs and non-obvious logic are commented; docs and **docs/*.md** (and related READMEs) are updated.
- [ ] No backward-compat, legacy, or dead code introduced; any such code in the change set is removed.
- [ ] SDK/docs use node-based spawn, annotation-based actors, and the canonical message constructors; docs no longer reference deprecated patterns (e.g. ActorTrait, actor-factory, `Message::new`) where the new approach applies.
