# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
"""
Redis Cluster with PlexSpaces Actors (Python WASM)

Demonstrates how PlexSpaces' distributed actor primitives map to the "Rust Projects -
Write a Redis Clone" book (Ch5-9), expressed as Python WASM actors.

Book Concept                     PlexSpaces Primitive          Benefit
───────────────────────────────  ──────────────────────────   ─────────────────────
Server actor + MPSC (Ch5)        Shard Group (StorageActor)   Auto-partitioned shards
ConnectionHandler per client     Virtual Actor (ConnectionActor) Auto-lifecycle
Command modules (Ch6)            @handler annotations          Declarative dispatch
Replication broadcast (Ch7-8)    broadcast_shard_group         One call, all replicas
WAIT for N ACKs (Ch8)            scatter_gather + offset check Built-in threshold
Transactions MULTI/EXEC (Ch9)    Per-VirtualActor queue        No locks needed
Cross-shard queries (KEYS,SIZE)  scatter_gather + reduce(SUM)  Parallel fan-out
Active key expiry                broadcast expire_sweep        Parallel across shards
Coordinated snapshot             map(snapshot)                 Simultaneous all-shard

Actors in this module:
  StorageActor     — shard member, owns a hash-partitioned slice of the keyspace
  ConnectionActor  — virtual actor per client, manages MULTI/EXEC/DISCARD state
  RedisCoordinator — cluster coordinator, creates shard groups, routes cluster ops
"""

import time
from typing import Any, Dict, List, Optional

from plexspaces import actor, handler, host, init_handler, state


# =============================================================================
# Utility
# =============================================================================

def now_ms() -> int:
    return int(time.time() * 1000)


def is_expired(entry: dict) -> bool:
    exp = entry.get("expires_at_ms")
    if exp is None:
        return False
    return now_ms() > exp


def _stable_hash(key: str) -> int:
    h = 5381
    for c in key:
        h = ((h << 5) + h + ord(c)) & 0xFFFFFFFF
    return h


def _safe_metrics_add(application_id: str, metrics: dict) -> None:
    try:
        host.application_metrics_add(application_id, metrics)
    except Exception:
        pass


# =============================================================================
# StorageActor — Shard Group Member
# =============================================================================

