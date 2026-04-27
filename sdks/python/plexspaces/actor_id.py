# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2025 PlexSpaces Contributors
#
# ActorID — parsed representation of a canonical PlexSpaces actor ID.
#
# Canonical format: {name}//{actor_type}::{namespace}@{node_id}
#
# The name is typically a stable role name for supervisor-spawned actors,
# or a ULID for dynamically spawned actors.
#
# Usage — peer/sibling discovery from within a WASM actor:
#
#     self_id = ActorID.parse(host.self_id())
#     budget_id = self_id.sibling("budget_manager")
#     reply = host.ask(budget_id.to_str(), payload, 5000)

from __future__ import annotations
from dataclasses import dataclass


@dataclass
class ActorID:
    """Parsed form of a canonical PlexSpaces actor ID.

    Canonical string format: {name}//{actor_type}::{namespace}@{node_id}
    """

    name: str
    """Unique instance identifier (role name or ULID)."""

    actor_type: str
    """Behavior type registered in the application (e.g. 'budget_manager')."""

    namespace: str
    """Application namespace (e.g. 'go-resource-aware-inference')."""

    node_id: str
    """Node hosting the actor (e.g. 'test-node-8091'). May be empty."""

    @staticmethod
    def parse(id: str) -> "ActorID":
        """Parse a canonical actor ID string into an ActorID.

        Expected format: {name}//{actor_type}::{namespace}@{node_id}

        Raises ValueError if the string does not contain the expected separators.
        """
        slash_idx = id.find("//")
        if slash_idx < 0:
            raise ValueError(f"parse_actor_id: missing '//' in {id!r}")

        name = id[:slash_idx]
        rest = id[slash_idx + 2:]  # "{actor_type}::{namespace}@{node_id}"

        # Split on "@" to separate type::namespace from node_id
        at_parts = rest.split("@", 1)
        node_id = at_parts[1] if len(at_parts) == 2 else ""
        type_ns = at_parts[0]  # "{actor_type}::{namespace}"

        # Split on "::" to separate actor_type from namespace
        colon_parts = type_ns.split("::", 1)
        actor_type = colon_parts[0]
        namespace = colon_parts[1] if len(colon_parts) == 2 else ""

        return ActorID(name=name, actor_type=actor_type, namespace=namespace, node_id=node_id)

    def to_str(self) -> str:
        """Return the canonical actor ID string: {name}//{actor_type}::{namespace}@{node_id}."""
        if self.node_id:
            return f"{self.name}//{self.actor_type}::{self.namespace}@{self.node_id}"
        return f"{self.name}//{self.actor_type}::{self.namespace}"

    def __str__(self) -> str:
        return self.to_str()

    def sibling(self, name: str, actor_type: str = "") -> "ActorID":
        """Return a canonical ID for a same-application sibling actor.

        For supervisor-spawned actors where name == type (the common case)::

            peer = self_id.sibling("inference_worker_a", "inference_worker")
            # -> "inference_worker_a//inference_worker::namespace@node"

        If actor_type is omitted, name is used as the type::

            peer = self_id.sibling("pipeline_supervisor")
            # -> "pipeline_supervisor//pipeline_supervisor::namespace@node"
        """
        t = actor_type if actor_type else name
        return ActorID(name=name, actor_type=t, namespace=self.namespace, node_id=self.node_id)

    def with_type_and_name(self, actor_type: str, name: str) -> "ActorID":
        """Return a copy with an explicit actor_type and name.

        Use this to build a canonical ID for a peer actor with the given type and name,
        keeping the same namespace and node.

        For supervisor-spawned actors with stable role names (name == type == role)::

            peer = self.with_type_and_name("budget_manager", "budget_manager")

        For actors where name and type differ (e.g. ULID-named workers of a shared type)::

            peer = self.with_type_and_name("inference_worker", ulid)
        """
        return ActorID(name=name, actor_type=actor_type, namespace=self.namespace, node_id=self.node_id)

    def with_name(self, name: str) -> "ActorID":
        """Return a copy with a different name."""
        return ActorID(name=name, actor_type=self.actor_type, namespace=self.namespace, node_id=self.node_id)

    def with_type(self, name: str, actor_type: str) -> "ActorID":
        """Return a copy with a different name and actor_type."""
        return ActorID(name=name, actor_type=actor_type, namespace=self.namespace, node_id=self.node_id)
