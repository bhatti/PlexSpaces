# SPDX-License-Identifier: AGPL-3.0-or-later
"""ChatAgentActor — Cloudflare Agents SDK equivalent (Python WASM).

Demonstrates conversation state in KV, LLM calls via service link, and
durable alarm for periodic summarization.

Cloudflare Agents SDK vs PlexSpaces Python:

  Cloudflare Agents SDK              | PlexSpaces Python
  -----------------------------------|----------------------------------------------
  this.env.AI.run(model, {messages}) | ServiceHttpClient("llm-link").post(...)
  await this.storage.get('history')  | host.kv.get_json("history")
  await this.storage.put('history')  | host.kv.put_json("history", v)
  storage.setAlarm(ts)               | host.alarm.set(ts)
  async onAlarm() { ... }            | @handler("__alarm__")
  connection.send(reply)             | return dict (sync response)
  env.AI binding in wrangler.toml    | [service_links.llm-link] in app-config.toml
  Durable Object per-agent           | virtual_actor + reminder facets

NOTE: LLM calls require ANTHROPIC_API_KEY configured in service_links.
test.sh validates state and alarm logic only.
"""

import json
from typing import Any, Dict, List, Optional

from plexspaces import actor, state, handler, init_handler, host
from plexspaces.host import ServiceHttpClient

# How many messages before scheduling the summarization alarm.
_ALARM_THRESHOLD = 10
# Delay before summarization alarm fires (5 minutes in ms).
_ALARM_DELAY_MS = 300_000


@actor
class ChatAgentActor:
    """Minimal chat agent: conversation in KV, LLM via service link, alarm for summarization."""

    actor_id: str = state(default="")
    total_messages: int = state(default=0)
    total_summarizations: int = state(default=0)

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        host.info(f"ChatAgentActor init actor_id={self.actor_id}")

    @handler("chat")
    def chat(self, message: str = "") -> dict:
        """Handle a chat message.

        Equivalent to Cloudflare Agents SDK onMessage():
          1. Load history from KV (this.storage.get)
          2. Append user message
          3. Call LLM via service link (this.env.AI.run)
          4. Append response, persist to KV (this.storage.put)
          5. Schedule alarm after threshold (storage.setAlarm)
        """
        if not message:
            return {"error": "message is required"}

        # Load history from KV — equivalent to: await this.storage.get('history')
        history: List[Dict[str, Any]] = host.kv.get_json("history") or []

        history.append({
            "role": "user",
            "content": message,
            "timestamp": host.now_ms(),
        })

        # Call LLM via service link — equivalent to: await this.env.AI.run(model, {messages})
        assistant_reply = self._call_llm(history)

        history.append({
            "role": "assistant",
            "content": assistant_reply,
            "timestamp": host.now_ms(),
        })

        # Persist history — equivalent to: await this.storage.put('history', history)
        host.kv.put_json("history", history)

        self.total_messages += 1

        # Schedule alarm after threshold — equivalent to: storage.setAlarm(ts)
        if len(history) > _ALARM_THRESHOLD:
            if host.alarm.get() == 0:
                host.alarm.set(host.now_ms() + _ALARM_DELAY_MS)
                host.info("ChatAgentActor: alarm set for summarization in 5 minutes")

        return {
            "status": "ok",
            "reply": assistant_reply,
            "history_length": len(history),
        }

    @handler("get_history")
    def get_history(self) -> dict:
        """Return the stored conversation history from KV."""
        history = host.kv.get_json("history") or []
        return {
            "status": "ok",
            "history": history,
            "count": len(history),
        }

    @handler("clear")
    def clear(self) -> dict:
        """Clear history, summary, and pending alarm.

        Equivalent to: storage.delete('history'); storage.deleteAlarm()
        """
        host.kv.delete("history")
        host.kv.delete("summary")
        host.alarm.delete()
        return {"status": "ok", "cleared": True}

    @handler("__alarm__")
    def on_alarm(self) -> dict:
        """Durable alarm callback — equivalent to Cloudflare Agents SDK onAlarm().

        Summarizes conversation history and stores a summary KV key,
        then clears history.
        """
        host.info("ChatAgentActor: alarm fired — summarizing history")

        history = host.kv.get_json("history") or []
        if not history:
            return {"status": "ok", "action": "no_history_to_summarize"}

        # Summarize via LLM
        summary_prompt = (
            f"Summarize this conversation concisely (2-3 sentences): "
            f"{json.dumps([{'role': m['role'], 'content': m['content']} for m in history])}"
        )
        summary = self._call_llm([{"role": "user", "content": summary_prompt}])

        # Persist summary, clear history — equivalent to: storage.put('summary', s); storage.delete('history')
        host.kv.put("summary", summary)
        host.kv.delete("history")

        self.total_summarizations += 1

        host.info(f"ChatAgentActor: summarized {len(history)} messages")
        return {
            "status": "ok",
            "action": "summarized",
            "messages_summarized": len(history),
        }

    # ---- internal helpers ----

    def _call_llm(self, messages: List[Dict[str, Any]]) -> str:
        """Call the LLM service link with the given messages."""
        try:
            http = ServiceHttpClient("llm-link")
            body = {
                "model": "claude-3-5-haiku-20241022",
                "max_tokens": 1024,
                "messages": [{"role": m["role"], "content": m["content"]} for m in messages],
            }
            resp = http.post("/v1/messages", body)
            # Parse Anthropic response: content[0].text
            if isinstance(resp, dict):
                content = resp.get("content")
                if content and isinstance(content, list) and len(content) > 0:
                    block = content[0]
                    if isinstance(block, dict) and "text" in block:
                        return block["text"]
                # Fallback: OpenAI-compatible
                choices = resp.get("choices")
                if choices and isinstance(choices, list) and len(choices) > 0:
                    msg = choices[0].get("message", {})
                    if "content" in msg:
                        return msg["content"]
        except Exception as e:
            host.warn(f"ChatAgentActor: LLM call failed: {e}")
        return "[LLM unavailable — message stored]"
