# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2025 PlexSpaces Contributors
#
# WsThinClient — Python thin-node WebSocket client skeleton.
#
# TODO: Implement WsFrame protobuf encoding over the `websockets` library.
# Wire protocol: proto/plexspaces/v1/transport/websocket.proto
# Reference implementation: sdks/typescript/src/ws_thin_client.ts

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Callable, Optional


@dataclass
class WsThinClientOptions:
    """Options for connecting a thin node via WebSocket."""
    ws_url: str
    """WebSocket URL, e.g. "ws://localhost:8091/ws"."""
    jwt_token: Optional[str] = None
    """JWT Bearer token. Appended as ?token=<jwt> if present."""
    node_id: Optional[str] = None
    """ULID preferred. Server assigns one if omitted or collision detected."""
    tenant: str = "default"
    namespace: str = "default"


@dataclass
class ThinNodePingResult:
    """Resource hints from a PingResponse (proto fields 9–11 of PingResponse)."""
    node_id: str
    cpu_percent: float
    memory_available_mb: int
    available_cores: int


class WsThinClient:
    """Python thin-node WebSocket client skeleton.

    All methods raise ``NotImplementedError``.

    TODO: implement WsFrame protobuf encoding over the ``websockets`` library.
    Reference: sdks/typescript/src/ws_thin_client.ts
    Proto: proto/plexspaces/v1/transport/websocket.proto
    """

    def __init__(self, opts: WsThinClientOptions) -> None:
        raise NotImplementedError(
            "WsThinClient: implement WsFrame protobuf encoding over websockets library"
        )

    async def connect(self) -> str:
        """Open the WebSocket and complete the NodeRegistration handshake.

        Returns the server-assigned node_id.
        """
        ...

    async def tell(self, actor_id: str, msg_type: str, payload: dict[str, Any]) -> None:
        """Fire-and-forget tell to a canonical actor ID."""
        ...

    async def ask(
        self,
        actor_id: str,
        msg_type: str,
        payload: dict[str, Any],
        timeout_ms: int = 5000,
    ) -> dict[str, Any]:
        """Request-reply ask. Returns the response payload."""
        ...

    def on_message(self, handler: Callable[[str, str, Any], None]) -> None:
        """Register a handler for incoming tell frames addressed to this thin node."""
        ...

    def node_id(self) -> str:
        """The server-assigned node_id (available after connect())."""
        ...

    def local_actor_id(self, name: str, actor_type: str, namespace: Optional[str] = None) -> str:
        """Build a canonical actor ID on this thin node.

        Format: {name}//{type}::{namespace}@{nodeId}
        """
        ns = namespace or "default"
        return f"{name}//{actor_type}::{ns}@{self.node_id()}"

    async def ping_node(self, target_node_id: str) -> ThinNodePingResult:
        """Send a SWIM-compatible ping and return resource hints from the response."""
        ...

    async def heartbeat(self) -> None:
        """Send a heartbeat frame to keep the WS session alive."""
        ...

    async def disconnect(self) -> None:
        """Close the WebSocket connection."""
        ...
