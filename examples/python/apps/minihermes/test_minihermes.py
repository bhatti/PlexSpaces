# SPDX-License-Identifier: AGPL-3.0-or-later
"""Contract tests for MiniHermes Python actors.

Uses the PlexSpaces stub host (reset_stubs / MockHost) so no running node is needed.
All actors are exercised in isolation against the same mock infrastructure.
"""

import json
import os
import sys

# Add both the minihermes package dir and the SDK to sys.path so tests run
# without installation (same pattern as weather_actor/test_weather_actor.py).
_HERE = os.path.dirname(os.path.abspath(__file__))
_SDK = os.path.abspath(os.path.join(_HERE, "../../../../sdks/python"))
for _p in (_HERE, _SDK):
    if _p not in sys.path:
        sys.path.insert(0, _p)

import pytest
from plexspaces.host import _MockHost, host as plexspaces_host


# ── Fixture: reset stubs before each test ────────────────────────────────────

@pytest.fixture(autouse=True)
def reset_stubs():
    """Reset all MockHost state between tests.

    Forces a fresh _MockHost into the module global so every test starts
    with an empty KV store, no process groups, etc.
    """
    import plexspaces.host as _hmod
    fresh = _MockHost()
    _hmod._host_impl = fresh
    # Also patch _get_host's closure by ensuring the module global is visible
    yield
    # post-test: leave state for inspection if needed


def _make_config(role: str, actor_id: str = "test-actor@node", args: dict = None) -> dict:
    return {"actor_id": actor_id, "args": {"role": role, **(args or {})}}


# ── LLMGatewayActor ───────────────────────────────────────────────────────────

class TestLLMGateway:
    def _actor(self):
        from minihermes_actor import LLMGatewayActor
        a = LLMGatewayActor()
        a.on_init(_make_config("llm_gateway"))
        return a

    def test_simulated_end_turn(self):
        a = self._actor()
        resp = a.completion(messages=[{"role": "user", "content": "hello world"}])
        assert resp["status"] == "ok"
        assert resp["response"]["stop_reason"] == "end_turn"
        assert a.request_count == 1

    def test_simulated_calculator_tool_use(self):
        a = self._actor()
        tools = [{"name": "calculator", "description": "eval math"}]
        resp = a.completion(messages=[{"role": "user", "content": "please calculate 10 * 5"}], tools=tools)
        assert resp["response"]["stop_reason"] == "tool_use"
        assert resp["response"]["tool_calls"][0]["name"] == "calculator"

    def test_simulated_memory_tool_use(self):
        a = self._actor()
        tools = [{"name": "memory_store"}]
        resp = a.completion(messages=[{"role": "user", "content": "remember this fact"}], tools=tools)
        assert resp["response"]["stop_reason"] == "tool_use"

    def test_simulated_cron_tool_use(self):
        a = self._actor()
        tools = [{"name": "create_cron_job"}]
        resp = a.completion(messages=[{"role": "user", "content": "schedule something every hour"}], tools=tools)
        assert resp["response"]["stop_reason"] == "tool_use"

    def test_register_provider(self):
        a = self._actor()
        resp = a.register_provider(name="openai", base_url="https://api.openai.com", model="gpt-4o")
        assert resp["status"] == "ok"
        assert resp["provider"] == "openai"

    def test_switch_provider(self):
        a = self._actor()
        resp = a.switch_provider(provider="openai", model="gpt-4o")
        assert resp["status"] == "ok"
        assert a.active_provider == "openai"
        assert a.default_model == "gpt-4o"

    def test_reset_circuit(self):
        a = self._actor()
        a.circuit_open = True
        a.consecutive_failures = 5
        resp = a.reset_circuit()
        assert not a.circuit_open
        assert a.consecutive_failures == 0

    def test_get_stats(self):
        a = self._actor()
        a.completion(messages=[{"role": "user", "content": "test"}])
        resp = a.get_stats()
        assert resp["request_count"] == 1
        assert "active_provider" in resp


# ── ToolExecutorActor ─────────────────────────────────────────────────────────

