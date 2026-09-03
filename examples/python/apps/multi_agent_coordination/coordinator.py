# SPDX-License-Identifier: AGPL-3.0-or-later
"""CoordinatorWorkflow — durable workflow orchestrating the full multi-agent pipeline.

Demonstrates: Pipeline, Scatter-Gather, Dynamic Task Delegation,
Two-Phase Commit, Capability Discovery, Generator-Verifier, Voting.
"""

from plexspaces import workflow_actor, state, run_handler, signal_handler, query_handler, init_handler, host
from .helpers import fire_audit, discover_service, sibling_actor_target, ask


@workflow_actor
class CoordinatorWorkflow:
    """Durable workflow: decomposes security audit tasks and coordinates all agents."""

    status: str = state(default="idle")
    actor_id: str = state(default="")
    subtask_count: int = state(default=0)
    findings_count: int = state(default=0)
    analyses_count: int = state(default=0)
    vetoes_count: int = state(default=0)
    votes_count: int = state(default=0)
    iterations: int = state(default=0)

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        try:
            host.ts.write(["svc", "coordinator", self.actor_id])
        except Exception:
            pass
        try:
            host.process_groups.join("coordination-events")
        except Exception:
            pass
        host.info(f"CoordinatorWorkflow init actor_id={self.actor_id}")

    @run_handler
    def run(self, payload: dict = None) -> dict:
        payload = payload or {}
        task = payload.get("task", "Analyze security vulnerabilities")
        self.status = "running"

        fsm_target = sibling_actor_target("coordination_fsm")
        research_target = sibling_actor_target("research")
        analysis_target = sibling_actor_target("analysis")
        verifier_target = sibling_actor_target("verifier")
        synthesizer_target = sibling_actor_target("synthesizer")

        # Step 1: Transition FSM -> decomposing
        ask(fsm_target, "transition", {"target_state": "decomposing"})

        # Step 2: Decompose task into subtasks
        subtasks = _decompose_task(task)
        self.subtask_count = len(subtasks)

        # Step 3: Write task tuples (Dynamic Task Delegation)
        for i, st in enumerate(subtasks):
            tid = f"coord-task-{host.now_ms()}-{i}"
            host.ts.write(["task", tid, "pending", st, i])
        fire_audit("tasks_delegated", self.actor_id, {"count": len(subtasks)})

        # Step 4: Transition FSM -> researching
        ask(fsm_target, "transition", {"target_state": "researching"})

        # Step 5: Research each subtask (try scatter-gather, fallback sequential)
        research_results = []
        try:
            sg_result = host.scatter_gather({
                "group_id": f"research-{host.now_ms()}",
                "query": {"op": "research", "topic": task},
                "aggregation": "concat",
                "min_responses": len(subtasks),
                "timeout_ms": 15000,
            })
            shard_responses = sg_result.get("shard_responses", [])
            if shard_responses:
                research_results = shard_responses
        except Exception:
            pass

        if not research_results:
            for st in subtasks:
                resp = ask(research_target, "research", {"topic": st}, 10000)
                if resp:
                    research_results.append(resp)
                    self.findings_count += 1

        # Step 6: Transition FSM -> analyzing
        ask(fsm_target, "transition", {"target_state": "analyzing"})

        # Step 7: Analyze findings
        analysis_resp = ask(analysis_target, "analyze", {"topic": task}, 10000)
        if analysis_resp:
            self.analyses_count += 1

        # Step 8: Transition FSM -> verifying
        ask(fsm_target, "transition", {"target_state": "verifying"})

        # Step 9: Generator-verifier loop (max 3 iterations)
        analysis_id = ""
        severity = "medium"
        if analysis_resp:
            analysis_id = analysis_resp.get("analysis_id", "")
            severity = analysis_resp.get("severity", "medium")

        confidence = 0.6
        for attempt in range(3):
            self.iterations += 1
            verify_resp = ask(verifier_target, "verify", {
                "analysis_id": analysis_id,
                "summary": analysis_resp.get("summary", "") if analysis_resp else "",
                "severity": severity,
                "confidence": confidence,
            }, 10000)
            if verify_resp and verify_resp.get("approved"):
                break
            if verify_resp and verify_resp.get("veto_issued"):
                self.vetoes_count += 1
            confidence = min(confidence + 0.2, 0.95)
            feedback = verify_resp.get("feedback", "") if verify_resp else ""
            if feedback and research_results:
                extra = ask(research_target, "research", {
                    "topic": task, "feedback": feedback,
                }, 10000)
                if extra:
                    self.findings_count += 1

        # Step 10: Transition FSM -> voting
        ask(fsm_target, "transition", {"target_state": "voting"})

        # Step 11: Voting (3 votes)
        for vid in ("v1", "v2", "v3"):
            vote_resp = ask(verifier_target, "vote", {
                "proposal_id": f"security-review-{host.now_ms()}",
                "voter_id": vid,
                "analysis": {"severity": severity},
            }, 5000)
            if vote_resp:
                self.votes_count += 1

        # Step 12: Transition FSM -> synthesizing
        ask(fsm_target, "transition", {"target_state": "synthesizing"})

        # Step 13: Synthesize final report
        synth_resp = ask(synthesizer_target, "synthesize", {"topic": task}, 10000)

        # Step 14: Transition FSM -> complete
        ask(fsm_target, "transition", {"target_state": "complete"})

        self.status = "completed"
        report = ""
        if synth_resp:
            report = synth_resp.get("report", "")

        fire_audit("workflow_completed", self.actor_id, {
            "subtasks": self.subtask_count,
            "findings": self.findings_count,
        })

        return {
            "status": "completed",
            "report": report,
            "metrics": {
                "subtasks": self.subtask_count,
                "findings": self.findings_count,
                "analyses": self.analyses_count,
                "vetoes": self.vetoes_count,
                "votes": self.votes_count,
                "iterations": self.iterations,
            },
        }

    @signal_handler("cancel")
    def cancel(self) -> None:
        self.status = "cancelled"
        fsm_target = sibling_actor_target("coordination_fsm")
        ask(fsm_target, "transition", {"target_state": "failed"})
        host.info("CoordinatorWorkflow cancelled")

    @query_handler("progress")
    def query_progress(self) -> dict:
        return {
            "status": self.status,
            "subtasks": self.subtask_count,
            "findings": self.findings_count,
            "analyses": self.analyses_count,
            "vetoes": self.vetoes_count,
            "votes": self.votes_count,
            "iterations": self.iterations,
        }


def _decompose_task(task: str) -> list:
    """Split a security audit task into 3 domain-specific subtasks."""
    lower = task.lower()
    domains = []
    if "injection" in lower or "sql" in lower or "input" in lower:
        domains.append(f"SQL injection analysis: {task}")
    if "auth" in lower or "login" in lower or "bypass" in lower:
        domains.append(f"Authentication bypass analysis: {task}")
    if "xss" in lower or "cross-site" in lower or "template" in lower:
        domains.append(f"Cross-site scripting analysis: {task}")

    if len(domains) < 3:
        base_domains = [
            f"Input validation vulnerabilities: {task}",
            f"Authentication and access control: {task}",
            f"Output encoding and injection: {task}",
        ]
        for d in base_domains:
            if d not in domains and len(domains) < 3:
                domains.append(d)

    return domains[:3]
