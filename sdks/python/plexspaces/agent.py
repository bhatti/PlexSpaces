# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2025 PlexSpaces Contributors
#
# This file is part of PlexSpaces.
#
# PlexSpaces is free software: you can redistribute it and/or modify it under
# the terms of the GNU Affero General Public License as published by the Free
# Software Foundation, either version 3 of the License, or (at your option)
# any later version.
#
# PlexSpaces is distributed in the hope that it will be useful, but WITHOUT
# ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS
# FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License for more
# details.
#
# You should have received a copy of the GNU Affero General Public License
# along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

"""
Agent harness utilities for PlexSpaces Python SDK.

``AgentLoop`` is a **standalone class** (no decorator magic, no class injection)
that provides structured OODA step tracking, token budget enforcement, and
suspend/resume helpers.  Use it with ``@workflow_actor`` for durable agent
execution.

Example::

    from plexspaces import workflow_actor, state, run_handler, host
    from plexspaces.agent import AgentLoop, AgentConfig

    @workflow_actor
    class ResearchAgent:
        task: str = state(default="")

        @run_handler
        def run(self, task: str = "") -> dict:
            loop = AgentLoop(
                actor_id=host.actor_id(),
                config=AgentConfig(max_iterations=10, token_budget=4096),
            )
            while not loop.iteration_limit_reached() and not loop.budget_exceeded():
                obs    = loop.observe({"task": task})
                plan   = loop.orient(obs)
                action = loop.decide(plan)
                result = loop.act(action)
                loop.increment_iteration()
                if result.get("done"):
                    break
            return loop.finalize_trajectory("completed", "task done")
"""

from __future__ import annotations

import time
import uuid
from dataclasses import dataclass
from typing import Any, Dict, List, Optional


# ---------------------------------------------------------------------------
# Utilities
# ---------------------------------------------------------------------------

def _now_ms() -> int:
    """Current time as integer milliseconds since the Unix epoch."""
    return int(time.time() * 1000)


def _new_ulid() -> str:
    """Generate a new ULID-compatible unique string (UUID4 fallback)."""
    return str(uuid.uuid4())


# ---------------------------------------------------------------------------
# AgentStepKind constants
# ---------------------------------------------------------------------------

class AgentStepKind:
    OBSERVE = "observe"
    ORIENT = "orient"
    DECIDE = "decide"
    ACT = "act"
    TOOL_CALL = "tool_call"
    SUSPEND = "suspend"


# ---------------------------------------------------------------------------
# AgentStep
# ---------------------------------------------------------------------------

class AgentStep:
    """A single step in an agent's OODA trajectory."""

    def __init__(self, kind: str, method: str, input_data: Any = None):
        self.step_id: str = _new_ulid()
        self.kind: str = kind
        self.method: str = method
        self.input: Any = input_data
        self.output: Any = None
        self.started_at_ms: int = _now_ms()
        self.completed_at_ms: Optional[int] = None
        self.success: bool = False
        self.error: Optional[str] = None
        self.input_tokens: int = 0
        self.output_tokens: int = 0
        self.model: str = ""
        self.metadata: Dict[str, str] = {}

    def complete(
        self,
        output: Any,
        input_tokens: int = 0,
        output_tokens: int = 0,
        model: str = "",
    ) -> "AgentStep":
        """Mark the step as successfully completed."""
        self.output = output
        self.completed_at_ms = _now_ms()
        self.success = True
        self.input_tokens = input_tokens
        self.output_tokens = output_tokens
        self.model = model
        return self

    def fail(self, error: str) -> "AgentStep":
        """Mark the step as failed."""
        self.completed_at_ms = _now_ms()
        self.success = False
        self.error = error
        return self

    @property
    def duration_ms(self) -> int:
        end = self.completed_at_ms or _now_ms()
        return max(0, end - self.started_at_ms)

    def to_dict(self) -> Dict[str, Any]:
        return {
            "step_id": self.step_id,
            "kind": self.kind,
            "method": self.method,
            "input": self.input,
            "output": self.output,
            "started_at_ms": self.started_at_ms,
            "completed_at_ms": self.completed_at_ms,
            "duration_ms": self.duration_ms,
            "success": self.success,
            "error": self.error,
            "input_tokens": self.input_tokens,
            "output_tokens": self.output_tokens,
            "model": self.model,
            "metadata": self.metadata,
        }


