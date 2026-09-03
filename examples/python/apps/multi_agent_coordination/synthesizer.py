# SPDX-License-Identifier: AGPL-3.0-or-later
"""SynthesizerAgent — produces final reports respecting vetoes.

Demonstrates: Veto Protocol (check vetoes before inclusion), Pipeline (final stage).
"""

from plexspaces import actor, state, handler, init_handler, host
from .helpers import fire_audit


@actor
class SynthesizerAgent:
    """GenServer: reads analyses, filters out vetoed ones, produces final report."""

    syntheses_performed: int = state(default=0)
    actor_id: str = state(default="")

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        try:
            host.ts.write(["svc", "synthesizer", self.actor_id])
        except Exception:
            pass
        try:
            host.process_groups.join("coordination-events")
        except Exception:
            pass
        host.info(f"SynthesizerAgent init actor_id={self.actor_id}")

    @handler("synthesize")
    def synthesize(self, topic: str = "") -> dict:
        analyses = host.ts.read_all(["analysis", None, None, None, None])

        included = []
        vetoed_count = 0

        for a in analyses:
            if len(a) < 5:
                continue
            analysis_id = str(a[1])
            summary = str(a[3])
            severity = str(a[4])

            veto = host.ts.read(["veto", analysis_id, None, None])
            if veto:
                vetoed_count += 1
                continue

            included.append({
                "analysis_id": analysis_id,
                "summary": summary,
                "severity": severity,
            })

        all_vetoes = host.ts.read_all(["veto", None, None, None])
        if len(all_vetoes) > vetoed_count:
            vetoed_count = len(all_vetoes)

        if included:
            sections = []
            for item in included:
                sections.append(
                    f"[{item['severity'].upper()}] {item['analysis_id']}: {item['summary']}"
                )
            report = (
                f"Security Vulnerability Assessment Report\n"
                f"=========================================\n"
                f"Analyses included: {len(included)}, Vetoed: {vetoed_count}\n\n"
                + "\n\n".join(sections)
            )
        else:
            report = "No analyses available for synthesis."

        self.syntheses_performed += 1
        ts = host.now_ms()

        fire_audit("synthesis_completed", self.actor_id, {
            "included_count": len(included), "vetoed_count": vetoed_count,
        })

        return {
            "report": report,
            "included_count": len(included),
            "vetoed_count": vetoed_count,
            "timestamp": ts,
        }

    @handler("get_stats")
    def get_stats(self) -> dict:
        return {"syntheses_performed": self.syntheses_performed}