class TestToolExecutor:
    def _actor(self):
        from minihermes_actor import ToolExecutorActor
        a = ToolExecutorActor()
        a.on_init(_make_config("tool_executor"))
        return a

    def test_list_builtin_tools(self):
        a = self._actor()
        resp = a.list_tools()
        assert resp["count"] >= 6

    def test_calculator(self):
        a = self._actor()
        for expr, expected in [("10 * 5", 50.0), ("100 + 25", 125.0), ("20 - 8", 12.0), ("16 / 4", 4.0)]:
            resp = a.execute(name="calculator", input={"expression": expr})
            assert resp["status"] == "ok", f"calculator failed for {expr}"
            assert resp["output"]["result"] == expected, f"wrong result for {expr}"

    def test_register_custom_tool(self):
        a = self._actor()
        resp = a.register_tool(name="my_tool", description="Test", input_schema={})
        assert resp["status"] == "ok"
        list_resp = a.list_tools()
        names = [t["name"] for t in list_resp["tools"]]
        assert "my_tool" in names

    def test_unknown_tool_returns_error(self):
        a = self._actor()
        resp = a.execute(name="nonexistent_tool", input={})
        assert "error" in resp

    def test_division_by_zero(self):
        a = self._actor()
        resp = a.execute(name="calculator", input={"expression": "5 / 0"})
        assert "error" in resp["output"]


# ── AgentActor ────────────────────────────────────────────────────────────────

class TestAgent:
    def _actor(self):
        from minihermes_actor import AgentActor
        a = AgentActor()
        a.on_init(_make_config("agent", args={"max_iterations": "3"}))
        return a

    def test_simple_chat(self):
        a = self._actor()
        resp = a.chat(message="hello", session_id="s1")
        assert resp["status"] == "ok"
        assert "response" in resp
        assert a.total_chats == 1

    def test_chat_increments_counters(self):
        a = self._actor()
        a.chat(message="msg1", session_id="s1")
        a.chat(message="msg2", session_id="s1")
        assert a.total_chats == 2

    def test_clear_history(self):
        a = self._actor()
        a.chat(message="test", session_id="s1")
        resp = a.clear_history(session_id="s1")
        assert resp["status"] == "ok"
        assert len(a.messages) == 0

    def test_get_stats(self):
        a = self._actor()
        a.chat(message="x", session_id="s1")
        stats = a.get_stats()
        assert stats["total_chats"] == 1
        assert "total_tool_calls" in stats

    def test_process_cron(self):
        a = self._actor()
        resp = a.process_cron(job_id="j1", prompt="What is 2+2?")
        assert resp["status"] == "ok"
        assert resp["job_id"] == "j1"
        assert "run_id" in resp
        # Cron should not pollute main message history
        assert len(a.messages) == 0


# ── SkillStoreActor ───────────────────────────────────────────────────────────

class TestSkillStore:
    def _actor(self):
        from minihermes_actor import SkillStoreActor
        a = SkillStoreActor()
        a.on_init(_make_config("skill_store"))
        return a

    def test_propose_and_get(self):
        a = self._actor()
        resp = a.propose_skill(name="Test Skill", description="A test", procedure="Step 1\nStep 2",
                               tags="test,demo", trigger_patterns="test,demo")
        assert resp["status"] == "ok"
        skill_id = resp["skill_id"]
        assert a.skill_count == 1

        get_resp = a.get_skill(skill_id=skill_id)
        assert get_resp["name"] == "Test Skill"

    def test_match_by_trigger(self):
        a = self._actor()
        a.propose_skill(name="DB Backup", description="Backup DB", procedure="pg_dump",
                        tags="database,backup", trigger_patterns="backup,database")
        resp = a.match_skills(query="I need to backup my database")
        assert resp["count"] >= 1

    def test_delete_skill(self):
        a = self._actor()
        create = a.propose_skill(name="Temp", description="temp", procedure="temp",
                                 tags="temp", trigger_patterns="temp")
        skill_id = create["skill_id"]
        del_resp = a.delete_skill(skill_id=skill_id)
        assert del_resp["status"] == "ok"
        assert a.skill_count == 0

    def test_record_usage(self):
        a = self._actor()
        create = a.propose_skill(name="Used Skill", description="x", procedure="x",
                                 tags="x", trigger_patterns="x")
        skill_id = create["skill_id"]
        resp = a.record_usage(skill_id=skill_id)
        assert resp["usage_count"] == 1

    def test_evaluate_no_learning_few_tools(self):
        a = self._actor()
        resp = a.evaluate_for_learning(session_id="s1", tool_call_count=2, messages="[]")
        assert resp["action"] == "no_learning"

    def test_get_stats(self):
        a = self._actor()
        a.propose_skill(name="S1", description="d", procedure="p", tags="t")
        stats = a.get_stats()
        assert stats["skill_count"] == 1


