# SPDX-License-Identifier: AGPL-3.0-or-later
"""SkillStoreActor — propose, match, and manage reusable agent skills.

Lifecycle: active → stale (30 days) → archived (90 days).
"""

import json
from plexspaces import actor, state, handler, init_handler, host
from helpers import registry_first, fire_audit, ask

_STALE_MS = 30 * 24 * 3600 * 1000
_ARCHIVE_MS = 90 * 24 * 3600 * 1000
_MIN_TOOL_CALLS = 3


@actor
class SkillStoreActor:
    """Stores, indexes, and retrieves reusable procedural skills."""

    skill_count: int = state(default=0)
    match_count: int = state(default=0)
    learn_count: int = state(default=0)
    archive_count: int = state(default=0)
    skill_ids: list = state(default_factory=list)
    actor_id: str = state(default="")

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        host.process_groups.join("svc:skills")
        host.send_after(24 * 3600 * 1000, "maintenance_tick", {"op": "maintenance_tick"})
        host.info(f"SkillStoreActor init actor_id={self.actor_id}")

    @handler("propose_skill")
    def propose_skill(self, name: str = "", description: str = "", procedure: str = "",
                      tags: str = "", trigger_patterns: str = "") -> dict:
        if not name:
            return {"error": "name is required"}
        import hashlib
        skill_id = hashlib.md5(name.encode()).hexdigest()[:8]

        meta = {
            "skill_id": skill_id,
            "name": name,
            "description": description,
            "tags": [t.strip() for t in tags.split(",") if t.strip()],
            "trigger_patterns": [p.strip() for p in trigger_patterns.split(",") if p.strip()],
            "usage_count": 0,
            "status": "active",
            "created_at": host.now_ms(),
            "last_used_at": host.now_ms(),
        }

        host.kv.put(f"skill_meta:{skill_id}", json.dumps(meta))

        try:
            host.blob.upload("skills", f"skill_procedure_{skill_id}", procedure.encode() if isinstance(procedure, str) else procedure)
        except Exception as e:
            host.debug(f"SkillStore: blob upload failed: {e}")

        # Index in TupleSpace
        for tag in meta["tags"]:
            try:
                host.ts.write(["skill_tag", tag, skill_id, name])
            except Exception:
                pass
        for pattern in meta["trigger_patterns"]:
            try:
                host.ts.write(["skill_trigger", pattern, skill_id, name])
            except Exception:
                pass

        if skill_id not in self.skill_ids:
            self.skill_ids.append(skill_id)
            self.skill_count += 1

        fire_audit("skill_proposed", f"skill_id={skill_id} name={name}")
        return {"status": "ok", "skill_id": skill_id, "name": name}

    @handler("get_skill")
    def get_skill(self, skill_id: str = "") -> dict:
        if not skill_id:
            return {"error": "skill_id is required"}
        raw = host.kv.get(f"skill_meta:{skill_id}")
        if not raw:
            return {"error": "skill not found", "skill_id": skill_id}
        meta = json.loads(raw)
        # Fetch procedure from blob
        try:
            proc_bytes = host.blob.download("skills", f"skill_procedure_{skill_id}")
            meta["procedure"] = proc_bytes.decode() if proc_bytes else ""
        except Exception:
            meta["procedure"] = ""
        meta["status_field"] = meta.get("status", "active")
        return meta

    @handler("match_skills")
    def match_skills(self, query: str = "", limit: int = 5) -> dict:
        if not query:
            return {"status": "ok", "skills": [], "count": 0}

        self.match_count += 1
        matched: dict = {}
        query_lower = query.lower()

        try:
            tuples = host.ts.read_all(["skill_trigger", None, None, None])
            for tup in tuples or []:
                if len(tup) >= 4:
                    pattern, skill_id, name = str(tup[1]), str(tup[2]), str(tup[3])
                    if pattern.lower() in query_lower and skill_id not in matched:
                        raw = host.kv.get(f"skill_meta:{skill_id}")
                        if raw:
                            try:
                                meta = json.loads(raw)
                                if meta.get("status") == "active":
                                    matched[skill_id] = meta
                            except Exception:
                                pass
        except Exception:
            pass

        try:
            tuples = host.ts.read_all(["skill_tag", None, None, None])
            for tup in tuples or []:
                if len(tup) >= 4:
                    tag, skill_id = str(tup[1]), str(tup[2])
                    if skill_id not in matched and tag.lower() in query_lower:
                        raw = host.kv.get(f"skill_meta:{skill_id}")
                        if raw:
                            try:
                                meta = json.loads(raw)
                                if meta.get("status") == "active":
                                    matched[skill_id] = meta
                            except Exception:
                                pass
        except Exception:
            pass

        skills = list(matched.values())[:int(limit)]
        return {"status": "ok", "skills": skills, "count": len(skills)}

    @handler("record_usage")
    def record_usage(self, skill_id: str = "") -> dict:
        if not skill_id:
            return {"error": "skill_id is required"}
        raw = host.kv.get(f"skill_meta:{skill_id}")
        if not raw:
            return {"error": "skill not found"}
        meta = json.loads(raw)
        meta["usage_count"] = meta.get("usage_count", 0) + 1
        meta["last_used_at"] = host.now_ms()
        host.kv.put(f"skill_meta:{skill_id}", json.dumps(meta))
        host.incr_counter("skill_uses", 1)
        return {"status": "ok", "usage_count": meta["usage_count"]}

    @handler("delete_skill")
    def delete_skill(self, skill_id: str = "") -> dict:
        if not skill_id:
            return {"error": "skill_id is required"}
        host.kv.delete(f"skill_meta:{skill_id}")
        try:
            host.blob.delete("skills", f"skill_procedure_{skill_id}")
        except Exception:
            pass
        if skill_id in self.skill_ids:
            self.skill_ids.remove(skill_id)
            self.skill_count = max(0, self.skill_count - 1)
        fire_audit("skill_deleted", f"skill_id={skill_id}")
        return {"status": "ok", "skill_id": skill_id}

    @handler("evaluate_for_learning")
    def evaluate_for_learning(self, session_id: str = "", tool_call_count: int = 0, messages: str = "[]") -> dict:
        if int(tool_call_count) < _MIN_TOOL_CALLS:
            return {"status": "ok", "action": "no_learning", "reason": "too few tool calls"}

        try:
            msgs = json.loads(messages)
        except Exception:
            return {"status": "ok", "action": "no_learning", "reason": "invalid messages"}

        # Extract tool sequence
        tools_used = []
        user_intent = ""
        for m in msgs:
            if m.get("role") == "user" and not user_intent:
                user_intent = str(m.get("content", ""))[:100]
            if m.get("role") == "assistant" and m.get("tool_calls"):
                for tc in m["tool_calls"]:
                    tools_used.append(tc.get("name", ""))

        if len(tools_used) < _MIN_TOOL_CALLS:
            return {"status": "ok", "action": "no_learning", "reason": "insufficient tool sequence"}

        words = user_intent.split()
        if len(words) < 3:
            return {"status": "ok", "action": "no_learning", "reason": "intent too short"}

        # Ask LLM to name and describe the skill
        llm_id, _ = registry_first("llm_gateway", fallback_group="svc:llm_gateway")
        skill_name = f"Auto-{'-'.join(words[:3])}"
        skill_description = f"Automated procedure for: {user_intent}"
        procedure = f"1. Understand the user intent: {user_intent}\n"
        for i, t in enumerate(tools_used, 1):
            procedure += f"{i + 1}. Execute {t}\n"

        # Check if similar skill already exists
        resp = ask(llm_id, "completion", {
            "messages": [{"role": "user", "content": f"Name a reusable skill for this task: {user_intent}. Tools used: {', '.join(tools_used)}. Reply with just: name|description"}],
            "tools": [],
        }, 5000) if llm_id else None

        if resp and resp.get("response", {}).get("content"):
            parts = resp["response"]["content"].split("|", 1)
            if len(parts) == 2:
                skill_name = parts[0].strip()[:50]
                skill_description = parts[1].strip()[:200]

        result = self.propose_skill(
            name=skill_name,
            description=skill_description,
            procedure=procedure,
            tags=",".join(set(tools_used)),
            trigger_patterns=",".join(words[:3]),
        )
        self.learn_count += 1
        fire_audit("skill_learned", f"session_id={session_id} name={skill_name}")
        return {"status": "ok", "action": "learned", "skill_id": result.get("skill_id"), "name": skill_name}

    @handler("maintenance_tick", "cast")
    def maintenance_tick(self) -> None:
        now = host.now_ms()
        for skill_id in list(self.skill_ids):
            raw = host.kv.get(f"skill_meta:{skill_id}")
            if not raw:
                continue
            try:
                meta = json.loads(raw)
                age = now - meta.get("last_used_at", meta.get("created_at", now))
                current_status = meta.get("status", "active")
                if current_status == "active" and age >= _STALE_MS:
                    meta["status"] = "stale"
                    host.kv.put(f"skill_meta:{skill_id}", json.dumps(meta))
                elif current_status == "stale" and age >= _ARCHIVE_MS:
                    meta["status"] = "archived"
                    host.kv.put(f"skill_meta:{skill_id}", json.dumps(meta))
                    self.archive_count += 1
            except Exception:
                pass
        host.send_after(24 * 3600 * 1000, "maintenance_tick", {"op": "maintenance_tick"})

    @handler("get_stats")
    def get_stats(self) -> dict:
        return {
            "status": "ok",
            "skill_count": self.skill_count,
            "match_count": self.match_count,
            "learn_count": self.learn_count,
            "archive_count": self.archive_count,
        }
