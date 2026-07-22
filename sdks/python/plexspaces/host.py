# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2025 PlexSpaces Contributors
#
# PlexSpaces Host Functions
#
# Provides Pythonic access to PlexSpaces host functions when running inside WASM.
# Outside WASM (e.g., during testing), provides mock implementations.

"""
Host function wrappers for PlexSpaces actors.

Usage:
    from plexspaces import host
    
    # Send message to another actor
    host.send("other-actor", "ping", {"data": "hello"})
    
    # Log a message
    host.log("info", "Processing request")
    
    # Get current timestamp
    ts = host.now_ms()
    
    # Process groups
    host.process_groups.join("room")
    host.process_groups.broadcast("room", "chat", {"msg": "hello"})
"""

import json
from typing import Any, Dict, List, Optional, Union
from .decorators import _desanitize_from_wasm
from .proto_wire import (
    encode_write_request,
    encode_read_request,
    decode_read_response_first,
    decode_read_response_all,
    decode_lock_response,
    encode_create_shard_group_request,
    encode_scatter_gather_request,
    encode_broadcast_shard_group_request,
    encode_reduce_shard_group_request,
    encode_all_reduce_shard_group_request,
    encode_map_shard_group_request,
    encode_barrier_shard_group_request,
    encode_application_metrics,
    decode_create_shard_group_response,
    decode_scatter_gather_response,
    decode_broadcast_shard_group_response,
    decode_reduce_shard_group_response,
    decode_all_reduce_shard_group_response,
    decode_map_shard_group_response,
    decode_barrier_shard_group_response,
    decode_application_metrics_response,
    decode_application_get_status_response,
    decode_http_fetch_response,
    encode_http_fetch_request,
)

# Global reference to actual host module (set by runtime)
_host_impl = None

# Whether the host is a real WIT host (payload = list<u8> = bytes) or mock (string)
_host_is_wit = False

# Eager import at module load time - componentize-py does NOT support
# dynamic imports during handler execution (causes WASM trap)
try:
    from wit_world.imports import host_logging as _wit_host_logging
    _host_logging_is_wit = True
except ImportError:
    _wit_host_logging = None
    _host_logging_is_wit = False

try:
    from wit_world.imports import host_actor as _wit_host_actor
    _host_actor_is_wit = True
except ImportError:
    _wit_host_actor = None
    _host_actor_is_wit = False

try:
    from wit_world.imports import host_kv as _wit_host_kv
    _host_kv_is_wit = True
except ImportError:
    _wit_host_kv = None
    _host_kv_is_wit = False

try:
    from wit_world.imports import host_ts as _wit_host_ts
    _host_ts_is_wit = True
except ImportError:
    _wit_host_ts = None
    _host_ts_is_wit = False

try:
    from wit_world.imports import host_locks as _wit_host_locks
    _host_locks_is_wit = True
except ImportError:
    _wit_host_locks = None
    _host_locks_is_wit = False

try:
    from wit_world.imports import host_blob as _wit_host_blob
    _host_blob_is_wit = True
except ImportError:
    _wit_host_blob = None
    _host_blob_is_wit = False

try:
    from wit_world.imports import host_pool as _wit_host_pool
    _host_pool_is_wit = True
except ImportError:
    _wit_host_pool = None
    _host_pool_is_wit = False

try:
    from wit_world.imports import host_shard as _wit_host_shard
    _host_shard_is_wit = True
except ImportError:
    _wit_host_shard = None
    _host_shard_is_wit = False

try:
    from wit_world.imports import host_http as _wit_host_http
    _host_http_is_wit = True
except ImportError:
    _wit_host_http = None
    _host_http_is_wit = False

_host_is_wit = (
    _host_logging_is_wit or _host_actor_is_wit or _host_kv_is_wit
    or _host_ts_is_wit or _host_locks_is_wit or _host_blob_is_wit
    or _host_pool_is_wit or _host_shard_is_wit or _host_http_is_wit
)

_channels_impl = None
try:
    from wit_world.imports import channels as _wit_channels_eager
    _channels_impl = _wit_channels_eager
except ImportError:
    pass

_registry_impl = None
try:
    from wit_world.imports import registry as _wit_registry_eager
    _registry_impl = _wit_registry_eager
except ImportError:
    pass


def _to_payload_bytes(data: str) -> bytes:
    """Encode a JSON/string payload to bytes for WIT payload (list<u8>) parameters.

    WIT ``payload = list<u8>`` maps to Python ``bytes`` in componentize-py.
    The mock host accepts plain strings, so this only encodes when using
    the real WIT host.
    """
    if _host_is_wit:
        if isinstance(data, (bytes, bytearray)):
            return data
        return data.encode("utf-8") if data else b""
    return data


def _from_payload_bytes(data) -> str:
    """Decode bytes returned from a WIT host function back to a string.

    WIT ``result<payload, actor-error>`` returns ``bytes`` on Ok and ``str``
    on Err in componentize-py.  The mock host returns plain strings.
    """
    if isinstance(data, (bytes, bytearray)):
        return data.decode("utf-8")
    return str(data) if data else ""


def _decode_lock_payload(data) -> str:
    """Decode WIT lock payload bytes into the JSON string expected by SDK callers."""
    if isinstance(data, (bytes, bytearray)):
        return json.dumps(decode_lock_response(bytes(data)))
    return str(data) if data else ""


def _get_mock_host():
    """Get (and cache) the shared MockHost instance."""
    global _host_impl
    if _host_impl is None:
        _host_impl = _MockHost()
    return _host_impl


def _get_host_logging():
    """Get the host_logging WIT module, or mock."""
    if _wit_host_logging is not None:
        return _wit_host_logging
    return _get_mock_host()


def _get_host_actor():
    """Get the host_actor WIT module, or mock."""
    if _wit_host_actor is not None:
        return _wit_host_actor
    return _get_mock_host()


def _get_host_kv():
    """Get the host_kv WIT module, or mock."""
    if _wit_host_kv is not None:
        return _wit_host_kv
    return _get_mock_host()


def _get_host_ts():
    """Get the host_ts WIT module, or mock."""
    if _wit_host_ts is not None:
        return _wit_host_ts
    return _get_mock_host()


def _get_host_locks():
    """Get the host_locks WIT module, or mock."""
    if _wit_host_locks is not None:
        return _wit_host_locks
    return _get_mock_host()


def _get_host_blob():
    """Get the host_blob WIT module, or mock."""
    if _wit_host_blob is not None:
        return _wit_host_blob
    return _get_mock_host()


def _get_host_pool():
    """Get the host_pool WIT module, or mock."""
    if _wit_host_pool is not None:
        return _wit_host_pool
    return _get_mock_host()


def _get_host_shard():
    """Get the host_shard WIT module, or mock."""
    if _wit_host_shard is not None:
        return _wit_host_shard
    return _get_mock_host()


def _get_host_http():
    """Get the host_http WIT module, or mock."""
    if _wit_host_http is not None:
        return _wit_host_http
    return _get_mock_host()


def _get_channels():
    """Get the channels WIT import (real or mock fallback to _get_host_actor())."""
    global _channels_impl
    if _channels_impl is not None:
        return _channels_impl
    return _get_host_actor()


def _get_registry():
    """Get the registry WIT import (real or mock fallback to _get_host_actor())."""
    global _registry_impl
    if _registry_impl is not None:
        return _registry_impl
    return _get_host_actor()


