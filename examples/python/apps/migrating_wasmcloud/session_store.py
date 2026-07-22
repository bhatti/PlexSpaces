"""
Session Store Service - wasmCloud-style Capability-Based Design (Python WASM)

Demonstrates wasmCloud's capability-based security model:
- KeyValue capability: Distributed session storage
- Timer capability: Periodic cleanup of expired sessions
- Inter-actor communication: Session validation via host.ask() (simulating HTTP capability)

Real-world use case: Distributed session management (Redis Session Store, AWS ElastiCache).

## wasmCloud Capabilities Showcased

1. **KeyValue Capability**: Store sessions in distributed KV (`session:{id}` → JSON)
2. **Timer Capability**: Periodic cleanup job (every 60s) removes expired sessions
3. **Inter-Actor Communication**: Validate sessions via `host.ask()` to auth service
   (simulates HTTP capability for calling external services)

## SDK Features Used

- @actor: Marks class as PlexSpaces actor
- state(): Persistent state (stats, config)
- @handler(): Routes session operations (create, get, refresh, delete, stats)
- `KeyValue` helper class: Convenient KV operations with auto JSON encoding/decoding
- host.kv.get() / host.kv.put() / host.kv.delete() / host.kv.list(): KV capability (raw functions)
- host.ask(): Inter-actor communication (simulates HTTP capability)
- host.now_ms(): Timestamp for TTL checks
- host.send_after(): Timer capability (periodic cleanup)
"""

import json
import time
from typing import Dict, Any, Optional, List
from plexspaces import actor, state, handler, init_handler, host

# KV key prefixes
PREFIX_SESSION = "session:"
PREFIX_STATS = "stats:"

# Default session TTL (seconds)
DEFAULT_SESSION_TTL = 3600  # 1 hour


class KeyValue:
    """
    Helper class for KeyValue operations (wasmCloud-style capability).
    
    Provides convenient methods for KV operations with automatic JSON encoding/decoding.
    """
    
    def __init__(self):
        """Initialize KeyValue helper."""
        pass
    
    def get(self, key: str, default: Optional[str] = None) -> Optional[str]:
        """
        Get value for key.
        
        Args:
            key: Key to retrieve
            default: Default value if key not found
        
        Returns:
            Value string or default if not found
        """
        value = host.kv.get(key)
        return value if value else default
    
    def get_json(self, key: str, default: Optional[Dict[str, Any]] = None) -> Optional[Dict[str, Any]]:
        """
        Get JSON value for key (auto-deserialize).
        
        Args:
            key: Key to retrieve
            default: Default value if key not found or invalid JSON
        
        Returns:
            Deserialized JSON dict or default
        """
        value = host.kv.get(key)
        if not value:
            return default
        try:
            return json.loads(value)
        except (json.JSONDecodeError, ValueError):
            return default
    
    def put(self, key: str, value: str) -> bool:
        """
        Store value for key.
        
        Args:
            key: Key to store
            value: Value string
        
        Returns:
            True on success, False on failure
        """
        result = host.kv.put(key, value)
        return not result.startswith("ERROR:")
    
    def put_json(self, key: str, value: Dict[str, Any]) -> bool:
        """
        Store JSON value for key (auto-serialize).
        
        Args:
            key: Key to store
            value: Value dict to serialize
        
        Returns:
            True on success, False on failure
        """
        try:
            value_str = json.dumps(value)
            return self.put(key, value_str)
        except (TypeError, ValueError):
            return False
    
    def delete(self, key: str) -> bool:
        """
        Delete key.
        
        Args:
            key: Key to delete
        
        Returns:
            True on success, False on failure
        """
        result = host.kv.delete(key)
        return not result.startswith("ERROR:")
    
    def list(self, prefix: str) -> List[str]:
        """
        List keys with prefix.
        
        Args:
            prefix: Key prefix to match
        
        Returns:
            List of matching keys
        """
        keys_json = host.kv.list(prefix)
        if not keys_json or keys_json.startswith("ERROR:"):
            return []
        try:
            return json.loads(keys_json)
        except (json.JSONDecodeError, ValueError):
            return []


