# SPDX-License-Identifier: AGPL-3.0-or-later
"""MiniClaw — Mini Agent Framework (Python WASM).

Entry point: imports and re-exports all actor classes so the PlexSpaces
Python SDK can discover them via the role-based dispatch table below.

Each actor class is implemented in its own module for clarity:
  llm_router.py    — LLMRouterActor
  tool_registry.py — ToolRegistryActor
  agent.py         — AgentActor, SessionManagerActor
  orchestrator.py  — OrchestratorActor
  memory.py        — MemoryActor, AuditEventActor, AgentStateFSM
  infra.py         — TaskQueueActor, HealthMonitorActor
  helpers.py       — shared utilities (pg_first, fire_audit, …)

## Patterns demonstrated
- GenServer (request-reply): LLMRouter, ToolRegistry, Agent, SessionManager, Memory, TaskQueue, HealthMonitor
- Workflow (durable multi-step): Orchestrator
- GenEvent (fire-and-forget): AuditEvent
- GenFSM (state machine): AgentStateFSM
- Channel as Message Queue: TaskQueueActor uses host.channel for durable delivery
- Process Groups for service discovery: pg_first("svc:xxx")
- KV for persistent state: session metadata, memory
- TupleSpace for coordination: orchestrator results, memory queries, health snapshots
- send_after for polling: HealthMonitor tick loop
"""

from .llm_router import LLMRouterActor
from .tool_registry import ToolRegistryActor
from .agent import AgentActor, SessionManagerActor
from .orchestrator import OrchestratorActor
from .memory import MemoryActor, AuditEventActor, AgentStateFSM
from .infra import TaskQueueActor, HealthMonitorActor

# Role → actor class dispatch table.
# The `role` field in app-config.toml args selects which class handles each
# supervisor child, letting all actors share a single WASM binary.
ACTOR_REGISTRY = {
    "llm_router":      LLMRouterActor,
    "tool_registry":   ToolRegistryActor,
    "agent":           AgentActor,
    "session_manager": SessionManagerActor,
    "orchestrator":    OrchestratorActor,
    "memory":          MemoryActor,
    "audit_event":     AuditEventActor,
    "agent_fsm":       AgentStateFSM,
    "task_queue":      TaskQueueActor,
    "health_monitor":  HealthMonitorActor,
}

__all__ = [
    "LLMRouterActor",
    "ToolRegistryActor",
    "AgentActor",
    "SessionManagerActor",
    "OrchestratorActor",
    "MemoryActor",
    "AuditEventActor",
    "AgentStateFSM",
    "TaskQueueActor",
    "HealthMonitorActor",
    "ACTOR_REGISTRY",
]
