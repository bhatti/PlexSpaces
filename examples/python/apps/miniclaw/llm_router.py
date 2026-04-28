# SPDX-License-Identifier: AGPL-3.0-or-later
"""LLMRouterActor — simulated LLM with tool-call support.

Simulates an LLM that decides when to call tools vs. return a final answer.
In production replace the simulation logic with a real API call (OpenAI,
Anthropic, Bedrock, …) via host.http_fetch() over a named service link.
"""

from plexspaces import actor, state, handler, init_handler, host

TOOL_CALL_TRIGGERS = ("weather", "search", "calculate", "lookup", "find")


@actor
class LLMRouterActor:
    """Simulated LLM router with tool-calling capability."""

    model: str = state(default="miniclaw-simulated-v1")
    request_count: int = state(default=0)
    actor_id: str = state(default="")

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        self.model = config.get("args", {}).get("model", self.model)
        host.process_groups.join("svc:llm_router")
        host.info(f"LLMRouterActor init actor_id={self.actor_id} model={self.model}")

    @handler("chat_completion")
    def chat_completion(self, messages: list = None, tools: list = None) -> dict:
        """Simulate an LLM chat completion, optionally routing through tools."""
        messages = messages or []
        tools = tools or []
        self.request_count += 1

        # Determine last user message
        user_msg = ""
        for m in reversed(messages):
            if m.get("role") == "user":
                user_msg = str(m.get("content", "")).lower()
                break

        # Decide: tool_use or end_turn
        should_use_tool = tools and any(kw in user_msg for kw in TOOL_CALL_TRIGGERS)

        if should_use_tool:
            tool = tools[0] if tools else {}
            tool_name = tool.get("name", "search") if isinstance(tool, dict) else "search"
            response = {
                "stop_reason": "tool_use",
                "content": "",
                "tool_calls": [{
                    "id": f"tc_{self.request_count}",
                    "name": tool_name,
                    "input": {"query": user_msg},
                }],
            }
        else:
            response = {
                "stop_reason": "end_turn",
                "content": f"[{self.model}] Processed: {user_msg}",
                "tool_calls": [],
            }

        host.info(f"LLM chat_completion stop_reason={response['stop_reason']} req={self.request_count}")
        return {"status": "ok", "response": response, "model": self.model}

    @handler("get_stats")
    def get_stats(self) -> dict:
        return {"status": "ok", "model": self.model, "request_count": self.request_count}
