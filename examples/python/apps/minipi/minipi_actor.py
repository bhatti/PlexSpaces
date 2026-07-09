# SPDX-License-Identifier: AGPL-3.0-or-later
"""MiniPi — Agent Harness & Eval (Python WASM).

Entry point: imports and re-exports all actor classes so the PlexSpaces
Python SDK can discover them via the role-based dispatch table below.

Each actor class is implemented in its own module for clarity:
  llm_gateway.py       — LLMGatewayActor
  tool_registry.py     — ToolRegistryActor
  agent.py             — AgentActor
  eval_runner.py       — EvalRunnerActor
  scenario_store.py    — ScenarioStoreActor
  scorer.py            — ScorerActor
  trajectory_store.py  — TrajectoryStoreActor
  regression_detector.py — RegressionDetectorActor
  benchmark.py         — BenchmarkActor
  advisor.py           — AdvisorActor
  approval_gate.py     — ApprovalGateActor
  dashboard.py         — DashboardActor

## Patterns demonstrated
- GenServer (request-reply): LLMGateway, ToolRegistry, ScenarioStore, Scorer,
  TrajectoryStore, RegressionDetector, Advisor, Dashboard
- Workflow (durable multi-step): EvalRunner, Benchmark, Agent
- GenFSM (state machine): ApprovalGate
- SchemaValidationFacet (priority 95): validates method inputs before actor sees them
- ExecutionTraceFacet (priority 85): ordered OODA step capture, exports to KV
- DurabilityFacet (priority 90): journal replay, crash-safe eval workflow
- Supervision tree (one_for_one): crashed actor restarts, eval keeps running
- TupleSpace: fan-out/collect coordination between EvalRunner and AgentActors
- Two-tier LLM (AdvisorActor): cheap executor + expensive advisor on low confidence
"""

from .llm_gateway import LLMGatewayActor
from .tool_registry import ToolRegistryActor
from .agent import AgentActor
from .eval_runner import EvalRunnerActor
from .scenario_store import ScenarioStoreActor
from .scorer import ScorerActor
from .trajectory_store import TrajectoryStoreActor
from .regression_detector import RegressionDetectorActor
from .benchmark import BenchmarkActor
from .advisor import AdvisorActor
from .approval_gate import ApprovalGateActor
from .dashboard import DashboardActor

# Role → actor class dispatch table.
# The `role` field in app-config.toml args selects which class handles each
# supervisor child, letting all actors share a single WASM binary.
ACTOR_REGISTRY = {
    "llm_gateway":        LLMGatewayActor,
    "tool_registry":      ToolRegistryActor,
    "agent":              AgentActor,
    "agent_runner":       AgentActor,
    "eval_runner":        EvalRunnerActor,
    "scenario_store":     ScenarioStoreActor,
    "scorer":             ScorerActor,
    "trajectory_store":   TrajectoryStoreActor,
    "regression_detector": RegressionDetectorActor,
    "benchmark":          BenchmarkActor,
    "advisor":            AdvisorActor,
    "approval_gate":      ApprovalGateActor,
    "dashboard":          DashboardActor,
}

# ACTOR_ROLES is the name the build tool (plexspaces_cli/build.py) looks for.
# It maps role strings (from app-config.toml args.role) to actor classes,
# allowing all 12 actors to share a single WASM binary.
ACTOR_ROLES = ACTOR_REGISTRY

__all__ = [
    "LLMGatewayActor",
    "ToolRegistryActor",
    "AgentActor",
    "EvalRunnerActor",
    "ScenarioStoreActor",
    "ScorerActor",
    "TrajectoryStoreActor",
    "RegressionDetectorActor",
    "BenchmarkActor",
    "AdvisorActor",
    "ApprovalGateActor",
    "DashboardActor",
    "ACTOR_REGISTRY",
    "ACTOR_ROLES",
]