class _MockHost:
    """Mock host for testing outside WASM environment."""

    def __init__(self):
        self._kv: Dict[str, str] = {}
        self._tuples: List[List[Any]] = []
        self._blobs: Dict[str, str] = {}
        self._groups: Dict[str, List[str]] = {}
        self._sent_messages: List[Dict[str, str]] = []
        self._group_messages: List[Dict[str, str]] = []
        self._self_id = "mock-actor"
        self.ts = _TupleSpaceHelper(self)
        self._channel_queues: Dict[str, List[Dict[str, Any]]] = {}
        self._channel_topics: Dict[str, List[Dict[str, Any]]] = {}
        self._channel_acked: Dict[str, bool] = {}
        self._channel_nacked: Dict[str, bool] = {}
        self._channel_subs: Dict[str, str] = {}
        self._channel_sub_counter: int = 0

    def _matches_tuple(self, tuple_value: List[Any], pattern: List[Any]) -> bool:
        if len(tuple_value) != len(pattern):
            return False
        for actual, expected in zip(tuple_value, pattern):
            if expected is None or expected == "*":
                continue
            if actual != expected:
                return False
        return True

    def send(self, to: str, msg_type: str, payload_json: str) -> str:
        self._sent_messages.append(
            {
                "to": to,
                "msg_type": msg_type,
                "payload_json": payload_json,
            }
        )
        return ""

    def log(self, level: str, message: str) -> None:
        print(f"[{level.upper()}] {message}")

    def now_ms(self) -> int:
        import time
        return int(time.time() * 1000)

    def kv_get(self, key: str) -> str:
        return self._kv.get(key, "")

    def kv_put(self, key: str, value: str) -> str:
        self._kv[key] = value
        return ""

    def ts_write(self, tuple_json: str) -> str:
        """TupleSpace write (mock). Returns empty on success."""
        try:
            tuple_value = json.loads(tuple_json)
            if isinstance(tuple_value, list):
                self._tuples.append(tuple_value)
        except json.JSONDecodeError:
            return "ERROR: invalid tuple JSON"
        return ""

    def ts_read(self, pattern_json: str) -> str:
        """TupleSpace read (mock). Returns empty if not found."""
        try:
            pattern = json.loads(pattern_json)
            if not isinstance(pattern, list):
                return ""
        except json.JSONDecodeError:
            return "ERROR: invalid pattern JSON"
        for tuple_value in self._tuples:
            if self._matches_tuple(tuple_value, pattern):
                return json.dumps(tuple_value)
        return ""

    def ts_take(self, pattern_json: str) -> str:
        """TupleSpace take (mock). Returns empty if not found."""
        try:
            pattern = json.loads(pattern_json)
            if not isinstance(pattern, list):
                return ""
        except json.JSONDecodeError:
            return "ERROR: invalid pattern JSON"
        for index, tuple_value in enumerate(self._tuples):
            if self._matches_tuple(tuple_value, pattern):
                self._tuples.pop(index)
                return json.dumps(tuple_value)
        return ""

    def ts_read_all(self, pattern_json: str) -> str:
        """TupleSpace read-all (mock). Returns empty array."""
        try:
            pattern = json.loads(pattern_json)
            if not isinstance(pattern, list):
                return "[]"
        except json.JSONDecodeError:
            return "ERROR: invalid pattern JSON"
        matches = [tuple_value for tuple_value in self._tuples if self._matches_tuple(tuple_value, pattern)]
        return json.dumps(matches)

    def kv_delete(self, key: str) -> str:
        """Key-value delete (mock). Returns empty on success."""
        if key in self._kv:
            del self._kv[key]
        return ""

    def kv_list(self, prefix: str) -> str:
        """Key-value list (mock). Returns JSON array of keys."""
        keys = [k for k in self._kv.keys() if k.startswith(prefix)]
        return json.dumps(keys)

    def kv_put_with_ttl(self, key: str, value: str, ttl_seconds: int) -> str:
        self._kv[key] = value
        return ""

    def kv_get_ttl(self, key: str) -> int:
        return 0

    def kv_cas(self, key: str, expected: str, new_value: str) -> bool:
        if expected == "":
            if key in self._kv:
                return False
        else:
            if self._kv.get(key) != expected:
                return False
        self._kv[key] = new_value
        return True

    def kv_increment(self, key: str, delta: int) -> int:
        current = 0
        try:
            current = int(self._kv.get(key, "0"))
        except (ValueError, TypeError):
            pass
        new_val = current + delta
        self._kv[key] = str(new_val)
        return new_val

    def kv_multi_get(self, keys: list) -> list:
        return [self._kv.get(k) for k in keys]

    def kv_multi_put(self, entries: dict) -> str:
        self._kv.update(entries)
        return ""

    def alarm_set(self, timestamp_ms: int) -> str:
        self._alarm: int = timestamp_ms
        return ""

    def alarm_get(self) -> int:
        return getattr(self, "_alarm", 0)

    def alarm_delete(self) -> str:
        self._alarm = 0
        return ""

    def lock_acquire(
        self,
        holder_id: str,
        lock_name: str,
        lease_duration_secs: int = 30,
        timeout_ms: int = 0,
    ) -> str:
        """Lock acquire (mock). Returns JSON with lock_key, version, holder_id, etc."""
        return json.dumps({
            "lock_key": lock_name,
            "version": "mock-version-1",
            "holder_id": holder_id,
            "locked": True,
            "lease_duration_secs": lease_duration_secs,
            "expires_at_ms": 0,
        })

    def lock_release(
        self,
        lock_id: str,
        holder_id: str,
        lock_version: str,
    ) -> str:
        """Lock release (mock). Returns empty on success."""
        return ""

    def lock_renew(
        self,
        lock_id: str,
        holder_id: str,
        lock_version: str,
        lease_duration_secs: int = 30,
    ) -> str:
        """Lock renew lease (mock). Returns same version on success."""
        return lock_version

    def blob_upload(self, blob_id: str, data: str, content_type: str) -> str:
        """Blob upload (mock). Returns empty on success."""
        self._blobs[blob_id] = data
        return ""

    def blob_download(self, blob_id: str) -> str:
        """Blob download (mock). Returns empty if not found."""
        return self._blobs.get(blob_id, "")

    def blob_delete(self, blob_id: str) -> str:
        """Blob delete (mock). Returns empty on success."""
        self._blobs.pop(blob_id, None)
        return ""

    def blob_list(self, prefix: str) -> str:
        """Blob list (mock). Returns empty array."""
        return json.dumps([blob_id for blob_id in self._blobs if blob_id.startswith(prefix)])

    def ask(self, to: str, msg_type: str, payload_json: str, timeout_ms: int) -> str:
        """Ask (mock). Returns empty JSON."""
        return "{}"

    def self_id(self) -> str:
        """Self ID (mock). Returns mock actor ID."""
        return self._self_id

    def spawn(self, module_ref: str, actor_name: str, role: str, args_json: str) -> str:
        """Spawn (mock). Returns spawned actor name (canonical ID in real WASM)."""
        spawned_id = actor_name if actor_name else f"mock-{module_ref}-1"
        return spawned_id

    def stop(self, actor_id: str) -> str:
        """Stop (mock). Returns empty on success."""
        return ""

    def link(self, actor_id: str) -> str:
        """Link (mock). Returns empty on success."""
        return ""

    def unlink(self, actor_id: str) -> str:
        """Unlink (mock). Returns empty on success."""
        return ""

    def monitor(self, actor_id: str) -> str:
        """Monitor (mock). Returns mock monitor ref."""
        return "mock-monitor-1"

    def demonitor(self, monitor_ref: str) -> str:
        """Demonitor (mock). Returns empty on success."""
        return ""

    def send_after(self, delay_ms: int, msg_type: str, payload_json: str) -> str:
        """Send-after (mock). Returns timer ID."""
        return "mock-timer-1"

    def pg_join(self, group_name: str) -> str:
        """Process group join (mock). Returns empty on success."""
        members = self._groups.setdefault(group_name, [])
        if self._self_id not in members:
            members.append(self._self_id)
        return ""

    def pg_leave(self, group_name: str) -> str:
        """Process group leave (mock). Returns empty on success."""
        members = self._groups.get(group_name, [])
        if self._self_id in members:
            members.remove(self._self_id)
        return ""

    def pg_members(self, group_name: str) -> str:
        """Process group members (mock). Returns empty array."""
        return json.dumps(self._groups.get(group_name, []))

    def pg_broadcast(self, group_name: str, msg_type: str, payload_json: str) -> str:
        """Process group broadcast (mock). Returns empty on success."""
        self._group_messages.append(
            {
                "group": group_name,
                "msg_type": msg_type,
                "payload_json": payload_json,
            }
        )
        return ""

    def pool_checkout(self, pool_name: str, timeout_ms: int) -> str:
        """Elastic pool checkout (mock). Returns JSON handle or ERROR."""
        return json.dumps({
            "actor_id": "mock-worker-0",
            "pool_name": pool_name,
            "checkout_id": "mock-checkout-1",
        })

    def pool_checkin(
        self,
        pool_name: str,
        actor_id: str,
        checkout_id: str,
        healthy: bool,
    ) -> str:
        """Elastic pool checkin (mock). Returns empty on success."""
        return ""

    def pool_get_metrics(self, pool_name: str) -> str:
        """Elastic pool metrics (mock). Returns JSON."""
        return json.dumps({
            "total_actors": 2,
            "available_actors": 1,
            "busy_actors": 0,
            "current_load": 0.0,
        })

    def create_shard_group(self, request_json: str) -> str:
        return json.dumps({
            "group_id": "mock-group",
            "actor_type": "worker",
            "shard_actor_ids": ["worker-0@test-node"],
        })

    def bulk_update_shard_group(self, request_json: str) -> str:
        return json.dumps({
            "updates_sent": 1,
            "updates_succeeded": 1,
            "updates_failed": 0,
            "errors": [],
        })

    def map_shard_group(self, request_json: str) -> str:
        return json.dumps({
            "results": [],
            "stats": {"succeeded": 0, "failed": 0, "total": 0},
        })

    def scatter_gather(self, request_json: str) -> str:
        return json.dumps({
            "result": None,
            "shard_responses": [],
            "stats": {"shards_queried": 0, "shards_responded": 0, "shards_failed": 0},
        })

    def broadcast_shard_group(self, request_json: str) -> str:
        return json.dumps({
            "shard_responses": [],
            "stats": {"shards_queried": 0, "shards_responded": 0, "shards_failed": 0},
        })

    def reduce_shard_group(self, request_json: str) -> str:
        return json.dumps({
            "result": None,
            "shard_responses": [],
            "stats": {"shards_queried": 0, "shards_responded": 0, "shards_failed": 0},
        })

    def all_reduce_shard_group(self, request_json: str) -> str:
        return json.dumps({
            "result": None,
            "shard_responses": [],
            "stats": {"shards_queried": 0, "shards_responded": 0, "shards_failed": 0},
        })

    def barrier_shard_group(self, request_json: str) -> str:
        return json.dumps({
            "shard_responses": [],
            "stats": {"shards_queried": 0, "shards_responded": 0, "shards_failed": 0},
        })

    def spawn_actors(self, request_json: str) -> str:
        request = json.loads(request_json)
        results = []
        for item in request.get("requests", []):
            actor_id = item.get("actor_id") or item.get("actor_type", "actor")
            results.append({
                "success": True,
                "error": "",
                "response": {
                    "actor_ref": f"{actor_id}@test-node",
                    "actor_id": actor_id,
                },
            })
        return json.dumps({"results": results})

    def application_metrics_add(self, application_id: str, metrics_json: str) -> str:
        return metrics_json

    def application_get_metrics(self, application_id: str, node_id: str) -> str:
        return json.dumps({
            "actor_counts": {},
            "supervisor_count": 1,
            "uptime_seconds": 0,
            "message_count": 0,
            "error_count": 0,
            "counter_metrics": {},
            "latency_totals_ms": {},
            "latency_max_ms": {},
            "latency_samples": {},
        })

    def application_get_status(self, application_id: str, node_id: str) -> str:
        return json.dumps({
            "node_id": node_id,
            "node_address": "http://localhost:8092",
            "application": {
                "application_id": application_id,
                "name": application_id,
                "version": "1.0.0",
                "status": 2,
                "metrics": {
                    "actor_counts": {},
                    "supervisor_count": 1,
                    "uptime_seconds": 0,
                    "message_count": 0,
                    "error_count": 0,
                    "counter_metrics": {},
                    "latency_totals_ms": {},
                    "latency_max_ms": {},
                    "latency_samples": {},
                },
            },
        })

    def http_fetch(
        self,
        link_name: str,
        method: str,
        path_and_query: str,
        headers_json: str,
        body: str,
    ) -> str:
        """Outbound HTTP fetch via named service link (mock).
        Returns JSON: {"status":200,"headers":{},"body":""} or "ERROR:..."
        """
        return json.dumps({
            "status": 200,
            "headers": {},
            "body": "",
        })

    # ========================================================================
    # Channel (queue + pub/sub) mock
    # ========================================================================

    def channel_send(self, ctx: str, channel_name: str, msg_type: str, payload_json: str) -> str:
        queue = self._channel_queues.setdefault(channel_name, [])
        msg_id = f"msg-{len(queue) + 1}"
        queue.append({"id": msg_id, "msg_type": msg_type, "payload": payload_json})
        return msg_id

    def channel_send_with_options(
        self, ctx: str, channel_name: str, msg_type: str, payload_json: str,
        delay_ms: int, ttl_ms: int, headers_json: str
    ) -> str:
        return self.channel_send(ctx, channel_name, msg_type, payload_json)

    def channel_receive(self, ctx: str, channel_name: str, timeout_ms: int) -> str:
        queue = self._channel_queues.get(channel_name, [])
        if not queue:
            return ""
        msg = queue.pop(0)
        return json.dumps({
            "id": msg["id"],
            "msg_type": msg["msg_type"],
            "payload": msg["payload"],
            "timestamp": 0,
            "delivery_count": 1,
            "headers": [],
        })

    def channel_publish(self, ctx: str, channel_name: str, msg_type: str, payload_json: str) -> str:
        topic = self._channel_topics.setdefault(channel_name, [])
        msg_id = f"pub-{len(topic) + 1}"
        topic.append({"id": msg_id, "msg_type": msg_type, "payload": payload_json})
        return msg_id

    def channel_subscribe(self, ctx: str, channel_name: str, filter_str: str) -> str:
        self._channel_sub_counter += 1
        sub_id = f"sub-{self._channel_sub_counter}"
        self._channel_subs[sub_id] = channel_name
        return sub_id

    def channel_unsubscribe(self, subscription_id: str) -> str:
        self._channel_subs.pop(subscription_id, None)
        return ""

    def channel_ack(self, ctx: str, channel_name: str, message_id: str) -> str:
        self._channel_acked[message_id] = True
        return ""

    def channel_nack(self, ctx: str, channel_name: str, message_id: str, requeue: bool) -> str:
        self._channel_nacked[message_id] = True
        return ""

    def channel_create(self, ctx: str, channel_name: str, max_size: int, message_ttl_ms: int) -> str:
        self._channel_queues.setdefault(channel_name, [])
        return ""

    def channel_delete(self, ctx: str, channel_name: str) -> str:
        self._channel_queues.pop(channel_name, None)
        return ""

    def channel_depth(self, ctx: str, channel_name: str) -> str:
        return str(len(self._channel_queues.get(channel_name, [])))

    # ========================================================================
    # Object Registry mock
    # ========================================================================

    def registry_register(self, ctx: str, object_id: str, object_type: str, grpc_address: str,
                          object_category: Optional[str], capabilities: List[str],
                          labels: List[str]) -> str:
        if not hasattr(self, "_registry"):
            self._registry: Dict[str, Any] = {}
        self._registry[object_id] = {
            "object_id": object_id,
            "object_type": object_type,
            "grpc_address": grpc_address,
            "object_category": object_category or "",
            "capabilities": capabilities,
            "labels": labels,
            "health_status": "healthy",
        }
        return ""

    def registry_unregister(self, ctx: str, object_type: str, object_id: str) -> str:
        if hasattr(self, "_registry"):
            self._registry.pop(object_id, None)
        return ""

    def registry_lookup(self, ctx: str, object_type: str, object_id: str) -> str:
        if not hasattr(self, "_registry"):
            return "null"
        reg = self._registry.get(object_id)
        return json.dumps(reg) if reg else "null"

    def registry_lookup_by_alias(self, ctx: str, alias: str) -> str:
        if not hasattr(self, "_registry"):
            return "null"
        for reg in self._registry.values():
            if reg.get("alias") == alias:
                return json.dumps(reg)
        return "null"

    def registry_discover(self, ctx: str, object_type: Optional[str], object_category: Optional[str],
                          capabilities: List[str], labels: List[str], health_status: Optional[str],
                          offset: int, limit: int) -> str:
        if not hasattr(self, "_registry"):
            return "[]"
        results = []
        for reg in self._registry.values():
            if object_type and reg.get("object_type") != object_type:
                continue
            if object_category and reg.get("object_category") != object_category:
                continue
            if health_status and reg.get("health_status") != health_status:
                continue
            results.append(reg)
        return json.dumps(results[offset:offset + limit])

    def registry_heartbeat(self, ctx: str, object_type: str, object_id: str) -> str:
        return ""


