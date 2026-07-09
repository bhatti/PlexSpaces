// SPDX-License-Identifier: AGPL-3.0-or-later
// ScenarioStoreActor — persists eval scenario definitions.
//
// Demonstrates: structured KV storage, scenario catalog, rubric management.
// Each scenario has: input, expected output, rubric type, tags, difficulty.
package main

import (
	"encoding/json"
	"fmt"
	"strings"

	"github.com/bhatti/PlexSpaces/sdks/go/plexspaces"
)

// builtinScenarios are seeded at init time.
var builtinScenarios = []map[string]any{
	{
		"scenario_id": "sc-math-01",
		"name":        "Simple multiplication",
		"input":       "What is 6 * 7?",
		"expected":    "42",
		"rubric":      "task_completion",
		"tags":        []any{"math"},
		"difficulty":  "easy",
	},
	{
		"scenario_id": "sc-calc-01",
		"name":        "Step-by-step arithmetic",
		"input":       "Compute (17 * 24) + (89 - 45) step by step",
		"expected":    "452",
		"rubric":      "task_completion",
		"tags":        []any{"math", "calculator"},
		"difficulty":  "easy",
	},
	{
		"scenario_id": "sc-search-01",
		"name":        "Web search intent",
		"input":       "Search for information about the Pythagorean theorem",
		"expected":    nil,
		"rubric":      "tool_use",
		"tags":        []any{"search", "tool_use"},
		"difficulty":  "medium",
	},
	{
		"scenario_id": "sc-reason-01",
		"name":        "Logical deduction",
		"input":       "If all Bloops are Razzies and all Razzies are Lazzies, are all Bloops definitely Lazzies?",
		"expected":    "Yes",
		"rubric":      "task_completion",
		"tags":        []any{"reasoning"},
		"difficulty":  "medium",
	},
	{
		"scenario_id": "sc-budget-01",
		"name":        "Quadratic equation summary",
		"input":       "Summarize the key steps to solve a quadratic equation ax^2 + bx + c = 0",
		"expected":    nil,
		"rubric":      "task_completion",
		"tags":        []any{"math", "summary"},
		"difficulty":  "medium",
	},
	{
		"scenario_id": "sc-contract-01",
		"name":        "Expression validation",
		"input":       "Validate: is the expression '(2 + 3) * (4 - 1)' valid? What is its value?",
		"expected":    "15",
		"rubric":      "task_completion",
		"tags":        []any{"validation", "math"},
		"difficulty":  "easy",
	},
	{
		"scenario_id": "sc-multi-01",
		"name":        "Multi-step tool use",
		"input":       "Search for the capital of France, then compute 3 * 7, then report both results",
		"expected":    nil,
		"rubric":      "tool_use",
		"tags":        []any{"multi-step", "search", "tool_use"},
		"difficulty":  "hard",
	},
	{
		"scenario_id": "sc-kv-01",
		"name":        "KV store round-trip",
		"input":       "Store the value 'hello world' under key 'test_key', then read it back and verify",
		"expected":    nil,
		"rubric":      "tool_use",
		"tags":        []any{"kv", "tool_use"},
		"difficulty":  "medium",
	},
	{
		"scenario_id": "sc-chain-01",
		"name":        "Chained computation",
		"input":       "Compute sqrt(144), then add 5 to the result, then multiply by 2",
		"expected":    "34",
		"rubric":      "task_completion",
		"tags":        []any{"math", "chain"},
		"difficulty":  "medium",
	},
	{
		"scenario_id": "sc-compare-01",
		"name":        "Power comparison",
		"input":       "Which is larger: 2^10 or 10^3? Show your calculation",
		"expected":    "2^10",
		"rubric":      "task_completion",
		"tags":        []any{"math", "comparison"},
		"difficulty":  "easy",
	},
}

// ScenarioStoreActor stores, retrieves, and lists eval scenarios.
type ScenarioStoreActor struct {
	plexspaces.BaseActor
	ActorID       string `json:"actor_id"`
	ScenarioCount int    `json:"scenario_count"`
}