@actor
class StorageActor:
    """
    One shard in the distributed key-value store.

    Instances are created by create_shard_group.  Handlers mirror the Rust
    StorageActor so that the two examples are directly comparable.
    """

    shard_id: int = state(default=0)
    num_shards: int = state(default=1)
    data: dict = state(default_factory=dict)           # key -> {value, expires_at_ms}
    role: str = state(default="master")
    replication_offset: int = state(default=0)

    @init_handler
    def on_init(self, config: dict):
        args = config.get("args", {})
        self.shard_id = int(args.get("shard_id", 0))
        self.num_shards = int(args.get("num_shards", 1))
        self.role = args.get("role", "master")
        self.data = {}
        self.replication_offset = 0

    def _owns_key(self, key: str) -> bool:
        return (_stable_hash(key) % self.num_shards) == self.shard_id

    # -------------------------------------------------------------------------
    # Basic commands (Ch5-6)
    # -------------------------------------------------------------------------

    @handler("ping")
    def handle_ping(self) -> dict:
        return {"result": "PONG"}

    @handler("echo")
    def handle_echo(self, arg: str = "") -> dict:
        return {"result": arg}

    @handler("get")
    def handle_get(self, key: str = "") -> dict:
        entry = self.data.get(key)
        if entry is None:
            return {"result": None, "found": False}
        if is_expired(entry):
            del self.data[key]
            return {"result": None, "found": False}
        return {"result": entry["value"], "found": True}

    @handler("set")
    def handle_set(
        self,
        key: str = "",
        value: str = "",
        nx: bool = False,
        xx: bool = False,
        ex: Optional[int] = None,
        px: Optional[int] = None,
    ) -> dict:
        if self.num_shards > 1 and not self._owns_key(key):
            return {"result": None, "skip": True}
        if nx and key in self.data:
            return {"result": None, "ok": False}
        if xx and key not in self.data:
            return {"result": None, "ok": False}

        expires_at_ms: Optional[int] = None
        if ex is not None:
            expires_at_ms = now_ms() + ex * 1000
        elif px is not None:
            expires_at_ms = now_ms() + px

        self.data[key] = {"value": value, "expires_at_ms": expires_at_ms}
        self.replication_offset += 1
        return {"result": "OK", "ok": True}

    @handler("incr")
    def handle_incr(self, key: str = "") -> dict:
        if self.num_shards > 1 and not self._owns_key(key):
            return {"result": None, "skip": True}
        entry = self.data.get(key)
        if entry is not None and is_expired(entry):
            del self.data[key]
            entry = None

        if entry is None:
            new_val = 1
        else:
            try:
                new_val = int(entry["value"]) + 1
            except (ValueError, TypeError):
                return {
                    "result": None,
                    "error": "ERR value is not an integer or out of range",
                }

        self.data[key] = {"value": str(new_val), "expires_at_ms": None}
        self.replication_offset += 1
        return {"result": new_val, "error": None}

    @handler("del")
    def handle_del(self, key: str = "") -> dict:
        if self.num_shards > 1 and not self._owns_key(key):
            return {"result": 0, "skip": True}
        deleted = 1 if self.data.pop(key, None) is not None else 0
        if deleted:
            self.replication_offset += 1
        return {"result": deleted}

    # -------------------------------------------------------------------------
    # Cross-shard query handlers (scatter_gather / reduce / map)
    # -------------------------------------------------------------------------

    @handler("dbsize")
    def handle_dbsize(self) -> dict:
        count = sum(1 for e in self.data.values() if not is_expired(e))
        return {"count": count, "shard_id": self.shard_id}

    @handler("keys")
    def handle_keys(self) -> dict:
        keys = [k for k, e in self.data.items() if not is_expired(e)]
        return {"keys": keys, "shard_id": self.shard_id}

    @handler("info")
    def handle_info(self) -> dict:
        key_count = sum(1 for e in self.data.values() if not is_expired(e))
        return {
            "role": self.role,
            "shard_id": self.shard_id,
            "replication_offset": self.replication_offset,
            "keys": key_count,
        }

    # -------------------------------------------------------------------------
    # Replication handlers (Ch7-8)
    # -------------------------------------------------------------------------

    @handler("replicate")
    def handle_replicate(
        self,
        command: str = "",
        key: str = "",
        value: str = "",
        expires_at_ms: Optional[int] = None,
        offset: int = 0,
    ) -> dict:
        cmd = command.upper()
        if cmd == "SET":
            self.data[key] = {"value": value, "expires_at_ms": expires_at_ms}
        elif cmd == "DEL":
            self.data.pop(key, None)
        elif cmd == "INCR":
            entry = self.data.get(key)
            cur = int(entry["value"]) if entry else 0
            self.data[key] = {"value": str(cur + 1), "expires_at_ms": None}
        self.replication_offset = offset
        return {"result": "OK", "offset": self.replication_offset}

    @handler("get_ack")
    def handle_get_ack(self) -> dict:
        return {"offset": self.replication_offset, "shard_id": self.shard_id}

    @handler("handshake")
    def handle_handshake(self, step: str = "", args: List[str] = None) -> dict:
        step = step.lower()
        if step == "ping":
            response = "PONG"
        elif step == "replconf":
            response = "OK"
        elif step == "psync":
            response = f"FULLRESYNC {self.shard_id} 0"
        else:
            response = "ERR unknown handshake step"
        return {"status": response}

    @handler("bulk_sync")
    def handle_bulk_sync(self, data: dict = None, offset: int = 0) -> dict:
        self.data = data or {}
        self.replication_offset = offset
        self.role = "replica"
        return {"status": "OK", "keys_synced": len(self.data)}

    # -------------------------------------------------------------------------
    # Parallel primitive handlers
    # -------------------------------------------------------------------------

    @handler("expire_sweep")
    def handle_expire_sweep(self) -> dict:
        expired = [k for k, e in self.data.items() if is_expired(e)]
        for key in expired:
            del self.data[key]
        return {"result": "OK", "swept": len(expired)}

    @handler("snapshot")
    def handle_snapshot(self) -> dict:
        live = {k: e for k, e in self.data.items() if not is_expired(e)}
        return {
            "shard_id": self.shard_id,
            "role": self.role,
            "replication_offset": self.replication_offset,
            "data": live,
        }