class _TupleSpaceHelper:
    """
    Tuple space helper: list-in, list-out API. Use None in patterns for wildcards.

    Example:
        from plexspaces import host

        host.ts.write(["ensemble", "job-1", "task", "t0", 0])
        t = host.ts.take(["ensemble", "job-1", "task", None, None])  # None = wildcard
        if t:
            ...
        all_results = host.ts.read_all(["ensemble", "job-1", "result", None, None])
    """

    def __init__(self, host_ref: Any):
        self._host = host_ref

    def write(self, tuple_list: List[Any]) -> str:
        """
        Write a tuple to the tuple space. Elements must be JSON-serializable.

        Returns:
            Empty string on success, "ERROR:..." on failure.
        """
        json_str = json.dumps(tuple_list)
        return self._host.ts_write(json_str)

    def take(self, pattern: List[Any]) -> Optional[List[Any]]:
        """
        Take one matching tuple (destructive). Use None in pattern for wildcards.

        Returns:
            Matched tuple as a list, or None if no match or error.
        """
        json_str = json.dumps(pattern)
        raw = self._host.ts_take(json_str)
        if not raw or raw.startswith("ERROR"):
            return None
        try:
            return json.loads(raw)
        except (json.JSONDecodeError, TypeError):
            return None

    def read(self, pattern: List[Any]) -> Optional[List[Any]]:
        """
        Read one matching tuple (non-destructive). Use None in pattern for wildcards.

        Returns:
            Matched tuple as a list, or None if no match or error.
        """
        json_str = json.dumps(pattern)
        raw = self._host.ts_read(json_str)
        if not raw or raw.startswith("ERROR"):
            return None
        try:
            return json.loads(raw)
        except (json.JSONDecodeError, TypeError):
            return None

    def read_all(self, pattern: List[Any]) -> List[List[Any]]:
        """
        Read all matching tuples (non-destructive). Use None in pattern for wildcards.

        Returns:
            List of tuples (each tuple is a list of elements).
        """
        json_str = json.dumps(pattern)
        raw = self._host.ts_read_all(json_str)
        if not raw or raw.startswith("ERROR"):
            return []
        try:
            out = json.loads(raw)
            return out if isinstance(out, list) else []
        except (json.JSONDecodeError, TypeError):
            return []


