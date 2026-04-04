# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 PlexSpaces Contributors
#
# PlexSpaces Python SDK
#
# Provides a Pythonic interface for building PlexSpaces actors with minimal boilerplate.
# Inspired by Ray's @ray.remote and Temporal's @workflow.defn decorators.
#
# Example:
#     from plexspaces import actor, event_actor, state, handler
#
#     @event_actor  # GenEvent-style (fire-and-forget)
#     class SensorStream:
#         @handler("ingest")
#         def ingest(self, sensor_id: str, value: str) -> str: ...
#
#     @actor  # GenServer-style (request-reply)
#     class BankAccount:
#         balance: int = state(default=0)
#
#         @handler("deposit")
#         def deposit(self, amount: int) -> dict:
#             self.balance += amount
#             return {"balance": self.balance}
#
# Build with:
#     plexspaces-py build bank_account.py -o bank_account_actor.wasm

"""
PlexSpaces Python SDK - Build actors with minimal boilerplate.

This SDK provides:
- @actor: Decorator to define a PlexSpaces actor class
- @handler: Decorator to define message handlers
- state(): Define persistent state fields (auto-serialized)
- host: Access to PlexSpaces host functions (send, log, process_groups, etc.)
"""

__version__ = "0.1.0"

from .decorators import (
    actor,
    event_actor,
    gen_server_actor,
    fsm_actor,
    workflow_actor,
    handler,
    state,
    init_handler,
    run_handler,
    signal_handler,
    query_handler,
)
from .host import host, ServiceHttpClient
from .workflow import default_retry_config, with_retry
from .leader_worker import LeaderWorkerClient, list_worker_node_ids

__all__ = [
    "actor",
    "default_retry_config",
    "event_actor",
    "gen_server_actor",
    "fsm_actor",
    "workflow_actor",
    "handler",
    "state",
    "init_handler",
    "run_handler",
    "signal_handler",
    "query_handler",
    "host",
    "ServiceHttpClient",
    "with_retry",
    "LeaderWorkerClient",
    "list_worker_node_ids",
    "__version__",
]
