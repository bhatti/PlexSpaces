# SPDX-License-Identifier: AGPL-3.0-or-later
"""AgentActor — core agentic loop (user message → LLM → tool_use → repeat → end_turn).

SessionManagerActor — session lifecycle backed by KV.
"""

from plexspaces import actor, state, handler, init_handler, host
from .helpers import pg_first, fire_audit, write_actor_info, ask

_MAX_ITER = 5


@actor
class AgentActor:
    """Core agent: receive user message, call LLM, execute tools, loop until end_turn."""

    system_prompt: str = state(default="You are a helpful AI assistant with access to tools.")
    messages: list = state(default_factory=list)
    max_history: int = state(default=50)
    total_chats: int = state(default=0)
    agent_name: str = state(default="general-assistant")
    capabilities: list = state(default_factory=list)
    actor_id: str = state(default="")

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        args = config.get("args", {})
        self.agent_name = args.get("agent_name", self.agent_name)
        self.system_prompt = args.get("system_prompt", self.system_prompt)
        if args.get("max_history"):
            self.max_history = int(args["max_history"])
        self.capabilities = ["chat", "tool_use", "memory"]
        host.process_groups.join("svc:agent")
        write_actor_info(self.actor_id, self.agent_name,
                         "Core agent loop with tool calling and session memory",
                         self.capabilities)
        host.info(f"AgentActor init actor_id={self.actor_id} name={self.agent_name}")

    @handler("chat")
    def chat(self, message: str = "", session_id: str = "") -> dict:
        if not message:
            return {"error": "message is required"}

        self.messages.append({"role": "user", "content": message})

        # Discover tools
        tool_reg_id, _ = pg_first("svc:tool_registry")
        tools = []
        if tool_reg_id:
            resp = ask(tool_reg_id, "list_tools", {})
            if resp:
                tools = resp.get("tools", [])

        # Signal FSM: processing
        fsm_id, _ = pg_first("svc:agent_fsm")
        if fsm_id:
            host.send(fsm_id, "transition", {"op": "transition", "to": "processing"})

        final_response = ""
        for i in range(_MAX_ITER):
            llm_id, err = pg_first("svc:llm_router")
            if err or not llm_id:
                final_response = f"[no LLM] Processed: {message}"
                break

            llm_resp = ask(llm_id, "chat_completion", {"messages": [{"role": "system", "content": self.system_prompt}] + self.messages, "tools": tools}, 10000)
            if not llm_resp or "error" in llm_resp:
                final_response = f"LLM unavailable: {llm_resp}"
                break

            response = llm_resp.get("response", {})
            stop_reason = response.get("stop_reason", "end_turn")
            content = response.get("content", "")

            assistant_msg = {"role": "assistant", "content": content, "stop_reason": stop_reason}
            if response.get("tool_calls"):
                assistant_msg["tool_calls"] = response["tool_calls"]
            self.messages.append(assistant_msg)

            if stop_reason == "end_turn":
                final_response = content
                break

            if stop_reason == "tool_use":
                if fsm_id:
                    host.send(fsm_id, "transition", {"op": "transition", "to": "tool_executing"})

                for tc in response.get("tool_calls", []):
                    tc_name = tc.get("name", "")
                    tc_input = tc.get("input", {})
                    tool_output = {}
                    if tool_reg_id:
                        tool_output = ask(tool_reg_id, "execute_tool", {"name": tc_name, "input": tc_input}) or {}

                    self.messages.append({
                        "role": "tool",
                        "tool_call_id": tc.get("id", ""),
                        "content": str(tool_output),
                    })
                    fire_audit("tool_called", f"tool={tc_name} session={session_id}")

                if fsm_id:
                    host.send(fsm_id, "transition", {"op": "transition", "to": "processing"})
                final_response = f"Tool results applied (iteration {i + 1})"
            else:
                final_response = content
                break

        # FSM: responding → idle
        if fsm_id:
            host.send(fsm_id, "transition", {"op": "transition", "to": "responding"})
            host.send(fsm_id, "transition", {"op": "transition", "to": "idle"})

        # Compact history if needed
        if len(self.messages) > self.max_history:
            keep = self.max_history // 2
            self.messages = self.messages[:1] + self.messages[-keep:]

        # Persist history in KV if session provided
        if session_id:
            import json
            host.kv.put(f"session_history:{session_id}", json.dumps(self.messages))

        self.total_chats += 1
        fire_audit("agent_chat", f"session={session_id}")
        return {
            "status": "ok",
            "response": final_response,
            "session_id": session_id,
            "messages_count": len(self.messages),
        }

    @handler("set_system_prompt")
    def set_system_prompt(self, prompt: str = "") -> dict:
        self.system_prompt = prompt or self.system_prompt
        return {"status": "ok"}

    @handler("get_history")
    def get_history(self) -> dict:
        return {"status": "ok", "messages": self.messages, "count": len(self.messages)}

    @handler("get_capabilities")
    def get_capabilities(self) -> dict:
        return {"status": "ok", "capabilities": self.capabilities}

    @handler("get_stats")
    def get_stats(self) -> dict:
        return {"status": "ok", "total_chats": self.total_chats, "agent_name": self.agent_name}