class ProcessGroups:
    """
    Process groups host functions for pub/sub coordination.

    Process groups allow actors to:
    - Join named groups
    - Broadcast messages to all group members
    - List members
    - Leave groups

    Example:
        from plexspaces import host

        host.process_groups.join("chat-room")
        members = host.process_groups.members("chat-room")
        host.process_groups.broadcast("chat-room", "chat", {"text": "Hello!"})
        host.process_groups.leave("chat-room")
    """

    def join(self, group: str) -> None:
        """Join a process group (uses self actor ID)."""
        h = _get_host_actor()
        result = h.pg_join(group)
        if result and isinstance(result, str) and result.startswith("ERROR:"):
            raise RuntimeError(result)

    def leave(self, group: str) -> None:
        """Leave a process group."""
        h = _get_host_actor()
        result = h.pg_leave(group)
        if result and isinstance(result, str) and result.startswith("ERROR:"):
            raise RuntimeError(result)

    def members(self, group: str) -> List[str]:
        """Get all members of a group. Returns list of actor IDs."""
        h = _get_host_actor()
        raw = h.pg_members(group)
        result = _from_payload_bytes(raw) if isinstance(raw, (bytes, bytearray)) else (raw or "[]")
        if isinstance(result, str) and result.startswith("ERROR:"):
            raise RuntimeError(result)
        try:
            return json.loads(result) if isinstance(result, str) else result
        except (json.JSONDecodeError, ValueError):
            return []

    def broadcast(self, group: str, msg_type: str, payload: Any = None) -> None:
        """Broadcast a message to all members of a group."""
        h = _get_host_actor()
        payload_json = json.dumps(payload) if payload is not None else "{}"
        result = h.pg_broadcast(group, msg_type, _to_payload_bytes(payload_json))
        if result and isinstance(result, str) and result.startswith("ERROR:"):
            raise RuntimeError(result)

    def first(self, group: str) -> Optional[str]:
        """Return the first member of a process group, or None if empty."""
        members = self.members(group)
        return members[0] if members else None

    def first_or_raise(self, group: str) -> str:
        """Return the first member of a process group, raising if empty."""
        members = self.members(group)
        if not members:
            raise RuntimeError(f"no members in process group {group!r}")
        return members[0]


class EventLog:
    """
    Two-cursor monotonic append-only log backed by KV.

    Embed in actor state — it serializes via __dict__ / dataclass / attrs.
    Each consumer tracks its own read cursor so they advance independently.

    Example::

        log = EventLog()

        # append
        seq = log.append(host, "audit:", {"action": "login"})

        # poll (returns new events since last call for this consumer)
        events, new_cursor = log.poll(host, "audit:", "consumer-1", limit=20)
    """

    def __init__(self, watermark: int = 0) -> None:
        self.watermark: int = watermark

    def append(self, h: "Host", prefix: str, entry: Any) -> int:
        """Write entry to KV and advance the watermark. Returns the assigned sequence number."""
        self.watermark += 1
        key = f"{prefix}seq:{self.watermark}"
        try:
            serialized = json.dumps(entry)
            _get_host_kv().kv_put(key, _to_payload_bytes(serialized))
        except Exception as e:
            self.watermark -= 1
            raise RuntimeError(f"EventLog.append: {e}") from e
        return self.watermark

    def poll(self, h: "Host", prefix: str, consumer_id: str, limit: int = 100):
        """
        Return up to *limit* events for *consumer_id* that arrived after its last cursor.

        Returns ``(events, new_cursor)`` where events is a list of deserialized entries
        and new_cursor is the last sequence number consumed.
        The new cursor is persisted in KV so the next call resumes from where this left off.
        """
        cursor_key = f"{prefix}cursor:{consumer_id}"
        raw_cursor = _from_payload_bytes(_get_host_kv().kv_get(cursor_key))
        cursor = int(raw_cursor) if raw_cursor and raw_cursor.isdigit() else 0

        events: list = []
        new_cursor = cursor
        seq = cursor + 1
        while seq <= self.watermark and len(events) < limit:
            raw = _from_payload_bytes(_get_host_kv().kv_get(f"{prefix}seq:{seq}"))
            if raw:
                try:
                    entry = json.loads(raw)
                except (json.JSONDecodeError, ValueError):
                    entry = None
                if entry is not None:
                    events.append(entry)
                    new_cursor = seq
            seq += 1

        if new_cursor != cursor:
            _get_host_kv().kv_put(cursor_key, _to_payload_bytes(str(new_cursor)))
        return events, new_cursor


class Channel:
    """
    Channel host functions for queue and pub/sub messaging patterns.

    Channels unify queue (one consumer) and pub/sub (all subscribers) under
    a single API. The provider (InMemory, Redis, Kafka, etc.) is a runtime concern.

    Example::

        from plexspaces import host

        # Queue (point-to-point)
        msg_id = host.channel.send(ctx, "work-queue", "process", {"task": "data"})
        msg, ok, err = host.channel.receive(ctx, "work-queue", timeout_ms=5000)
        if ok:
            host.channel.ack(ctx, "work-queue", msg["id"])

        # Pub/sub (broadcast)
        host.channel.publish(ctx, "events", "user_login", {"user": "alice"})
    """

    def send(
        self,
        ctx: Any,
        channel_name: str,
        msg_type: str,
        payload: Any = None,
    ) -> str:
        """Send a message to a channel (queue semantics). Returns message ID."""
        h = _get_channels()
        ctx_json = json.dumps(ctx) if not isinstance(ctx, str) else ctx
        payload_json = json.dumps(payload) if payload is not None else "{}"
        result = h.channel_send(ctx_json, channel_name, msg_type, _to_payload_bytes(payload_json))
        if isinstance(result, (bytes, bytearray)):
            result = _from_payload_bytes(result)
        if result and result.startswith("ERROR:"):
            raise RuntimeError(result)
        return result

    def send_with_options(
        self,
        ctx: Any,
        channel_name: str,
        msg_type: str,
        payload: Any = None,
        delay_ms: int = 0,
        ttl_ms: int = 0,
        headers: Optional[Dict[str, str]] = None,
    ) -> str:
        """Send with delay, TTL, and custom headers. Returns message ID."""
        h = _get_channels()
        ctx_json = json.dumps(ctx) if not isinstance(ctx, str) else ctx
        payload_json = json.dumps(payload) if payload is not None else "{}"
        headers_json = json.dumps(headers or {})
        result = h.channel_send_with_options(ctx_json, channel_name, msg_type, _to_payload_bytes(payload_json), delay_ms, ttl_ms, headers_json)
        if isinstance(result, (bytes, bytearray)):
            result = _from_payload_bytes(result)
        if result and result.startswith("ERROR:"):
            raise RuntimeError(result)
        return result

    def receive(
        self,
        ctx: Any,
        channel_name: str,
        timeout_ms: int = 0,
    ) -> tuple:
        """
        Receive one message from a channel.

        Returns:
            (message_dict, True, None) on receipt, or (None, False, None) on timeout/empty.
            message_dict has keys: id, msg_type, payload, timestamp, delivery_count, headers.
        """
        h = _get_channels()
        ctx_json = json.dumps(ctx) if not isinstance(ctx, str) else ctx
        raw = h.channel_receive(ctx_json, channel_name, timeout_ms)
        if isinstance(raw, (bytes, bytearray)):
            raw = _from_payload_bytes(raw)
        if not raw:
            return None, False, None
        if raw.startswith("ERROR:"):
            raise RuntimeError(raw)
        try:
            return json.loads(raw), True, None
        except (json.JSONDecodeError, ValueError) as e:
            raise RuntimeError(f"channel receive decode: {e}") from e

    def publish(
        self,
        ctx: Any,
        channel_name: str,
        msg_type: str,
        payload: Any = None,
    ) -> str:
        """Publish a message to a channel (pub/sub — all subscribers receive). Returns message ID."""
        h = _get_channels()
        ctx_json = json.dumps(ctx) if not isinstance(ctx, str) else ctx
        payload_json = json.dumps(payload) if payload is not None else "{}"
        result = h.channel_publish(ctx_json, channel_name, msg_type, _to_payload_bytes(payload_json))
        if isinstance(result, (bytes, bytearray)):
            result = _from_payload_bytes(result)
        if result and result.startswith("ERROR:"):
            raise RuntimeError(result)
        return result

    def subscribe(self, ctx: Any, channel_name: str, filter_str: str = "") -> str:
        """Subscribe to a channel (pub/sub). Returns a subscription ID."""
        h = _get_channels()
        ctx_json = json.dumps(ctx) if not isinstance(ctx, str) else ctx
        result = h.channel_subscribe(ctx_json, channel_name, filter_str)
        if isinstance(result, (bytes, bytearray)):
            result = _from_payload_bytes(result)
        if result and result.startswith("ERROR:"):
            raise RuntimeError(result)
        return result

    def unsubscribe(self, subscription_id: str) -> None:
        """Cancel a subscription by ID."""
        h = _get_channels()
        result = h.channel_unsubscribe(subscription_id)
        if isinstance(result, (bytes, bytearray)):
            result = _from_payload_bytes(result)
        if result and result.startswith("ERROR:"):
            raise RuntimeError(result)

    def ack(self, ctx: Any, channel_name: str, message_id: str) -> None:
        """Acknowledge successful processing (prevents redelivery)."""
        h = _get_channels()
        ctx_json = json.dumps(ctx) if not isinstance(ctx, str) else ctx
        result = h.channel_ack(ctx_json, channel_name, message_id)
        if isinstance(result, (bytes, bytearray)):
            result = _from_payload_bytes(result)
        if result and result.startswith("ERROR:"):
            raise RuntimeError(result)

    def nack(self, ctx: Any, channel_name: str, message_id: str, requeue: bool = True) -> None:
        """Negative-acknowledge a message. requeue=True retries; False sends to dead-letter channel."""
        h = _get_channels()
        ctx_json = json.dumps(ctx) if not isinstance(ctx, str) else ctx
        result = h.channel_nack(ctx_json, channel_name, message_id, requeue)
        if isinstance(result, (bytes, bytearray)):
            result = _from_payload_bytes(result)
        if result and result.startswith("ERROR:"):
            raise RuntimeError(result)

    def create(self, ctx: Any, channel_name: str, max_size: int = 0, message_ttl_ms: int = 0) -> None:
        """Create a channel if it does not exist. max_size=0 means unbounded."""
        h = _get_channels()
        ctx_json = json.dumps(ctx) if not isinstance(ctx, str) else ctx
        result = h.channel_create(ctx_json, channel_name, max_size, message_ttl_ms)
        if isinstance(result, (bytes, bytearray)):
            result = _from_payload_bytes(result)
        if result and result.startswith("ERROR:"):
            raise RuntimeError(result)

    def delete(self, ctx: Any, channel_name: str) -> None:
        """Delete a channel and all pending messages."""
        h = _get_channels()
        ctx_json = json.dumps(ctx) if not isinstance(ctx, str) else ctx
        result = h.channel_delete(ctx_json, channel_name)
        if isinstance(result, (bytes, bytearray)):
            result = _from_payload_bytes(result)
        if result and result.startswith("ERROR:"):
            raise RuntimeError(result)

    def depth(self, ctx: Any, channel_name: str) -> int:
        """Return the number of pending (unacked) messages in a channel."""
        h = _get_channels()
        ctx_json = json.dumps(ctx) if not isinstance(ctx, str) else ctx
        result = h.channel_depth(ctx_json, channel_name)
        if isinstance(result, (bytes, bytearray)):
            result = _from_payload_bytes(result)
        if result and result.startswith("ERROR:"):
            raise RuntimeError(result)
        try:
            return int(result) if result else 0
        except ValueError:
            return 0