# ---------------------------------------------------------------------------
# AgentTrajectory
# ---------------------------------------------------------------------------

class AgentTrajectory:
    """Complete trajectory for one agent run (all OODA steps, token totals)."""

    def __init__(
        self,
        agent_actor_id: str,
        eval_run_id: str = "",
        scenario_id: str = "",
    ):
        self.trajectory_id: str = _new_ulid()
        self.agent_actor_id: str = agent_actor_id
        self.eval_run_id: str = eval_run_id
        self.scenario_id: str = scenario_id
        self.steps: List[AgentStep] = []
        self.outcome: str = ""
        self.outcome_detail: str = ""
        self.started_at_ms: int = _now_ms()
        self.completed_at_ms: Optional[int] = None
        self.total_input_tokens: int = 0
        self.total_output_tokens: int = 0

    @property
    def duration_ms(self) -> int:
        end = self.completed_at_ms or _now_ms()
        return max(0, end - self.started_at_ms)

    @property
    def step_count(self) -> int:
        return len(self.steps)

    def add_step(self, step: AgentStep) -> None:
        """Append a completed step and accumulate token totals."""
        self.steps.append(step)
        self.total_input_tokens += step.input_tokens
        self.total_output_tokens += step.output_tokens

    def complete(self, outcome: str, detail: str = "") -> None:
        """Finalise the trajectory with an outcome string."""
        self.outcome = outcome
        self.outcome_detail = detail
        self.completed_at_ms = _now_ms()

    def to_dict(self) -> Dict[str, Any]:
        return {
            "trajectory_id": self.trajectory_id,
            "agent_actor_id": self.agent_actor_id,
            "eval_run_id": self.eval_run_id,
            "scenario_id": self.scenario_id,
            "steps": [s.to_dict() for s in self.steps],
            "step_count": len(self.steps),
            "outcome": self.outcome,
            "outcome_detail": self.outcome_detail,
            "started_at_ms": self.started_at_ms,
            "completed_at_ms": self.completed_at_ms,
            "duration_ms": self.duration_ms,
            "total_input_tokens": self.total_input_tokens,
            "total_output_tokens": self.total_output_tokens,
        }


# ---------------------------------------------------------------------------
# AgentConfig
# ---------------------------------------------------------------------------

@dataclass
class AgentConfig:
    """Configuration for an AgentLoop."""
    max_iterations: int = 10
    token_budget: int = 0          # 0 = no budget limit


# ---------------------------------------------------------------------------
# AgentLoop
# ---------------------------------------------------------------------------

