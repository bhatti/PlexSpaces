# Resource-Aware Inference (Go WASM)

Demonstrates **Resource-Based Affinity (Pattern 4)** and **Resource-Aware Optimization (Pattern 17)** using PlexSpaces actors:

- Label-based routing to GPU/CPU inference workers based on model tier
- Cost-aware model selection driven by prompt complexity and per-tenant budget
- Per-tenant token and USD budget tracking with enforcement

## Architecture

```
routing_workflow
  ├─ budget_manager      (check budget before routing)
  ├─ model_registry      (select model tier: small/medium/large)
  ├─ inference_worker_small   (gpt-nano,  CPU, 2 GB,  $0.001/1K tokens)
  ├─ inference_worker_medium  (gpt-base,  CPU, 8 GB,  $0.010/1K tokens)
  └─ inference_worker_large   (gpt-large, GPU, 32 GB, $0.050/1K tokens)
```

### Actors

| Actor | Role |
|-------|------|
| `model_registry` | Stores model specs in KV; selects tier by complexity + budget |
| `inference_worker` | Simulates inference; tracks per-tenant usage in KV |
| `budget_manager` | Enforces per-tenant USD limits; provides spend reports |
| `routing_workflow` | WorkflowActor that orchestrates the full pipeline |

### Routing Logic

1. **Budget check** — reject early if tenant has insufficient remaining budget
2. **Complexity estimation** — short prompt (<50 chars) → 0.2, medium (50–200) → 0.5, long (>200) → 0.8
3. **Model selection** — complexity maps to tier; falls back to cheaper tier if budget is tight
4. **Inference** — routed to the worker for the selected tier
5. **Cost deduction** — actual token cost deducted from tenant budget

## Usage

```bash
# Build the WASM binary
./build.sh

# Deploy and run all tests against a running PlexSpaces node
./test.sh [HTTP_PORT]   # default port: 8091
```

## Test Scenarios

1. `list_models` — verify 3 models (small/medium/large) are seeded
2. `select_model complexity=0.2` — returns small tier
3. `select_model complexity=0.8 prefer_gpu=true` — returns large tier
4. `set_budget tenant=team-a $1.00`
5. Direct `infer` via `inference_worker_small` (short prompt)
6. Direct `infer` via `inference_worker_large` (long prompt)
7. `check_budget team-a` after usage
8. `routing_workflow` short prompt → routed to small tier
9. `routing_workflow` long prompt + prefer_gpu → routed to medium/large tier
10. `get_report` from budget_manager
11. Exceed budget: set $0.001, consume it, verify workflow rejects with `budget_exceeded`
12. `workflow_query cost_report` via routing_workflow

## References

- [Architecture](../../../../docs/architecture.md)
- [Getting Started](../../../../docs/getting-started.md)
- [Detailed Design](../../../../docs/detailed-design.md)
