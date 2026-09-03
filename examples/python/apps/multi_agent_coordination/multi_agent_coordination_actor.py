# SPDX-License-Identifier: AGPL-3.0-or-later
"""Multi-Agent Coordination — 10 coordination patterns for multi-agent AI systems.

Entry point: imports and re-exports all actor classes so the PlexSpaces
Python SDK can discover them via the role-based dispatch table below.

Each actor class is implemented in its own module:
  research.py          — ResearchAgent (GenServer)
  analysis.py          — AnalysisAgent (GenServer)
  verifier.py          — VerifierAgent (GenServer)
  synthesizer.py       — SynthesizerAgent (GenServer)
  benchmark.py         — BenchmarkAgent (GenServer)
  audit.py             — AuditEventActor (GenEvent)
  coordination_fsm.py  — CoordinationFSM (GenFSM)
  coordinator.py       — CoordinatorWorkflow (Workflow)
  helpers.py           — shared utilities

## Patterns demonstrated
- Blackboard / Shared State (TupleSpace in/rd/out)
- Scatter-Gather / Fan-out (shard groups)
- Generator-Verifier (iterative refinement)
- Pipeline / Sequential (chained ask calls)
- Pub-Sub / Event Bus (process groups)
- Consensus / Voting (TupleSpace vote tuples)
- Dynamic Task Delegation (TupleSpace take for atomic claim)
- Veto Protocol (TupleSpace veto tuples)
- Two-Phase Commit / Barrier (ready signals + commit)
- Capability Discovery / Registry (TupleSpace service tuples)

Inspired by the METR/HuggingFace incident (Aug 2026) where ~1,200 agents
self-organized via shared state and invented coordination protocols.
"""

from .research import ResearchAgent
from .analysis import AnalysisAgent
from .verifier import VerifierAgent
from .synthesizer import SynthesizerAgent
from .benchmark import BenchmarkAgent
from .audit import AuditEventActor
from .coordination_fsm import CoordinationFSM
from .coordinator import CoordinatorWorkflow

ACTOR_ROLES = {
    "research":          ResearchAgent,
    "analysis":          AnalysisAgent,
    "verifier":          VerifierAgent,
    "synthesizer":       SynthesizerAgent,
    "benchmark":         BenchmarkAgent,
    "audit":             AuditEventActor,
    "coordination_fsm":  CoordinationFSM,
    "coordinator":       CoordinatorWorkflow,
}

__all__ = [
    "ResearchAgent",
    "AnalysisAgent",
    "VerifierAgent",
    "SynthesizerAgent",
    "BenchmarkAgent",
    "AuditEventActor",
    "CoordinationFSM",
    "CoordinatorWorkflow",
    "ACTOR_ROLES",
]
