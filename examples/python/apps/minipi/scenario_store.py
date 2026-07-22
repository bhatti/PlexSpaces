# SPDX-License-Identifier: AGPL-3.0-or-later
"""ScenarioStoreActor — persists eval scenario definitions.

Demonstrates: structured KV storage, scenario catalog, rubric management.
Each scenario has: input, expected output, rubric type, tags, difficulty.
"""

import json
from plexspaces import actor, state, init_handler, handler, host

# Built-in scenarios seeded at init time
_BUILTIN_SCENARIOS = [
    {
        "scenario_id": "sc-math-01",
        "name": "Simple multiplication",
        "input": "What is 6 * 7?",
        "expected": "42",
        "rubric": "task_completion",
        "tags": ["math"],
        "difficulty": "easy",
    },
    {
        "scenario_id": "sc-calc-01",
        "name": "Step-by-step arithmetic",
        "input": "Compute (17 * 24) + (89 - 45) step by step",
        "expected": "452",
        "rubric": "task_completion",
        "tags": ["math", "calculator"],
        "difficulty": "easy",
    },
    {
        "scenario_id": "sc-search-01",
        "name": "Web search intent",
        "input": "Search for information about the Pythagorean theorem",
        "expected": None,  # Open-ended — scored by rubric
        "rubric": "tool_use",
        "tags": ["search", "tool_use"],
        "difficulty": "medium",
    },
    {
        "scenario_id": "sc-reason-01",
        "name": "Logical deduction",
        "input": "If all Bloops are Razzies and all Razzies are Lazzies, are all Bloops definitely Lazzies?",
        "expected": "Yes",
        "rubric": "task_completion",
        "tags": ["reasoning"],
        "difficulty": "medium",
    },
    {
        "scenario_id": "sc-budget-01",
        "name": "Quadratic equation summary",
        "input": "Summarize the key steps to solve a quadratic equation ax^2 + bx + c = 0",
        "expected": None,
        "rubric": "task_completion",
        "tags": ["math", "summary"],
        "difficulty": "medium",
    },
    {
        "scenario_id": "sc-contract-01",
        "name": "Expression validation",
        "input": "Validate: is the expression '(2 + 3) * (4 - 1)' valid? What is its value?",
        "expected": "15",
        "rubric": "task_completion",
        "tags": ["validation", "math"],
        "difficulty": "easy",
    },
    {
        "scenario_id": "sc-multi-01",
        "name": "Multi-step tool use",
        "input": "Search for the capital of France, then compute 3 * 7, then report both results",
        "expected": None,
        "rubric": "tool_use",
        "tags": ["multi-step", "search", "tool_use"],
        "difficulty": "hard",
    },
    {
        "scenario_id": "sc-kv-01",
        "name": "KV store round-trip",
        "input": "Store the value 'hello world' under key 'test_key', then read it back and verify",
        "expected": None,
        "rubric": "tool_use",
        "tags": ["kv", "tool_use"],
        "difficulty": "medium",
    },
    {
        "scenario_id": "sc-chain-01",
        "name": "Chained computation",
        "input": "Compute sqrt(144), then add 5 to the result, then multiply by 2",
        "expected": "34",
        "rubric": "task_completion",
        "tags": ["math", "chain"],
        "difficulty": "medium",
    },
    {
        "scenario_id": "sc-compare-01",
        "name": "Power comparison",
        "input": "Which is larger: 2^10 or 10^3? Show your calculation",
        "expected": "2^10",
        "rubric": "task_completion",
        "tags": ["math", "comparison"],
        "difficulty": "easy",
    },
]