# ---------------------------------------------------------------------------


@actor
class SessionManagerActor:
    """Manages agent session lifecycle backed by KV storage."""

    active_sessions: int = state(default=0)
    total_created: int = state(default=0)
    session_ids: list = state(default_factory=list)
    actor_id: str = state(default="")

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        host.process_groups.join("svc:session_manager")
        host.info(f"SessionManagerActor init actor_id={self.actor_id}")

    @handler("create_session")
    def create_session(self, channel: str = "web", user_id: str = "anonymous", agent_id: str = "agent") -> dict:
        import json
        session_id = f"sess-{channel}-{user_id}-{host.now_ms()}"
        meta = {
            "session_id": session_id,
            "channel": channel,
            "user_id": user_id,
            "agent_id": agent_id,
            "created_at": host.now_ms(),
            "status": "active",
        }
        host.kv.put(f"session:{session_id}", json.dumps(meta))
        host.kv.put(f"session_map:{channel}:{user_id}", session_id)
        self.session_ids.append(session_id)
        self.active_sessions += 1
        self.total_created += 1
        fire_audit("session_created", f"session_id={session_id} channel={channel} user_id={user_id}")
        host.info(f"SessionManager: created session_id={session_id}")
        return {"status": "ok", "session_id": session_id}

    @handler("get_session")
    def get_session(self, session_id: str = "", channel: str = "", user_id: str = "") -> dict:
        import json
        if not session_id and channel and user_id:
            session_id = host.kv.get(f"session_map:{channel}:{user_id}")
        if not session_id:
            return {"error": "session not found"}
        raw = host.kv.get(f"session:{session_id}")
        if not raw:
            return {"error": "session not found", "session_id": session_id}
        meta = json.loads(raw)
        meta["status"] = "ok"
        return meta

    @handler("end_session")
    def end_session(self, session_id: str = "") -> dict:
        if not session_id:
            return {"error": "session_id is required"}
        host.kv.delete(f"session:{session_id}")
        self.session_ids = [s for s in self.session_ids if s != session_id]
        if self.active_sessions > 0:
            self.active_sessions -= 1
        fire_audit("session_ended", f"session_id={session_id}")
        return {"status": "ok", "session_id": session_id}

    @handler("list_sessions")
    def list_sessions(self) -> dict:
        import json
        sessions = []
        for sid in self.session_ids:
            raw = host.kv.get(f"session:{sid}")
            if raw:
                try:
                    sessions.append(json.loads(raw))
                except Exception:
                    pass
        return {"status": "ok", "sessions": sessions, "count": len(sessions)}

    @handler("get_stats")
    def get_stats(self) -> dict:
        return {"status": "ok", "active_sessions": self.active_sessions, "total_created": self.total_created}
