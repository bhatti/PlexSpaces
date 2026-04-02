# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 PlexSpaces Contributors
#
# Workflow retry helpers aligned with proto RetryConfig (plexspaces.workflow.v1.RetryConfig).
# Single run-with-config semantics: use default_retry_config() for one attempt;
# pass config with max_attempts > 1 for retries.

from typing import Any, Callable, Dict, List, Optional, TypeVar, Union

T = TypeVar("T")

# Use the proto-generated RetryConfig when available (native/server-side with betterproto).
# Falls back to a plain dataclass for WASM guest environments where generated/ is not bundled.
# The interface is identical in both cases: attribute access (cfg.max_attempts) works for
# both the betterproto dataclass and the fallback dataclass.
try:
    # betterproto generates plexspaces/workflow/v1/__init__.py with RetryConfig as a dataclass
    from plexspaces.generated.plexspaces.workflow.v1 import RetryConfig  # type: ignore[import]
except ImportError:
    from dataclasses import dataclass, field as _field

    @dataclass
    class RetryConfig:  # type: ignore[no-redef]
        """Fallback RetryConfig matching proto plexspaces.workflow.v1.RetryConfig fields."""
        max_attempts: int = 1
        initial_interval_ms: int = 100
        backoff_rate: float = 2.0
        max_interval_ms: int = 30000
        retryable_errors: List[str] = _field(default_factory=list)

# RetryConfigDict is a type alias kept for backward compatibility with callers that pass dicts.
# New code should use RetryConfig directly (attribute access instead of dict key access).
RetryConfigDict = Dict[str, Any]
RetryConfigInput = Union[RetryConfig, RetryConfigDict]


def _to_retry_config(config: Optional[RetryConfigInput]) -> Optional[RetryConfig]:
    """Normalize dict-or-proto input into the proto-shaped RetryConfig object."""
    if config is None:
        return None
    if isinstance(config, dict):
        return RetryConfig(
            max_attempts=config.get("max_attempts", 0),
            initial_interval_ms=config.get("initial_interval_ms", 0),
            backoff_rate=config.get("backoff_rate", 0.0),
            max_interval_ms=config.get("max_interval_ms", 0),
            retryable_errors=list(config.get("retryable_errors", [])),
        )
    return config


def default_retry_config() -> RetryConfig:
    """Default retry config as the canonical proto-shaped model."""
    return RetryConfig(
        max_attempts=1,
        initial_interval_ms=100,
        backoff_rate=2.0,
        max_interval_ms=30000,
        retryable_errors=[],
    )


def _effective_max_attempts(config: Optional[RetryConfig]) -> int:
    """Effective max attempts: 0 or unset means 1."""
    if not config:
        return 1
    n = getattr(config, "max_attempts", 0)
    return 1 if n == 0 else max(1, n)


def with_retry(
    fn: Callable[[], T],
    retry_config: Optional[RetryConfigInput] = None,
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
    normalized = _to_retry_config(retry_config)
    has_max = normalized is not None and getattr(normalized, "max_attempts", 0) != 0
    from_config = _effective_max_attempts(normalized) if has_max else 0
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