@actor
class SessionStore:
    """
    Distributed Session Store Service (wasmCloud-style capability-based design).
    
    Capabilities used:
    - KeyValue: Store sessions in distributed KV
    - Timer: Periodic cleanup of expired sessions
    - Inter-actor communication: Validate sessions via host.ask() (simulates HTTP)
    """

    # Statistics
    total_sessions_created: int = state(default=0)
    total_sessions_accessed: int = state(default=0)
    total_sessions_refreshed: int = state(default=0)
    total_sessions_deleted: int = state(default=0)
    total_sessions_expired: int = state(default=0)
    total_validations: int = state(default=0)
    total_validation_failures: int = state(default=0)

    # Configuration
    default_ttl_sec: int = state(default=DEFAULT_SESSION_TTL)
    cleanup_interval_sec: int = state(default=60)
    auth_service_actor: str = state(default="")  # Actor ID for auth validation

    # Cleanup timer ID (tracked for cancellation)
    cleanup_timer_id: str = state(default="")
    
    # KeyValue helper instance (wasmCloud-style capability helper)
    _kv: Optional[KeyValue] = None

    @init_handler
    def on_init(self, config: dict):
        """Initialize session store from framework config."""
        actor_id = config.get("actor_id", "")
        args = config.get("args", {})
        
        self.default_ttl_sec = int(args.get("default_ttl_sec", DEFAULT_SESSION_TTL))
        self.cleanup_interval_sec = int(args.get("cleanup_interval_sec", 60))
        self.auth_service_actor = args.get("auth_service_actor", "")
        
        # Initialize KeyValue helper (wasmCloud-style capability helper)
        self._kv = KeyValue()
        
        host.info(f"SessionStore initialized: ttl={self.default_ttl_sec}s, "
                  f"cleanup_interval={self.cleanup_interval_sec}s, "
                  f"auth_service={self.auth_service_actor}")
        
        # Note: Timer-based cleanup disabled due to self-messaging restriction
        # Use manual cleanup_expired handler instead (call via HTTP API)
        # Timer capability can be enabled when framework supports self-messaging for timers


    def _validate_user(self, user_id: str) -> bool:
        """
        Validate user via inter-actor communication (simulates HTTP capability).
        
        In wasmCloud, this would use HTTP capability to call external auth service.
        Here we simulate it via host.ask() to another actor.
        """
        if not self.auth_service_actor:
            # No auth service configured, allow all
            return True
        
        try:
            self.total_validations += 1
            payload = json.dumps({"user_id": user_id})
            # Prevent self-ask (cannot ask ourselves)
            if self.auth_service_actor == host.self_id():
                host.warn(f"Cannot ask self for validation: {self.auth_service_actor}")
                return False
            
            response_json = host.ask(
                self.auth_service_actor,
                "validate_user",
                payload,
                5000  # 5s timeout
            )
            response = json.loads(response_json) if response_json else {}
            valid = response.get("valid", False)
            
            if not valid:
                self.total_validation_failures += 1
            
            return valid
        except Exception as e:
            host.warn(f"User validation failed for {user_id}: {e}")
            self.total_validation_failures += 1
            return False

    @handler("create")
    def create_session(
        self,
        user_id: str = "",
        ttl_sec: int = 0,
        metadata: dict = None
    ) -> dict:
        """
        Create a new session (wasmCloud KeyValue capability).
        
        Args:
            user_id: User identifier
            ttl_sec: Session TTL in seconds (0 = use default)
            metadata: Optional session metadata
        
        Returns:
            Session ID and expiration timestamp
        """
        if not user_id:
            return {"error": "user_id required"}
        
        # Validate user (simulates HTTP capability via inter-actor call)
        if not self._validate_user(user_id):
            return {"error": "user validation failed"}
        
        ttl = ttl_sec if ttl_sec > 0 else self.default_ttl_sec
        now_ms = host.now_ms()
        expires_at_ms = now_ms + (ttl * 1000)
        
        # Generate session ID (simple ULID-like format)
        session_id = f"sess-{now_ms}-{user_id[:8]}"
        session_key = f"{PREFIX_SESSION}{session_id}"
        
        session_data = {
            "session_id": session_id,
            "user_id": user_id,
            "created_at_ms": now_ms,
            "expires_at_ms": expires_at_ms,
            "ttl_sec": ttl,
            "last_access_ms": now_ms,
            "metadata": metadata or {}
        }
        
        # Store in KV (wasmCloud KeyValue capability) - using helper class
        if not self._kv:
            self._kv = KeyValue()
        if not self._kv.put_json(session_key, session_data):
            return {"error": "failed to store session"}
        
        self.total_sessions_created += 1
        
        return {
            "status": "ok",
            "session_id": session_id,
            "expires_at_ms": expires_at_ms,
            "ttl_sec": ttl
        }

    @handler("get")
    def get_session(self, session_id: str = "") -> dict:
        """
        Get session data (wasmCloud KeyValue capability).
        
        Args:
            session_id: Session identifier
        
        Returns:
            Session data or error if not found/expired
        """
        if not session_id:
            return {"error": "session_id required"}
        
        session_key = f"{PREFIX_SESSION}{session_id}"
        
        # Use KeyValue helper for JSON operations
        if not self._kv:
            self._kv = KeyValue()
        session_data = self._kv.get_json(session_key)
        
        if not session_data:
            return {"error": "session not found"}
        
        # Check expiration
        now_ms = host.now_ms()
        expires_at_ms = session_data.get("expires_at_ms", 0)
        
        if now_ms >= expires_at_ms:
            # Session expired, delete it
            self._kv.delete(session_key)
            self.total_sessions_expired += 1
            return {"error": "session expired"}
        
        # Update last access time using KeyValue helper
        session_data["last_access_ms"] = now_ms
        self._kv.put_json(session_key, session_data)
        
        self.total_sessions_accessed += 1
        
        return {
            "status": "ok",
            "session": session_data
        }

    @handler("refresh")
    def refresh_session(
        self,
        session_id: str = "",
        extend_ttl_sec: int = 0
    ) -> dict:
        """
        Refresh session (extend TTL) (wasmCloud KeyValue capability).
        
        Args:
            session_id: Session identifier
            extend_ttl_sec: Additional TTL in seconds (0 = use default)
        
        Returns:
            Updated expiration timestamp
        """
        if not session_id:
            return {"error": "session_id required"}
        
        session_key = f"{PREFIX_SESSION}{session_id}"
        
        # Use KeyValue helper for JSON operations
        if not self._kv:
            self._kv = KeyValue()
        session_data = self._kv.get_json(session_key)
        
        if not session_data:
            return {"error": "session not found"}
        
        # Check expiration
        now_ms = host.now_ms()
        expires_at_ms = session_data.get("expires_at_ms", 0)
        
        if now_ms >= expires_at_ms:
            self._kv.delete(session_key)
            self.total_sessions_expired += 1
            return {"error": "session expired"}
        
        # Extend TTL
        extend_ttl = extend_ttl_sec if extend_ttl_sec > 0 else self.default_ttl_sec
        new_expires_at_ms = max(expires_at_ms, now_ms) + (extend_ttl * 1000)
        session_data["expires_at_ms"] = new_expires_at_ms
        session_data["last_access_ms"] = now_ms
        
        if not self._kv.put_json(session_key, session_data):
            return {"error": "failed to update session"}
        
        self.total_sessions_refreshed += 1
        
        return {
            "status": "ok",
            "expires_at_ms": new_expires_at_ms,
            "ttl_sec": extend_ttl
        }

    @handler("delete")
    def delete_session(self, session_id: str = "") -> dict:
        """
        Delete a session (wasmCloud KeyValue capability).
        
        Args:
            session_id: Session identifier
        
        Returns:
            Success status
        """
        if not session_id:
            return {"error": "session_id required"}
        
        session_key = f"{PREFIX_SESSION}{session_id}"
        
        if not self._kv:
            self._kv = KeyValue()
        if self._kv.delete(session_key):
            self.total_sessions_deleted += 1
            return {"status": "ok", "deleted": True}
        else:
            return {"status": "ok", "deleted": False}  # Already deleted or not found

    @handler("cleanup_expired")
    def cleanup_expired(self) -> dict:
        """
        Cleanup expired sessions (wasmCloud Timer capability).
        
        This handler is called periodically by the timer to remove expired sessions.
        """
        now_ms = host.now_ms()
        expired_count = 0
        checked_count = 0
        
        # List all session keys using KeyValue helper
        if not self._kv:
            self._kv = KeyValue()
        keys = self._kv.list(PREFIX_SESSION)
        
        for key in keys:
            checked_count += 1
            session_data = self._kv.get_json(key)
            
            if not session_data:
                continue
            
            expires_at_ms = session_data.get("expires_at_ms", 0)
            
            if now_ms >= expires_at_ms:
                self._kv.delete(key)
                expired_count += 1
                self.total_sessions_expired += 1
        
        # Note: Timer rescheduling disabled - use manual trigger via HTTP API
        # To enable periodic cleanup, call cleanup_expired handler externally
        
        host.info(f"Cleanup: checked {checked_count} sessions, expired {expired_count}")
        
        return {
            "status": "ok",
            "checked": checked_count,
            "expired": expired_count
        }

    @handler("stats")
    def stats(self) -> dict:
        """
        Get session store statistics and benchmarks.
        
        Returns:
            Statistics including coordination vs computation metrics
        """
        # Count active sessions using KeyValue helper
        if not self._kv:
            self._kv = KeyValue()
        keys = self._kv.list(PREFIX_SESSION)
        active_sessions = len(keys)
        
        return {
            "status": "ok",
            "sessions": {
                "active": active_sessions,
                "total_created": self.total_sessions_created,
                "total_accessed": self.total_sessions_accessed,
                "total_refreshed": self.total_sessions_refreshed,
                "total_deleted": self.total_sessions_deleted,
                "total_expired": self.total_sessions_expired
            },
            "validation": {
                "total_validations": self.total_validations,
                "total_failures": self.total_validation_failures,
                "success_rate": (
                    (self.total_validations - self.total_validation_failures) / self.total_validations * 100
                    if self.total_validations > 0 else 100.0
                )
            },
            "config": {
                "default_ttl_sec": self.default_ttl_sec,
                "cleanup_interval_sec": self.cleanup_interval_sec,
                "auth_service": self.auth_service_actor
            }
        }
