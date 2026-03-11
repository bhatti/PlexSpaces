# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 PlexSpaces Contributors
#
# Leader-worker client for multi-node: same API surface as Rust/TS/Go SDKs.
# Virtual actors are created lazily on first message; use spawn_actor_on_node
# only for non-virtual workers.

from __future__ import annotations

import base64
import json
import re
import urllib.error
import urllib.parse
import urllib.request
from typing import Any, Dict, List, Optional


def _grpc_port_to_http(url: str) -> str:
    """Derive HTTP URL from gRPC node_address (convention: HTTP = gRPC port + 1)."""
    if not url:
        return url
    # Match trailing :port (e.g. :8091 -> :8092)
    match = re.search(r":(\d+)\s*$", url.strip())
    if match:
        port = int(match.group(1)) + 1
        return re.sub(r":\d+\s*$", f":{port}", url.strip())
    return url


class LeaderWorkerClient:
    """
    Client for leader-worker multi-node patterns. Connect to the entry (leader) node
    via HTTP; list worker node IDs and spawn non-virtual actors on specific nodes.
    Virtual actors are created lazily on first message—no explicit spawn or ensure.
    """

    def __init__(self, entry_http_url: str) -> None:
        """
        Args:
            entry_http_url: Base URL of the entry/leader node (e.g. "http://localhost:8092").
        """
        self._entry_url = entry_http_url.rstrip("/")
        self._node_id_to_http: Dict[str, str] = {}

    def list_worker_node_ids(
        self,
        cluster: Optional[str] = None,
        page_size: int = 100,
        page_token: str = "",
    ) -> List[str]:
        """
        List node IDs that can run workers (peers + self). Populates internal
        cache so spawn_actor_on_node can resolve node_id to node HTTP URL.

        Returns:
            List of node_id strings.
        """
        params: Dict[str, str] = {"pageSize": str(page_size)}
        if cluster:
            params["cluster"] = cluster
        if page_token:
            params["pageToken"] = page_token
        qs = urllib.parse.urlencode(params)
        url = f"{self._entry_url}/api/v1/nodes?{qs}"
        req = urllib.request.Request(url, method="GET")
        req.add_header("Accept", "application/json")
        try:
            with urllib.request.urlopen(req, timeout=30) as resp:
                data = json.loads(resp.read().decode())
        except urllib.error.HTTPError as e:
            body = e.read().decode() if e.fp else ""
            raise RuntimeError(f"list_worker_node_ids failed: {e.code} {body}") from e
        except urllib.error.URLError as e:
            raise RuntimeError(f"list_worker_node_ids failed: {e.reason}") from e

        nodes = data.get("nodes") or data.get("nodeRegistrations") or []
        ids: List[str] = []
        for n in nodes:
            node_id = n.get("nodeId") or n.get("node_id") or ""
            node_address = n.get("nodeAddress") or n.get("node_address") or ""
            if node_id:
                ids.append(node_id)
                if node_address:
                    self._node_id_to_http[node_id] = _grpc_port_to_http(node_address)
        return ids

    def spawn_actor_on_node(
        self,
        node_id: str,
        actor_type: str,
        actor_id: str = "",
        initial_state: bytes = b"",
        config: Optional[Dict[str, Any]] = None,
        labels: Optional[Dict[str, str]] = None,
    ) -> str:
        """
        Spawn a non-virtual actor on a specific node. The node must be reachable;
        call list_worker_node_ids first so the client can resolve node_id to URL.

        Returns:
            Actor ref string (e.g. "worker-ulid@node_id").
        """
        node_http = self._node_id_to_http.get(node_id)
        if not node_http:
            raise RuntimeError(
                f"unknown node_id {node_id!r}; call list_worker_node_ids first"
            )
        payload = {"actorType": actor_type}
        if actor_id:
            payload["actorId"] = actor_id
        if initial_state:
            payload["initialState"] = base64.b64encode(initial_state).decode()
        if config is not None:
            payload["config"] = config
        if labels is not None:
            payload["labels"] = labels
        body = json.dumps(payload).encode()
        url = f"{node_http.rstrip('/')}/api/v1/actors/spawn"
        req = urllib.request.Request(url, data=body, method="POST")
        req.add_header("Content-Type", "application/json")
        req.add_header("Accept", "application/json")
        try:
            with urllib.request.urlopen(req, timeout=30) as resp:
                out = json.loads(resp.read().decode())
        except urllib.error.HTTPError as e:
            body_str = e.read().decode() if e.fp else ""
            raise RuntimeError(f"spawn_actor_on_node failed: {e.code} {body_str}") from e
        except urllib.error.URLError as e:
            raise RuntimeError(f"spawn_actor_on_node failed: {e.reason}") from e

        actor_ref = out.get("actorRef") or out.get("actor_ref") or ""
        if not actor_ref:
            raise RuntimeError("spawn_actor_on_node returned empty actorRef")
        return actor_ref


def list_worker_node_ids(
    entry_http_url: str,
    cluster: Optional[str] = None,
    page_size: int = 100,
) -> List[str]:
    """
    Convenience: list worker node IDs using a one-off client.
    For multiple calls or spawn_actor_on_node, use LeaderWorkerClient.
    """
    client = LeaderWorkerClient(entry_http_url)
    return client.list_worker_node_ids(cluster=cluster, page_size=page_size)
