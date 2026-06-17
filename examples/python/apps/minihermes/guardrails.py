# SPDX-License-Identifier: AGPL-3.0-or-later
"""GuardrailsGateActor — FSM-backed tool approval with configurable per-tool policies."""

import json
from plexspaces import actor, state, handler, init_handler, host
from helpers import fire_audit

_BUILTIN_POLICIES = {
    "http_request": "review",
    "delete_file": "deny",
    "rm_command": "deny",
    "format_disk": "deny",
}


@actor
class GuardrailsGateActor:
    """Policy-driven tool gating: allow / requires_approval / deny."""

    check_count: int = state(default=0)
    approval_count: int = state(default=0)
    deny_count: int = state(default=0)
    actor_id: str = state(default="")

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        host.process_groups.join("svc:guardrails")
        try:
            host.registry.register(
                ctx="", object_id=self.actor_id, object_type="actor", grpc_address="",
                object_category="guardrails", capabilities=["check", "approve"],
            )
        except Exception:
            pass
        for tool, policy in _BUILTIN_POLICIES.items():
            host.kv_put(f"guardrail_policy:{tool}", policy)
        host.info(f"GuardrailsGateActor init actor_id={self.actor_id}")

    @handler("check")
    def check(self, tool: str = "", input: dict = None) -> dict:
        if not tool:
            return {"error": "tool is required"}
        self.check_count += 1
        policy_raw = host.kv_get(f"guardrail_policy:{tool}")
        policy = policy_raw or "allow"

        if policy == "deny":
            self.deny_count += 1
            fire_audit("guardrail_denied", f"tool={tool}")
            return {"decision": "deny", "tool": tool, "reason": "policy=deny"}

        if policy == "review":
            import hashlib
            approval_id = hashlib.md5(f"{tool}{host.now_ms()}".encode()).hexdigest()[:8]
            approval = {"approval_id": approval_id, "tool": tool, "input": input or {},
                        "status": "pending", "created_at": host.now_ms()}
            host.kv_put(f"approval:{approval_id}", json.dumps(approval))
            try:
                host.ts.write(["approval_pending", approval_id, tool])
            except Exception:
                pass
            fire_audit("guardrail_review", f"tool={tool} approval_id={approval_id}")
            return {"decision": "requires_approval", "tool": tool, "approval_id": approval_id}

        return {"decision": "allow", "tool": tool}

    @handler("approve")
    def approve(self, approval_id: str = "") -> dict:
        if not approval_id:
            return {"error": "approval_id is required"}
        raw = host.kv_get(f"approval:{approval_id}")
        if not raw:
            return {"error": "approval not found"}
        approval = json.loads(raw)
        approval["status"] = "approved"
        host.kv_put(f"approval:{approval_id}", json.dumps(approval))
        self.approval_count += 1
        fire_audit("guardrail_approved", f"approval_id={approval_id}")
        return {"status": "ok", "decision": "approved", "approval_id": approval_id}

    @handler("deny_approval")
    def deny_approval(self, approval_id: str = "") -> dict:
        if not approval_id:
            return {"error": "approval_id is required"}
        raw = host.kv_get(f"approval:{approval_id}")
        if not raw:
            return {"error": "approval not found"}
        approval = json.loads(raw)
        approval["status"] = "denied"
        host.kv_put(f"approval:{approval_id}", json.dumps(approval))
        self.deny_count += 1
        return {"status": "ok", "decision": "denied", "approval_id": approval_id}

    @handler("set_policy")
    def set_policy(self, tool: str = "", policy: str = "allow") -> dict:
        if not tool:
            return {"error": "tool is required"}
        if policy not in ("allow", "review", "deny"):
            return {"error": f"invalid policy: {policy}"}
        host.kv_put(f"guardrail_policy:{tool}", policy)
        fire_audit("guardrail_policy_set", f"tool={tool} policy={policy}")
        return {"status": "ok", "tool": tool, "policy": policy}