class Registry:
    """
    Object Registry host functions for registration and discovery.

    Enables actors to register themselves and discover other actors,
    services, and nodes by type, category, capabilities, or alias.

    Example::

        from plexspaces import host

        # Register this actor
        host.registry.register(ctx, my_id, "actor", grpc_address, "GenServer", [], [])

        # Look up an actor by alias (Orleans grain directory pattern)
        reg = host.registry.lookup_by_alias(ctx, "Counter:my-counter:default:tenant1")
        if reg:
            actor_grpc = reg["grpc_address"]

        # Discover all actors of a given category
        actors = host.registry.discover(ctx, object_type="actor", object_category="GenServer")
    """

    def register(
        self,
        ctx: Any,
        object_id: str,
        object_type: str,
        grpc_address: str,
        object_category: Optional[str] = None,
        capabilities: Optional[List[str]] = None,
        labels: Optional[List[str]] = None,
    ) -> None:
        """Register an object in the registry."""
        h = _get_registry()
        if _registry_impl is not None:
            # Real WIT path: single JSON-encoded payload argument.
            req = {
                "object_id": object_id,
                "object_type": object_type,
                "grpc_address": grpc_address,
                "object_category": object_category or "",
                "capabilities": capabilities or [],
                "labels": labels or [],
            }
            h.register(_to_payload_bytes(json.dumps(req)))
        else:
            ctx_json = json.dumps(ctx) if not isinstance(ctx, str) else ctx
            fn = getattr(h, "registry_register", None)
            if fn is None:
                raise RuntimeError("registry.register not available")
            result = fn(ctx_json, object_id, object_type, grpc_address,
                        object_category, capabilities or [], labels or [])
            result_str = result or ""
            if isinstance(result_str, str) and result_str.startswith("ERROR:"):
                raise RuntimeError(result_str)

    def unregister(self, ctx: Any, object_type: str, object_id: str) -> None:
        """Unregister an object from the registry."""
        h = _get_registry()
        if _registry_impl is not None:
            req = {"object_type": object_type, "object_id": object_id}
            h.unregister(_to_payload_bytes(json.dumps(req)))
        else:
            ctx_json = json.dumps(ctx) if not isinstance(ctx, str) else ctx
            fn = getattr(h, "registry_unregister", None)
            if fn is None:
                raise RuntimeError("registry.unregister not available")
            result = fn(ctx_json, object_type, object_id)
            result_str = result or ""
            if isinstance(result_str, str) and result_str.startswith("ERROR:"):
                raise RuntimeError(result_str)

    def lookup(self, ctx: Any, object_type: str, object_id: str) -> Optional[Dict[str, Any]]:
        """Look up an object by ID. Returns the registration dict or None; raises on errors."""
        h = _get_registry()
        if _registry_impl is not None:
            req = {"object_type": object_type, "object_id": object_id}
            raw = h.lookup(_to_payload_bytes(json.dumps(req)))
            result = _from_payload_bytes(raw) if isinstance(raw, (bytes, bytearray)) else (raw or "null")
        else:
            ctx_json = json.dumps(ctx) if not isinstance(ctx, str) else ctx
            fn = getattr(h, "registry_lookup", None)
            if fn is None:
                raise RuntimeError("registry.lookup not available")
            result = fn(ctx_json, object_type, object_id) or "null"
        if isinstance(result, str) and result.startswith("ERROR:"):
            raise RuntimeError(result)
        try:
            parsed = json.loads(result)
            return parsed if parsed is not None else None
        except (json.JSONDecodeError, TypeError):
            return None

    def lookup_by_alias(self, ctx: Any, alias: str) -> Optional[Dict[str, Any]]:
        """Look up an object by alias. Returns the registration dict or None; raises on errors."""
        h = _get_registry()
        if _registry_impl is not None:
            # WIT: lookup-by-alias takes a plain string (not a payload).
            raw = h.lookup_by_alias(alias)
            result = _from_payload_bytes(raw) if isinstance(raw, (bytes, bytearray)) else (raw or "null")
        else:
            ctx_json = json.dumps(ctx) if not isinstance(ctx, str) else ctx
            fn = getattr(h, "registry_lookup_by_alias", None)
            if fn is None:
                raise RuntimeError("registry.lookup_by_alias not available")
            result = fn(ctx_json, alias) or "null"
        if isinstance(result, str) and result.startswith("ERROR:"):
            raise RuntimeError(result)
        try:
            parsed = json.loads(result)
            return parsed if parsed is not None else None
        except (json.JSONDecodeError, TypeError):
            return None

    def discover(
        self,
        ctx: Any,
        object_type: Optional[str] = None,
        object_category: Optional[str] = None,
        capabilities: Optional[List[str]] = None,
        labels: Optional[List[str]] = None,
        health_status: Optional[str] = None,
        offset: int = 0,
        limit: int = 100,
    ) -> List[Dict[str, Any]]:
        """Discover objects with optional filtering. Returns list of registration dicts."""
        h = _get_registry()
        if _registry_impl is not None:
            req: Dict[str, Any] = {"offset": offset, "limit": limit}
            if object_type is not None:
                req["object_type"] = object_type
            if object_category is not None:
                req["object_category"] = object_category
            if capabilities:
                req["capabilities"] = capabilities
            if labels:
                req["labels"] = labels
            if health_status is not None:
                req["health_status"] = health_status
            raw = h.discover(_to_payload_bytes(json.dumps(req)))
            result = _from_payload_bytes(raw) if isinstance(raw, (bytes, bytearray)) else (raw or "[]")
        else:
            ctx_json = json.dumps(ctx) if not isinstance(ctx, str) else ctx
            fn = getattr(h, "registry_discover", None)
            if fn is None:
                raise RuntimeError("registry.discover not available")
            result = fn(ctx_json, object_type, object_category, capabilities or [],
                        labels or [], health_status, offset, limit) or "[]"
        if isinstance(result, str) and result.startswith("ERROR:"):
            raise RuntimeError(result)
        try:
            parsed = json.loads(result)
            return parsed if isinstance(parsed, list) else []
        except (json.JSONDecodeError, TypeError):
            return []

    def heartbeat(self, ctx: Any, object_type: str, object_id: str) -> None:
        """Update the heartbeat for a registered object."""
        h = _get_registry()
        if _registry_impl is not None:
            req = {"object_type": object_type, "object_id": object_id}
            h.heartbeat(_to_payload_bytes(json.dumps(req)))
        else:
            ctx_json = json.dumps(ctx) if not isinstance(ctx, str) else ctx
            fn = getattr(h, "registry_heartbeat", None)
            if fn is None:
                raise RuntimeError("registry.heartbeat not available")
            result = fn(ctx_json, object_type, object_id) or ""
            if isinstance(result, str) and result.startswith("ERROR:"):
                raise RuntimeError(result)