# =============================================================================
# ConnectionActor — Virtual Actor per Client (MULTI/EXEC/DISCARD)
# =============================================================================

@actor
class ConnectionActor:
    """
    Per-client virtual actor that manages MULTI/EXEC/DISCARD transaction state.

    One ConnectionActor is lazily created per client ID.  Because actors process
    one message at a time there are no locks — isolation is structural.
    """

    client_id: str = state(default="")
    in_multi: bool = state(default=False)
    queued: List[Dict[str, Any]] = state(default_factory=list)

    @init_handler
    def on_init(self, config: dict):
        self.client_id = config.get("actor_id", "")
        self.in_multi = False
        self.queued = []

    @handler("execute")
    def execute(self, command: str = "", args: List[str] = None) -> dict:
        args = args or []
        cmd = command.upper()

        if cmd == "MULTI":
            if self.in_multi:
                return {"result": "ERR MULTI calls can not be nested"}
            self.in_multi = True
            self.queued = []
            return {"result": "OK"}

        if cmd == "DISCARD":
            if not self.in_multi:
                return {"result": "ERR DISCARD without MULTI"}
            self.in_multi = False
            self.queued = []
            return {"result": "OK"}

        if cmd == "EXEC":
            if not self.in_multi:
                return {"result": "ERR EXEC without MULTI"}
            queued = self.queued[:]
            self.in_multi = False
            self.queued = []
            return {"result": "EXEC", "queued": queued}

        if self.in_multi:
            self.queued.append({"command": cmd, "args": args})
            return {"result": "QUEUED"}

        return {"result": "ERR command not supported outside transaction context"}


# =============================================================================
# RedisCoordinator — Cluster Orchestrator
# =============================================================================