class AgentLoop:
    """
    Standalone agent harness utility.

    Records OODA steps, enforces token budget and iteration limits, and
    produces an ``AgentTrajectory`` on finalisation.

    This is a **plain class** — there is no class injection or metaclass magic.
    Use it inside a ``@workflow_actor`` ``run_handler``.

    :param actor_id: ID of the hosting actor.
    :param config:   Optional ``AgentConfig`` (max_iterations, token_budget).
    :param eval_run_id: Optional eval run ID to tag the trajectory.
    :param scenario_id: Optional scenario ID to tag the trajectory.
    """

    def __init__(
        self,
        actor_id: str,
        config: Optional[AgentConfig] = None,
        eval_run_id: str = "",
        scenario_id: str = "",
    ):
        self._actor_id = actor_id
        self._config = config or AgentConfig()
        self._trajectory = AgentTrajectory(
            agent_actor_id=actor_id,
            eval_run_id=eval_run_id,
            scenario_id=scenario_id,
        )
        self._iteration_count: int = 0
        self.is_suspended: bool = False

    # ── Private helpers ──────────────────────────────────────────────────────

    def _record(
        self,
        kind: str,
        input_data: Any,
        output: Any = None,
        input_tokens: int = 0,
        output_tokens: int = 0,
        model: str = "",
        error: Optional[str] = None,
    ) -> AgentStep:
        started = _now_ms()
        step = AgentStep(kind=kind, method=kind, input_data=input_data)
        step.started_at_ms = started
        if error:
            step.fail(error)
        else:
            step.complete(
                output=output,
                input_tokens=input_tokens,
                output_tokens=output_tokens,
                model=model,
            )
        self._trajectory.add_step(step)
        return step

    # ── OODA step methods ─────────────────────────────────────────────────────

    def observe(self, input_data: Any, **kwargs: Any) -> Any:
        """Record an OBSERVE step.  Returns *input_data* unchanged."""
        self._record(AgentStepKind.OBSERVE, input_data, output=input_data, **kwargs)
        return input_data

    def orient(self, input_data: Any, **kwargs: Any) -> Any:
        """Record an ORIENT step.  Returns *input_data* unchanged."""
        self._record(AgentStepKind.ORIENT, input_data, output=input_data, **kwargs)
        return input_data

    def decide(self, input_data: Any, **kwargs: Any) -> Any:
        """Record a DECIDE step.  Returns *input_data* unchanged."""
        self._record(AgentStepKind.DECIDE, input_data, output=input_data, **kwargs)
        return input_data

    def act(
        self,
        input_data: Any,
        output: Any = None,
        input_tokens: int = 0,
        output_tokens: int = 0,
        model: str = "",
        **kwargs: Any,
    ) -> Any:
        """Record an ACT step.  Returns *output* (or *input_data* if output is None)."""
        result = output if output is not None else input_data
        self._record(
            AgentStepKind.ACT,
            input_data,
            output=result,
            input_tokens=input_tokens,
            output_tokens=output_tokens,
            model=model,
        )
        return result

    def tool_call(
        self,
        name: str,
        args: Any,
        result: Any,
        started_at_ms: Optional[int] = None,
        input_tokens: int = 0,
        output_tokens: int = 0,
        model: str = "",
    ) -> Any:
        """Record a TOOL_CALL step.  Returns *result*."""
        step = AgentStep(
            kind=AgentStepKind.TOOL_CALL,
            method=name,
            input_data=args,
        )
        if started_at_ms is not None:
            step.started_at_ms = started_at_ms
        step.complete(
            output=result,
            input_tokens=input_tokens,
            output_tokens=output_tokens,
            model=model,
        )
        self._trajectory.add_step(step)
        return result

    # ── Control ───────────────────────────────────────────────────────────────

    def suspend(self, reason: str) -> None:
        """Mark the agent as suspended (waiting for external signal)."""
        self.is_suspended = True
        self._record(AgentStepKind.SUSPEND, reason)

    def increment_iteration(self) -> None:
        """Increment the OODA cycle counter.  Call once per full OODA cycle."""
        self._iteration_count += 1

    def budget_exceeded(self) -> bool:
        """True if the cumulative token count meets or exceeds the budget."""
        budget = self._config.token_budget
        if budget <= 0:
            return False
        total = (
            self._trajectory.total_input_tokens
            + self._trajectory.total_output_tokens
        )
        return total >= budget

    def iteration_limit_reached(self) -> bool:
        """True if the OODA cycle count meets or exceeds max_iterations.

        Returns False when max_iterations is 0 (unlimited), matching Go/TS/Rust.
        """
        if self._config.max_iterations <= 0:
            return False
        return self._iteration_count >= self._config.max_iterations

    # ── Trajectory accessors ──────────────────────────────────────────────────

    def get_trajectory(self) -> AgentTrajectory:
        """Return the in-progress ``AgentTrajectory`` object."""
        return self._trajectory

    def finalize_trajectory(self, outcome: str, detail: str = "") -> Dict[str, Any]:
        """
        Finalise the trajectory with an outcome and return it as a dict.

        :param outcome: "completed", "error", "timeout", "budget_exceeded", etc.
        :param detail:  Human-readable detail.
        :returns: ``AgentTrajectory.to_dict()``
        """
        self._trajectory.complete(outcome=outcome, detail=detail)
        return self._trajectory.to_dict()
