# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2025 PlexSpaces Contributors
#
# Unit tests for sdks/python/plexspaces/agent.py

import unittest
import sys
import os

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

from plexspaces.agent import (
    AgentConfig,
    AgentLoop,
    AgentStep,
    AgentStepKind,
    AgentTrajectory,
)


# ============================================================================
# TestAgentStep
# ============================================================================

class TestAgentStep(unittest.TestCase):
    """AgentStep creation, completion, failure, and serialisation."""

    def test_to_dict_includes_all_fields(self):
        step = AgentStep(kind="observe", method="observe", input_data={"x": 1})
        step.complete({"y": 2}, input_tokens=5, output_tokens=10, model="claude-3")
        d = step.to_dict()

        required_keys = {
            "step_id", "kind", "method", "input", "output",
            "started_at_ms", "completed_at_ms", "duration_ms",
            "success", "error", "input_tokens", "output_tokens", "model",
        }
        for key in required_keys:
            self.assertIn(key, d, f"Missing key: {key}")

        self.assertEqual(d["kind"], "observe")
        self.assertEqual(d["method"], "observe")
        self.assertEqual(d["input"], {"x": 1})
        self.assertEqual(d["output"], {"y": 2})
        self.assertTrue(d["success"])
        self.assertIsNone(d["error"])
        self.assertEqual(d["input_tokens"], 5)
        self.assertEqual(d["output_tokens"], 10)
        self.assertEqual(d["model"], "claude-3")
        self.assertGreater(len(d["step_id"]), 0)

    def test_fail_sets_error_and_success_false(self):
        step = AgentStep(kind="act", method="act", input_data="cmd")
        step.fail("timeout")
        d = step.to_dict()
        self.assertFalse(d["success"])
        self.assertEqual(d["error"], "timeout")

    def test_duration_ms_non_negative(self):
        step = AgentStep(kind="act", method="act", input_data=None)
        step.complete(None)
        self.assertGreaterEqual(step.duration_ms, 0)


# ============================================================================
# TestAgentTrajectory
# ============================================================================

class TestAgentTrajectory(unittest.TestCase):
    """AgentTrajectory add_step, token aggregation, and complete()."""

    def _make_step(self, kind: str, input_tokens: int = 0, output_tokens: int = 0) -> AgentStep:
        step = AgentStep(kind=kind, method=kind, input_data=None)
        step.complete(None, input_tokens=input_tokens, output_tokens=output_tokens)
        return step

    def test_add_step_accumulates_tokens(self):
        traj = AgentTrajectory(agent_actor_id="agent-1")
        traj.add_step(self._make_step("observe", 10, 5))
        traj.add_step(self._make_step("act", 20, 15))

        self.assertEqual(traj.total_input_tokens, 30)
        self.assertEqual(traj.total_output_tokens, 20)
        self.assertEqual(len(traj.steps), 2)

    def test_complete_sets_outcome_and_timestamps(self):
        traj = AgentTrajectory(agent_actor_id="agent-2", eval_run_id="eval-42")
        traj.complete(outcome="success", detail="all good")

        self.assertEqual(traj.outcome, "success")
        self.assertEqual(traj.outcome_detail, "all good")
        self.assertGreater(traj.completed_at_ms, 0)
        self.assertGreaterEqual(traj.duration_ms, 0)

    def test_to_dict_has_step_count(self):
        traj = AgentTrajectory(agent_actor_id="agent-3")
        traj.add_step(self._make_step("observe"))
        d = traj.to_dict()
        self.assertEqual(d["step_count"], 1)
        self.assertIn("trajectory_id", d)
        self.assertGreater(len(d["trajectory_id"]), 0)


# ============================================================================
# TestAgentLoopOODA
# ============================================================================

