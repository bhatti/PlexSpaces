# SPDX-License-Identifier: AGPL-3.0-or-later
"""LLMGatewayActor — unified LLM provider gateway.

Supports Ollama (default), OpenAI, and Anthropic via HTTPFetch service links.
Falls back to keyword-based simulated responses when no provider is reachable,
so test.sh passes without a running LLM.
"""

import json
from plexspaces import actor, state, handler, init_handler, host
from helpers import fire_audit

_MATH_KEYWORDS = ("calculat", "compute", "multiply", "divide", "add ", "subtract", "what is")
_MEMORY_KEYWORDS = ("remember", "recall", "store", "save", "what did")
_SKILL_KEYWORDS = ("how do", "steps to", "procedure", "workflow", "automat")
_HTTP_KEYWORDS = ("fetch", "request", "http", "api", "download", "get data")
_CRON_KEYWORDS = ("every hour", "every day", "every minute", "schedule", "automat", "periodic")

_MAX_FAILURES = 3


@actor
class LLMGatewayActor:
    """Routes LLM completions to Ollama / OpenAI / Anthropic with simulated fallback."""

    active_provider: str = state(default="ollama")
    default_model: str = state(default="llama3.2")
    request_count: int = state(default=0)
    total_tokens: int = state(default=0)
    cache_hits: int = state(default=0)
    simulated_count: int = state(default=0)
    consecutive_failures: int = state(default=0)
    circuit_open: bool = state(default=False)
    actor_id: str = state(default="")

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        args = config.get("args", {})
        self.active_provider = args.get("default_provider", self.active_provider)
        self.default_model = args.get("default_model", self.default_model)
        host.process_groups.join("svc:llm_gateway")
        host.send_after(30000, "health_tick", {"op": "health_tick"})
        host.info(f"LLMGatewayActor init actor_id={self.actor_id} provider={self.active_provider}")

    @handler("completion")
    def completion(self, messages: list = None, tools: list = None, model: str = "") -> dict:
        messages = messages or []
        tools = tools or []
        model = model or self.default_model

        # Check cache
        cache_key = self._cache_key(messages)
        cached = host.kv_get(f"llm_cache:{cache_key}")
        if cached:
            self.cache_hits += 1
            fire_audit("llm_cache_hit", f"provider={self.active_provider}")
            return {"status": "ok", "response": json.loads(cached), "cached": True}

        self.request_count += 1

        # Try real provider first, fall back to simulation
        response = None
        if not self.circuit_open:
            response = self._call_provider(messages, tools, model)

        if response is None:
            response = self._simulated_completion(messages, tools)
            self.simulated_count += 1
        else:
            self.consecutive_failures = 0
            # Cache successful real responses
            try:
                host.kv_put(f"llm_cache:{cache_key}", json.dumps(response))
            except Exception:
                pass

        self.total_tokens += response.get("usage", {}).get("total_tokens", 10)
        host.incr_counter("llm_requests", 1)
        fire_audit("llm_completion", f"provider={self.active_provider} simulated={response.get('simulated', False)}")
        return {"status": "ok", "response": response, "cached": False}

    @handler("register_provider")
    def register_provider(self, name: str = "", base_url: str = "", model: str = "", api_key: str = "") -> dict:
        if not name:
            return {"error": "name is required"}
        meta = {"name": name, "base_url": base_url, "model": model or self.default_model}
        host.kv_put(f"provider_config:{name}", json.dumps(meta))
        fire_audit("provider_registered", f"name={name}")
        return {"status": "ok", "provider": name}

    @handler("switch_provider")
    def switch_provider(self, provider: str = "", model: str = "") -> dict:
        if not provider:
            return {"error": "provider is required"}
        self.active_provider = provider
        if model:
            self.default_model = model
        self.circuit_open = False
        self.consecutive_failures = 0
        fire_audit("provider_switched", f"provider={provider} model={self.default_model}")
        return {"status": "ok", "provider": provider, "model": self.default_model}

    @handler("reset_circuit")
    def reset_circuit(self) -> dict:
        self.circuit_open = False
        self.consecutive_failures = 0
        fire_audit("circuit_reset", f"provider={self.active_provider}")
        return {"status": "ok", "circuit_open": False}

    @handler("get_stats")
    def get_stats(self) -> dict:
        return {
            "status": "ok",
            "active_provider": self.active_provider,
            "default_model": self.default_model,
            "request_count": self.request_count,
            "total_tokens": self.total_tokens,
            "cache_hits": self.cache_hits,
            "simulated_count": self.simulated_count,
            "circuit_open": self.circuit_open,
        }

    @handler("health_tick", "cast")
    def health_tick(self) -> None:
        if self.circuit_open and self.consecutive_failures < _MAX_FAILURES:
            self.circuit_open = False
        host.send_after(30000, "health_tick", {"op": "health_tick"})

    # ------------------------------------------------------------------

    def _call_provider(self, messages: list, tools: list, model: str):
        """Attempt HTTPFetch to the active provider. Returns normalized dict or None."""
        try:
            cfg_raw = host.kv_get(f"provider_config:{self.active_provider}")
            cfg = json.loads(cfg_raw) if cfg_raw else {}
            base_url = cfg.get("base_url", "")

            if self.active_provider == "ollama":
                body = json.dumps({"model": model, "messages": messages, "stream": False})
                resp = host.http_fetch(self.active_provider, "POST", "/api/chat", body)
                return self._normalize_ollama(resp)
            elif self.active_provider in ("openai", "anthropic"):
                body = json.dumps({"model": model, "messages": messages, "tools": tools, "max_tokens": 1024})
                path = "/v1/chat/completions" if self.active_provider == "openai" else "/v1/messages"
                resp = host.http_fetch(self.active_provider, "POST", path, body)
                return self._normalize_openai(resp)
            return None
        except Exception as e:
            self.consecutive_failures += 1
            if self.consecutive_failures >= _MAX_FAILURES:
                self.circuit_open = True
            host.debug(f"LLMGateway: provider call failed: {e}")
            return None

    def _normalize_ollama(self, raw) -> dict:
        if not raw:
            return None
        try:
            # raw is the HTTP response wrapper: {"status": N, "body": "...json..."}
            http = raw if isinstance(raw, dict) else json.loads(raw)
            if http.get("status", 0) != 200:
                return None
            body_str = http.get("body", "")
            if not body_str:
                return None
            data = json.loads(body_str)
            msg = data.get("message", {})
            content = msg.get("content", "")
            tool_calls = []
            if msg.get("tool_calls"):
                for tc in msg["tool_calls"]:
                    fn = tc.get("function", {})
                    tool_calls.append({
                        "id": f"tc_{self.request_count}",
                        "name": fn.get("name", ""),
                        "input": fn.get("arguments", {}),
                    })
            stop = "tool_use" if tool_calls else "end_turn"
            return {
                "content": content,
                "stop_reason": stop,
                "tool_calls": tool_calls,
                "usage": {"total_tokens": data.get("eval_count", 10)},
            }
        except Exception:
            return None

    def _normalize_openai(self, raw) -> dict:
        if not raw:
            return None
        try:
            http = raw if isinstance(raw, dict) else json.loads(raw)
            if http.get("status", 0) != 200:
                return None
            body_str = http.get("body", "")
            if not body_str:
                return None
            data = json.loads(body_str)
            choice = (data.get("choices") or [{}])[0]
            msg = choice.get("message", {})
            content = msg.get("content", "") or ""
            tool_calls = []
            for tc in msg.get("tool_calls") or []:
                fn = tc.get("function", {})
                input_data = fn.get("arguments", {})
                if isinstance(input_data, str):
                    try:
                        input_data = json.loads(input_data)
                    except Exception:
                        input_data = {}
                tool_calls.append({"id": tc.get("id", ""), "name": fn.get("name", ""), "input": input_data})
            stop = "tool_use" if tool_calls else "end_turn"
            usage = data.get("usage", {})
            return {
                "content": content,
                "stop_reason": stop,
                "tool_calls": tool_calls,
                "usage": {"total_tokens": usage.get("total_tokens", 10)},
            }
        except Exception:
            return None

    def _simulated_completion(self, messages: list, tools: list) -> dict:
        """Keyword-based simulated response for testing without a real LLM."""
        user_msg = ""
        for m in reversed(messages):
            if m.get("role") == "user":
                user_msg = str(m.get("content", "")).lower()
                break

        def has_tool(name: str) -> bool:
            return any(t.get("name") == name for t in tools if isinstance(t, dict))

        if any(kw in user_msg for kw in _MATH_KEYWORDS) and has_tool("calculator"):
            return {
                "content": "",
                "stop_reason": "tool_use",
                "tool_calls": [{"id": f"tc_{self.request_count}", "name": "calculator", "input": {"expression": "42 * 17"}}],
                "usage": {"total_tokens": 10},
                "simulated": True,
            }
        if any(kw in user_msg for kw in _MEMORY_KEYWORDS) and has_tool("memory_store"):
            return {
                "content": "",
                "stop_reason": "tool_use",
                "tool_calls": [{"id": f"tc_{self.request_count}", "name": "memory_store", "input": {"key": "fact", "value": user_msg[:50], "tier": "core"}}],
                "usage": {"total_tokens": 10},
                "simulated": True,
            }
        if any(kw in user_msg for kw in _CRON_KEYWORDS) and has_tool("create_cron_job"):
            return {
                "content": "",
                "stop_reason": "tool_use",
                "tool_calls": [{"id": f"tc_{self.request_count}", "name": "create_cron_job", "input": {"prompt": user_msg[:80], "schedule": "every_1h"}}],
                "usage": {"total_tokens": 10},
                "simulated": True,
            }
        if any(kw in user_msg for kw in _HTTP_KEYWORDS) and has_tool("http_request"):
            return {
                "content": "",
                "stop_reason": "tool_use",
                "tool_calls": [{"id": f"tc_{self.request_count}", "name": "http_request", "input": {"method": "GET", "url": "https://api.example.com"}}],
                "usage": {"total_tokens": 10},
                "simulated": True,
            }

        # Generic helpful response
        response_text = (
            f"I've processed your request: \"{user_msg[:60]}...\". "
            "MiniHermes is running with simulated LLM mode. "
            "Connect Ollama (ollama run llama3.2) for real AI responses."
        ) if len(user_msg) > 60 else (
            f"I understand your request about \"{user_msg}\". How can I help further?"
        )
        return {
            "content": response_text,
            "stop_reason": "end_turn",
            "tool_calls": [],
            "usage": {"total_tokens": 15},
            "simulated": True,
        }

    def _cache_key(self, messages: list) -> str:
        last = ""
        for m in reversed(messages):
            if m.get("role") == "user":
                last = str(m.get("content", ""))[:80]
                break
        h = 5381
        for c in last:
            h = ((h << 5) + h + ord(c)) & 0xFFFFFFFF
        return f"{h:08x}"
