# SPDX-License-Identifier: AGPL-3.0-or-later
"""MiniHermes — Self-Improving AI Agent (Python WASM).

Entry point: imports all actor classes and builds the role-dispatch table.
The PlexSpaces runtime calls handle(actor_id, caller_id, op, payload) for each
message; the SDK routes by the `role` field in app-config.toml args.

Actors:
  llm_gateway.py         — LLMGatewayActor: provider routing (Ollama/OpenAI/Anthropic + simulated)
  tool_executor.py       — ToolExecutorActor: 6 built-in tools + extensible registry
  agent.py               — AgentActor: self-improving loop, skill injection, cron execution
  skill_store.py         — SkillStoreActor: propose/match/lifecycle for learned skills
  skill_workflow.py      — SkillExtractionWorkflow: durable parallel skill extraction
  memory.py              — MemoryActor: core/reachable/deep tiered storage
  memory.py              — AuditEventActor: watermark audit trail + two-cursor polling
  context_compressor.py  — ContextCompressorActor: LLM-assisted context summarization
  cron_scheduler.py      — CronSchedulerActor: distributed cron with DistributedLock
  guardrails.py          — GuardrailsGateActor: per-tool allow/review/deny policies
  infra.py               — SessionManagerActor, HealthMonitorActor

PlexSpaces primitives demonstrated:
  KV, TupleSpace, BlobStorage, Channel, DistributedLock, ProcessGroups,
  ObjectRegistry, SendAfter, HTTPFetch, Ask/Send, Metrics, Durability, Workflow
"""

from llm_gateway import LLMGatewayActor
from tool_executor import ToolExecutorActor
from agent import AgentActor
from skill_store import SkillStoreActor
from skill_workflow import SkillExtractionWorkflow
from memory import MemoryActor, AuditEventActor
from context_compressor import ContextCompressorActor
from cron_scheduler import CronSchedulerActor
from guardrails import GuardrailsGateActor
from infra import SessionManagerActor, HealthMonitorActor

# Role → actor class. The `role` arg in app-config.toml selects the class for
# each supervisor child — all actors share a single WASM binary.
ACTOR_REGISTRY = {
    "llm_gateway":          LLMGatewayActor,
    "tool_executor":        ToolExecutorActor,
    "agent":                AgentActor,
    "skill_store":          SkillStoreActor,
    "skill_workflow":       SkillExtractionWorkflow,
    "memory":               MemoryActor,
    "audit_event":          AuditEventActor,
    "context_compressor":   ContextCompressorActor,
    "cron_scheduler":       CronSchedulerActor,
    "guardrails":           GuardrailsGateActor,
    "session_manager":      SessionManagerActor,
    "health_monitor":       HealthMonitorActor,
}

__all__ = [
    "LLMGatewayActor",
    "ToolExecutorActor",
    "AgentActor",
    "SkillStoreActor",
    "SkillExtractionWorkflow",
    "MemoryActor",
    "AuditEventActor",
    "ContextCompressorActor",
    "CronSchedulerActor",
    "GuardrailsGateActor",
    "SessionManagerActor",
    "HealthMonitorActor",
    "ACTOR_REGISTRY",
]