@actor
class RedisCoordinator:
    """
    Cluster coordinator that creates master and replica shard groups on startup
    and exposes Redis-level operations to clients.

    Uses PlexSpaces distributed primitives:
    - host.create_shard_group()        — spawn hash-partitioned actor fleet
    - host.bulk_update_shard_group()   — key-routed SET / INCR / DEL
    - host.scatter_gather()            — DBSIZE, KEYS, WAIT
    - host.broadcast_shard_group()     — expire_sweep, replication, snapshot
    - host.reduce_shard_group()        — DBSIZE aggregate (SUM)
    - host.map_shard_group()           — parallel snapshot across shards
    """

    num_shards: int = state(default=3)
    master_group_id: str = state(default="")
    replica_group_id: str = state(default="")
    initialized: bool = state(default=False)
    application_id: str = state(default="redis-coordinator")
    total_coord_ms: float = state(default=0.0)
    total_compute_ms: float = state(default=0.0)
    operation_count: int = state(default=0)

    @init_handler
    def on_init(self, config: dict):
        args = config.get("args", {})
        self.num_shards = int(args.get("num_shards", 3))
        self.master_group_id = "redis-masters"
        self.replica_group_id = "redis-replicas"
        self.initialized = False
        self.application_id = "redis-coordinator"
        self.total_coord_ms = 0.0
        self.total_compute_ms = 0.0
        self.operation_count = 0

    @handler("setup")
    def setup(self) -> dict:
        """Create master and replica shard groups, run replication handshake."""
        if self.initialized:
            return {"status": "already_initialized", "masters": self.master_group_id}

        t0 = time.time()
        # --- Create master shard group ---
        def _create_group(group_id: str, role: str) -> dict:
            # Try from_registry first (distributes across all cluster nodes).
            # Fall back to same_node if remote spawn fails (single-node mode or
            # unreachable peer in registry).
            for placement in ({"strategy": "from_registry"}, {"strategy": "same_node"}):
                try:
                    return host.create_shard_group({
                        "group_id": group_id,
                        "actor_type": "StorageActor",
                        "shard_count": self.num_shards,
                        "partition_strategy": "hash",
                        "rebalance_policy": "manual",
                        "placement": placement,
                        "initial_state": {"args": {"num_shards": self.num_shards, "role": role}},
                    })
                except Exception as e:
                    err = str(e)
                    if placement["strategy"] == "from_registry" and (
                        "tcp connect" in err or "remote spawn" in err.lower() or "connect error" in err.lower()
                    ):
                        continue  # retry with same_node
                    raise

        master_group = _create_group(self.master_group_id, "master")
        master_ids = master_group.get("shard_actor_ids", [])

        # --- Create replica shard group ---
        _create_group(self.replica_group_id, "replica")

        # --- Replication handshake: PING → REPLCONF → PSYNC ---
        for step in ("ping", "replconf", "psync"):
            host.broadcast_shard_group({
                "group_id": self.master_group_id,
                "payload": {"op": "handshake", "step": step, "args": []},
                "timeout_ms": 5000,
            })

        # --- Initial bulk sync (RDB equivalent) ---
        snapshot_resp = host.scatter_gather({
            "group_id": self.master_group_id,
            "payload": {"op": "snapshot"},
            "timeout_ms": 5000,
        })
        shards = snapshot_resp.get("shard_responses", [])
        total_keys = sum(
            len(s.get("payload", {}).get("data", {})) for s in shards
        )
        host.broadcast_shard_group({
            "group_id": self.replica_group_id,
            "payload": {
                "op": "bulk_sync",
                "data": {},
                "offset": 0,
            },
            "timeout_ms": 5000,
        })

        self.initialized = True
        coord_ms = (time.time() - t0) * 1000
        self.total_coord_ms += coord_ms
        self.operation_count += 1
        _safe_metrics_add(
            self.application_id,
            {
                "message_count": 1,
                "counter_metrics": {"setup_calls": 1, "shards_created": self.num_shards * 2},
                "latency_totals_ms": {"coord": int(self.total_coord_ms)},
                "latency_max_ms": {"coord": int(coord_ms)},
                "latency_samples": {"coord": 1},
            },
        )
        return {
            "status": "ok",
            "masters": self.master_group_id,
            "replicas": self.replica_group_id,
            "shard_count": self.num_shards,
            "master_actor_ids": master_ids,
            "keys_synced": total_keys,
        }

    # -------------------------------------------------------------------------
    # Basic commands
    # -------------------------------------------------------------------------

    @handler("ping")
    def ping(self) -> dict:
        t0 = time.time()
        resp = host.scatter_gather({
            "group_id": self.master_group_id,
            "query": {"op": "ping"},
            "aggregation": "concat",
            "min_responses": self.num_shards,
            "timeout_ms": 5000,
        })
        coord_ms = (time.time() - t0) * 1000
        self.total_coord_ms += coord_ms
        self.operation_count += 1
        _safe_metrics_add(
            self.application_id,
            {
                "message_count": 1,
                "counter_metrics": {"ping_calls": 1, "total_operations": self.operation_count},
                "latency_totals_ms": {"coord": int(self.total_coord_ms)},
                "latency_max_ms": {"coord": int(coord_ms)},
                "latency_samples": {"coord": 1},
            },
        )
        results = [s.get("payload", {}).get("result") for s in resp.get("shard_responses", [])]
        return {"result": results}

    @handler("get")
    def get(self, key: str = "") -> dict:
        t0 = time.time()
        resp = host.scatter_gather({
            "group_id": self.master_group_id,
            "payload": {"op": "get", "key": key},
            "timeout_ms": 5000,
            "min_responses": 1,
        })
        coord_ms = (time.time() - t0) * 1000
        self.total_coord_ms += coord_ms
        self.operation_count += 1
        _safe_metrics_add(
            self.application_id,
            {
                "message_count": 1,
                "counter_metrics": {"get_calls": 1, "total_operations": self.operation_count},
                "latency_totals_ms": {"coord": int(self.total_coord_ms)},
                "latency_max_ms": {"coord": int(coord_ms)},
                "latency_samples": {"coord": 1},
            },
        )
        payload = {}
        for sr in resp.get("shard_responses", []):
            p = sr.get("payload", {})
            if p.get("found"):
                payload = p
                break
        return {"result": payload.get("result"), "found": payload.get("found", False)}

    @handler("set")
    def set(
        self,
        key: str = "",
        value: str = "",
        nx: bool = False,
        xx: bool = False,
        ex: Optional[int] = None,
        px: Optional[int] = None,
    ) -> dict:
        t0 = time.time()
        resp = host.scatter_gather({
            "group_id": self.master_group_id,
            "payload": {"op": "set", "key": key, "value": value, "nx": nx, "xx": xx, "ex": ex, "px": px},
            "timeout_ms": 5000,
            "min_responses": 1,
        })
        coord_ms = (time.time() - t0) * 1000
        self.total_coord_ms += coord_ms
        self.operation_count += 1
        _safe_metrics_add(
            self.application_id,
            {
                "message_count": 1,
                "counter_metrics": {"set_calls": 1, "total_operations": self.operation_count},
                "latency_totals_ms": {"coord": int(self.total_coord_ms)},
                "latency_max_ms": {"coord": int(coord_ms)},
                "latency_samples": {"coord": 1},
            },
        )
        payload = {}
        for sr in resp.get("shard_responses", []):
            p = sr.get("payload", {})
            if not p.get("skip"):
                payload = p
                break
        return {"result": payload.get("result", "OK")}

    @handler("incr")
    def incr(self, key: str = "") -> dict:
        t0 = time.time()
        resp = host.scatter_gather({
            "group_id": self.master_group_id,
            "payload": {"op": "incr", "key": key},
            "timeout_ms": 5000,
            "min_responses": 1,
        })
        coord_ms = (time.time() - t0) * 1000
        self.total_coord_ms += coord_ms
        self.operation_count += 1
        _safe_metrics_add(
            self.application_id,
            {
                "message_count": 1,
                "counter_metrics": {"incr_calls": 1, "total_operations": self.operation_count},
                "latency_totals_ms": {"coord": int(self.total_coord_ms)},
                "latency_max_ms": {"coord": int(coord_ms)},
                "latency_samples": {"coord": 1},
            },
        )
        payload = {}
        for sr in resp.get("shard_responses", []):
            p = sr.get("payload", {})
            if not p.get("skip"):
                payload = p
                break
        return {"result": payload.get("result"), "error": payload.get("error")}

    @handler("del")
    def delete(self, key: str = "") -> dict:
        t0 = time.time()
        resp = host.scatter_gather({
            "group_id": self.master_group_id,
            "payload": {"op": "del", "key": key},
            "timeout_ms": 5000,
            "min_responses": 1,
        })
        coord_ms = (time.time() - t0) * 1000
        self.total_coord_ms += coord_ms
        self.operation_count += 1
        _safe_metrics_add(
            self.application_id,
            {
                "message_count": 1,
                "counter_metrics": {"del_calls": 1, "total_operations": self.operation_count},
                "latency_totals_ms": {"coord": int(self.total_coord_ms)},
                "latency_max_ms": {"coord": int(coord_ms)},
                "latency_samples": {"coord": 1},
            },
        )
        payload = {}
        for sr in resp.get("shard_responses", []):
            p = sr.get("payload", {})
            if not p.get("skip"):
                payload = p
                break
        return {"result": payload.get("result", 0)}

    # -------------------------------------------------------------------------
    # Cross-shard queries
    # -------------------------------------------------------------------------

    @handler("dbsize")
    def dbsize(self) -> dict:
        """DBSIZE — reduce(SUM) across all shards."""
        t0 = time.time()
        resp = host.reduce_shard_group({
            "group_id": self.master_group_id,
            "query": {"op": "dbsize"},
            "reduce_op": "SUM",
            "reduce_field": "count",
            "timeout_ms": 5000,
        })
        coord_ms = (time.time() - t0) * 1000
        self.total_coord_ms += coord_ms
        self.operation_count += 1
        _safe_metrics_add(
            self.application_id,
            {
                "message_count": 1,
                "counter_metrics": {"dbsize_calls": 1, "total_operations": self.operation_count},
                "latency_totals_ms": {"coord": int(self.total_coord_ms)},
                "latency_max_ms": {"coord": int(coord_ms)},
                "latency_samples": {"coord": 1},
            },
        )
        return {"result": int(resp.get("result", 0))}

    @handler("keys")
    def keys(self) -> dict:
        """KEYS — scatter_gather + concat across all shards."""
        t0 = time.time()
        resp = host.scatter_gather({
            "group_id": self.master_group_id,
            "query": {"op": "keys"},
            "aggregation": "concat",
            "min_responses": self.num_shards,
            "timeout_ms": 5000,
        })
        coord_ms = (time.time() - t0) * 1000
        self.total_coord_ms += coord_ms
        self.operation_count += 1
        _safe_metrics_add(
            self.application_id,
            {
                "message_count": 1,
                "counter_metrics": {"keys_calls": 1, "total_operations": self.operation_count},
                "latency_totals_ms": {"coord": int(self.total_coord_ms)},
                "latency_max_ms": {"coord": int(coord_ms)},
                "latency_samples": {"coord": 1},
            },
        )
        all_keys: List[str] = []
        for shard in resp.get("shard_responses", []):
            shard_keys = shard.get("payload", {}).get("keys", [])
            all_keys.extend(shard_keys)
        return {"result": all_keys, "count": len(all_keys)}

    # -------------------------------------------------------------------------
    # Replication (Ch7-8)
    # -------------------------------------------------------------------------

    @handler("replicate")
    def replicate(
        self,
        command: str = "",
        key: str = "",
        value: str = "",
        expires_at_ms: Optional[int] = None,
        offset: int = 0,
    ) -> dict:
        """Propagate a write to all replica shards via broadcast_shard_group."""
        t0 = time.time()
        resp = host.broadcast_shard_group({
            "group_id": self.replica_group_id,
            "payload": {
                "op": "replicate",
                "command": command,
                "key": key,
                "value": value,
                "expires_at_ms": expires_at_ms,
                "offset": offset,
            },
            "timeout_ms": 5000,
        })
        coord_ms = (time.time() - t0) * 1000
        self.total_coord_ms += coord_ms
        self.operation_count += 1
        _safe_metrics_add(
            self.application_id,
            {
                "message_count": 1,
                "counter_metrics": {"replication_calls": 1, "total_operations": self.operation_count},
                "latency_totals_ms": {"coord": int(self.total_coord_ms)},
                "latency_max_ms": {"coord": int(coord_ms)},
                "latency_samples": {"coord": 1},
            },
        )
        acks = len(resp.get("shard_responses", []))
        return {"result": "OK", "acks": acks}

    @handler("wait")
    def wait(self, num_replicas: int = 1, timeout_ms: int = 5000, min_offset: int = 0) -> dict:
        """WAIT — scatter_gather to collect replication ACKs (Ch8)."""
        t0 = time.time()
        resp = host.scatter_gather({
            "group_id": self.replica_group_id,
            "query": {"op": "get_ack"},
            "aggregation": "concat",
            "min_responses": num_replicas,
            "timeout_ms": timeout_ms,
        })
        coord_ms = (time.time() - t0) * 1000
        self.total_coord_ms += coord_ms
        self.operation_count += 1
        _safe_metrics_add(
            self.application_id,
            {
                "message_count": 1,
                "counter_metrics": {"replication_calls": 1, "total_operations": self.operation_count},
                "latency_totals_ms": {"coord": int(self.total_coord_ms)},
                "latency_max_ms": {"coord": int(coord_ms)},
                "latency_samples": {"coord": 1},
            },
        )
        confirmed = sum(
            1
            for shard in resp.get("shard_responses", [])
            if shard.get("payload", {}).get("offset", 0) >= min_offset
        )
        return {"result": confirmed, "requested": num_replicas}

    # -------------------------------------------------------------------------
    # Active expiry
    # -------------------------------------------------------------------------

    @handler("expire_sweep")
    def expire_sweep(self) -> dict:
        """Active expiry — broadcast expire_sweep to all master shards."""
        t0 = time.time()
        resp = host.broadcast_shard_group({
            "group_id": self.master_group_id,
            "payload": {"op": "expire_sweep"},
            "timeout_ms": 5000,
        })
        coord_ms = (time.time() - t0) * 1000
        self.total_coord_ms += coord_ms
        self.operation_count += 1
        _safe_metrics_add(
            self.application_id,
            {
                "message_count": 1,
                "counter_metrics": {"sweep_calls": 1, "total_operations": self.operation_count},
                "latency_totals_ms": {"coord": int(self.total_coord_ms)},
                "latency_max_ms": {"coord": int(coord_ms)},
                "latency_samples": {"coord": 1},
            },
        )
        total_swept = sum(
            shard.get("payload", {}).get("swept", 0)
            for shard in resp.get("shard_responses", [])
        )
        return {"result": "OK", "swept": total_swept}

    # -------------------------------------------------------------------------
    # Coordinated snapshot
    # -------------------------------------------------------------------------

    @handler("snapshot")
    def snapshot(self) -> dict:
        """Parallel map(snapshot) across all master shards simultaneously."""
        t0 = time.time()
        resp = host.map_shard_group({
            "group_id": self.master_group_id,
            "payload": {"op": "snapshot"},
            "timeout_ms": 5000,
        })
        coord_ms = (time.time() - t0) * 1000
        self.total_coord_ms += coord_ms
        self.operation_count += 1
        _safe_metrics_add(
            self.application_id,
            {
                "message_count": 1,
                "counter_metrics": {"snapshot_calls": 1, "total_operations": self.operation_count},
                "latency_totals_ms": {"coord": int(self.total_coord_ms)},
                "latency_max_ms": {"coord": int(coord_ms)},
                "latency_samples": {"coord": 1},
            },
        )
        shards = resp.get("shard_responses", [])
        total_keys = sum(
            len(s.get("payload", {}).get("data", {})) for s in shards
        )
        return {
            "result": "OK",
            "shard_count": len(shards),
            "total_keys": total_keys,
            "shards": [s.get("payload", {}) for s in shards],
        }

    # -------------------------------------------------------------------------
    # Cluster info
    # -------------------------------------------------------------------------

    @handler("cluster_info")
    def cluster_info(self) -> dict:
        return {
            "status": "ok" if self.initialized else "not_initialized",
            "master_group_id": self.master_group_id,
            "replica_group_id": self.replica_group_id,
            "num_shards": self.num_shards,
        }


