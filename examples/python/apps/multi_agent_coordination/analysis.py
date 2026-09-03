# SPDX-License-Identifier: AGPL-3.0-or-later
"""AnalysisAgent — reads findings from blackboard and produces analyses.

Demonstrates: Blackboard (read findings), Pipeline (stage 2).
"""

import json
from plexspaces import actor, state, handler, init_handler, host
from .helpers import fire_audit


def _categorize_severity(findings: list) -> str:
    count = len(findings)
    if count >= 5:
        return "critical"
    if count >= 3:
        return "high"
    if count >= 2:
        return "medium"
    return "low"


@actor
class AnalysisAgent:
    """GenServer: reads findings from TupleSpace, cross-references, and produces severity analyses."""

    analyses_performed: int = state(default=0)
    actor_id: str = state(default="")

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        try:
            host.ts.write(["svc", "analysis", self.actor_id])
        except Exception:
            pass
        try:
            host.process_groups.join("coordination-events")
        except Exception:
            pass
        host.info(f"AnalysisAgent init actor_id={self.actor_id}")

    @handler("analyze")
    def analyze(self, topic: str = "") -> dict:
        findings = host.ts.read_all(["finding", None, None, None, None, None])
        if not findings:
            return {"analysis_id": "", "summary": "no findings available", "severity": "low", "finding_count": 0}

        finding_ids = []
        topics = []
        for f in findings:
            if len(f) >= 4:
                finding_ids.append(str(f[1]))
                topics.append(str(f[2]))

        severity = _categorize_severity(findings)
        unique_topics = list(set(topics))
        summary = (
            f"Cross-referenced {len(findings)} findings across {len(unique_topics)} topics: "
            f"{', '.join(unique_topics[:5])}. "
            f"Overall severity: {severity}."
        )

        analysis_id = f"analysis-{host.now_ms()}"
        finding_ids_json = json.dumps(finding_ids)
        host.ts.write(["analysis", analysis_id, finding_ids_json, summary, severity])
        self.analyses_performed += 1

        fire_audit("analysis_completed", self.actor_id, {
            "analysis_id": analysis_id, "finding_count": len(findings),
        })

        return {
            "analysis_id": analysis_id,
            "summary": summary,
            "severity": severity,
            "finding_count": len(findings),
        }

    @handler("get_stats")
    def get_stats(self) -> dict:
        return {
            "analyses_performed": self.analyses_performed,
            "categories": ["critical", "high", "medium", "low"],
        }
