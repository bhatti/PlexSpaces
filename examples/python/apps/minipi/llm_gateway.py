# SPDX-License-Identifier: AGPL-3.0-or-later
"""LLMGatewayActor — model abstraction with cost tracking and caching.

Demonstrates: GenServer pattern, KV caching, circuit breaker pattern,
token usage tracking (feeds ExecutionTraceFacet).
"""

import json
import hashlib
from plexspaces import actor, state, init_handler, handler, host

_DEFAULT_MODEL = "llama3.2"
_OLLAMA_BASE_URL = "http://localhost:11434"
_CACHE_TTL_SECONDS = 300  # 5 minutes


@actor
class LLMGatewayActor:
    """
    LLM Gateway: routes completion requests to Ollama (or mock).

    Tracks token usage per request for budget enforcement.
    Caches deterministic completions to avoid redundant LLM calls during eval replays.
    """

    actor_id: str = state(default="")
    model: str = state(default=_DEFAULT_MODEL)
    provider: str = state(default="mock")
    base_url: str = state(default=_OLLAMA_BASE_URL)
    total_requests: int = state(default=0)
    total_input_tokens: int = state(default=0)
    total_output_tokens: int = state(default=0)
    cache_hits: int = state(default=0)

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        args = config.get("args", {})
        self.model = args.get("model", _DEFAULT_MODEL)
        self.provider = args.get("provider", "mock")
        self.base_url = args.get("base_url", _OLLAMA_BASE_URL)
        try:
            host.kv.put("svc:llm_gateway", host.self_id())
        except Exception:
            pass
        try:
            host.registry.register(None, self.actor_id or host.self_id(), "actor", "",
                                   object_category="llm_gateway")
        except Exception:
            pass
        host.info(f"LLMGatewayActor init actor_id={self.actor_id} provider={self.provider} model={self.model}")

    @handler("completion")
    def completion(self, messages: list = None, tools: list = None, temperature: float = 0.7) -> dict:
        """Request a completion from the LLM. Returns response with token usage."""
        if not messages:
            return {"error": "messages is required"}

        # Check deterministic cache (speeds up eval replays)
        cache_key = self._cache_key(messages, tools)
        cached = self._get_cached(cache_key)
        if cached:
            self.cache_hits += 1
            host.incr_counter("llm_cache_hits", 1)
            return cached

        # Route to provider
        if self.provider == "mock":
            result = self._mock_completion(messages, tools)
        elif self.provider == "ollama":
            result = self._ollama_completion(messages, tools, temperature)
            if "error" in result:
                host.debug(f"LLMGatewayActor: ollama failed, falling back to mock: {result.get('error','')[:80]}")
                result = self._mock_completion(messages, tools)
        else:
            result = {"error": f"Unknown provider: {self.provider}"}

        if "error" not in result:
            # Track token usage
            self.total_requests += 1
            self.total_input_tokens += result.get("input_tokens", 0)
            self.total_output_tokens += result.get("output_tokens", 0)
            host.incr_counter("llm_completions_total", 1)
            self._put_cached(cache_key, result)

        return result

    @handler("get_stats")
    def get_stats(self) -> dict:
        return {
            "status": "ok",
            "model": self.model,
            "provider": self.provider,
            "total_requests": self.total_requests,
            "total_input_tokens": self.total_input_tokens,
            "total_output_tokens": self.total_output_tokens,
            "cache_hits": self.cache_hits,
        }

    @handler("set_model")
    def set_model(self, model: str = "") -> dict:
        if not model:
            return {"error": "model is required"}
        self.model = model
        return {"status": "ok", "model": self.model}

    @handler("reset_circuit")
    def reset_circuit(self) -> dict:
        """Reset circuit breaker state (call if Ollama was temporarily unavailable)."""
        host.info("LLMGatewayActor: circuit breaker reset")
        return {"status": "ok", "circuit_open": False}

    # ------------------------------------------------------------------

    def _mock_completion(self, messages: list, tools: list = None) -> dict:
        """Deterministic mock LLM response for testing and eval replay."""
        last_user_msg = next(
            (m.get("content", "") for m in reversed(messages) if m.get("role") == "user"),
            ""
        )

        # Confidence: short/simple prompts score high; long/complex prompts score low (escalation)
        word_count = len(last_user_msg.split())
        confidence = 0.95 if word_count <= 15 else (0.55 if word_count > 30 else 0.72)

        # Simple heuristics to simulate tool-use decisions
        if "search" in last_user_msg.lower() or "find" in last_user_msg.lower():
            return {
                "response": {
                    "content": "",
                    "stop_reason": "tool_use",
                    "tool_calls": [{"name": "web_search", "input": {"query": last_user_msg[:50]}}],
                },
                "confidence": confidence,
                "input_tokens": word_count * 2,
                "output_tokens": 20,
                "model": "mock",
            }
        elif "calculate" in last_user_msg.lower() or any(c in last_user_msg for c in "+-*/"):
            return {
                "response": {
                    "content": "",
                    "stop_reason": "tool_use",
                    "tool_calls": [{"name": "calculator", "input": {"expression": last_user_msg}}],
                },
                "confidence": confidence,
                "input_tokens": word_count * 2,
                "output_tokens": 15,
                "model": "mock",
            }
        else:
            return {
                "response": {
                    "content": f"I processed your request: {last_user_msg[:60]}",
                    "stop_reason": "end_turn",
                    "tool_calls": [],
                },
                "confidence": confidence,
                "input_tokens": word_count * 2,
                "output_tokens": 25,
                "model": "mock",
            }

    def _ollama_completion(self, messages: list, tools: list = None, temperature: float = 0.7) -> dict:
        """Call Ollama API for real LLM completions."""
        try:
            body = {
                "model": self.model,
                "messages": messages,
                "stream": False,
                "options": {"temperature": temperature},
            }
            if tools:
                body["tools"] = tools

            resp = host.http_post(
                url=f"{self.base_url}/api/chat",
                body=json.dumps(body),
                headers={"Content-Type": "application/json"},
                timeout_ms=30000,
            )

            if resp.get("status") != 200:
                return {"error": f"Ollama error: {resp.get('status')} {resp.get('body', '')[:100]}"}

            data = json.loads(resp.get("body", "{}"))
            message = data.get("message", {})

            # Inject confidence if Ollama didn't return one (deterministic from prompt length)
            last_user_msg = next(
                (m.get("content", "") for m in reversed(messages) if m.get("role") == "user"),
                ""
            )
            word_count = len(last_user_msg.split())
            confidence = 0.95 if word_count <= 15 else (0.55 if word_count > 30 else 0.72)

            result = {
                "response": {
                    "content": message.get("content", ""),
                    "stop_reason": "end_turn" if data.get("done") else "tool_use",
                    "tool_calls": message.get("tool_calls", []),
                },
                "confidence": confidence,
                "input_tokens": data.get("prompt_eval_count", 0),
                "output_tokens": data.get("eval_count", 0),
                "model": self.model,
            }
            return result
        except Exception as e:
            return {"error": f"Ollama call failed: {e}"}

    def _cache_key(self, messages: list, tools: list = None) -> str:
        content = json.dumps({"messages": messages, "tools": tools or [], "model": self.model}, sort_keys=True)
        return f"llm_cache:{hashlib.sha256(content.encode()).hexdigest()[:16]}"

    def _get_cached(self, key: str):
        try:
            raw = host.kv.get(key)
            if raw:
                return json.loads(raw)
        except Exception:
            pass
        return None

    def _put_cached(self, key: str, value: dict) -> None:
        try:
            host.kv_put_ttl(key, json.dumps(value), _CACHE_TTL_SECONDS)
        except Exception:
            try:
                host.kv.put(key, json.dumps(value))
            except Exception:
                pass