# =============================================================================
# BenchmarkActor — measures real throughput via bulk_update_shard_group
#
# Runs N batches of bulk SET, then a sample of individual GETs, reporting:
#   - TPS (keys/sec) for bulk SET
#   - p50 / p95 / p99 per-batch latency for SET
#   - p50 / p95 / p99 per-key latency for GET
#
# Mirrors the pattern from examples/python/apps/parallel_ai_inference/.
# =============================================================================


def _pct(sorted_vals: List[float], p: int) -> float:
    """Return p-th percentile of a pre-sorted list."""
    if not sorted_vals:
        return 0.0
    idx = max(0, int(len(sorted_vals) * p / 100) - 1)
    return sorted_vals[min(idx, len(sorted_vals) - 1)]


@actor
class BenchmarkActor:
    """
    Throughput benchmark actor for the Redis cluster.

    Sends bulk_update_shard_group batches to the master shard group and
    measures sustained SET throughput and GET latency distributions.
    """

    application_id: str = state(default="redis-cluster")
    master_group_id: str = state(default="redis-masters")
    num_shards: int = state(default=3)

    @init_handler
    def on_init(self, config: dict) -> None:
        self.application_id = config.get("args", {}).get("application_id", "redis-cluster")

    @handler("run_throughput_benchmark")
    def run_throughput_benchmark(
        self,
        warmup_batches: int = 3,
        bench_batches: int = 20,
        keys_per_batch: int = 50,
    ) -> dict:
        """
        Run a throughput benchmark:
        1. Warmup: warmup_batches × bulk_update_shard_group (not measured)
        2. Bench: bench_batches × bulk_update_shard_group, measure per-batch wall latency
        3. GET sample: keys_per_batch individual scatter_gather GETs, measure per-key latency

        Returns TPS, p50/p95/p99 for SET (batched) and GET (individual).
        """
        # ── Warmup via bulk_update_shard_group ────────────────────────────────
        for b in range(warmup_batches):
            host.bulk_update_shard_group({
                "group_id": self.master_group_id,
                "updates": [
                    {"key": f"bench:warmup:{b}:{i}", "payload": {"op": "set", "key": f"bench:warmup:{b}:{i}", "value": f"v{i}"}}
                    for i in range(keys_per_batch)
                ],
                "timeout_ms": 10000,
                "wait_for_responses": True,
            })

        # ── Timed SET batches via bulk_update_shard_group ─────────────────────
        set_latencies_ms: List[float] = []
        bench_wall_start = time.time()

        for b in range(bench_batches):
            t0 = time.time()
            host.bulk_update_shard_group({
                "group_id": self.master_group_id,
                "updates": [
                    {"key": f"bench:set:{b}:{i}", "payload": {"op": "set", "key": f"bench:set:{b}:{i}", "value": f"value:{i}"}}
                    for i in range(keys_per_batch)
                ],
                "timeout_ms": 10000,
                "wait_for_responses": True,
            })
            set_latencies_ms.append((time.time() - t0) * 1000)

        bench_wall_elapsed = time.time() - bench_wall_start
        total_set_keys = bench_batches * keys_per_batch
        set_tps = total_set_keys / bench_wall_elapsed if bench_wall_elapsed > 0 else 0.0

        set_latencies_ms.sort()
        set_p50 = _pct(set_latencies_ms, 50)
        set_p95 = _pct(set_latencies_ms, 95)
        set_p99 = _pct(set_latencies_ms, 99)

        # ── Timed GET sample ──────────────────────────────────────────────────
        get_latencies_ms: List[float] = []
        for i in range(keys_per_batch):
            key = f"bench:set:0:{i}"
            t0 = time.time()
            host.scatter_gather({
                "group_id": self.master_group_id,
                "payload": {"op": "get", "key": key},
                "timeout_ms": 5000,
                "min_responses": 1,
            })
            get_latencies_ms.append((time.time() - t0) * 1000)

        get_latencies_ms.sort()
        get_p50 = _pct(get_latencies_ms, 50)
        get_p95 = _pct(get_latencies_ms, 95)
        get_p99 = _pct(get_latencies_ms, 99)
        get_tps = (
            keys_per_batch / (sum(get_latencies_ms) / 1000.0)
            if sum(get_latencies_ms) > 0
            else 0.0
        )

        _safe_metrics_add(
            self.application_id,
            {
                "message_count": total_set_keys + keys_per_batch,
                "counter_metrics": {
                    "benchmark_runs": 1,
                    "set_keys": total_set_keys,
                    "get_keys": keys_per_batch,
                },
                "latency_totals_ms": {
                    "set_batch": int(sum(set_latencies_ms)),
                    "get_key": int(sum(get_latencies_ms)),
                },
                "latency_max_ms": {
                    "set_batch": int(max(set_latencies_ms, default=0)),
                    "get_key": int(max(get_latencies_ms, default=0)),
                },
                "latency_samples": {
                    "set_batch": bench_batches,
                    "get_key": keys_per_batch,
                },
            },
        )

        return {
            "result": "OK",
            "set": {
                "total_keys": total_set_keys,
                "elapsed_ms": int(bench_wall_elapsed * 1000),
                "tps": round(set_tps, 1),
                "p50_ms": round(set_p50, 2),
                "p95_ms": round(set_p95, 2),
                "p99_ms": round(set_p99, 2),
            },
            "get": {
                "total_keys": keys_per_batch,
                "tps": round(get_tps, 1),
                "p50_ms": round(get_p50, 2),
                "p95_ms": round(get_p95, 2),
                "p99_ms": round(get_p99, 2),
            },
        }
