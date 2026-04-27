# SPDX-License-Identifier: AGPL-3.0-or-later
"""AI Monitor/Link Supervision — FLP/Byzantine fault detection in AI pipelines.

Demonstrates PlexSpaces monitor and link primitives using a realistic AI pipeline:
- InferenceWorker: LLM inference backend; normal or Byzantine mode
- ValidatorAgent:  Output validator with Byzantine detection (≥1/3 FLP threshold)
- PipelineSupervisor: Fault-aware dispatcher using host.Monitor/__DOWN__ events
- AuditLogActor: GenEvent fire-and-forget audit trail

Key patterns shown:
  host.monitor(actor_id)   → one-way DOWN notification (supervisor stays alive)
  host.demonitor(ref)      → cancel watch when actor replaced
  host.link(actor_id)      → bidirectional EXIT fate-sharing (abnormal exits only)
  host.unlink(actor_id)    → decouple before graceful shutdown (no cascade)
"""

from typing import Any, Dict, List, Optional

from plexspaces import (
    ActorID,
    actor,
    event_actor,
    gen_server_actor,
    handler,
    host,
    init_handler,
    state,
)

def _sibling_id(bare_id: str, self_actor_id: str) -> str:
    """Build a canonical actor ID for a supervised sibling.

    Supervised actors have stable IDs of the form:
        {child_id}//{child_id}::{namespace}@{node}
    (name == actor_type == child_id).

    If bare_id already contains '//' it is canonical and returned unchanged.
    Otherwise uses ActorID.with_type_and_name from the SDK to construct the
    sibling ID sharing the same namespace and node as self.
    """
    if "//" in bare_id:
        return bare_id
    try:
        return ActorID.parse(self_actor_id).with_type_and_name(bare_id, bare_id).to_str()
    except ValueError:
        return bare_id

# ─────────────────────────────────────────────────────────────────────────────
# InferenceWorker
# Role: "inference_worker"
# LLM inference backend. Supports normal and Byzantine modes. Uses host.link()
# to establish fate-sharing with a peer worker. Handles __EXIT__ on peer crash.
# ─────────────────────────────────────────────────────────────────────────────

DOWN_MSG = "__DOWN__"
EXIT_MSG = "__EXIT__"


