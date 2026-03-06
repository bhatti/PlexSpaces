# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 PlexSpaces Contributors
#
# Workflow retry helpers aligned with proto RetryConfig (plexspaces.workflow.v1.RetryConfig).
# Single run-with-config semantics: use default_retry_config() for one attempt;
# pass config with max_attempts > 1 for retries.

from typing import Any, Callable, Dict, List, Optional, TypeVar

T = TypeVar("T")

# Align with proto RetryConfig
RetryConfigDict = Dict[str, Any]


def default_retry_config() -> RetryConfigDict:
    """Default retry config: single attempt (no retries). Aligns with proto when fields unset."""
    return {
        "max_attempts": 1,
        "initial_interval_ms": 100,
        "backoff_rate": 2.0,
        "max_interval_ms": 30000,
        "retryable_errors": [],
    }


def _effective_max_attempts(config: Optional[RetryConfigDict]) -> int:
    """Effective max attempts: 0 or unset means 1."""
    if not config or "max_attempts" not in config:
        return 1
    n = config["max_attempts"]
    return 1 if n == 0 else max(1, n)


def with_retry(
    fn: Callable[[], T],
    retry_config: Optional[RetryConfigDict] = None,
) -> T:
    """
    Execute a step with retries. Uses RetryConfig. When retry_config is omitted or
    max_attempts unset, 3 attempts. When max_attempts is 0 or 1, single attempt.
    Aligns with Rust run(name, retry, op) and proto RetryConfig.

    Args:
        fn: Callable with no args that returns a value or raises.
        retry_config: Optional dict with max_attempts, initial_interval_ms, etc.
            Omitted => 3 attempts; max_attempts: 1 => one attempt.

    Returns:
        The result of fn() after a successful attempt.

    Raises:
        The last exception if all attempts fail.
    """
    has_max = retry_config and "max_attempts" in retry_config
    from_config = _effective_max_attempts(retry_config) if has_max else 0
    max_attempts = from_config if from_config > 0 else 3
    last_error: Optional[Exception] = None
    for attempt in range(1, max_attempts + 1):
        try:
            return fn()
        except Exception as e:
            last_error = e
            if attempt == max_attempts:
                raise
    if last_error is not None:
        raise last_error
    raise RuntimeError("with_retry: no result and no error")
