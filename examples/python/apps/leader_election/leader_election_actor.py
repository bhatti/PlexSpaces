# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 PlexSpaces Contributors
"""
Leader Election Actor - Distributed Lock Example (Python WASM with SDK)

Demonstrates leader election using a single distributed lock:
- Multiple actors contend for the "leader" lock
- One holds it (e.g. runs scheduled task, coordinates work)
- Others try_acquire; on release or TTL expiry another becomes leader

Real-world use cases:
- Cron singleton (one node runs cron jobs)
- Task scheduler (single active coordinator)
- Single active consumer (one consumer drains a queue)
- Cluster coordinator / master election

## APIs Used

- @actor: GenServer request-reply
- host.lock_acquire(tenant_id, namespace, holder_id, lock_name, lease_duration_secs, timeout_ms): Acquire lock; returns JSON with version or "ERROR:..."
- host.lock_release(lock_id, tenant_id, namespace, holder_id, lock_version): Release lock
- host.lock_renew(lock_id, tenant_id, namespace, holder_id, lock_version, lease_duration_secs): Renew lease (heartbeat)
- state(): Track whether this instance is leader and lock version

Leader election uses tenant_id="", namespace="leader-election", holder_id=candidate_id.
"""

import json
from plexspaces import actor, state, handler, init_handler, host

LOCK_ID_LEADER = "leader"
# Leader lock scope: all candidates contend in same namespace
TENANT_ID_LEADER = ""
NAMESPACE_LEADER = "leader-election"


@actor
class LeaderElection:
    """Actor that participates in leader election via a distributed lock."""

    # Candidate identity (from config or payload)
    candidate_id: str = state(default="")
    # Holder id used for the lock (set when we acquire; required for renew/release)
    current_holder_id: str = state(default="")
    # Lock version when we hold leader lock (empty if not leader)
    lock_version: str = state(default="")

    @init_handler
    def on_init(self, config: dict) -> None:
        """Initialize with candidate_id (e.g. node id or actor id)."""
        self.candidate_id = config.get("candidate_id", "unknown")
        self.current_holder_id = ""
        self.lock_version = ""
        host.log("info", f"LeaderElection initialized candidate_id={self.candidate_id}")

    def _display_id(self, candidate_id: str = None) -> str:
        """Use payload candidate_id for display when provided (e.g. owner-id for two-terminal test)."""
        return candidate_id if candidate_id else self.candidate_id

    @handler("try_lead")
    def try_lead(self, candidate_id: str = None) -> dict:
        """
        Try to become leader (non-blocking try-acquire).
        Returns whether this instance is now the leader.
        Optional candidate_id in payload is used for display (e.g. owner-id when running from two terminals).
        """
        display_id = self._display_id(candidate_id)
        if self.lock_version:
            return {"leader": True, "candidate_id": display_id, "lock_version": self.lock_version, "message": "already leader"}
        # Use payload candidate_id as holder so same WASM can run as two logical actors (actor1 / actor2)
        holder_id = self._display_id(candidate_id)
        out = host.lock_acquire(TENANT_ID_LEADER, NAMESPACE_LEADER, holder_id, LOCK_ID_LEADER, 30, 0)  # 0 = try-acquire
        if out and not out.startswith("ERROR"):
            try:
                data = json.loads(out)
                self.lock_version = data.get("version", out)
                self.current_holder_id = holder_id
            except json.JSONDecodeError:
                self.lock_version = out
                self.current_holder_id = holder_id
            host.log("info", f"Elected as leader: {holder_id}")
            return {"leader": True, "candidate_id": display_id, "lock_version": self.lock_version}
        return {"leader": False, "candidate_id": display_id, "message": "could not acquire leader lock"}

    @handler("acquire_lead")
    def acquire_lead(self, timeout_ms: int = 5000, candidate_id: str = None) -> dict:
        """
        Attempt to become leader, waiting up to timeout_ms.
        Use for startup: wait briefly to become leader if no one else has it.
        Optional candidate_id in payload is used for display.
        """
        display_id = self._display_id(candidate_id)
        if self.lock_version:
            return {"leader": True, "candidate_id": display_id}
        holder_id = self._display_id(candidate_id)
        out = host.lock_acquire(TENANT_ID_LEADER, NAMESPACE_LEADER, holder_id, LOCK_ID_LEADER, 30, timeout_ms)
        if out and not out.startswith("ERROR"):
            try:
                data = json.loads(out)
                self.lock_version = data.get("version", out)
                self.current_holder_id = holder_id
            except json.JSONDecodeError:
                self.lock_version = out
                self.current_holder_id = holder_id
            host.log("info", f"Elected as leader: {holder_id}")
            return {"leader": True, "candidate_id": display_id}
        return {"leader": False, "candidate_id": display_id, "error": out or "timeout"}

    @handler("renew_lead")
    def renew_lead(self, lease_duration_secs: int = 30, candidate_id: str = None) -> dict:
        """
        Renew lease on the leader lock (heartbeat). Leader should call this periodically to keep holding the lock.
        If the backend returns a new version on renew, we update self.lock_version for subsequent renew/release.
        """
        display_id = self._display_id(candidate_id)
        if not self.lock_version:
            return {"leader": False, "candidate_id": display_id, "lock_version": "", "message": "was not leader", "renewed": False}
        out = host.lock_renew(
            LOCK_ID_LEADER, TENANT_ID_LEADER, NAMESPACE_LEADER, self.current_holder_id,
            self.lock_version, lease_duration_secs
        )
        if out and out.startswith("ERROR"):
            return {"leader": True, "candidate_id": display_id, "lock_version": self.lock_version, "renewed": False, "error": out}
        self.lock_version = out
        host.log("info", f"Renewed leader lease: {self.current_holder_id} (lease_secs={lease_duration_secs})")
        return {"leader": True, "candidate_id": display_id, "lock_version": self.lock_version, "renewed": True}

    @handler("release_lead")
    def release_lead(self, candidate_id: str = None) -> dict:
        """Release leadership so another candidate can become leader."""
        display_id = self._display_id(candidate_id)
        if not self.lock_version:
            return {"leader": False, "candidate_id": display_id, "message": "was not leader"}
        out = host.lock_release(
            LOCK_ID_LEADER, TENANT_ID_LEADER, NAMESPACE_LEADER, self.current_holder_id, self.lock_version
        )
        self.lock_version = ""
        self.current_holder_id = ""
        if out and out.startswith("ERROR"):
            return {"leader": False, "candidate_id": display_id, "error": out}
        host.log("info", f"Released leadership: {self.current_holder_id}")
        return {"leader": False, "candidate_id": display_id, "message": "released"}

    @handler("status")
    def status(self, candidate_id: str = None) -> dict:
        """Report whether this instance is currently the leader. Optional candidate_id for display."""
        return {
            "leader": bool(self.lock_version),
            "candidate_id": self._display_id(candidate_id),
            "lock_version": self.lock_version or "",
        }
