# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2025 PlexSpaces Contributors
#
# Minimal two-class WASM fixture for multi-actor dispatch integration tests.
# Each class has a unique handler that returns a unique key so the test can
# assert which class actually handled the message.

from plexspaces import actor, handler, state


@actor
class PingActor:
    count: int = state(default=0)

    @handler("ping")
    def ping(self) -> dict:
        self.count += 1
        return {"pong": True, "count": self.count}


@actor
class EchoActor:
    last: str = state(default="")

    @handler("echo")
    def echo(self, message: str = "") -> dict:
        self.last = message
        return {"echoed": message}