func NewScenarioStoreActor() plexspaces.Actor {
	a := &ScenarioStoreActor{}
	a.SetSelf(a)
	return a
}

func (s *ScenarioStoreActor) Init(configJSON string) string {
	var config struct {
		ActorID string `json:"actor_id"`
	}
	if err := json.Unmarshal([]byte(configJSON), &config); err != nil {
		return "ERROR: " + err.Error()
	}
	s.SetRuntimeMetadata(config.ActorID)
	s.ActorID = config.ActorID
	if err := host.PG().Join("svc:scenario_store"); err != nil {
		host.Warn(fmt.Sprintf("ScenarioStoreActor: failed to join svc:scenario_store: %v", err))
	}
	s.seedBuiltinScenarios()
	host.Info(fmt.Sprintf("ScenarioStoreActor Init actor_id=%s", config.ActorID))
	return ""
}

func (s *ScenarioStoreActor) Handle(fromActor, msgType, payloadJSON string) string {
	p := parsePayload(payloadJSON)
	op := stringVal(p, "op", msgType)
	switch op {
	case "get_scenario":
		return s.getScenario(p)
	case "list_scenarios":
		return s.listScenarios(p)
	case "put_scenario":
		return s.putScenario(p)
	case "get_suite":
		return s.getSuite(p)
	case "put_suite":
		return s.putSuite(p)
	case "get_stats":
		return marshal(map[string]any{
			"status":         "ok",
			"actor_id":       s.ActorID,
			"scenario_count": s.ScenarioCount,
		})
	default:
		return marshal(map[string]any{"error": "unknown_op", "op": op})
	}
}

func (s *ScenarioStoreActor) getScenario(p map[string]any) string {
	scenarioID := stringVal(p, "scenario_id", "")
	if scenarioID == "" {
		return marshal(map[string]any{"error": "scenario_id is required"})
	}
	raw := host.KVGet("scenario:" + scenarioID)
	if raw == "" {
		return marshal(map[string]any{"error": fmt.Sprintf("scenario %s not found", scenarioID)})
	}
	var sc map[string]any
	if err := json.Unmarshal([]byte(raw), &sc); err != nil {
		return marshal(map[string]any{"error": "failed to parse scenario"})
	}
	return marshal(map[string]any{"status": "ok", "scenario": sc})
}

func (s *ScenarioStoreActor) listScenarios(p map[string]any) string {
	difficulty := stringVal(p, "difficulty", "")
	limit := intVal(p, "limit", 50)
	tagsRaw := sliceVal(p, "tags")
	tags := extractStringSliceFromAny(tagsRaw)

	// Load all known scenario IDs from our seeded scenarios
	scenarios := make([]any, 0)
	for _, sc := range builtinScenarios {
		if difficulty != "" && sc["difficulty"] != difficulty {
			continue
		}
		if len(tags) > 0 {
			scTags := extractStringSliceFromAny(sc["tags"].([]any))
			matched := false
			for _, t := range tags {
				for _, st := range scTags {
					if t == st {
						matched = true
						break
					}
				}
				if matched {
					break
				}
			}
			if !matched {
				continue
			}
		}
		scID := sc["scenario_id"].(string)
		raw := host.KVGet("scenario:" + scID)
		if raw != "" {
			var loaded map[string]any
			if err := json.Unmarshal([]byte(raw), &loaded); err == nil {
				scenarios = append(scenarios, loaded)
			}
		} else {
			scenarios = append(scenarios, sc)
		}
		if len(scenarios) >= limit {
			break
		}
	}
	return marshal(map[string]any{"status": "ok", "scenarios": scenarios, "count": len(scenarios)})
}

func (s *ScenarioStoreActor) putScenario(p map[string]any) string {
	scenario, _ := p["scenario"].(map[string]any)
	if scenario == nil {
		return marshal(map[string]any{"error": "scenario is required"})
	}
	scenarioID := stringVal(scenario, "scenario_id", "")
	if scenarioID == "" {
		scenarioID = fmt.Sprintf("sc-%d", host.NowMs())
		scenario["scenario_id"] = scenarioID
	}
	scJSON, _ := json.Marshal(scenario)
	host.KVPut("scenario:"+scenarioID, string(scJSON))
	s.ScenarioCount++
	s.IncrCounter(host, "scenarios_stored_total")
	return marshal(map[string]any{"status": "ok", "scenario_id": scenarioID})
}

