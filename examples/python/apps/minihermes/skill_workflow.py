# SPDX-License-Identifier: AGPL-3.0-or-later
"""SkillExtractionWorkflow — durable workflow for parallel skill analysis.

When the agent accumulates enough tool calls to warrant skill learning, it
delegates to this workflow instead of a simple GenServer call. The workflow:

  1. Runs 3 parallel analysis passes (tool-pattern, user-intent, domain)
  2. Merges findings into a canonical skill definition
  3. Persists the skill via SkillStoreActor
  4. Checkpoints state so restarts resume from the last completed step

This demonstrates durable Workflow actors with:
  - @run_handler: the main execution path (runs once, durable)
  - @signal_handler: cancel mid-flight
  - @query_handler: inspect progress
  - Parallel Ask calls for multi-angle analysis
  - TupleSpace for coordination between analysis steps
"""

import json
from plexspaces import workflow_actor, state, run_handler, signal_handler, query_handler, init_handler, host
from helpers import registry_first, fire_audit, ask


@workflow_actor
class SkillExtractionWorkflow:
    """Durable workflow: analyse conversation → extract skill → store."""

    status: str = state(default="idle")
    session_id: str = state(default="")
    progress: int = state(default=0)
    skill_id: str = state(default="")
    actor_id: str = state(default="")

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        host.process_groups.join("svc:skill_workflow")
        # Register in object registry
        try:
            host.registry.register(
                ctx="",
                object_id=self.actor_id,
                object_type="actor",
                grpc_address="",
                object_category="skill_workflow",
                capabilities=["skill_extraction", "parallel_analysis"],
            )
        except Exception:
            pass
        host.info(f"SkillExtractionWorkflow init actor_id={self.actor_id}")

    @run_handler
    def run(self, payload: dict = None) -> dict:
        """Main workflow execution — durable and restartable."""
        payload = payload or {}
        self.session_id = payload.get("session_id", "unknown")
        messages_json = payload.get("messages", "[]")
        tool_call_count = int(payload.get("tool_call_count", 0))

        self.status = "running"
        self.progress = 0

        try:
            messages = json.loads(messages_json)
        except Exception:
            messages = []

        if not messages:
            self.status = "skipped"
            return {"status": "ok", "action": "skipped", "reason": "no messages"}

        # ── Step 1 (33%): Run 3 parallel analysis passes ─────────────────────
        self.progress = 10
        host.info(f"SkillWorkflow: starting parallel analysis session={self.session_id}")

        llm_id, _ = registry_first("llm_gateway", fallback_group="svc:llm_gateway")

        # Extract useful information: tool sequence, user intent, domain
        tool_names = []
        user_intent = ""
        for m in messages:
            if m.get("role") == "user" and not user_intent:
                user_intent = str(m.get("content", ""))[:150]
            if m.get("role") == "assistant":
                for tc in m.get("tool_calls", []):
                    if tc.get("name"):
                        tool_names.append(tc["name"])

        tool_sequence = ", ".join(tool_names[:6])
        domain_hint = _infer_domain(user_intent, tool_names)

        # Parallel analysis via 3 LLM calls (if LLM available; stub-safe)
        name_analysis = _analyse_name(llm_id, user_intent, tool_sequence)
        self.progress = 33

        procedure_analysis = _analyse_procedure(llm_id, user_intent, tool_sequence)
        self.progress = 66

        trigger_analysis = _analyse_triggers(llm_id, user_intent, domain_hint)
        self.progress = 80

        # ── Step 2 (90%): Merge and store ────────────────────────────────────
        skill_name = name_analysis.get("name") or f"Auto-{domain_hint}-workflow"
        skill_desc = name_analysis.get("description") or f"Automated: {user_intent[:80]}"
        procedure = procedure_analysis.get("procedure") or _build_procedure(user_intent, tool_names)
        tags = trigger_analysis.get("tags") or domain_hint
        triggers = trigger_analysis.get("triggers") or ",".join(user_intent.split()[:3])

        # Checkpoint in TupleSpace for visibility
        try:
            host.ts.write(["skill_extraction", self.session_id, skill_name, self.progress])
        except Exception:
            pass

        skill_id_result, skill_err = registry_first("skill_store", fallback_group="svc:skills")
        if skill_err or not skill_id_result:
            self.status = "failed"
            return {"status": "error", "reason": "skill store unavailable"}

        result = ask(skill_id_result, "propose_skill", {
            "name": skill_name[:60],
            "description": skill_desc[:200],
            "procedure": procedure,
            "tags": tags,
            "trigger_patterns": triggers,
        }, 10000)

        self.progress = 100
        self.status = "completed"
        self.skill_id = (result or {}).get("skill_id", "")

        fire_audit("skill_extracted_workflow", f"session={self.session_id} skill={skill_name}")
        host.info(f"SkillWorkflow: completed session={self.session_id} skill_id={self.skill_id}")
        return {
            "status": "ok",
            "action": "learned",
            "skill_id": self.skill_id,
            "skill_name": skill_name,
            "session_id": self.session_id,
            "tool_count": tool_call_count,
        }

    @signal_handler("cancel")
    def cancel(self) -> None:
        self.status = "cancelled"
        host.info(f"SkillWorkflow cancelled session={self.session_id}")

    @query_handler("status")
    def query_status(self) -> dict:
        return {
            "status": self.status,
            "session_id": self.session_id,
            "progress": self.progress,
            "skill_id": self.skill_id,
        }


