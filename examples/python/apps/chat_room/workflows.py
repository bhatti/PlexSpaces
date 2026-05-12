# SPDX-License-Identifier: AGPL-3.0-or-later

"""Workflow actors for the chat example."""

from __future__ import annotations

from plexspaces import host, query_handler, run_handler, signal_handler, state, workflow_actor

from helpers import actor_application_id, actor_instance_name, safe_metrics_add


@workflow_actor(facets=["virtual_actor", "durability"])
class ModerationWorkflow:
    """Durable moderation review flow for flagged messages."""

    application_id: str = state(default="")
    report_id: str = state(default="")
    status: str = state(default="pending")
    message_id: str = state(default="")
    reason: str = state(default="")
    reporter_id: str = state(default="")
    resolution: str = state(default="")
    signals: list[str] = state(default_factory=list)

    @run_handler
    def start(
        self,
        report_id: str,
        message_id: str,
        reporter_id: str,
        reason: str,
    ) -> dict:
        if not self.application_id:
            self.application_id = actor_application_id(host.self_id())
        if not self.report_id:
            self.report_id = report_id or actor_instance_name(host.self_id())
        self.message_id = message_id
        self.reporter_id = reporter_id
        self.reason = reason
        self.status = "under_review"
        safe_metrics_add(self.application_id, {"chat_moderation_reports": 1})
        return {
            "report_id": self.report_id,
            "status": self.status,
            "message_id": self.message_id,
        }

    @signal_handler("review")
    def review(self, moderator_id: str, resolution: str) -> None:
        self.resolution = resolution
        self.status = "reviewed"
        self.signals = [*self.signals, f"review:{moderator_id}:{resolution}"]

    @signal_handler("close")
    def close(self, resolution: str = "dismissed") -> None:
        self.resolution = resolution
        self.status = "closed"
        self.signals = [*self.signals, f"close::{resolution}"]

    @query_handler("status")
    def current_status(self) -> dict:
        return {
            "report_id": self.report_id,
            "status": self.status,
            "message_id": self.message_id,
            "reporter_id": self.reporter_id,
            "reason": self.reason,
            "resolution": self.resolution,
            "signals": list(self.signals),
        }