func (s *ScenarioStoreActor) getSuite(p map[string]any) string {
	suiteName := stringVal(p, "suite_name", "")
	scenarioIDsRaw := sliceVal(p, "scenario_ids")
	scenarioIDs := extractStringSliceFromAny(scenarioIDsRaw)

	if len(scenarioIDs) > 0 {
		scenarios := s.loadScenariosByIDs(scenarioIDs)
		return marshal(map[string]any{"status": "ok", "suite_name": suiteName, "scenarios": scenarios, "count": len(scenarios)})
	}

	// Named suites
	var ids []string
	switch suiteName {
	case "smoke":
		ids = []string{"sc-math-01"}
	case "standard":
		ids = []string{"sc-math-01", "sc-calc-01", "sc-search-01", "sc-reason-01", "sc-budget-01"}
	case "full":
		for _, sc := range builtinScenarios {
			ids = append(ids, sc["scenario_id"].(string))
		}
	default:
		// Try stored suite definition
		raw := host.KVGet("suite:" + suiteName)
		if raw == "" {
			return marshal(map[string]any{"error": fmt.Sprintf("unknown suite: %s", suiteName)})
		}
		var suiteDef map[string]any
		if err := json.Unmarshal([]byte(raw), &suiteDef); err != nil {
			return marshal(map[string]any{"error": "failed to parse suite"})
		}
		idsRaw := sliceVal(suiteDef, "scenario_ids")
		ids = extractStringSliceFromAny(idsRaw)
	}

	scenarios := s.loadScenariosByIDs(ids)
	return marshal(map[string]any{"status": "ok", "suite_name": suiteName, "scenarios": scenarios, "count": len(scenarios)})
}

func (s *ScenarioStoreActor) loadScenariosByIDs(ids []string) []any {
	scenarios := make([]any, 0, len(ids))
	for _, id := range ids {
		raw := host.KVGet("scenario:" + id)
		if raw != "" {
			var sc map[string]any
			if err := json.Unmarshal([]byte(raw), &sc); err == nil {
				scenarios = append(scenarios, sc)
				continue
			}
		}
		// Fall back to builtin if not in KV
		for _, sc := range builtinScenarios {
			if sc["scenario_id"] == id {
				scenarios = append(scenarios, sc)
				break
			}
		}
	}
	return scenarios
}

func (s *ScenarioStoreActor) putSuite(p map[string]any) string {
	suiteName := stringVal(p, "suite_name", "")
	scenarioIDsRaw := sliceVal(p, "scenario_ids")
	if suiteName == "" || len(scenarioIDsRaw) == 0 {
		return marshal(map[string]any{"error": "suite_name and scenario_ids are required"})
	}
	ids := extractStringSliceFromAny(scenarioIDsRaw)
	suiteJSON, _ := json.Marshal(map[string]any{"scenario_ids": ids})
	host.KVPut("suite:"+suiteName, string(suiteJSON))
	return marshal(map[string]any{"status": "ok", "suite_name": suiteName, "count": len(ids)})
}

func (s *ScenarioStoreActor) seedBuiltinScenarios() {
	seeded := 0
	for _, sc := range builtinScenarios {
		id := sc["scenario_id"].(string)
		if existing := host.KVGet("scenario:" + id); existing == "" {
			scJSON, _ := json.Marshal(sc)
			host.KVPut("scenario:"+id, string(scJSON))
			seeded++
		}
	}
	s.ScenarioCount = len(builtinScenarios)
	if seeded > 0 {
		host.Info(fmt.Sprintf("ScenarioStoreActor seeded %d built-in scenarios (ids: %s)",
			seeded, strings.Join(func() []string {
				ids := make([]string, 0, len(builtinScenarios))
				for _, sc := range builtinScenarios {
					ids = append(ids, sc["scenario_id"].(string))
				}
				return ids
			}(), ", ")))
	}
}
