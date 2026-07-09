# MiniPi — Agent Harness & Eval (Python WASM)

MiniPi demonstrates how to build production-grade agent harness and eval infrastructure on PlexSpaces — without touching model code. It implements the full eval pipeline (Scenario → Run → Trace → Score → Diagnose → Rerun) using PlexSpaces actor primitives.

Read the companion blog post: [The Other Half of Your AI Agent](../../../../docs/blog-agent-harness.md)

WASM binary: 47M. Most readable actor code — all 12 actors in separate `.py` files with Python SDK decorators.

---

## Actors

| File | Actor | Behavior | Key Abstractions |
|------|-------|----------|------------------|
| `agent.py` | `AgentActor` | `@workflow_actor` + AgentLoop | OODA loop, token budget, trajectory capture, human-in-the-loop suspend |
| `llm_gateway.py` | `LLMGatewayActor` | `@actor` (GenServer) | Ollama + mock providers, KV response cache, confidence injection |
| `tool_registry.py` | `ToolRegistryActor` | `@actor` (GenServer) | SchemaValidationFacet validates method input; 4 built-in tools |
| `eval_runner.py` | `EvalRunnerActor` | `@workflow_actor` | Durable eval orchestration, parallel agent fan-out, TupleSpace collection |
| `scenario_store.py` | `ScenarioStoreActor` | `@actor` (GenServer) | KV scenario catalog, named suites, 10 built-in scenarios |
| `scorer.py` | `ScorerActor` | `@actor` (GenServer) | Rubric scoring (task_completion, tool_use, efficiency, llm_judge) |
| `trajectory_store.py` | `TrajectoryStoreActor` | `@actor` (GenServer) | KV trajectory persistence, TupleSpace index |
| `regression_detector.py` | `RegressionDetectorActor` | `@actor` (GenServer) | Score comparison, ±5% threshold, regression/improvement flags |
| `benchmark.py` | `BenchmarkActor` | `@workflow_actor` | N-config eval fan-out, comparison table |
| `advisor.py` | `AdvisorActor` | `@actor` (GenServer) | Two-tier LLM: cheap executor + expensive advisor on low confidence |
| `approval_gate.py` | `ApprovalGateActor` | GenFSM | Human-in-the-loop: idle → awaiting_approval → approved/rejected → idle |
| `dashboard.py` | `DashboardActor` | `@actor` (GenServer) | Read-only result aggregation, KV scan |

---

## Facets Demonstrated

```
virtual_actor(100) → schema_validation(95) → durability(90) → execution_trace(85) → metrics(80)
```

- **SchemaValidationFacet** (priority 95) — rejects malformed tool calls before actor sees them
- **ExecutionTraceFacet** (priority 85) — captures ordered OODA steps, exports trajectory to KV + TupleSpace
- **DurabilityFacet** (priority 90) — journals every workflow step; crash-safe with checkpoint replay
- **Supervision tree** `one_for_one` — each crashed actor restarts independently

---

## Object Registry Integration

Each actor self-registers in `on_init` using `host.registry.register`:

```python
host.registry.register(None, self.actor_id, "actor", "", object_category="llm_gateway")
```

Service discovery uses `host.registry.discover` first, with fallback to same-node peer ID:

```python
def _find_service(self, service_type: str) -> str:
    try:
        regs = host.registry.discover(None, object_category=service_type, limit=1)
        if regs:
            return regs[0]["object_id"]
    except Exception:
        pass
    # Fallback: same-node peer ID construction
    idx = self.actor_id.find("//")
    if idx >= 0:
        return service_type + self.actor_id[idx:]
    return service_type
```

This makes multi-node deployments work correctly — the registry returns the actual actor ID regardless of which node it runs on.

---

## Live Test Output

