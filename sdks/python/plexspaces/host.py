# SPDX-License-Identifier: LGPL-2.1-or-later
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
    host.process_groups.join("room", "user-123")
    host.process_groups.publish("room", {"msg": "hello"})
"""

import json
from typing import Any, Dict, List, Optional

# Global reference to actual host module (set by runtime)
_host_impl = None
_host_init_attempted = False

# Eager import at module load time - componentize-py does NOT support
# dynamic imports during handler execution (causes WASM trap)
try:
    from wit_world.imports import host as _wit_host_eager
    _host_impl = _wit_host_eager
    _host_init_attempted = True
except ImportError:
    # Not in WASM environment, will use mock
    _host_init_attempted = True


def _get_host():
    """Get the host implementation (real or mock)."""
    global _host_impl, _host_init_attempted
    if _host_impl is not None:
        return _host_impl
    
    # Eager import already attempted - return mock if not in WASM
    if _host_init_attempted:
        _host_impl = _MockHost()
        return _host_impl
    
    # Fallback (should not reach here in WASM)
    try:
        from wit_world.imports import host as wit_host
        _host_impl = wit_host
        return _host_impl
    except ImportError:
        _host_impl = _MockHost()
        return _host_impl


class _MockHost:
    """Mock host for testing outside WASM environment."""

    def __init__(self):
        self._kv: Dict[str, str] = {}

    def send(self, to: str, msg_type: str, payload_json: str) -> str:
        print(f"[MOCK] send({to}, {msg_type}, {payload_json})")
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
        return ""

    def ts_read(self, pattern_json: str) -> str:
        """TupleSpace read (mock). Returns empty if not found."""
        return ""

    def ts_take(self, pattern_json: str) -> str:
        """TupleSpace take (mock). Returns empty if not found."""
        return ""

    def ts_read_all(self, pattern_json: str) -> str:
        """TupleSpace read-all (mock). Returns empty array."""
        return "[]"

    def kv_delete(self, key: str) -> str:
        """Key-value delete (mock). Returns empty on success."""
        if key in self._kv:
            del self._kv[key]
        return ""

    def kv_list(self, prefix: str) -> str:
        """Key-value list (mock). Returns JSON array of keys."""
        import json
        keys = [k for k in self._kv.keys() if k.startswith(prefix)]
        return json.dumps(keys)

    def lock_acquire(
        self,
        tenant_id: str,
        namespace: str,
        holder_id: str,
        lock_name: str,
        lease_duration_secs: int = 30,
        timeout_ms: int = 0,
    ) -> str:
        """Lock acquire (mock). Returns JSON with lock_key, version, holder_id, etc."""
        import json
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
        tenant_id: str,
        namespace: str,
        holder_id: str,
        lock_version: str,
    ) -> str:
        """Lock release (mock). Returns empty on success."""
        return ""

    def lock_renew(
        self,
        lock_id: str,
        tenant_id: str,
        namespace: str,
        holder_id: str,
        lock_version: str,
        lease_duration_secs: int = 30,
    ) -> str:
        """Lock renew lease (mock). Returns same version on success."""
        return lock_version

    def blob_upload(self, blob_id: str, data: str, content_type: str) -> str:
        """Blob upload (mock). Returns empty on success."""
        return ""

    def blob_download(self, blob_id: str) -> str:
        """Blob download (mock). Returns empty if not found."""
        return ""

    def blob_delete(self, blob_id: str) -> str:
        """Blob delete (mock). Returns empty on success."""
        return ""

    def blob_list(self, prefix: str) -> str:
        """Blob list (mock). Returns empty array."""
        return "[]"

    def ask(self, to: str, msg_type: str, payload_json: str, timeout_ms: int) -> str:
        """Ask (mock). Returns empty JSON."""
        print(f"[MOCK] ask({to}, {msg_type}, timeout={timeout_ms})")
        return "{}"

    def self_id(self) -> str:
        """Self ID (mock). Returns mock actor ID."""
        return "mock-actor"

    def spawn(self, module_ref: str, actor_id: str, init_config_json: str) -> str:
        """Spawn (mock). Returns spawned actor ID."""
        spawned_id = actor_id if actor_id else f"mock-{module_ref}-1"
        print(f"[MOCK] spawn({module_ref}, {actor_id}) -> {spawned_id}")
        return spawned_id

    def stop(self, actor_id: str) -> str:
        """Stop (mock). Returns empty on success."""
        print(f"[MOCK] stop({actor_id})")
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
        return ""

    def pg_leave(self, group_name: str) -> str:
        """Process group leave (mock). Returns empty on success."""
        return ""

    def pg_members(self, group_name: str) -> str:
        """Process group members (mock). Returns empty array."""
        return "[]"

    def pg_broadcast(self, group_name: str, msg_type: str, payload_json: str) -> str:
        """Process group broadcast (mock). Returns empty on success."""
        return ""


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
        h = _get_host()
        result = h.pg_join(group)
        if result.startswith("ERROR:"):
            raise RuntimeError(result)

    def leave(self, group: str) -> None:
        """Leave a process group."""
        h = _get_host()
        result = h.pg_leave(group)
        if result.startswith("ERROR:"):
            raise RuntimeError(result)

    def members(self, group: str) -> List[str]:
        """Get all members of a group. Returns list of actor IDs."""
        h = _get_host()
        result = h.pg_members(group)
        if result.startswith("ERROR:"):
            raise RuntimeError(result)
        try:
            return json.loads(result)
        except (json.JSONDecodeError, ValueError):
            return []

    def broadcast(self, group: str, msg_type: str, payload: Any = None) -> None:
        """Broadcast a message to all members of a group."""
        h = _get_host()
        payload_json = json.dumps(payload) if payload is not None else "{}"
        result = h.pg_broadcast(group, msg_type, payload_json)
        if result.startswith("ERROR:"):
            raise RuntimeError(result)


class Host:
    """
    PlexSpaces host function interface.
    
    Provides access to all host capabilities from within an actor.
    """
    
    def __init__(self):
        self.process_groups = ProcessGroups()
    
    def send(self, to: str, msg_type: str, payload: Any = None) -> str:
        """
        Send a message to another actor.
        
        Args:
            to: Target actor ID
            msg_type: Message type
            payload: Message payload (will be JSON-serialized if not a string)
        
        Returns:
            Response string (empty on success, error message on failure)
        """
        h = _get_host()
        payload_json = ""
        if payload is not None:
            if isinstance(payload, str):
                payload_json = payload
            else:
                payload_json = json.dumps(payload)
        return h.send(to, msg_type, payload_json)
    
    def log(self, level: str, message: str) -> None:
        """
        Log a message.
        
        Args:
            level: Log level ("debug", "info", "warn", "error")
            message: Log message
        """
        h = _get_host()
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
        h = _get_host()
        return h.now_ms()

    def kv_get(self, key: str) -> str:
        """
        Key-value get (string-only, WASM-safe).
        Returns value or empty string if not found.
        """
        h = _get_host()
        if hasattr(h, "kv_get"):
            return h.kv_get(key)
        return getattr(h, "kv-get", lambda k: "")(key)

    def kv_put(self, key: str, value: str) -> str:
        """
        Key-value put (string-only). Returns empty on success, ERROR:... on failure.
        """
        h = _get_host()
        if hasattr(h, "kv_put"):
            return h.kv_put(key, value)
        return getattr(h, "kv-put", lambda k, v: "")(key, value)

    def ts_write(self, tuple_json: str) -> str:
        """
        TupleSpace write (existing API). tuple_json: JSON array of strings, e.g. ["AUDIT","action",...].
        Returns empty on success, ERROR:... on failure.
        """
        h = _get_host()
        if hasattr(h, "ts_write"):
            return h.ts_write(tuple_json)
        return getattr(h, "ts-write", lambda _: "")(tuple_json)

    def ts_read(self, pattern_json: str) -> str:
        """
        TupleSpace read (non-destructive). pattern_json: JSON array with wildcards (null or "*").
        Returns matched tuple as JSON array, or empty if not found.
        """
        h = _get_host()
        if hasattr(h, "ts_read"):
            return h.ts_read(pattern_json)
        return getattr(h, "ts-read", lambda _: "")(pattern_json)

    def ts_take(self, pattern_json: str) -> str:
        """
        TupleSpace take (destructive read). pattern_json: JSON array with wildcards.
        Returns matched tuple as JSON array and removes it, or empty if not found.
        """
        h = _get_host()
        if hasattr(h, "ts_take"):
            return h.ts_take(pattern_json)
        return getattr(h, "ts-take", lambda _: "")(pattern_json)

    def ts_read_all(self, pattern_json: str) -> str:
        """
        TupleSpace read-all matching tuples (non-destructive).
        Returns JSON array of matched tuples, e.g. [["task","w1",1],["task","w2",2]].
        """
        h = _get_host()
        if hasattr(h, "ts_read_all"):
            return h.ts_read_all(pattern_json)
        return getattr(h, "ts-read-all", lambda _: "[]")(pattern_json)

    def kv_delete(self, key: str) -> str:
        """
        Key-value delete. Returns empty on success, ERROR:... on failure.
        """
        h = _get_host()
        if hasattr(h, "kv_delete"):
            return h.kv_delete(key)
        return getattr(h, "kv-delete", lambda _: "")(key)

    def kv_list(self, prefix: str) -> str:
        """
        Key-value list keys with prefix. Returns JSON array of keys, e.g. ["key1","key2"].
        """
        h = _get_host()
        if hasattr(h, "kv_list"):
            return h.kv_list(prefix)
        return getattr(h, "kv-list", lambda _: "[]")(prefix)

    def lock_acquire(
        self,
        tenant_id: str,
        namespace: str,
        holder_id: str,
        lock_name: str,
        lease_duration_secs: int = 30,
        timeout_ms: int = 0,
    ) -> str:
        """
        Acquire a distributed lock.
        Required: tenant_id, namespace, holder_id, lock_name.
        Returns JSON on success: {"lock_key","version","holder_id","locked","lease_duration_secs","expires_at_ms"},
        or "ERROR:..." on failure.
        """
        h = _get_host()
        if hasattr(h, "lock_acquire"):
            return h.lock_acquire(
                tenant_id, namespace, holder_id, lock_name, lease_duration_secs, timeout_ms
            )
        return getattr(h, "lock-acquire", lambda *_: "ERROR: not implemented")(
            tenant_id, namespace, holder_id, lock_name, lease_duration_secs, timeout_ms
        )

    def lock_release(
        self,
        lock_id: str,
        tenant_id: str,
        namespace: str,
        holder_id: str,
        lock_version: str,
    ) -> str:
        """
        Release a distributed lock.
        Required: lock_id, tenant_id, namespace, holder_id, lock_version (from acquire or last renew).
        Returns empty on success, ERROR:... on failure.
        """
        h = _get_host()
        if hasattr(h, "lock_release"):
            return h.lock_release(lock_id, tenant_id, namespace, holder_id, lock_version)
        return getattr(h, "lock-release", lambda *_: "")(
            lock_id, tenant_id, namespace, holder_id, lock_version
        )

    def lock_renew(
        self,
        lock_id: str,
        tenant_id: str,
        namespace: str,
        holder_id: str,
        lock_version: str,
        lease_duration_secs: int = 30,
    ) -> str:
        """
        Renew lease on a held lock (heartbeat).
        Required: lock_id, tenant_id, namespace, holder_id, lock_version.
        Returns new lock version on success, or ERROR:... on failure.
        """
        h = _get_host()
        if hasattr(h, "lock_renew"):
            return h.lock_renew(
                lock_id, tenant_id, namespace, holder_id, lock_version, lease_duration_secs
            )
        return getattr(h, "lock-renew", lambda *_: "ERROR: not implemented")(
            lock_id, tenant_id, namespace, holder_id, lock_version, lease_duration_secs
        )

    def blob_upload(self, blob_id: str, data: str, content_type: str = "application/octet-stream") -> str:
        """
        Upload blob data (base64-encoded). Returns empty on success, ERROR:... on failure.
        """
        h = _get_host()
        if hasattr(h, "blob_upload"):
            return h.blob_upload(blob_id, data, content_type)
        return getattr(h, "blob-upload", lambda *_: "")(blob_id, data, content_type)

    def blob_download(self, blob_id: str) -> str:
        """
        Download blob data. Returns base64-encoded content, empty if not found.
        """
        h = _get_host()
        if hasattr(h, "blob_download"):
            return h.blob_download(blob_id)
        return getattr(h, "blob-download", lambda _: "")(blob_id)

    def blob_delete(self, blob_id: str) -> str:
        """
        Delete blob. Returns empty on success, ERROR:... on failure.
        """
        h = _get_host()
        if hasattr(h, "blob_delete"):
            return h.blob_delete(blob_id)
        return getattr(h, "blob-delete", lambda _: "")(blob_id)

    def blob_list(self, prefix: str) -> str:
        """
        List blobs with prefix. Returns JSON array of blob IDs.
        """
        h = _get_host()
        if hasattr(h, "blob_list"):
            return h.blob_list(prefix)
        return getattr(h, "blob-list", lambda _: "[]")(prefix)

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
        h = _get_host()
        payload_json = ""
        if payload is not None:
            payload_json = json.dumps(payload) if not isinstance(payload, str) else payload
        result = h.ask(to, msg_type, payload_json, timeout_ms)
        if result.startswith("ERROR:"):
            raise RuntimeError(result)
        try:
            return json.loads(result)
        except (json.JSONDecodeError, ValueError):
            return result

    # ========================================================================
    # Actor Identity
    # ========================================================================

    def self_id(self) -> str:
        """Get own actor ID."""
        h = _get_host()
        return h.self_id()

    # ========================================================================
    # Actor Lifecycle
    # ========================================================================

    def spawn(self, module_ref: str, actor_id: str = "", init_config: Any = None) -> str:
        """
        Spawn a new actor. Delegates to ActorFactory::spawn_actor() via the host.

        Args:
            module_ref: Actor type/module reference (must be a deployed WASM module or registered behavior)
            actor_id: Unique ID for the new actor (empty = auto-generated ULID)
            init_config: Optional config passed to the new actor's init()

        Returns:
            Spawned actor ID string (may be auto-generated if actor_id was empty).
            Raises RuntimeError on failure.
        """
        h = _get_host()
        config_json = json.dumps(init_config) if init_config is not None else "{}"
        result = h.spawn(module_ref, actor_id, config_json)
        if result.startswith("ERROR:"):
            raise RuntimeError(result)
        return result

    def stop(self, actor_id: str) -> str:
        """
        Stop an actor gracefully.

        Args:
            actor_id: ID of the actor to stop

        Returns:
            Empty string on success, raises RuntimeError on failure
        """
        h = _get_host()
        result = h.stop(actor_id)
        if result.startswith("ERROR:"):
            raise RuntimeError(result)
        return result

    # ========================================================================
    # Actor Linking & Monitoring (Erlang/OTP patterns)
    # ========================================================================

    def link(self, actor_id: str) -> str:
        """Bidirectional link: if either actor crashes, the other is notified."""
        h = _get_host()
        result = h.link(actor_id)
        if result.startswith("ERROR:"):
            raise RuntimeError(result)
        return result

    def unlink(self, actor_id: str) -> str:
        """Remove a bidirectional link."""
        h = _get_host()
        result = h.unlink(actor_id)
        if result.startswith("ERROR:"):
            raise RuntimeError(result)
        return result

    def monitor(self, actor_id: str) -> str:
        """
        Monitor an actor (unidirectional). Returns monitor reference string.
        """
        h = _get_host()
        result = h.monitor(actor_id)
        if result.startswith("ERROR:"):
            raise RuntimeError(result)
        return result

    def demonitor(self, monitor_ref: str) -> str:
        """Cancel a monitor."""
        h = _get_host()
        result = h.demonitor(monitor_ref)
        if result.startswith("ERROR:"):
            raise RuntimeError(result)
        return result

    # ========================================================================
    # Timers (Delayed Messaging)
    # ========================================================================

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
        h = _get_host()
        payload_json = json.dumps(payload) if payload is not None else "{}"
        return h.send_after(delay_ms, msg_type, payload_json)


# Global host instance
host = Host()