class TestAgentLoopOODA(unittest.TestCase):
    """AgentLoop: four-step OODA cycle, passthrough semantics."""

    def test_four_ooda_steps_recorded(self):
        loop = AgentLoop(actor_id="agent-001")
        obs = loop.observe({"task": "summarise"})
        plan = loop.orient(obs)
        action = loop.decide(plan)
        loop.act(action, input_tokens=5, output_tokens=8, model="claude-3-haiku")

        traj = loop.get_trajectory()
        self.assertEqual(len(traj.steps), 4)
        kinds = [s.kind for s in traj.steps]
        self.assertEqual(kinds, ["observe", "orient", "decide", "act"])

    def test_observe_returns_input_unchanged(self):
        loop = AgentLoop(actor_id="a")
        inp = {"data": 42}
        self.assertEqual(loop.observe(inp), inp)

    def test_orient_returns_input_unchanged(self):
        loop = AgentLoop(actor_id="a")
        inp = {"plan": "do-x"}
        self.assertEqual(loop.orient(inp), inp)

    def test_decide_returns_input_unchanged(self):
        loop = AgentLoop(actor_id="a")
        inp = {"action": "call-tool"}
        self.assertEqual(loop.decide(inp), inp)

    def test_act_returns_output_when_given(self):
        loop = AgentLoop(actor_id="a")
        result = loop.act({"cmd": "run"}, output={"status": "ok"})
        self.assertEqual(result, {"status": "ok"})

    def test_act_returns_input_when_no_output(self):
        loop = AgentLoop(actor_id="a")
        inp = {"cmd": "run"}
        result = loop.act(inp)
        self.assertEqual(result, inp)


# ============================================================================
# TestAgentLoopToolCall
# ============================================================================

class TestAgentLoopToolCall(unittest.TestCase):
    """tool_call records TOOL_CALL step and returns result."""

    def test_tool_call_step_kind_and_method(self):
        loop = AgentLoop(actor_id="a")
        result = loop.tool_call("web_search", {"query": "actors"}, {"hits": 5})

        self.assertEqual(result, {"hits": 5})
        traj = loop.get_trajectory()
        self.assertEqual(len(traj.steps), 1)
        self.assertEqual(traj.steps[0].kind, AgentStepKind.TOOL_CALL)
        self.assertEqual(traj.steps[0].method, "web_search")

    def test_tool_call_accumulates_tokens(self):
        loop = AgentLoop(actor_id="a")
        loop.tool_call("calc", {}, {}, input_tokens=10, output_tokens=5)
        traj = loop.get_trajectory()
        self.assertEqual(traj.total_input_tokens, 10)
        self.assertEqual(traj.total_output_tokens, 5)


# ============================================================================
# TestAgentLoopBudget
# ============================================================================

class TestAgentLoopBudget(unittest.TestCase):
    """Token budget enforcement."""

    def test_not_exceeded_below_budget(self):
        loop = AgentLoop(actor_id="a", config=AgentConfig(token_budget=100))
        loop.act({}, input_tokens=40, output_tokens=40)
        self.assertFalse(loop.budget_exceeded())

    def test_exceeded_at_exact_budget(self):
        loop = AgentLoop(actor_id="a", config=AgentConfig(token_budget=100))
        loop.act({}, input_tokens=50, output_tokens=50)
        self.assertTrue(loop.budget_exceeded())

    def test_exceeded_over_budget(self):
        loop = AgentLoop(actor_id="a", config=AgentConfig(token_budget=100))
        loop.act({}, input_tokens=60, output_tokens=60)
        self.assertTrue(loop.budget_exceeded())

    def test_no_limit_when_budget_is_zero(self):
        loop = AgentLoop(actor_id="a", config=AgentConfig(token_budget=0))
        loop.act({}, input_tokens=10000, output_tokens=10000)
        self.assertFalse(loop.budget_exceeded())


# ============================================================================
# TestAgentLoopIterationLimit
# ============================================================================

