# SPDX-License-Identifier: AGPL-3.0-or-later
"""OrchestratorActor — durable workflow that decomposes tasks and aggregates results.

Dispatches sub-tasks to agents discovered via process groups, then writes
intermediate results to TupleSpace for coordination.
"""

from plexspaces import workflow_actor, state, run_handler, signal_handler, query_handler, init_handler, host
from .helpers import pg_first, fire_audit, ask


@workflow_actor
class OrchestratorActor:
    """Durable workflow: decompose task → delegate to agents → aggregate results."""

    status: str = state(default="idle")
    task_id: str = state(default="")
    progress: int = state(default=0)
    actor_id: str = state(default="")

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        host.info(f"OrchestratorActor init actor_id={self.actor_id}")

    @run_handler
    def run(self, payload: dict = None) -> dict:
        payload = payload or {}
        task = payload.get("task", "explain how agents work")
        task_id = payload.get("task_id", f"orch-{host.now_ms()}")

        self.status = "running"
        self.task_id = task_id
        self.progress = 0
        host.info(f"Orchestrator run task_id={task_id} task={task}")

        agent_id, err = pg_first("svc:agent")
        if err or not agent_id:
            self.status = "failed"
            return {"error": "no agents in svc:agent", "task_id": task_id}

        # Decompose: split on " and " for multi-step tasks
        lower = task.lower()
        idx = lower.find(" and ")
        sub_tasks = [task[:idx].strip(), task[idx + 5:].strip()] if idx >= 0 else [task]

        sub_results = []
        for i, sub_task in enumerate(sub_tasks):
            self.progress = (i + 1) * 100 // len(sub_tasks)
            resp = ask(agent_id, "chat", {"message": sub_task, "session_id": f"orch-{task_id}-{i}"}, 15000)
            if not resp:
                self.status = "failed"
                return {"error": "sub-task failed", "task_id": task_id}
            host.ts.write(["orch_result", task_id, i, str(resp.get("response", ""))])
            sub_results.append(resp)

        summaries = [r.get("response", "") for r in sub_results if r.get("response")]
        aggregated = " | ".join(summaries)

        self.status = "completed"
        self.progress = 100
        fire_audit("orchestrator_completed", f"task_id={task_id} subtasks={len(sub_tasks)}")
        return {
            "status": "ok",
            "task_id": task_id,
            "result": aggregated,
            "sub_results": sub_results,
            "sub_tasks": len(sub_tasks),
        }

    @signal_handler("cancel")
    def cancel(self) -> None:
        self.status = "cancelled"
        host.info(f"Orchestrator cancelled task_id={self.task_id}")

    @query_handler("status")
    def query_status(self) -> dict:
        return {"task_id": self.task_id, "status": self.status, "progress": self.progress}
