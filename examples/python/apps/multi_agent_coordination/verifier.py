# SPDX-License-Identifier: AGPL-3.0-or-later
"""VerifierAgent — validates analyses and casts votes.

Demonstrates: Generator-Verifier (validate), Voting (consensus),
Veto Protocol (block low-confidence findings).
"""

from plexspaces import actor, state, handler, init_handler, host
from .helpers import fire_audit


@actor
class VerifierAgent:
    """GenServer: verifies analyses, casts votes on proposals, issues vetoes."""

    verifications: int = state(default=0)
    vetoes_issued: int = state(default=0)
    votes_cast: int = state(default=0)
    actor_id: str = state(default="")

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        try:
            host.ts.write(["svc", "verifier", self.actor_id])
        except Exception:
            pass
        try:
            host.process_groups.join("coordination-events")
        except Exception:
            pass
        host.info(f"VerifierAgent init actor_id={self.actor_id}")

    @handler("verify")
    def verify(
        self,
        analysis_id: str = "",
        summary: str = "",
        severity: str = "medium",
        confidence: float = 0.5,
    ) -> dict:
        self.verifications += 1
        ts = host.now_ms()

        if confidence < 0.3:
            reason = f"Insufficient evidence (confidence={confidence})"
            host.ts.write(["veto", analysis_id, reason, ts])
            self.vetoes_issued += 1
            fire_audit("veto_issued", self.actor_id, {
                "analysis_id": analysis_id, "reason": reason,
            })
            return {
                "approved": False,
                "veto_issued": True,
                "feedback": reason,
                "analysis_id": analysis_id,
            }

        return {
            "approved": True,
            "veto_issued": False,
            "feedback": f"Verified: {severity} severity analysis accepted",
            "analysis_id": analysis_id,
        }

    @handler("vote")
    def vote(
        self,
        proposal_id: str = "",
        voter_id: str = "",
        analysis: dict = None,
    ) -> dict:
        analysis = analysis or {}
        severity = analysis.get("severity", "medium")
        ts = host.now_ms()

        if severity in ("critical", "high"):
            decision = "approve"
        elif severity == "medium":
            last_char = voter_id[-1] if voter_id else "0"
            decision = "approve" if last_char in ("1", "3", "5", "7", "9") else "reject"
        else:
            decision = "reject"

        host.ts.write(["vote", proposal_id, voter_id, decision, ts])
        self.votes_cast += 1

        fire_audit("vote_cast", self.actor_id, {
            "proposal_id": proposal_id, "voter_id": voter_id, "decision": decision,
        })

        return {"voter_id": voter_id, "decision": decision, "proposal_id": proposal_id}

    @handler("get_stats")
    def get_stats(self) -> dict:
        return {
            "verifications": self.verifications,
            "vetoes_issued": self.vetoes_issued,
            "votes_cast": self.votes_cast,
        }