@gen_server_actor
class InferenceWorker:
    """LLM inference worker with Byzantine fault injection support."""

    actor_id: str = state(default="")
    worker_id: str = state(default="")
    mode: str = state(default="normal")  # "normal" | "byzantine"
    total_requests: int = state(default=0)
    error_count: int = state(default=0)
    linked_peers: list = state(default_factory=list)

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        args = config.get("args") or {}
        self.worker_id = args.get("worker_id") or config.get("worker_id", "default-worker")
        self.mode = config.get("mode", "normal")
        self.linked_peers = []

    @handler(EXIT_MSG, "cast")
    def on_exit(
        self,
        exit_from: str = "",
        exit_reason: str = "",
    ) -> None:
        """Linked peer died abnormally — log and clean up local state."""
        host.log("info",f"[{self.worker_id}] __EXIT__ exit_from={exit_from} exit_reason={exit_reason}")
        if exit_from in self.linked_peers:
            self.linked_peers.remove(exit_from)

    @handler("infer")
    def on_infer(
        self,
        prompt: str = "",
        request_id: str = "",
    ) -> dict:
        """Run inference. Byzantine mode returns inconsistent outputs."""
        self.total_requests += 1

        if self.mode == "byzantine":
            # Byzantine: syntactically valid but semantically wrong
            byzantine_responses = [
                "42 is the answer to everything",
                "The sky is green on Tuesdays",
                "null",
                "ERROR: model checkpoint corrupted",
            ]
            idx = self.total_requests % len(byzantine_responses)
            result = byzantine_responses[idx]
            self.error_count += 1
            return {
                "status": "ok",
                "request_id": request_id,
                "result": result,
                "worker_id": self.worker_id,
                "mode": "byzantine",
            }

        # Normal: deterministic response based on prompt keywords
        if "actor" in prompt.lower():
            result = "The actor model is a mathematical model of concurrent computation where each actor processes messages asynchronously."
        elif "fault" in prompt.lower() or "tolerance" in prompt.lower():
            result = "Fault tolerance is achieved through redundancy, isolation, and supervision trees that restart failed components."
        elif "flp" in prompt.lower() or "impossibility" in prompt.lower():
            result = "The FLP theorem proves no deterministic async protocol guarantees consensus with even one crash-faulty process."
        elif "byzantine" in prompt.lower():
            result = "Byzantine faults are arbitrary failures where a node may send inconsistent messages. Requires 3f+1 replicas to tolerate f faults."
        else:
            result = f"Processed: {prompt[:60]}"

        return {
            "status": "ok",
            "request_id": request_id,
            "result": result,
            "worker_id": self.worker_id,
            "mode": "normal",
        }

    @handler("set_mode")
    def on_set_mode(self, mode: str = "normal") -> dict:
        """Switch between normal and byzantine modes."""
        old_mode = self.mode
        self.mode = mode
        host.log("info",f"[{self.worker_id}] mode changed {old_mode} → {mode}")
        return {"status": "ok", "mode": self.mode}

    @handler("link_with")
    def on_link_with(self, peer_id: str = "") -> dict:
        """Establish bidirectional link with a peer worker.

        Both actors will receive __EXIT__ if either dies abnormally.
        Normal exits and Shutdown do NOT propagate.
        """
        canonical = _sibling_id(peer_id, self.actor_id)
        host.link(canonical)
        if canonical not in self.linked_peers:
            self.linked_peers.append(canonical)
        host.log("info",f"[{self.worker_id}] linked with {canonical}")
        return {"status": "ok", "peer_id": canonical}

    @handler("unlink_from")
    def on_unlink_from(self, peer_id: str = "") -> dict:
        """Remove link before graceful shutdown to prevent cascade failures."""
        canonical = _sibling_id(peer_id, self.actor_id)
        host.unlink(canonical)
        if canonical in self.linked_peers:
            self.linked_peers.remove(canonical)
        host.log("info",f"[{self.worker_id}] unlinked from {canonical}")
        return {"status": "ok", "peer_id": canonical}

    @handler("status")
    def on_status(self) -> dict:
        return {
            "status": "ok",
            "worker_id": self.worker_id,
            "mode": self.mode,
            "total_requests": self.total_requests,
            "error_count": self.error_count,
            "linked_peers": self.linked_peers,
        }


# ─────────────────────────────────────────────────────────────────────────────
# ValidatorAgent
# Role: "validator_agent"
# Validates inference outputs. Monitors workers via host.monitor(). Detects
# Byzantine behavior using FLP-inspired threshold (≥1/3 failures = flag).
# Receives __DOWN__ when a monitored worker stops for any reason.
# ─────────────────────────────────────────────────────────────────────────────