class TestAgentLoopIterationLimit(unittest.TestCase):
    """Iteration limit enforcement."""

    def test_not_reached_below_max(self):
        loop = AgentLoop(actor_id="a", config=AgentConfig(max_iterations=2))
        loop.increment_iteration()
        self.assertFalse(loop.iteration_limit_reached())

    def test_reached_at_max(self):
        loop = AgentLoop(actor_id="a", config=AgentConfig(max_iterations=2))
        loop.increment_iteration()
        loop.increment_iteration()
        self.assertTrue(loop.iteration_limit_reached())

    def test_reached_over_max(self):
        loop = AgentLoop(actor_id="a", config=AgentConfig(max_iterations=2))
        for _ in range(3):
            loop.increment_iteration()
        self.assertTrue(loop.iteration_limit_reached())

    def test_unlimited_when_max_iterations_zero(self):
        """max_iterations=0 means unlimited — matches Go/TS/Rust behaviour."""
        loop = AgentLoop(actor_id="a", config=AgentConfig(max_iterations=0))
        for _ in range(1000):
            loop.increment_iteration()
        self.assertFalse(loop.iteration_limit_reached())


# ============================================================================
# TestAgentLoopSuspend
# ============================================================================

class TestAgentLoopSuspend(unittest.TestCase):
    """Suspend sets flag and records a SUSPEND step."""

    def test_is_suspended_false_by_default(self):
        loop = AgentLoop(actor_id="a")
        self.assertFalse(loop.is_suspended)

    def test_suspend_sets_flag(self):
        loop = AgentLoop(actor_id="a")
        loop.suspend("awaiting_approval")
        self.assertTrue(loop.is_suspended)

    def test_suspend_records_step(self):
        loop = AgentLoop(actor_id="a")
        loop.suspend("needs_review")
        traj = loop.get_trajectory()
        self.assertEqual(len(traj.steps), 1)
        self.assertEqual(traj.steps[0].kind, AgentStepKind.SUSPEND)
        self.assertEqual(traj.steps[0].input, "needs_review")


# ============================================================================
# TestAgentLoopFinalizeTrajectory
# ============================================================================

class TestAgentLoopFinalizeTrajectory(unittest.TestCase):
    """finalize_trajectory returns dict with correct fields."""

    def test_returns_dict_with_outcome(self):
        loop = AgentLoop(actor_id="a")
        loop.act({"cmd": "run"}, input_tokens=5, output_tokens=10)
        result = loop.finalize_trajectory("success", "done")

        self.assertIsInstance(result, dict)
        self.assertEqual(result["outcome"], "success")
        self.assertEqual(result["outcome_detail"], "done")

    def test_trajectory_id_non_empty(self):
        loop = AgentLoop(actor_id="a")
        result = loop.finalize_trajectory("failure", "crashed")
        self.assertIn("trajectory_id", result)
        self.assertGreater(len(result["trajectory_id"]), 0)

    def test_token_totals_correct(self):
        loop = AgentLoop(actor_id="a")
        loop.act({}, input_tokens=20, output_tokens=30)
        loop.act({}, input_tokens=10, output_tokens=5)
        result = loop.finalize_trajectory("success", "")
        self.assertEqual(result["total_input_tokens"], 30)
        self.assertEqual(result["total_output_tokens"], 35)

    def test_step_count_in_result(self):
        loop = AgentLoop(actor_id="a")
        loop.observe("a")
        loop.orient("b")
        result = loop.finalize_trajectory("success", "")
        self.assertEqual(result["step_count"], 2)

    def test_agent_actor_id_in_trajectory(self):
        loop = AgentLoop(actor_id="my-actor-007")
        result = loop.finalize_trajectory("success", "")
        self.assertEqual(result["agent_actor_id"], "my-actor-007")

    def test_eval_run_id_and_scenario_id_propagated(self):
        loop = AgentLoop(
            actor_id="a",
            eval_run_id="eval-42",
            scenario_id="sc-math-01",
        )
        result = loop.finalize_trajectory("success", "")
        self.assertEqual(result["eval_run_id"], "eval-42")
        self.assertEqual(result["scenario_id"], "sc-math-01")


if __name__ == "__main__":
    unittest.main()
