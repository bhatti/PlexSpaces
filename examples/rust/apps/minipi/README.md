# MiniPi — Agent Harness & Eval (Rust WASM)

A 12-actor eval pipeline compiled to a single WASM binary and deployed on PlexSpaces. Demonstrates durable execution, supervision trees, schema validation, human-in-the-loop, and two-tier LLM escalation — all as actor harness infrastructure rather than in-model logic.

Rust WASM port of the same pipeline available in Python, Go, and TypeScript. All four share an identical `app-config.toml` structure and `test.sh` protocol.

## Prerequisites

- Rust with `wasm32-wasip1` target: `rustup target add wasm32-wasip1`
- [`wasm-tools`](https://github.com/bytecodealliance/wasm-tools): `cargo install wasm-tools`
- WASI adapter: `npm install -g @bytecodealliance/jco`
- A running PlexSpaces node on `localhost:8091` (or pass `HTTP_PORT`)
- Optional: [Ollama](https://ollama.ai) with `llama3.2` pulled (falls back to mock)

## Usage

```bash
./build.sh              # compile → wasm-tools component embed → component new
./test.sh [HTTP_PORT]   # deploy + run all 15 integration steps
./undeploy.sh [PORT]    # tear down
```

## Actors

| Role | Behavior | Purpose |
|------|----------|---------|
| `llm_gateway` | GenServer | Ollama/mock LLM abstraction with KV response cache |
| `tool_registry` | GenServer | Tool catalog (web_search, calculator, kv_read, kv_write) with schema validation |
| `agent_runner` | Workflow | OODA loop: observe → orient → decide → act with trajectory capture |
| `eval_runner` | Workflow | Durable scenario orchestration: fan-out → score → report |
| `scorer` | GenServer | Score trajectories against rubrics (task_completion, tool_use, efficiency) |
| `scenario_store` | GenServer | KV-backed scenario catalog with named suites (smoke, standard, full) |
| `trajectory_store` | GenServer | Store/retrieve agent trajectories by ID or eval_run_id |
| `regression_detector` | GenServer | Compare score sets across eval runs, flag ±5% threshold regressions |
| `advisor` | GenServer | Two-tier LLM: cheap fast model + expensive advisor on low confidence |
| `benchmark` | Workflow | Fan-out N configs over the same scenarios, rank by pass rate |
| `approval_gate` | GenStateMachine | Human-in-the-loop FSM: idle → awaiting_approval → idle |
| `dashboard` | GenServer | Read-only aggregate view of eval runs and trajectories |

## What It Demonstrates

- **SchemaValidationFacet** (priority 95) — method input validation; bad tool calls never reach the actor
- **ExecutionTraceFacet** (priority 85) — ordered OODA step capture exported to KV on completion
- **DurabilityFacet** (priority 90) — journal every step; eval survives node crash
- **Supervision tree** (`one_for_one`) — crashed subactor restarts without stopping the eval
- **GenFSM** (ApprovalGate) — durable FSM wait; approval state survives restarts
- **Two-tier LLM** (Advisor) — cheap executor for most turns; expensive advisor on low-confidence turns

## Architecture

All 12 actors share a single WASM binary (`minipi_actor.wasm`). The `role` field in each actor's `args` config selects behavior at init time:

```
init(config) → read args.role → init_<role>()
handle(from, msg_type, payload) → read state.role → handle_<role>(msg_type, payload)
```

The single-binary pattern keeps the deployment surface small and lets PlexSpaces's virtual actor runtime place actor instances across nodes without per-role binaries.

## Live Test Output

WASM binary: 6.3M. The Rust WASM implementation has the most complete benchmark scoring and AdvisorActor implementation:

```
Step 12: BenchmarkActor — 3-config comparison
  Configs tested: 3  Winner: aggressive  Best score: 0.83  Worst: 0.73
  aggressive:   0.830  (8192tok, 20 iter)
  balanced:     0.800  (4096tok, 10 iter)
  conservative: 0.730  (1024tok, 3 iter)
  (on multi-step tasks, aggressive config wins — benchmark your actual tasks)

Step 14: AdvisorActor — two-tier LLM
  Escalation rate: 60.0%  Advisor token share: 57.3%
  (complex prompts trigger expensive advisor; 3/5 turns escalated)

Step 15: DashboardActor
  Eval runs: 2  Avg score: 0.81
```

## References

- [Blog: The Other Half of Your AI Agent](../../../../docs/blog-agent-harness.md)
- [Architecture](../../../../docs/architecture.md)
- [Getting Started](../../../../docs/getting-started.md)
- [Go minipi](../../../go/apps/minipi/) — reference implementation
- [Python minipi](../../../python/apps/minipi/) — Python SDK port
- [TypeScript minipi](../../../typescript/apps/minipi/) — TypeScript port
- [Rust embedded minipi](../../embedded/minipi/) — same pipeline using SDK macros, no WASM
