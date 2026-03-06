# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 PlexSpaces Contributors
"""Integration tests for host.ts (TupleSpace list-in/list-out API)."""

import json
import pytest
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent))

from plexspaces.host import Host, _TupleSpaceHelper


class MockHostForTS:
    """Mock that records calls and returns configurable results for TupleSpace."""

    def __init__(self):
        self.write_calls = []
        self.take_returns = []  # list of string results (empty, "ERROR", or JSON tuple)
        self.read_returns = []
        self.read_all_returns = []  # list of string results (JSON array of tuples)

    def ts_write(self, tuple_json: str) -> str:
        self.write_calls.append(tuple_json)
        return ""

    def ts_take(self, pattern_json: str) -> str:
        if self.take_returns:
            return self.take_returns.pop(0)
        return ""

    def ts_read(self, pattern_json: str) -> str:
        if self.read_returns:
            return self.read_returns.pop(0)
        return ""

    def ts_read_all(self, pattern_json: str) -> str:
        if self.read_all_returns:
            return self.read_all_returns.pop(0)
        return "[]"


class TestTupleSpaceHelper:
    """Tests for _TupleSpaceHelper (host.ts)."""

    def test_write_serializes_list(self):
        mock = MockHostForTS()
        ts = _TupleSpaceHelper(mock)
        out = ts.write(["job", "j1", "task", "t0", 1])
        assert out == ""
        assert len(mock.write_calls) == 1
        parsed = json.loads(mock.write_calls[0])
        assert parsed == ["job", "j1", "task", "t0", 1]

    def test_take_returns_none_when_empty(self):
        mock = MockHostForTS()
        mock.take_returns = [""]
        ts = _TupleSpaceHelper(mock)
        result = ts.take(["job", "j1", "task", None, None])
        assert result is None

    def test_take_returns_none_on_error(self):
        mock = MockHostForTS()
        mock.take_returns = ["ERROR: timeout"]
        ts = _TupleSpaceHelper(mock)
        result = ts.take(["job", "j1", "task", None, None])
        assert result is None

    def test_take_returns_list_when_match(self):
        mock = MockHostForTS()
        mock.take_returns = [json.dumps(["job", "j1", "task", "t0", 42])]
        ts = _TupleSpaceHelper(mock)
        result = ts.take(["job", "j1", "task", None, None])
        assert result is not None
        assert result == ["job", "j1", "task", "t0", 42]

    def test_read_all_returns_empty_list_when_empty(self):
        mock = MockHostForTS()
        mock.read_all_returns = ["[]"]
        ts = _TupleSpaceHelper(mock)
        result = ts.read_all(["job", "j1", "result", None, None])
        assert result == []

    def test_read_all_returns_list_of_lists(self):
        mock = MockHostForTS()
        mock.read_all_returns = [json.dumps([["job", "j1", "result", "t0", {}], ["job", "j1", "result", "t1", {}]])]
        ts = _TupleSpaceHelper(mock)
        result = ts.read_all(["job", "j1", "result", None, None])
        assert len(result) == 2
        assert result[0] == ["job", "j1", "result", "t0", {}]
        assert result[1] == ["job", "j1", "result", "t1", {}]

    def test_read_returns_none_when_empty(self):
        mock = MockHostForTS()
        mock.read_returns = [""]
        ts = _TupleSpaceHelper(mock)
        result = ts.read(["job", None, None])
        assert result is None

    def test_read_returns_list_when_match(self):
        mock = MockHostForTS()
        mock.read_returns = [json.dumps(["job", "j1", "task", "t0", 1])]
        ts = _TupleSpaceHelper(mock)
        result = ts.read(["job", "j1", "task", None, None])
        assert result == ["job", "j1", "task", "t0", 1]