class KVStore:
    """Key-value store accessor. Equivalent to Cloudflare DO storage API."""

    def get(self, key: str) -> str:
        """Get value by key. Returns empty string if not found."""
        return _from_payload_bytes(_get_host_kv().kv_get(key))

    def put(self, key: str, value: str) -> None:
        """Store a string value under key."""
        _get_host_kv().kv_put(key, _to_payload_bytes(value))

    def delete(self, key: str) -> None:
        """Delete a key."""
        _get_host_kv().kv_delete(key)

    def list(self, prefix: str = "") -> list:
        """List keys with optional prefix. Returns list of key strings."""
        raw = _get_host_kv().kv_list(prefix)
        if raw is None:
            return []
        if isinstance(raw, list):
            return raw
        if isinstance(raw, (bytes, bytearray)):
            raw = _from_payload_bytes(raw)
        try:
            result = json.loads(raw)
            return result if isinstance(result, list) else []
        except (json.JSONDecodeError, ValueError):
            return []

    def put_with_ttl(self, key: str, value: str, ttl_seconds: int) -> None:
        """Store a value with automatic expiry after ttl_seconds."""
        h = _get_host_kv()
        fn = getattr(h, "kv_put_with_ttl", None) or getattr(h, "kv-put-with-ttl", None)
        if fn:
            fn(key, _to_payload_bytes(value), ttl_seconds)

    def get_ttl(self, key: str) -> int:
        """Returns remaining TTL in seconds, or 0 if no TTL / key not found."""
        h = _get_host_kv()
        fn = getattr(h, "kv_get_ttl", None) or getattr(h, "kv-get-ttl", None)
        if not fn:
            return 0
        result = fn(key)
        if result is None:
            return 0
        try:
            return int(result)
        except (TypeError, ValueError):
            return 0

    def cas(self, key: str, expected: Optional[str], new_value: str) -> bool:
        """Compare-and-swap. Returns True if the swap was applied."""
        h = _get_host_kv()
        fn = getattr(h, "kv_cas", None) or getattr(h, "kv-cas", None)
        if fn is None:
            return False
        exp_bytes = _to_payload_bytes(expected) if expected is not None else b""
        return bool(fn(key, exp_bytes, _to_payload_bytes(new_value)))

    def increment(self, key: str, delta: int = 1) -> int:
        """Atomically increment a numeric counter by delta. Returns new value."""
        h = _get_host_kv()
        fn = getattr(h, "kv_increment", None) or getattr(h, "kv-increment", None)
        if not fn:
            return 0
        result = fn(key, delta)
        if result is None:
            return 0
        try:
            return int(result)
        except (TypeError, ValueError):
            return 0

    def multi_get(self, keys: list) -> list:
        """Fetch multiple keys in one call. Returns list of values (None for missing)."""
        import base64
        h = _get_host_kv()
        fn = getattr(h, "kv_multi_get", None) or getattr(h, "kv-multi-get", None)
        if not fn:
            return [None] * len(keys)
        if _host_kv_is_wit:
            raw = fn(json.dumps(keys).encode("utf-8"))
            decoded = _from_payload_bytes(raw)
            try:
                items = json.loads(decoded)
                return [base64.b64decode(v).decode("utf-8") if v is not None else None for v in items]
            except Exception:
                return [None] * len(keys)
        else:
            result = fn(keys)
            if isinstance(result, list):
                return [_from_payload_bytes(v) if isinstance(v, (bytes, bytearray)) else v for v in result]
            return [None] * len(keys)

    def multi_put(self, entries: dict) -> None:
        """Store multiple key-value pairs in one call."""
        import base64
        h = _get_host_kv()
        fn = getattr(h, "kv_multi_put", None) or getattr(h, "kv-multi-put", None)
        if not fn:
            return
        if _host_kv_is_wit:
            encoded = {k: base64.b64encode(v.encode("utf-8")).decode("ascii") for k, v in entries.items()}
            fn(json.dumps(encoded).encode("utf-8"))
        else:
            fn(entries)

    def get_json(self, key: str) -> Optional[Any]:
        """Retrieve a JSON value by key. Returns deserialized object or None if not found."""
        raw = self.get(key)
        if not raw:
            return None
        try:
            return json.loads(raw)
        except (json.JSONDecodeError, ValueError):
            return None

    def put_json(self, key: str, value: Any) -> None:
        """Serialize value to JSON and store under key."""
        try:
            serialized = json.dumps(value)
        except (TypeError, ValueError) as e:
            raise ValueError(f"kv.put_json({key!r}): serialization failed: {e}") from e
        result = _get_host_kv().kv_put(key, _to_payload_bytes(serialized))
        if result and isinstance(result, str) and result.startswith("ERROR:"):
            raise RuntimeError(f"kv.put_json({key!r}): {result}")


class AlarmClient:
    """Durable alarm accessor. Equivalent to Cloudflare DO setAlarm/deleteAlarm."""

    def set(self, timestamp_ms: int) -> None:
        """Schedule a durable alarm at an absolute timestamp (ms since epoch)."""
        h = _get_host_kv()
        fn = getattr(h, "alarm_set", None) or getattr(h, "alarm-set", None)
        if fn:
            fn(timestamp_ms)

    def set_in(self, delay_ms: int) -> None:
        """Schedule an alarm relative to now (delay in milliseconds)."""
        try:
            now = _get_host_logging().now_ms()
        except Exception:
            import time
            now = int(time.time() * 1000)
        self.set(now + delay_ms)

    def get(self) -> int:
        """Returns the scheduled alarm timestamp in ms, or 0 if no alarm is set."""
        h = _get_host_kv()
        fn = getattr(h, "alarm_get", None) or getattr(h, "alarm-get", None)
        if not fn:
            return 0
        result = fn()
        if result is None:
            return 0
        try:
            return int(result)
        except (TypeError, ValueError):
            return 0

    def delete(self) -> None:
        """Cancel the pending durable alarm."""
        h = _get_host_kv()
        fn = getattr(h, "alarm_delete", None) or getattr(h, "alarm-delete", None)
        if fn:
            fn()


class LockClient:
    """Distributed lock accessor."""

    def acquire(self, holder_id: str, lock_name: str, lease_duration_secs: int = 30, timeout_ms: int = 0) -> bytes:
        """Acquire a lock. Returns raw response bytes."""
        result = _get_host_locks().lock_acquire(holder_id, lock_name, lease_duration_secs, timeout_ms)
        return bytes(result) if result else b""

    def release(self, lock_id: str, holder_id: str, lock_version: str) -> None:
        """Release a held lock."""
        _get_host_locks().lock_release(lock_id, holder_id, lock_version)

    def renew(self, lock_id: str, holder_id: str, lock_version: str, lease_duration_secs: int = 30) -> bytes:
        """Renew a held lock lease. Returns raw response bytes."""
        result = _get_host_locks().lock_renew(lock_id, holder_id, lock_version, lease_duration_secs)
        return bytes(result) if result else b""


class BlobClient:
    """Blob storage accessor."""

    def upload(self, name: str, data: bytes, content_type: str = "application/octet-stream") -> str:
        """Upload blob data. Returns blob ID or empty on success."""
        return _get_host_blob().blob_upload(name, list(data), content_type)

    def download(self, blob_id: str) -> bytes:
        """Download blob data. Returns bytes or empty bytes if not found."""
        result = _get_host_blob().blob_download(blob_id)
        return bytes(result) if result else b""

    def delete(self, blob_id: str) -> None:
        """Delete a blob."""
        _get_host_blob().blob_delete(blob_id)

    def list(self, prefix: str = "") -> list:
        """List blob IDs with optional prefix."""
        return _get_host_blob().blob_list(prefix) or []


class ActorRef:
    """Lightweight handle to a named virtual actor.

    Equivalent to Cloudflare DO's env.BINDING.get(id).
    Use get_actor_ref() to obtain one.

    Example::

        room = get_actor_ref("ChatRoomActor", room_id, "default")
        result = room.ask("send_message", payload, timeout_ms=5000)
    """

    def __init__(self, actor_type: str, name: str, namespace: str) -> None:
        self.actor_id = f"{name}//{actor_type}::{namespace}@*"

    def id(self) -> str:
        return self.actor_id

    def tell(self, msg_type: str, payload: bytes) -> None:
        _get_host_actor().send(self.actor_id, msg_type, list(payload))

    def ask(self, msg_type: str, payload: bytes, timeout_ms: int = 5000) -> bytes:
        result = _get_host_actor().ask(self.actor_id, msg_type, list(payload), timeout_ms)
        return bytes(result) if result else b""


def get_actor_ref(actor_type: str, name: str, namespace: str) -> "ActorRef":
    """Construct an ActorRef for a named virtual actor.

    The actor is created automatically on first contact (virtual actor pattern).
    Equivalent to Cloudflare: ``env.CHAT_ROOM.get(room_id)``

    Args:
        actor_type: Actor class name (e.g. "ChatRoomActor")
        name: Unique instance name (e.g. "room-123")
        namespace: Namespace/tenant (e.g. "default")

    Returns:
        ActorRef handle for sending messages
    """
    return ActorRef(actor_type, name, namespace)


