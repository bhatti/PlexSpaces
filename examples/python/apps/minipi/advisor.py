# SPDX-License-Identifier: AGPL-3.0-or-later
"""AdvisorActor — two-tier LLM pattern: fast executor + expensive advisor on-demand.

Demonstrates the Advisor strategy (Anthropic 2026):
- Executor (cheap model, every turn) handles most decisions
- Advisor (expensive model) is consulted only when executor confidence is low
- Token cost split: executor tokens vs advisor tokens (trackable per eval run)
- Escalation rate feeds into BenchmarkActor for cost/quality tradeoff analysis

Config knob: confidence_threshold (0.0–1.0). Lower = more advisor calls.
BenchmarkActor can sweep this: same scenarios, 3 thresholds → cost/quality table.
"""

from plexspaces import actor, state, init_handler, handler, host

_DEFAULT_THRESHOLD = 0.8
_FAST_MODEL = "llama3.2"
_EXPENSIVE_MODEL = "llama3.3:70b"
_ESCALATION_PROMPT = (
    "You are an expert advisor. The primary agent was not confident about this decision. "
    "Review the task and provide a better answer."
)


@actor
class AdvisorActor:
    """Two-tier LLM: fast executor for most turns, expensive model on low confidence."""

    actor_id: str = state(default="")
    confidence_threshold: float = state(default=_DEFAULT_THRESHOLD)
    total_requests: int = state(default=0)
    escalation_count: int = state(default=0)
    fast_input_tokens: int = state(default=0)
    fast_output_tokens: int = state(default=0)
    advisor_input_tokens: int = state(default=0)
    advisor_output_tokens: int = state(default=0)

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        args = config.get("args", {})
        threshold = args.get("confidence_threshold", "")
        if threshold:
            try:
                t = float(threshold)
                if 0.0 <= t <= 1.0:
                    self.confidence_threshold = t
            except (ValueError, TypeError):
                pass
        try:
            host.kv_put("svc:advisor", host.self_id())
        except Exception:
            pass
        try:
            host.registry.register(None, self.actor_id or host.self_id(), "actor", "",
                                   object_category="advisor")
        except Exception:
            pass
        host.info(f"AdvisorActor init actor_id={self.actor_id} threshold={self.confidence_threshold:.2f}")

    @handler("advise")
    def advise(self, messages: list = None, prompt: str = "", context: str = "") -> dict:
        """Route to fast or expensive model based on executor confidence."""
        if not messages:
            # Accept prompt/context shorthand
            if not prompt:
                return {"error": "messages or prompt is required"}
            system_content = f"You are a helpful assistant. Context: {context}" if context else "You are a helpful assistant."
            messages = [
                {"role": "system", "content": system_content},
                {"role": "user", "content": prompt},
            ]

        self.total_requests += 1

        llm_id = self._find_service("llm_gateway")
        if not llm_id:
            return {"error": "llm_gateway unavailable"}

        # ── Step 1: Fast executor ──────────────────────────────────────────
        fast_resp = host.ask(llm_id, "completion", {
            "messages": messages,
            "model": _FAST_MODEL,
        }, timeout_ms=15000) or {}

        self.fast_input_tokens += fast_resp.get("input_tokens", 0)
        self.fast_output_tokens += fast_resp.get("output_tokens", 0)

        confidence = float(fast_resp.get("confidence", 1.0))
        response = fast_resp.get("response", {}) or {}

        if confidence >= self.confidence_threshold:
            host.incr_counter("advisor_fast_path", 1)
            return {
                "status": "ok",
                "tier": "fast",
                "confidence": confidence,
                "response": response,
                "escalation_rate": self._escalation_rate(),
            }

        # ── Step 2: Escalate to advisor model ─────────────────────────────
        self.escalation_count += 1
        host.incr_counter("advisor_escalations", 1)
        fast_content = response.get("content", "")
        advisor_messages = list(messages) + [
            {"role": "assistant", "content": f"[Tentative answer, low confidence {confidence:.2f}]: {fast_content}"},
            {"role": "user", "content": _ESCALATION_PROMPT},
        ]

        advisor_resp = host.ask(llm_id, "completion", {
            "messages": advisor_messages,
            "model": _EXPENSIVE_MODEL,
        }, timeout_ms=30000) or {}

        if not advisor_resp or advisor_resp.get("error"):
            # Fall back to fast result rather than error
            host.warn(f"AdvisorActor: advisor model failed, using fast result")
            return {
                "status": "ok",
                "tier": "fast_fallback",
                "confidence": confidence,
                "response": response,
                "escalation_rate": self._escalation_rate(),
            }

        self.advisor_input_tokens += advisor_resp.get("input_tokens", 0)
        self.advisor_output_tokens += advisor_resp.get("output_tokens", 0)
        advisor_response = advisor_resp.get("response", {}) or {}

        return {
            "status": "ok",
            "tier": "advisor",
            "confidence": confidence,
            "response": advisor_response,
            "fast_response": response,
            "escalation_rate": self._escalation_rate(),
            "total_input_tokens": self.fast_input_tokens + self.advisor_input_tokens,
            "total_output_tokens": self.fast_output_tokens + self.advisor_output_tokens,
            "fast_input_tokens": self.fast_input_tokens,
            "advisor_input_tokens": self.advisor_input_tokens,
        }

    @handler("get_stats")
    def get_stats(self) -> dict:
        total_in = self.fast_input_tokens + self.advisor_input_tokens
        total_out = self.fast_output_tokens + self.advisor_output_tokens
        advisor_share = round(self.advisor_input_tokens / total_in * 100, 1) if total_in > 0 else 0.0
        return {
            "status": "ok",
            "actor_id": self.actor_id,
            "confidence_threshold": self.confidence_threshold,
            "total_requests": self.total_requests,
            "escalation_count": self.escalation_count,
            "escalation_rate_pct": self._escalation_rate(),
            "fast_input_tokens": self.fast_input_tokens,
            "fast_output_tokens": self.fast_output_tokens,
            "advisor_input_tokens": self.advisor_input_tokens,
            "advisor_output_tokens": self.advisor_output_tokens,
            "total_input_tokens": total_in,
            "total_output_tokens": total_out,
            "advisor_token_share_pct": advisor_share,
        }

    @handler("reset_stats")
    def reset_stats(self) -> dict:
        self.total_requests = 0
        self.escalation_count = 0
        self.fast_input_tokens = 0
        self.fast_output_tokens = 0
        self.advisor_input_tokens = 0
        self.advisor_output_tokens = 0
        return {"status": "ok"}

    def _find_service(self, service_type: str) -> str:
        """Discover service actor ID via object registry; falls back to peer ID on same node."""
        try:
            regs = host.registry.discover(None, object_category=service_type, limit=1)
            if regs:
                return regs[0]["object_id"]
        except Exception:
            pass
        idx = self.actor_id.find("//")
        if idx >= 0:
            return service_type + self.actor_id[idx:]
        return service_type

    def _escalation_rate(self) -> float:
        if self.total_requests == 0:
            return 0.0
        return round(self.escalation_count / self.total_requests * 100, 1)