@gen_server_actor
class ValidatorAgent:
    """Output validator with Byzantine detection and monitor-based fault awareness."""

    actor_id: str = state(default="")
    total_validations: int = state(default=0)
    pass_count: int = state(default=0)
    fail_count: int = state(default=0)
    byzantine_count: int = state(default=0)
    monitor_refs: dict = state(default_factory=dict)  # worker_id → monitor_ref
    down_events: list = state(default_factory=list)

    FLP_THRESHOLD = 1.0 / 3.0

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        self.monitor_refs = {}
        self.down_events = []

    @handler(DOWN_MSG, "cast")
    def on_down(
        self,
        monitor_ref: str = "",
        down_from: str = "",
        down_reason: str = "",
    ) -> None:
        """Monitored worker stopped — one-way notification. Validator stays alive."""
        host.log("info",
            f"[validator_agent] __DOWN__ ref={monitor_ref} down_from={down_from} down_reason={down_reason}"
        )
        self.down_events.append(
            {"monitor_ref": monitor_ref, "down_from": down_from, "down_reason": down_reason}
        )
        # Remove stale monitor ref
        for wid, ref in list(self.monitor_refs.items()):
            if ref == monitor_ref:
                del self.monitor_refs[wid]
                break

    @handler("monitor_worker")
    def on_monitor_worker(self, worker_id: str = "") -> dict:
        """Attach a monitor to a worker actor via host.monitor().

        Returns the monitor_ref for later cancellation via host.demonitor().
        """
        canonical = _sibling_id(worker_id, self.actor_id)
        monitor_ref = host.monitor(canonical)
        self.monitor_refs[canonical] = monitor_ref
        host.log("info",f"[validator_agent] monitoring {canonical} ref={monitor_ref}")
        return {"status": "ok", "monitor_ref": monitor_ref, "worker_id": canonical}

    @handler("demonitor_worker")
    def on_demonitor_worker(self, worker_id: str = "") -> dict:
        """Cancel monitor — stop receiving __DOWN__ for this worker."""
        canonical = _sibling_id(worker_id, self.actor_id)
        ref = self.monitor_refs.pop(canonical, None)
        if ref:
            host.demonitor(ref)
            host.log("info",f"[validator_agent] demonitored {canonical} ref={ref}")
            return {"status": "ok", "worker_id": canonical}
        return {"status": "not_found", "worker_id": canonical}

    @handler("validate")
    def on_validate(
        self,
        worker_id: str = "",
        result: str = "",
        expected_format: str = "text",
    ) -> dict:
        """Validate an inference result. Apply FLP-inspired Byzantine threshold.

        If ≥1/3 of all validations are flagged Byzantine, raise an alert.
        This approximates the classical 3f+1 Byzantine fault tolerance bound.
        """
        self.total_validations += 1

        # Heuristic Byzantine detection: implausible or known-bad patterns
        byzantine_patterns = [
            "42 is the answer",
            "sky is green",
            "null",
            "checkpoint corrupted",
        ]
        is_byzantine = any(
            p.lower() in result.lower() for p in byzantine_patterns
        )
        # Also flag results that are too short to be meaningful
        if len(result.strip()) < 10:
            is_byzantine = True

        if is_byzantine:
            self.byzantine_count += 1
            self.fail_count += 1
            valid = False
        else:
            self.pass_count += 1
            valid = True

        # FLP threshold check: ≥1/3 byzantine → alert
        flp_ratio = (
            self.byzantine_count / self.total_validations
            if self.total_validations > 0
            else 0.0
        )
        flp_exceeded = flp_ratio >= self.FLP_THRESHOLD

        if flp_exceeded:
            host.log("info",
                f"[validator_agent] FLP threshold exceeded: {self.byzantine_count}/{self.total_validations} byzantine ({flp_ratio:.1%})"
            )

        return {
            "status": "ok",
            "valid": valid,
            "worker_id": worker_id,
            "byzantine_suspected": is_byzantine,
            "flp_threshold_exceeded": flp_exceeded,
            "flp_ratio": round(flp_ratio, 3),
        }

    @handler("status")
    def on_status(self) -> dict:
        flp_ratio = (
            self.byzantine_count / self.total_validations
            if self.total_validations > 0
            else 0.0
        )
        return {
            "status": "ok",
            "total_validations": self.total_validations,
            "pass_count": self.pass_count,
            "fail_count": self.fail_count,
            "byzantine_count": self.byzantine_count,
            "flp_threshold": self.FLP_THRESHOLD,
            "flp_ratio": round(flp_ratio, 3),
            "monitor_count": len(self.monitor_refs),
            "down_events_received": len(self.down_events),
        }


# ─────────────────────────────────────────────────────────────────────────────
# PipelineSupervisor
# Role: "pipeline_supervisor"
# Routes inference requests across workers. Monitors workers and handles
# __DOWN__ events to maintain availability. Demonitors replaced workers.
# ─────────────────────────────────────────────────────────────────────────────