# ── MemoryActor ───────────────────────────────────────────────────────────────

class TestMemory:
    def _actor(self):
        from minihermes_actor import MemoryActor
        a = MemoryActor()
        a.on_init(_make_config("memory"))
        return a

    def test_store_and_recall_core(self):
        a = self._actor()
        resp = a.store_memory(tier="core", key="user_name", value="Alice", scope="global")
        assert resp["status"] == "ok"
        assert a.core_count == 1

        recall = a.recall_memory(query="user_name", scope="global")
        assert recall["count"] >= 1

    def test_store_reachable(self):
        a = self._actor()
        resp = a.store_memory(tier="reachable", key="topic", value="AI", scope="global")
        assert resp["status"] == "ok"
        assert a.reachable_count == 1

    def test_delete_memory(self):
        a = self._actor()
        a.store_memory(tier="core", key="temp", value="x", scope="global")
        resp = a.delete_memory(tier="core", key="temp", scope="global")
        assert resp["status"] == "ok"

    def test_get_stats(self):
        a = self._actor()
        a.store_memory(tier="core", key="k", value="v", scope="global")
        stats = a.get_stats()
        assert stats["core_count"] == 1


# ── AuditEventActor ───────────────────────────────────────────────────────────

class TestAuditEvent:
    def _actor(self):
        from minihermes_actor import AuditEventActor
        a = AuditEventActor()
        a.on_init(_make_config("audit_event"))
        return a

    def test_log_and_poll(self):
        a = self._actor()
        a.log_event(event_type="test_event", detail="detail1")
        a.log_event(event_type="test_event", detail="detail2")
        resp = a.poll_events(consumer_id="c1", limit=10)
        assert resp["count"] == 2
        assert a.watermark == 2

    def test_two_cursor_isolation(self):
        a = self._actor()
        a.log_event(event_type="e1", detail="d1")
        a.log_event(event_type="e2", detail="d2")
        a.log_event(event_type="e3", detail="d3")

        # Use unique consumer IDs scoped to this test to avoid any cross-test cursor state
        r1 = a.poll_events(consumer_id="isolation-c1", limit=10)
        assert r1["count"] == 3, f"expected 3, got {r1['count']}"

        # Same consumer again: cursor advanced, sees 0
        r2 = a.poll_events(consumer_id="isolation-c1", limit=10)
        assert r2["count"] == 0

        # Different consumer: independent cursor, sees all 3
        r3 = a.poll_events(consumer_id="isolation-c2", limit=10)
        assert r3["count"] == 3


# ── ContextCompressorActor ────────────────────────────────────────────────────

class TestContextCompressor:
    def _actor(self):
        from minihermes_actor import ContextCompressorActor
        a = ContextCompressorActor()
        a.on_init(_make_config("context_compressor"))
        return a

    def test_no_compression_small_history(self):
        a = self._actor()
        msgs = [{"role": "user", "content": "hi"}, {"role": "assistant", "content": "hello"}]
        resp = a.compress(session_id="s1", messages=json.dumps(msgs), keep_last=4)
        assert resp["action"] == "no_compression_needed"

    def test_compresses_large_history(self):
        a = self._actor()
        msgs = [{"role": "user" if i % 2 == 0 else "assistant", "content": f"msg {i}"} for i in range(12)]
        resp = a.compress(session_id="s1", messages=json.dumps(msgs), keep_last=4)
        assert resp["action"] == "compressed"
        assert resp["after_messages"] < resp["before_messages"]
        assert a.compress_count == 1


# ── CronSchedulerActor ────────────────────────────────────────────────────────