@actor
class ScenarioStoreActor:
    """
    Scenario catalog: stores, retrieves, and lists eval scenarios.

    Scenarios are persisted in KV storage keyed by scenario_id.
    Built-in scenarios are seeded at init time.

    A scenario contains:
    - input: what to tell the agent
    - expected: expected output (None for open-ended)
    - rubric: scoring method (task_completion, tool_use, efficiency, llm_judge)
    - tags: for filtering
    - difficulty: easy/medium/hard
    """

    actor_id: str = state(default="")
    scenario_count: int = state(default=0)

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        try:
            host.kv.put("svc:scenario_store", host.self_id())
        except Exception:
            pass
        try:
            host.registry.register(None, self.actor_id or host.self_id(), "actor", "",
                                   object_category="scenario_store")
        except Exception:
            pass
        host.info(f"ScenarioStoreActor init actor_id={self.actor_id}")
        self._seed_builtin_scenarios()

    @handler("get_scenario")
    def get_scenario(self, scenario_id: str = "") -> dict:
        """Get a single scenario by ID."""
        if not scenario_id:
            return {"error": "scenario_id is required"}
        raw = host.kv.get(f"scenario:{scenario_id}")
        if not raw:
            return {"error": f"scenario {scenario_id} not found"}
        try:
            return {"status": "ok", "scenario": json.loads(raw)}
        except Exception:
            return {"error": "failed to parse scenario"}

    @handler("list_scenarios")
    def list_scenarios(self, tags: list = None, difficulty: str = "", limit: int = 50) -> dict:
        """List scenarios, optionally filtered by tags or difficulty."""
        try:
            raw_keys = host.kv.list("scenario:")
            keys = json.loads(raw_keys) if raw_keys and not raw_keys.startswith("ERROR:") else []
            scenarios = []
            for key in keys[:limit * 2]:  # Oversample to account for filtering
                raw = host.kv.get(key)
                if not raw:
                    continue
                try:
                    sc = json.loads(raw)
                except Exception:
                    continue

                if difficulty and sc.get("difficulty") != difficulty:
                    continue
                if tags:
                    sc_tags = sc.get("tags", [])
                    if not any(t in sc_tags for t in tags):
                        continue

                scenarios.append(sc)
                if len(scenarios) >= limit:
                    break

            return {"status": "ok", "scenarios": scenarios, "count": len(scenarios)}
        except Exception as e:
            return {"error": str(e)}

    @handler("put_scenario")
    def put_scenario(self, scenario: dict = None) -> dict:
        """Store or update a scenario."""
        if not scenario:
            return {"error": "scenario is required"}
        scenario_id = scenario.get("scenario_id", "")
        if not scenario_id:
            scenario_id = host.new_id()
            scenario["scenario_id"] = scenario_id

        try:
            host.kv.put(f"scenario:{scenario_id}", json.dumps(scenario))
            self.scenario_count += 1
            host.incr_counter("scenarios_stored_total", 1)
            return {"status": "ok", "scenario_id": scenario_id}
        except Exception as e:
            return {"error": str(e)}

    @handler("get_suite")
    def get_suite(self, suite_name: str = "", scenario_ids: list = None) -> dict:
        """
        Get a named suite of scenarios.

        Built-in suites:
        - "smoke": 1 easy scenario
        - "standard": all easy + medium scenarios
        - "full": all scenarios

        Or pass explicit scenario_ids.
        """
        if scenario_ids:
            scenarios = []
            for sid in scenario_ids:
                raw = host.kv.get(f"scenario:{sid}")
                if raw:
                    try:
                        scenarios.append(json.loads(raw))
                    except Exception:
                        pass
            return {"status": "ok", "suite_name": suite_name, "scenarios": scenarios}

        # Named suites
        if suite_name == "smoke":
            ids = ["sc-math-01"]
        elif suite_name == "standard":
            ids = ["sc-math-01", "sc-calc-01", "sc-search-01", "sc-reason-01", "sc-budget-01"]
        elif suite_name == "full":
            ids = [s["scenario_id"] for s in _BUILTIN_SCENARIOS]
        else:
            # Try to load a stored suite definition
            raw = host.kv.get(f"suite:{suite_name}")
            if raw:
                try:
                    suite_def = json.loads(raw)
                    ids = suite_def.get("scenario_ids", [])
                except Exception:
                    ids = []
            else:
                return {"error": f"unknown suite: {suite_name}"}

        scenarios = []
        for sid in ids:
            raw = host.kv.get(f"scenario:{sid}")
            if raw:
                try:
                    scenarios.append(json.loads(raw))
                except Exception:
                    pass

        return {"status": "ok", "suite_name": suite_name, "scenarios": scenarios, "count": len(scenarios)}

    @handler("put_suite")
    def put_suite(self, suite_name: str = "", scenario_ids: list = None) -> dict:
        """Define a named suite."""
        if not suite_name or not scenario_ids:
            return {"error": "suite_name and scenario_ids are required"}
        try:
            host.kv.put(f"suite:{suite_name}", json.dumps({"scenario_ids": scenario_ids}))
            return {"status": "ok", "suite_name": suite_name, "count": len(scenario_ids)}
        except Exception as e:
            return {"error": str(e)}

    @handler("get_stats")
    def get_stats(self) -> dict:
        return {
            "status": "ok",
            "actor_id": self.actor_id,
            "scenario_count": self.scenario_count,
        }

    # ------------------------------------------------------------------

    def _seed_builtin_scenarios(self) -> None:
        """Seed built-in scenarios if they don't already exist."""
        seeded = 0
        for sc in _BUILTIN_SCENARIOS:
            key = f"scenario:{sc['scenario_id']}"
            existing = host.kv.get(key)
            if not existing:
                try:
                    host.kv.put(key, json.dumps(sc))
                    seeded += 1
                except Exception as e:
                    host.warn(f"Failed to seed scenario {sc['scenario_id']}: {e}")

        self.scenario_count = len(_BUILTIN_SCENARIOS)
        if seeded > 0:
            host.info(f"ScenarioStoreActor seeded {seeded} built-in scenarios")
