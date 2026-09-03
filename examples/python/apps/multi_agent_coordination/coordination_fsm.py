# SPDX-License-Identifier: AGPL-3.0-or-later
"""CoordinationFSM — state machine tracking the coordination workflow lifecycle.

Demonstrates: GenFSM with 9 states tracking the full pipeline progression.
"""

from plexspaces import fsm_actor, state, handler, init_handler, host

_VALID_TRANSITIONS = {
    "idle": {"decomposing", "failed"},
    "decomposing": {"researching", "failed"},
    "researching": {"analyzing", "failed"},
    "analyzing": {"verifying", "failed"},
    "verifying": {"voting", "failed"},
    "voting": {"synthesizing", "failed"},
    "synthesizing": {"complete", "failed"},
    "complete": {"idle"},
    "failed": {"idle"},
}


@fsm_actor(
    states=["idle", "decomposing", "researching", "analyzing", "verifying", "voting", "synthesizing", "complete", "failed"],
    initial="idle",
)
class CoordinationFSM:
    """GenFSM: tracks coordination workflow state transitions."""

    fsm_state: str = state(default="idle")
    transition_count: int = state(default=0)
    actor_id: str = state(default="")

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        host.info(f"CoordinationFSM init actor_id={self.actor_id}")

    @handler("transition")
    def transition(self, target_state: str = "") -> dict:
        allowed = _VALID_TRANSITIONS.get(self.fsm_state, set())
        if target_state not in allowed:
            return {
                "previous": self.fsm_state,
                "current": self.fsm_state,
                "valid": False,
                "error": f"invalid transition: {self.fsm_state} -> {target_state}",
            }
        prev = self.fsm_state
        self.fsm_state = target_state
        self.transition_count += 1
        host.debug(f"FSM: {prev} -> {target_state}")
        return {"previous": prev, "current": self.fsm_state, "valid": True}

    @handler("get_state")
    def get_state(self) -> dict:
        return {"current_state": self.fsm_state, "transitions_count": self.transition_count}

    @handler("reset")
    def reset(self) -> dict:
        prev = self.fsm_state
        self.fsm_state = "idle"
        self.transition_count = 0
        return {"previous": prev, "current": "idle", "reset": True}