@gen_server_actor
class PipelineSupervisor:
    """Fault-aware pipeline supervisor using monitor/demonitor for worker tracking."""

    actor_id: str = state(default="")
    worker_pool: list = state(default_factory=list)
    monitor_refs: dict = state(default_factory=dict)  # worker_id → monitor_ref
    down_events_received: int = state(default=0)
    total_dispatched: int = state(default=0)
    next_worker_idx: int = state(default=0)

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        self.worker_pool = []
        self.monitor_refs = {}

    @handler(DOWN_MSG, "cast")
    def on_down(
        self,
        monitor_ref: str = "",
        down_from: str = "",
        down_reason: str = "",
    ) -> None:
        """Worker stopped — remove from pool and clean up monitor ref."""
        host.log("info",
            f"[pipeline_supervisor] __DOWN__ ref={monitor_ref} down_from={down_from} down_reason={down_reason}"
        )
        self.down_events_received += 1

        # Remove dead worker from pool
        if down_from in self.worker_pool:
            self.worker_pool.remove(down_from)

        # Clean up monitor ref
        for wid, ref in list(self.monitor_refs.items()):
            if ref == monitor_ref:
                del self.monitor_refs[wid]
                break

    @handler("monitor_worker")
    def on_monitor_worker(self, worker_id: str = "") -> dict:
        """Add worker to pool and attach monitor."""
        canonical = _sibling_id(worker_id, self.actor_id)
        monitor_ref = host.monitor(canonical)
        self.monitor_refs[canonical] = monitor_ref
        if canonical not in self.worker_pool:
            self.worker_pool.append(canonical)
        host.log("info",f"[pipeline_supervisor] monitoring {canonical} ref={monitor_ref}")
        return {"status": "ok", "monitor_ref": monitor_ref, "worker_id": canonical}

    @handler("demonitor_worker")
    def on_demonitor_worker(self, worker_id: str = "") -> dict:
        """Remove worker from pool and cancel monitor."""
        canonical = _sibling_id(worker_id, self.actor_id)
        ref = self.monitor_refs.pop(canonical, None)
        if ref:
            host.demonitor(ref)
        if canonical in self.worker_pool:
            self.worker_pool.remove(canonical)
        return {"status": "ok", "worker_id": canonical}

    @handler("dispatch")
    def on_dispatch(
        self,
        prompt: str = "",
        request_id: str = "",
    ) -> dict:
        """Route inference request to next available worker (round-robin)."""
        if not self.worker_pool:
            return {"status": "error", "reason": "no_workers_available"}

        # Round-robin selection — worker_pool stores canonical IDs
        idx = self.next_worker_idx % len(self.worker_pool)
        self.next_worker_idx += 1
        worker_id = self.worker_pool[idx]

        self.total_dispatched += 1
        result = host.ask(worker_id, {"op": "infer", "prompt": prompt, "request_id": request_id})

        byzantine_detected = result.get("mode") == "byzantine" if isinstance(result, dict) else False
        return {
            "status": "ok",
            "worker_used": worker_id,
            "request_id": request_id,
            "result": result,
            "byzantine_detected": byzantine_detected,
        }

    @handler("status")
    def on_status(self) -> dict:
        return {
            "status": "ok",
            "worker_pool": self.worker_pool,
            "monitor_count": len(self.monitor_refs),
            "down_events_received": self.down_events_received,
            "total_dispatched": self.total_dispatched,
        }


# ─────────────────────────────────────────────────────────────────────────────
# AuditLogActor
# Role: "audit_log"
# GenEvent fire-and-forget event log for the pipeline.
# ─────────────────────────────────────────────────────────────────────────────


@event_actor
class AuditLog:
    """GenEvent actor: fire-and-forget audit trail for pipeline events."""

    events_received: int = state(default=0)
    last_event: dict = state(default_factory=dict)

    @init_handler
    def on_init(self, config: dict) -> None:
        self.events_received = 0
        self.last_event = {}

    @handler("log_event", "cast")
    def on_log_event(
        self,
        event_type: str = "",
        actor_id: str = "",
        details: str = "",
    ) -> None:
        self.events_received += 1
        self.last_event = {
            "event_type": event_type,
            "actor_id": actor_id,
            "details": details,
        }
        host.log("info",f"[audit_log] #{self.events_received} {event_type} actor={actor_id} {details}")

    @handler("get_stats")
    def on_get_stats(self) -> dict:
        return {
            "status": "ok",
            "events_received": self.events_received,
            "last_event": self.last_event,
        }
