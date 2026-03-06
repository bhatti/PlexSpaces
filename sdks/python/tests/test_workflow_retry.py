# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 PlexSpaces Contributors

"""Tests for workflow retry helpers (default_retry_config, with_retry). Align with proto RetryConfig."""

import pytest
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent))

from plexspaces.workflow import default_retry_config, with_retry


class TestDefaultRetryConfig:
    """default_retry_config() returns single-attempt config aligned with proto."""

    def test_returns_single_attempt(self):
        c = default_retry_config()
        assert c["max_attempts"] == 1
        assert c["initial_interval_ms"] == 100
        assert c["backoff_rate"] == 2.0
        assert c["max_interval_ms"] == 30000
        assert c["retryable_errors"] == []


class TestWithRetry:
    """with_retry(fn, retry_config) aligns with Rust run(name, retry, op) and proto."""

    def test_returns_result_on_first_success(self):
        assert with_retry(lambda: 42) == 42

    def test_succeeds_after_one_retry(self):
        calls = [0]

        def fn():
            calls[0] += 1
            if calls[0] < 2:
                raise ValueError("transient")
            return 100

        assert with_retry(fn, {"max_attempts": 3}) == 100
        assert calls[0] == 2

    def test_throws_after_max_attempts_exhausted(self):
        calls = [0]

        def fn():
            calls[0] += 1
            raise RuntimeError("permanent")

        with pytest.raises(RuntimeError, match="permanent"):
            with_retry(fn, {"max_attempts": 3})
        assert calls[0] == 3

    def test_default_three_attempts_when_config_omitted(self):
        calls = [0]

        def fn():
            calls[0] += 1
            raise ValueError("fail")

        with pytest.raises(ValueError, match="fail"):
            with_retry(fn)
        assert calls[0] == 3

    def test_max_attempts_one_throws_on_first_failure(self):
        calls = [0]

        def fn():
            calls[0] += 1
            raise ValueError("once")

        with pytest.raises(ValueError, match="once"):
            with_retry(fn, {"max_attempts": 1})
        assert calls[0] == 1