class Host:
    """
    PlexSpaces host function interface.

    Provides access to all host capabilities from within an actor.
    """

    def __init__(self):
        self.process_groups = ProcessGroups()
        self.ts = _TupleSpaceHelper(self)
        self.channel = Channel()
        self.registry = Registry()
        self.kv = KVStore()
        self.alarm = AlarmClient()
        self.locks = LockClient()
        self.blob = BlobClient()

    def send(self, to: str, msg_type: str, payload: Optional[Union[str, Dict[str, Any], List[Any]]] = None) -> str:
        """
        Send a message to another actor (fire-and-forget).

        Args:
            to: Target actor ID
            msg_type: Message type
            payload: Message payload (will be JSON-serialized if not a string)

        Returns:
            Response string (empty on success, error message on failure)
        """
        h = _get_host_actor()
        payload_json = ""
        if payload is not None:
            if isinstance(payload, str):
                payload_json = payload
            else:
                payload_json = json.dumps(payload)
        return h.send(to, msg_type, _to_payload_bytes(payload_json))

    def log(self, level: str, message: str) -> None:
        """
        Log a message.

        Args:
            level: Log level ("debug", "info", "warn", "error")
            message: Log message
        """
        h = _get_host_logging()
        h.log(level, message)

    def debug(self, message: str) -> None:
        """Log a debug message."""
        self.log("debug", message)

    def info(self, message: str) -> None:
        """Log an info message."""
        self.log("info", message)

    def warn(self, message: str) -> None:
        """Log a warning message."""
        self.log("warn", message)

    def error(self, message: str) -> None:
        """Log an error message."""
        self.log("error", message)

    def now_ms(self) -> int:
        """
        Get current timestamp in milliseconds.

        Returns:
            Unix timestamp in milliseconds
        """
        h = _get_host_logging()
        return h.now_ms()

    def incr_counter(self, application_id: str, name: str) -> None:
        """Increment a single named application metric counter by 1."""
        self.incr_counters(application_id, {name: 1})

    def incr_counters(self, application_id: str, counters: Dict[str, int]) -> None:
        """Increment one or more named application metric counters. Errors are logged, never raised."""
        try:
            self.application_metrics_add(application_id, {
                "message_count": len(counters),
                "counter_metrics": counters,
            })
        except Exception as e:
            self.warn(f"incr_counters: metrics update failed: {e}")

    # ========================================================================
    # Messaging: ask (request-reply)
    # ========================================================================

    def ask(self, to: str, msg_type: str, payload: Any = None, timeout_ms: int = 5000) -> Any:
        """
        Send request and wait for response (request-reply pattern).

        Args:
            to: Target actor ID
            msg_type: Message type
            payload: Message payload (will be JSON-serialized)
            timeout_ms: Max wait time in milliseconds (default 5000)

        Returns:
            Parsed JSON response, or raw string if not valid JSON
        """
        h = _get_host_actor()
        payload_json = ""
        if payload is not None:
            payload_json = json.dumps(payload) if not isinstance(payload, str) else payload
        raw = h.ask(to, msg_type, _to_payload_bytes(payload_json), timeout_ms)
        result = _from_payload_bytes(raw)
        if result.startswith("ERROR:"):
            raise RuntimeError(result)
        try:
            parsed = json.loads(result)
            # Defense-in-depth: restore stringified numbers from older WASM
            # modules that may still sanitize floats to strings in handle().
            return _desanitize_from_wasm(parsed)
        except (json.JSONDecodeError, ValueError):
            return result

    # ========================================================================
    # Actor Identity
    # ========================================================================

    def self_id(self) -> str:
        """Get own actor ID."""
        h = _get_host_actor()
        return h.self_id()

    # ========================================================================
    # Actor Lifecycle
    # ========================================================================

    def spawn(self, module_ref: str, actor_name: str = "", role: str = "", args: Optional[Dict[str, str]] = None) -> str:
        """
        Spawn a new actor. Returns the canonical actor ID assigned by the framework.
        Use the returned ID (not actor_name) for all subsequent ask/send/stop calls.

        Args:
            module_ref: Actor type/module reference (must be a deployed WASM module or registered behavior)
            actor_name: Requested name for the new actor. The framework forms the full canonical ID
                        from this name, module_ref, namespace and node. Pass empty string to
                        let the framework auto-generate a ULID name.
            role: Disambiguation key used ONLY when multiple actors in the same supervisor share the
                  same actor_type (module_ref). Pass empty string when module_ref is unique.
            args: Key-value init arguments forwarded to the new actor's init()

        Returns:
            Canonical actor ID string assigned by the framework.
            Raises RuntimeError on failure.
        """
        h = _get_host_actor()
        args_json = json.dumps(args) if args else "{}"
        result = h.spawn(module_ref, actor_name, role, args_json)
        if result and isinstance(result, str) and result.startswith("ERROR:"):
            raise RuntimeError(result)
        return result

    def stop(self, actor_id: str) -> None:
        """
        Stop an actor gracefully.

        Args:
            actor_id: ID of the actor to stop

        Raises:
            RuntimeError: on failure
        """
        h = _get_host_actor()
        result = h.stop(actor_id)
        if result and isinstance(result, str) and result.startswith("ERROR:"):
            raise RuntimeError(result)

    # ========================================================================
    # Actor Linking & Monitoring (Erlang/OTP patterns)
    # ========================================================================

    def link(self, actor_id: str) -> None:
        """Bidirectional link: if either actor crashes, the other is notified."""
        h = _get_host_actor()
        result = h.link(actor_id)
        if result and isinstance(result, str) and result.startswith("ERROR:"):
            raise RuntimeError(result)

    def unlink(self, actor_id: str) -> None:
        """Remove a bidirectional link."""
        h = _get_host_actor()
        result = h.unlink(actor_id)
        if result and isinstance(result, str) and result.startswith("ERROR:"):
            raise RuntimeError(result)

    def monitor(self, actor_id: str) -> str:
        """
        Monitor an actor (unidirectional). Returns monitor reference string.
        """
        h = _get_host_actor()
        result = h.monitor(actor_id)
        if result and isinstance(result, str) and result.startswith("ERROR:"):
            raise RuntimeError(result)
        return result or ""

    def demonitor(self, monitor_ref: str) -> None:
        """Cancel a monitor."""
        h = _get_host_actor()
        result = h.demonitor(monitor_ref)
        if result and isinstance(result, str) and result.startswith("ERROR:"):
            raise RuntimeError(result)

    # ========================================================================
    # Elastic pool (checkout/checkin)
    # ========================================================================

    def pool_checkout(self, pool_name: str, timeout_ms: int = 5000) -> Optional[Dict[str, Any]]:
        """
        Checkout an actor from a named pool.

        Args:
            pool_name: Pool name (e.g. "merlin-workers").
            timeout_ms: Max wait in milliseconds.

        Returns:
            Dict with actor_id, pool_name, checkout_id on success.
            None on failure (pool not configured, timeout, or pool empty).
        """
        h = _get_host_pool()
        fn = getattr(h, "pool_checkout", None) or getattr(h, "pool-checkout", None)
        if fn is None:
            return None
        raw = fn(pool_name, timeout_ms)
        result = _from_payload_bytes(raw)
        if not result or (isinstance(result, str) and result.startswith("ERROR:")):
            return None
        try:
            return json.loads(result)
        except (json.JSONDecodeError, ValueError):
            return None

    def pool_checkin(
        self,
        pool_name: str,
        actor_id: str,
        checkout_id: str,
        healthy: bool = True,
    ) -> None:
        """
        Checkin an actor to the pool.

        Args:
            pool_name: Pool name.
            actor_id: From the handle returned by pool_checkout.
            checkout_id: From the handle returned by pool_checkout.
            healthy: True if the actor is healthy and can be reused.

        Raises:
            RuntimeError: on failure.
        """
        h = _get_host_pool()
        fn = getattr(h, "pool_checkin", None) or getattr(h, "pool-checkin", None)
        if fn is None:
            raise RuntimeError("pool checkin not available")
        result = fn(pool_name, actor_id, checkout_id, healthy)
        if result and isinstance(result, str) and result.startswith("ERROR:"):
            raise RuntimeError(result)

    def pool_get_metrics(self, pool_name: str) -> Optional[Dict[str, Any]]:
        """
        Get pool metrics (size, available, busy, load).

        Args:
            pool_name: Pool name.

        Returns:
            Dict with total_actors, available_actors, busy_actors, current_load, etc.
            None if not available or on error.
        """
        h = _get_host_pool()
        fn = getattr(h, "pool_get_metrics", None) or getattr(h, "pool-get-metrics", None)
        if fn is None:
            return None
        raw = fn(pool_name)
        result = _from_payload_bytes(raw)
        if not result or (isinstance(result, str) and result.startswith("ERROR:")):
            return None
        try:
            return json.loads(result)
        except (json.JSONDecodeError, ValueError):
            return None

    def _call_shard_fn(
        self,
        name: str,
        encode_fn,
        decode_fn,
        request: Dict[str, Any],
    ) -> Dict[str, Any]:
        """Call a shard-group host function with proto encoding/decoding."""
        h = _get_host_shard()
        fn = getattr(h, name, None) or getattr(h, name.replace("_", "-"), None)
        if fn is None:
            raise RuntimeError(f"{name} not available")
        if _host_shard_is_wit:
            wire = encode_fn(request)
            raw = fn(wire)
            result_bytes = bytes(raw) if raw else b""
            return decode_fn(result_bytes)
        else:
            # Mock host takes/returns JSON strings
            raw = fn(json.dumps(request))
            result = _from_payload_bytes(raw)
            if isinstance(result, str) and result.startswith("ERROR:"):
                raise RuntimeError(result)
            return json.loads(result)

    def create_shard_group(self, request: Dict[str, Any]) -> Dict[str, Any]:
        """Create a shard group."""
        return self._call_shard_fn(
            "create_shard_group",
            encode_create_shard_group_request,
            decode_create_shard_group_response,
            request,
        )

    def bulk_update_shard_group(self, request: Dict[str, Any]) -> Dict[str, Any]:
        """Bulk update a shard group."""
        h = _get_host_shard()
        fn = getattr(h, "bulk_update_shard_group", None) or getattr(h, "bulk-update-shard-group", None)
        if fn is None:
            raise RuntimeError("bulk_update_shard_group not available")
        if _host_shard_is_wit:
            raise RuntimeError("bulk_update_shard_group: proto wire not implemented for Python WASM")
        raw = fn(json.dumps(request))
        result = _from_payload_bytes(raw)
        if isinstance(result, str) and result.startswith("ERROR:"):
            raise RuntimeError(result)
        return json.loads(result)

    def map_shard_group(self, request: Dict[str, Any]) -> Dict[str, Any]:
        """Map across shards."""
        return self._call_shard_fn(
            "map_shard_group",
            encode_map_shard_group_request,
            decode_map_shard_group_response,
            request,
        )

    def scatter_gather(self, request: Dict[str, Any]) -> Dict[str, Any]:
        """Scatter/gather across a shard group."""
        return self._call_shard_fn(
            "scatter_gather",
            encode_scatter_gather_request,
            decode_scatter_gather_response,
            request,
        )

    def broadcast_shard_group(self, request: Dict[str, Any]) -> Dict[str, Any]:
        """Broadcast a message to every shard in a group."""
        return self._call_shard_fn(
            "broadcast_shard_group",
            encode_broadcast_shard_group_request,
            decode_broadcast_shard_group_response,
            request,
        )

    def reduce_shard_group(self, request: Dict[str, Any]) -> Dict[str, Any]:
        """Reduce values returned by a shard-group map operation."""
        return self._call_shard_fn(
            "reduce_shard_group",
            encode_reduce_shard_group_request,
            decode_reduce_shard_group_response,
            request,
        )

    def all_reduce_shard_group(self, request: Dict[str, Any]) -> Dict[str, Any]:
        """Reduce values across a shard group and broadcast the reduced result."""
        return self._call_shard_fn(
            "all_reduce_shard_group",
            encode_all_reduce_shard_group_request,
            decode_all_reduce_shard_group_response,
            request,
        )

    def barrier_shard_group(self, request: Dict[str, Any]) -> Dict[str, Any]:
        """Synchronize a shard group at a barrier round."""
        return self._call_shard_fn(
            "barrier_shard_group",
            encode_barrier_shard_group_request,
            decode_barrier_shard_group_response,
            request,
        )

    def spawn_actors(self, request: Dict[str, Any]) -> Dict[str, Any]:
        """Spawn multiple actors using the framework actor service."""
        h = _get_host_actor()
        fn = getattr(h, "spawn_actors", None) or getattr(h, "spawn-actors", None)
        if fn is None:
            raise RuntimeError("spawn_actors not available")
        if _host_actor_is_wit:
            raise RuntimeError("spawn_actors: proto wire not implemented for Python WASM")
        raw = fn(json.dumps(request))
        result = _from_payload_bytes(raw)
        if isinstance(result, str) and result.startswith("ERROR:"):
            raise RuntimeError(result)
        return json.loads(result)

    def application_metrics_add(
        self, application_id: str, metrics: Dict[str, Any]
    ) -> Dict[str, Any]:
        """Merge a node-local application metrics delta."""
        h = _get_host_actor()
        fn = getattr(h, "application_metrics_add", None) or getattr(
            h, "application-metrics-add", None
        )
        if fn is None:
            raise RuntimeError("application_metrics_add not available")
        if _host_actor_is_wit:
            wire = encode_application_metrics(metrics)
            raw = fn(application_id, wire)
            result_bytes = bytes(raw) if raw else b""
            return decode_application_metrics_response(result_bytes)
        else:
            raw = fn(application_id, _to_payload_bytes(json.dumps(metrics)))
            result = _from_payload_bytes(raw)
            if isinstance(result, str) and result.startswith("ERROR:"):
                raise RuntimeError(result)
            return json.loads(result)

    def application_get_metrics(self, application_id: str, node_id: str) -> Dict[str, Any]:
        """Get application metrics for a participating node."""
        h = _get_host_actor()
        fn = getattr(h, "application_get_metrics", None) or getattr(
            h, "application-get-metrics", None
        )
        if fn is None:
            raise RuntimeError("application_get_metrics not available")
        raw = fn(application_id, node_id)
        if _host_actor_is_wit:
            result_bytes = bytes(raw) if raw else b""
            return decode_application_metrics_response(result_bytes)
        else:
            result = _from_payload_bytes(raw)
            if isinstance(result, str) and result.startswith("ERROR:"):
                raise RuntimeError(result)
            return json.loads(result)

    def application_get_status(self, application_id: str, node_id: str) -> Dict[str, Any]:
        """Get application status for a participating node."""
        h = _get_host_actor()
        fn = getattr(h, "application_get_status", None) or getattr(
            h, "application-get-status", None
        )
        if fn is None:
            raise RuntimeError("application_get_status not available")
        raw = fn(application_id, node_id)
        if _host_actor_is_wit:
            result_bytes = bytes(raw) if raw else b""
            return decode_application_get_status_response(result_bytes)
        else:
            result = _from_payload_bytes(raw)
            if isinstance(result, str) and result.startswith("ERROR:"):
                raise RuntimeError(result)
            return json.loads(result)

    def send_after(self, delay_ms: int, msg_type: str, payload: Any = None) -> str:
        """
        Send a message to self after a delay.

        The host spawns a tracked background task that delivers the message
        after delay_ms milliseconds. The timer-id is returned for observability.

        Note: Timer cancellation is managed by the framework's TimerFacet/ReminderFacet,
        not by individual actors. Stop the actor to cancel pending timers.

        Args:
            delay_ms: Delay in milliseconds
            msg_type: Message type
            payload: Optional payload (will be JSON-serialized)

        Returns:
            Timer ID string (for tracking/observability)
        """
        h = _get_host_actor()
        payload_json = json.dumps(payload) if payload is not None else "{}"
        return h.send_after(delay_ms, msg_type, _to_payload_bytes(payload_json))

