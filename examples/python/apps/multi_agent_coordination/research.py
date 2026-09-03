# SPDX-License-Identifier: AGPL-3.0-or-later
"""ResearchAgent — generates findings and claims tasks from the blackboard.

Demonstrates: Blackboard (write findings), Dynamic Task Delegation (claim),
Generator-Verifier (generate with refinement on feedback).
"""

import json
from plexspaces import actor, state, handler, init_handler, host
from .helpers import fire_audit


def _generate_finding_id() -> str:
    return f"finding-{host.now_ms()}"


def _generate_task_id(index: int) -> str:
    return f"task-{host.now_ms()}-{index}"


def _compute_confidence(topic: str, feedback: str = "") -> float:
    words = topic.split()
    base = min(len(words) / 10.0, 0.7) + 0.2
    if feedback:
        base = min(base + 0.2, 0.95)
    return round(base, 2)


def _generate_content(topic: str, feedback: str = "") -> str:
    keywords = [w for w in topic.split() if len(w) > 3]
    analysis = f"Analysis of {topic}: identified {len(keywords)} key areas"
    if keywords:
        analysis += f" including {', '.join(keywords[:5])}"
    if feedback:
        analysis += f". Refined based on feedback: {feedback[:100]}"
    analysis += ". Potential vulnerabilities detected in input validation and access control layers."
    return analysis


@actor
class ResearchAgent:
    """GenServer: researches topics, writes findings to blackboard, claims delegated tasks."""

    findings_generated: int = state(default=0)
    tasks_claimed: int = state(default=0)
    actor_id: str = state(default="")

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        try:
            host.ts.write(["svc", "research", self.actor_id])
        except Exception:
            pass
        try:
            host.process_groups.join("coordination-events")
        except Exception:
            pass
        host.info(f"ResearchAgent init actor_id={self.actor_id}")

    @handler("research")
    def research(self, topic: str = "", depth: int = 1, feedback: str = "") -> dict:
        if not topic:
            return {"error": "topic is required"}

        confidence = _compute_confidence(topic, feedback)
        content = _generate_content(topic, feedback)
        finding_id = _generate_finding_id()
        ts = host.now_ms()

        host.ts.write(["finding", finding_id, topic, content, confidence, ts])
        self.findings_generated += 1

        fire_audit("finding_written", self.actor_id, {
            "finding_id": finding_id, "topic": topic,
        })

        return {
            "finding_id": finding_id,
            "content": content,
            "confidence": confidence,
            "topic": topic,
        }

    @handler("claim_task")
    def claim_task(self, batch_key: str = "") -> dict:
        if batch_key:
            claimed = host.ts.take(["dtask", batch_key, None, "pending", None, None])
        else:
            claimed = host.ts.take(["dtask", None, None, "pending", None, None])
        if claimed and len(claimed) >= 5:
            self.tasks_claimed += 1
            task_id = str(claimed[2])
            description = str(claimed[4])
            fire_audit("task_claimed", self.actor_id, {"task_id": task_id})
            return {
                "task_id": task_id,
                "description": description,
                "claimed": True,
            }
        return {"task": None, "claimed": False}

    @handler("prepare_tasks")
    def prepare_tasks(self, count: int = 5, prefix: str = "delegation") -> dict:
        """Write N task tuples to TupleSpace for testing dynamic task delegation."""
        run_id = f"{host.now_ms()}"
        batch_key = f"{prefix}-{run_id}"
        task_ids = []
        for i in range(count):
            tid = f"{batch_key}-{i}"
            desc = f"Task {i}: investigate area {i}"
            host.ts.write(["dtask", batch_key, tid, "pending", desc, i + 1])
            task_ids.append(tid)
        return {"tasks_written": len(task_ids), "task_ids": task_ids, "batch_key": batch_key}

    @handler("get_stats")
    def get_stats(self) -> dict:
        return {
            "findings_generated": self.findings_generated,
            "tasks_claimed": self.tasks_claimed,
        }