```
Step 9: ScorerActor — score trajectory
  Score: 0.85  (rubric: task_completion)
  Score: 0.80  (rubric: tool_use)

Step 10: EvalRunnerActor — 5-scenario standard suite
  Pass rate: 0.4  Avg score: 0.76  Completed: 5 / 5
  Tokens: 0 in / 0 out  (est. cost: $0)
  sc-math-01: 0.70  sc-search-01: 0.70  sc-calc-01: 0.70
  sc-reason-01: 0.85  sc-budget-01: 0.85

Step 11: RegressionDetectorActor
  Regressions: 1  (sc-search-01 degraded by 0.20)
  Improvements: 1  (sc-reason-01 improved by 0.05)

Step 12: BenchmarkActor — 3-config comparison
  Configs tested: 3  Winner: conservative  Best score: 0.7
  conservative: 0.700 (1024tok, 3 iter)
  balanced:     0.700 (4096tok, 10 iter)
  aggressive:   0.700 (8192tok, 20 iter)
  (on simple arithmetic tasks, all configs tie — benchmark your actual tasks)

Step 13: ApprovalGateActor — human-in-the-loop
  idle → awaiting_approval → idle  Decisions: 1

Step 14: AdvisorActor — two-tier LLM
  Escalation rate: 40.0%  Advisor token share: 33.6%

Step 15: DashboardActor
  Total evals: 4  Avg score: 0.767
  bench-001: 0.700/70%  eval-smoke-001: 0.760/40%
  eval-smoke-002: 0.730/40%  test-999: 0.880/90%
```

---

## Architecture

```
User / CLI
  │
  ├── ask eval_runner ──► EvalRunnerActor (Workflow, durable)
  │                           │
  │                           ├── ScenarioStoreActor (get scenarios)
  │                           ├── spawn N AgentActors (OODA loop)
  │                           │       └── LLMGatewayActor + ToolRegistryActor
  │                           ├── collect trajectories (TupleSpace)
  │                           ├── ScorerActor (score each trajectory)
  │                           └── RegressionDetectorActor (compare to baseline)
  │
  ├── ask benchmark ──► BenchmarkActor (Workflow)
  │                         └── N × EvalRunnerActor with different configs
  │
  └── ask dashboard ──► DashboardActor (read-only aggregator)
```

---

## Prerequisites

- PlexSpaces node running on `localhost:8091`
- Python 3.11+ with `componentize-py` (`pip install componentize-py`)
- PlexSpaces Python SDK (`pip install -e ../../../../sdks/python`)
- Ollama with `llama3.2` for real LLM calls (optional — mock fallback included)

---

## Running

```bash
# 1. Build the WASM binary
./build.sh    # produces minipi_actor.wasm (47M)

# 2. Run integration tests
./test.sh 8091

# 3. Use real Ollama (optional)
# Edit app-config.toml, set: args = { provider = "ollama", model = "llama3.2" }
```

---

## Built-in Scenarios (10)

| ID | Name | Rubric | Tags |
|----|------|--------|------|
| `sc-math-01` | Simple multiplication | task_completion | math |
| `sc-calc-01` | Step-by-step arithmetic | task_completion | math, calculator |
| `sc-search-01` | Web search intent | tool_use | search |
| `sc-reason-01` | Logical deduction | task_completion | reasoning |
| `sc-budget-01` | Quadratic equation summary | task_completion | math |
| `sc-contract-01` | Expression validation | task_completion | validation, math |
| `sc-multi-01` | Multi-step tool use | tool_use | multi-step |
| `sc-kv-01` | KV store round-trip | tool_use | kv |
| `sc-chain-01` | Chained computation | task_completion | math |
| `sc-compare-01` | Power comparison | task_completion | math |

Named suites: `smoke` (1 scenario), `standard` (5), `full` (10).

---

## References

- [Blog: The Other Half of Your AI Agent](../../../../docs/blog-agent-harness.md)
- [Architecture](../../../../docs/architecture.md)
- [Go minipi](../../../go/apps/minipi/) — reference implementation (1.5M WASM)
- [TypeScript minipi](../../../typescript/apps/minipi/) — TypeScript port
- [Rust WASM minipi](../../../rust/apps/minipi/) — Rust WASM port
- [Rust embedded minipi](../../../rust/embedded/minipi/) — Rust native port