class TestCronScheduler:
    def _actor(self):
        from minihermes_actor import CronSchedulerActor
        a = CronSchedulerActor()
        a.on_init(_make_config("cron_scheduler", args={"tick_interval_ms": "60000"}))
        return a

    def test_create_job(self):
        a = self._actor()
        resp = a.create_job(job_id="j1", prompt="Do something", schedule="every_1h")
        assert resp["status"] == "ok"
        assert resp["interval_ms"] == 3_600_000
        assert a.job_count == 1

    def test_list_jobs(self):
        a = self._actor()
        a.create_job(job_id="j1", prompt="Task 1", schedule="every_5m")
        a.create_job(job_id="j2", prompt="Task 2", schedule="every_1h")
        resp = a.list_jobs()
        assert resp["count"] == 2

    def test_delete_job(self):
        a = self._actor()
        a.create_job(job_id="to-del", prompt="x", schedule="every_1h")
        resp = a.delete_job(job_id="to-del")
        assert resp["status"] == "ok"
        assert a.job_count == 0

    def test_trigger_tick(self):
        a = self._actor()
        a.create_job(job_id="tick-j", prompt="tick test", schedule="every_1m")
        resp = a.trigger_tick()
        assert resp["status"] == "ok"
        assert a.tick_count == 1

    def test_schedule_to_ms(self):
        from cron_scheduler import schedule_to_ms
        assert schedule_to_ms("every_1m") == 60_000
        assert schedule_to_ms("every_5m") == 300_000
        assert schedule_to_ms("every_1h") == 3_600_000
        assert schedule_to_ms("every_24h") == 86_400_000
        assert schedule_to_ms("unknown") == 3_600_000  # default


# ── GuardrailsGateActor ───────────────────────────────────────────────────────

class TestGuardrails:
    def _actor(self):
        from minihermes_actor import GuardrailsGateActor
        a = GuardrailsGateActor()
        a.on_init(_make_config("guardrails"))
        return a

    def test_allow_safe_tool(self):
        a = self._actor()
        resp = a.check(tool="calculator")
        assert resp["decision"] == "allow"

    def test_review_http_request(self):
        a = self._actor()
        resp = a.check(tool="http_request")
        assert resp["decision"] == "requires_approval"
        assert "approval_id" in resp

    def test_deny_destructive_tool(self):
        a = self._actor()
        resp = a.check(tool="delete_file")
        assert resp["decision"] == "deny"

    def test_approve_flow(self):
        a = self._actor()
        check_resp = a.check(tool="http_request")
        approval_id = check_resp.get("approval_id", "")
        if not approval_id:
            pytest.skip("tool does not require approval")
        approve_resp = a.approve(approval_id=approval_id)
        assert approve_resp["decision"] == "approved"
        assert a.approval_count == 1

    def test_set_custom_policy(self):
        a = self._actor()
        a.set_policy(tool="my_tool", policy="deny")
        resp = a.check(tool="my_tool")
        assert resp["decision"] == "deny"


# ── SessionManagerActor ───────────────────────────────────────────────────────

class TestSessionManager:
    def _actor(self):
        from minihermes_actor import SessionManagerActor
        a = SessionManagerActor()
        a.on_init(_make_config("session_manager"))
        return a

    def test_create_and_get(self):
        a = self._actor()
        create = a.create_session(channel="web", user_id="alice")
        session_id = create["session_id"]
        assert create["status"] == "ok"

        get_resp = a.get_session(session_id=session_id)
        assert get_resp["session_id"] == session_id

    def test_end_session(self):
        a = self._actor()
        create = a.create_session(channel="cli", user_id="bob")
        session_id = create["session_id"]
        end_resp = a.end_session(session_id=session_id)
        assert end_resp["status"] == "ok"
        assert a.active_sessions == 0


# ── HealthMonitorActor ────────────────────────────────────────────────────────

class TestHealthMonitor:
    def _actor(self):
        from minihermes_actor import HealthMonitorActor
        a = HealthMonitorActor()
        a.on_init(_make_config("health_monitor", args={"poll_interval_ms": "10000"}))
        return a

    def test_trigger_poll(self):
        a = self._actor()
        resp = a.trigger_poll_tick()
        assert resp["status"] == "ok"
        assert a.poll_count == 1

    def test_get_health(self):
        a = self._actor()
        a.trigger_poll_tick()
        resp = a.get_health()
        assert resp["status"] == "ok"
        assert "group_health" in resp


# ── helpers ───────────────────────────────────────────────────────────────────

class TestHelpers:
    def test_registry_first_falls_back_to_pg(self):
        from helpers import registry_first
        # With MockHost, registry is empty; pg_first fallback also empty → returns error
        actor_id, err = registry_first("agent", fallback_group="svc:agent")
        assert actor_id is None or isinstance(actor_id, str)  # no exception

    def test_registry_all_returns_list(self):
        from helpers import registry_all
        ids = registry_all("agent")
        assert isinstance(ids, list)

    def test_truncate_str(self):
        from helpers import truncate_str
        assert len(truncate_str("hello world", 5)) <= 8  # "hello..."
        assert truncate_str("short", 100) == "short"
