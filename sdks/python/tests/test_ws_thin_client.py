# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2025 PlexSpaces Contributors
#
# Unit tests for WsThinClient types and interface contracts.
#
# Full lifecycle tests (connect → register → ping → disconnect → unregistered)
# require a live server and are covered by the Rust integration tests in
# crates/node/tests/suite/ws_integration_tests.rs.

import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).parent.parent))

from plexspaces.ws_thin_client import ThinNodePingResult, WsThinClient, WsThinClientOptions


class TestWsThinClientOptions:
    def test_fields_populated(self):
        opts = WsThinClientOptions(
            ws_url="ws://localhost:8091/ws",
            jwt_token="test.jwt.token",
            node_id="thin-node-py-01",
            tenant="default",
            namespace="test-ns",
        )
        assert opts.ws_url == "ws://localhost:8091/ws"
        assert opts.jwt_token == "test.jwt.token"
        assert opts.node_id == "thin-node-py-01"
        assert opts.tenant == "default"
        assert opts.namespace == "test-ns"

    def test_defaults(self):
        opts = WsThinClientOptions(ws_url="ws://localhost:8091/ws")
        assert opts.jwt_token is None
        assert opts.node_id is None
        assert opts.tenant == "default"
        assert opts.namespace == "default"


class TestThinNodePingResult:
    def test_fields_populated(self):
        result = ThinNodePingResult(
            node_id="server-node-1",
            cpu_percent=23.5,
            memory_available_mb=4096,
            available_cores=8,
        )
        assert result.node_id == "server-node-1"
        assert result.cpu_percent == pytest.approx(23.5)
        assert result.memory_available_mb == 4096
        assert result.available_cores == 8

    def test_zero_resource_hints(self):
        result = ThinNodePingResult(
            node_id="node-with-no-resources",
            cpu_percent=0.0,
            memory_available_mb=0,
            available_cores=0,
        )
        assert result.cpu_percent == 0.0
        assert result.memory_available_mb == 0
        assert result.available_cores == 0


class TestWsThinClientStub:
    """Verify that the stub raises NotImplementedError on construction
    (full implementation deferred — tests document expected future API shape)."""

    def test_constructor_raises_not_implemented(self):
        with pytest.raises(NotImplementedError):
            WsThinClient(WsThinClientOptions(ws_url="ws://127.0.0.1:0/ws"))

    def test_constructor_error_message_mentions_websockets(self):
        with pytest.raises(NotImplementedError, match="websockets"):
            WsThinClient(WsThinClientOptions(ws_url="ws://127.0.0.1:0/ws"))
