# SPDX-License-Identifier: AGPL-3.0-or-later
"""ContextCompressorActor — LLM-assisted middle-history summarization with KV checkpoint."""

import json
from plexspaces import actor, state, handler, init_handler, host
from helpers import registry_first, fire_audit, ask


@actor
class ContextCompressorActor:
    """Compresses conversation history when token budget is exceeded."""

    compress_count: int = state(default=0)
    tokens_saved: int = state(default=0)
    actor_id: str = state(default="")

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        host.process_groups.join("svc:compressor")
        try:
            host.registry.register(
                ctx="", object_id=self.actor_id, object_type="actor", grpc_address="",
                object_category="compressor", capabilities=["compress"],
            )
        except Exception:
            pass
        host.info(f"ContextCompressorActor init actor_id={self.actor_id}")

    @handler("compress")
    def compress(self, session_id: str = "", messages: str = "[]", keep_last: int = 4) -> dict:
        try:
            msgs = json.loads(messages)
        except Exception:
            return {"error": "invalid messages JSON"}

        keep_last = int(keep_last)
        if len(msgs) <= keep_last + 2:
            return {"status": "ok", "action": "no_compression_needed",
                    "before_messages": len(msgs), "after_messages": len(msgs)}

        # Split: system messages stay, middle is summarized, recent is kept
        system_msgs = [m for m in msgs if m.get("role") == "system"]
        non_system = [m for m in msgs if m.get("role") != "system"]
        middle = non_system[:-keep_last]
        recent = non_system[-keep_last:]

        # Checkpoint original in KV
        checkpoint_key = f"compression_checkpoint:{session_id}:{host.now_ms()}"
        try:
            host.kv_put(checkpoint_key, messages[:4000])
        except Exception:
            pass

        # Try LLM summarization; fall back to simple count
        summary = self._summarize_via_llm(middle) or self._simple_summary(middle)

        summary_msg = {"role": "assistant", "content": f"[Context summary: {summary}]"}
        compressed = system_msgs + [summary_msg] + recent

        self.compress_count += 1
        self.tokens_saved += max(0, len(middle) * 50)
        host.incr_counter("compressions", 1)
        fire_audit("context_compressed", f"session={session_id} before={len(msgs)} after={len(compressed)}")

        return {
            "status": "ok",
            "action": "compressed",
            "before_messages": len(msgs),
            "after_messages": len(compressed),
            "compressed_messages": json.dumps(compressed),
        }

    @handler("get_stats")
    def get_stats(self) -> dict:
        return {"status": "ok", "compress_count": self.compress_count, "tokens_saved": self.tokens_saved}

    def _summarize_via_llm(self, messages: list) -> str:
        llm_id, err = registry_first("llm_gateway", fallback_group="svc:llm_gateway")
        if err or not llm_id:
            return ""
        text = " ".join(str(m.get("content", ""))[:100] for m in messages[:5])
        resp = ask(llm_id, "completion", {
            "messages": [{"role": "user", "content": f"Summarize in one sentence: {text[:400]}"}],
            "tools": [],
        }, 8000)
        if resp and resp.get("response", {}).get("content"):
            return resp["response"]["content"][:200]
        return ""

    def _simple_summary(self, messages: list) -> str:
        user_count = sum(1 for m in messages if m.get("role") == "user")
        tool_count = sum(1 for m in messages if m.get("role") == "tool")
        return f"{len(messages)} messages summarized ({user_count} user, {tool_count} tool calls)"