def _http_fetch(
    link_name: str,
    method: str,
    path_and_query: str,
    headers: Optional[Dict[str, str]] = None,
    body: Optional[Union[str, bytes]] = None,
) -> Dict[str, Any]:
    """Execute an outbound HTTP request via a named service link."""
    import base64
    hdrs = headers or {}
    headers_json = json.dumps(hdrs)
    if isinstance(body, bytes):
        body_str = base64.b64encode(body).decode("ascii")
        body_bytes = body
    else:
        body_str = body or ""
        body_bytes = body_str.encode("utf-8") if body_str else b""
    h = _get_host_http()
    fn = getattr(h, "http_fetch", None) or getattr(h, "http-fetch", None)
    if fn is None:
        raise RuntimeError("http_fetch not available (no service links configured)")
    if _host_http_is_wit:
        wire_req = encode_http_fetch_request(hdrs, body_bytes)
        raw = fn(link_name, method, path_and_query, _to_payload_bytes(wire_req))
        result_bytes = bytes(raw) if raw else b""
        status, resp_headers, resp_body = decode_http_fetch_response(result_bytes)
        try:
            body_out = resp_body.decode("utf-8")
        except UnicodeDecodeError:
            body_out = base64.b64encode(resp_body).decode("ascii")
        return {
            "status": int(status),
            "headers": dict(resp_headers),
            "body": body_out,
        }
    else:
        result = fn(link_name, method, path_and_query, headers_json, body_str)
    if isinstance(result, str) and result.startswith("ERROR:"):
        raise RuntimeError(result)
    if isinstance(result, str):
        return json.loads(result)
    return result


class ServiceHttpClient:
    """
    Ergonomic outbound HTTP client backed by a named service link.

    The link must be pre-configured in RuntimeConfig.service_links.
    The host handles retries, circuit breaking, and auth injection.

    Usage::

        from plexspaces import host

        http = ServiceHttpClient("payments-api")
        balance = http.get("/v1/balance?account=123")
        result = http.post("/v1/transfer", {"amount": 100})

    """

    def __init__(self, link_name: str):
        self._link_name = link_name

    def get(
        self,
        path_and_query: str,
        headers: Optional[Dict[str, str]] = None,
    ) -> Dict[str, Any]:
        """GET request. Returns response dict with status, headers, body."""
        return _http_fetch(self._link_name, "GET", path_and_query, headers)

    def post(
        self,
        path_and_query: str,
        body: Any = None,
        headers: Optional[Dict[str, str]] = None,
    ) -> Dict[str, Any]:
        """POST JSON request. body is serialized to JSON."""
        body_str = json.dumps(body) if body is not None else ""
        return _http_fetch(self._link_name, "POST", path_and_query, headers, body_str)

    def put(
        self,
        path_and_query: str,
        body: Any = None,
        headers: Optional[Dict[str, str]] = None,
    ) -> Dict[str, Any]:
        """PUT JSON request."""
        body_str = json.dumps(body) if body is not None else ""
        return _http_fetch(self._link_name, "PUT", path_and_query, headers, body_str)

    def delete(
        self,
        path_and_query: str,
        headers: Optional[Dict[str, str]] = None,
    ) -> Dict[str, Any]:
        """DELETE request."""
        return _http_fetch(self._link_name, "DELETE", path_and_query, headers)


# Global host instance
host = Host()


def pg_first(group: str) -> str | None:
    """Return the first process-group member, or None when the group is empty."""
    try:
        members = host.process_groups.members(group)
    except Exception:
        return None
    return members[0] if members else None