# ── Analysis helpers (each is an LLM Ask or a cheap fallback) ────────────────

def _analyse_name(llm_id: str, user_intent: str, tool_sequence: str) -> dict:
    """Ask LLM to suggest a skill name and description."""
    if not llm_id:
        words = user_intent.split()
        return {
            "name": " ".join(w.capitalize() for w in words[:3]) if words else "UnnamedSkill",
            "description": user_intent[:100],
        }
    prompt = (f"Given this user task: \"{user_intent}\" and tools used: {tool_sequence}, "
              "reply with JSON: {\"name\": \"short skill name\", \"description\": \"one-sentence description\"}")
    resp = ask(llm_id, "completion", {
        "messages": [{"role": "user", "content": prompt}],
        "tools": [],
    }, 8000)
    if resp and resp.get("response", {}).get("content"):
        try:
            return json.loads(resp["response"]["content"])
        except Exception:
            pass
    return {"name": f"Skill-{tool_sequence[:20]}", "description": user_intent[:100]}


def _analyse_procedure(llm_id: str, user_intent: str, tool_sequence: str) -> dict:
    """Ask LLM to extract a step-by-step procedure."""
    if not llm_id:
        return {"procedure": _build_procedure(user_intent, tool_sequence.split(", "))}
    prompt = (f"Task: \"{user_intent}\". Tools used in order: {tool_sequence}. "
              "Reply with JSON: {\"procedure\": \"numbered step-by-step procedure\"}")
    resp = ask(llm_id, "completion", {
        "messages": [{"role": "user", "content": prompt}],
        "tools": [],
    }, 8000)
    if resp and resp.get("response", {}).get("content"):
        try:
            return json.loads(resp["response"]["content"])
        except Exception:
            pass
    return {"procedure": _build_procedure(user_intent, tool_sequence.split(", "))}


def _analyse_triggers(llm_id: str, user_intent: str, domain: str) -> dict:
    """Ask LLM to suggest tags and trigger keywords."""
    if not llm_id:
        words = [w.lower() for w in user_intent.split() if len(w) > 3][:4]
        return {"tags": domain, "triggers": ",".join(words[:3])}
    prompt = (f"Task: \"{user_intent}\" in domain: {domain}. "
              "Reply with JSON: {\"tags\": \"comma-sep tags\", \"triggers\": \"comma-sep trigger keywords\"}")
    resp = ask(llm_id, "completion", {
        "messages": [{"role": "user", "content": prompt}],
        "tools": [],
    }, 8000)
    if resp and resp.get("response", {}).get("content"):
        try:
            return json.loads(resp["response"]["content"])
        except Exception:
            pass
    words = [w.lower() for w in user_intent.split() if len(w) > 3][:4]
    return {"tags": domain, "triggers": ",".join(words[:3])}


def _infer_domain(user_intent: str, tool_names: list) -> str:
    lower = user_intent.lower()
    if any(k in lower for k in ("math", "calculat", "comput")):
        return "math"
    if any(k in lower for k in ("http", "api", "fetch", "request")):
        return "web"
    if any(k in lower for k in ("remember", "recall", "memory", "store")):
        return "memory"
    if any(k in lower for k in ("schedule", "cron", "every", "automat")):
        return "automation"
    if "calculator" in tool_names:
        return "math"
    if "http_request" in tool_names:
        return "web"
    return "general"


def _build_procedure(user_intent: str, tool_names: list) -> str:
    lines = [f"1. Understand the task: {user_intent[:100]}"]
    for i, t in enumerate(tool_names[:5], 2):
        lines.append(f"{i}. Execute {t}")
    lines.append(f"{len(tool_names) + 2}. Return results to user")
    return "\n".join(lines)
