# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 PlexSpaces Contributors

"""Tests for ActorID — canonical PlexSpaces actor ID parser."""

import pytest
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent))

from plexspaces.actor_id import ActorID


class TestParseActorIDFull:
    def test_all_fields(self):
        id_str = "01KP8WMBRKP6KGQTARATQQ1H5M//agent_registry::go-a2a-multi-agent@test-node-8091"
        a = ActorID.parse(id_str)
        assert a.name == "01KP8WMBRKP6KGQTARATQQ1H5M"
        assert a.actor_type == "agent_registry"
        assert a.namespace == "go-a2a-multi-agent"
        assert a.node_id == "test-node-8091"
        assert a.to_str() == id_str

    def test_roundtrip(self):
        id_str = "01KP8WMBRKP6KGQTARATQQ1H5M//agent_registry::go-a2a-multi-agent@test-node-8091"
        assert str(ActorID.parse(id_str)) == id_str


class TestParseActorIDNoNode:
    def test_no_node(self):
        id_str = "myname//mytype::mynamespace"
        a = ActorID.parse(id_str)
        assert a.name == "myname"
        assert a.actor_type == "mytype"
        assert a.namespace == "mynamespace"
        assert a.node_id == ""
        assert a.to_str() == id_str


class TestParseActorIDMissingSlashes:
    def test_raises(self):
        with pytest.raises(ValueError, match="missing '//'"):
            ActorID.parse("noslashes")


class TestActorIDWithTypeAndName:
    def test_same_type_and_name(self):
        self_id = ActorID.parse("01KP//routing_workflow::go-resource-aware-inference@test-node-8091")
        peer = self_id.with_type_and_name("budget_manager", "budget_manager")
        assert peer.to_str() == "budget_manager//budget_manager::go-resource-aware-inference@test-node-8091"

    def test_different_type_and_name(self):
        self_id = ActorID.parse("01KP//routing_workflow::go-resource-aware-inference@test-node-8091")
        peer = self_id.with_type_and_name("inference_worker", "01KP8WORKER1")
        assert peer.to_str() == "01KP8WORKER1//inference_worker::go-resource-aware-inference@test-node-8091"

    def test_preserves_namespace_and_node(self):
        self_id = ActorID.parse("01KP//routing_workflow::my-ns@my-node")
        peer = self_id.with_type_and_name("analysis_agent", "analysis_agent")
        assert peer.namespace == "my-ns"
        assert peer.node_id == "my-node"
        assert peer.name == "analysis_agent"
        assert peer.actor_type == "analysis_agent"


class TestActorIDWithName:
    def test_with_name(self):
        self_id = ActorID.parse("01KP//worker::ns@node")
        got = self_id.with_name("newname").to_str()
        assert got == "newname//worker::ns@node"


class TestActorIDWithType:
    def test_with_type(self):
        self_id = ActorID.parse("01KP//worker::ns@node")
        got = self_id.with_type("other", "other_type").to_str()
        assert got == "other//other_type::ns@node"
