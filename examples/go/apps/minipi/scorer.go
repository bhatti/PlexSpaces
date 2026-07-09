// SPDX-License-Identifier: AGPL-3.0-or-later
// ScorerActor — trajectory scoring for eval pipelines.
//
// Demonstrates: LLM-as-judge pattern, heuristic scoring, rubric evaluation.
package main

import (
	"encoding/json"
	"fmt"

	"github.com/bhatti/PlexSpaces/sdks/go/plexspaces"
)

// ScorerActor scores agent trajectories against rubrics.
//
// Supports two scoring modes:
// - heuristic: rule-based scoring (fast, deterministic)
// - llm_judge: LLM-as-judge (expensive, flexible) — uses LLMGateway
type ScorerActor struct {
	plexspaces.BaseActor
	ActorID      string `json:"actor_id"`
	TotalScored  int    `json:"total_scored"`
}

func NewScorerActor() plexspaces.Actor {
	a := &ScorerActor{}
	a.SetSelf(a)
	return a
}

func (s *ScorerActor) Init(configJSON string) string {
	var config struct {
		ActorID string `json:"actor_id"`
	}
	if err := json.Unmarshal([]byte(configJSON), &config); err != nil {
		return "ERROR: " + err.Error()
	}
	s.SetRuntimeMetadata(config.ActorID)
	s.ActorID = config.ActorID
	if err := host.PG().Join("svc:scorer"); err != nil {
		host.Warn(fmt.Sprintf("ScorerActor: failed to join svc:scorer: %v", err))
	}
	host.Info(fmt.Sprintf("ScorerActor Init actor_id=%s", config.ActorID))
	return ""
}

func (s *ScorerActor) Handle(fromActor, msgType, payloadJSON string) string {
	p := parsePayload(payloadJSON)
	op := stringVal(p, "op", msgType)
	switch op {
	case "score":
		return s.score(p)
	case "batch_score":
		return s.batchScore(p)
	case "get_stats":
		return marshal(map[string]any{"status": "ok", "total_scored": s.TotalScored})
	default:
		return marshal(map[string]any{"error": "unknown_op", "op": op})
	}
}

func (s *ScorerActor) score(p map[string]any) string {
	traj, _ := p["trajectory"].(map[string]any)
	if traj == nil {
		return marshal(map[string]any{"error": "trajectory is required", "score": 0.0})
	}
	// rubric can be a string or a map
	rubricType := "task_completion"
	var rubric map[string]any
	switch rv := p["rubric"].(type) {
	case string:
		rubricType = rv
		rubric = map[string]any{"type": rv}
	case map[string]any:
		rubric = rv
		rubricType = stringVal(rv, "type", "task_completion")
	}
	if rubric == nil {
		rubric = map[string]any{"type": "task_completion"}
	}

	var sc float64
	var detail string
	switch rubricType {
	case "task_completion":
		sc, detail = s.scoreTaskCompletion(traj, rubric)
	case "tool_use":
		sc, detail = s.scoreToolUse(traj, rubric)
	case "efficiency":
		sc, detail = s.scoreEfficiency(traj, rubric)
	case "llm_judge":
		sc, detail = s.scoreLLMJudge(traj, rubric)
	default:
		sc, detail = s.scoreTaskCompletion(traj, rubric)
	}

	s.TotalScored++
	s.IncrCounter(host, "trajectories_scored")

	return marshal(map[string]any{
		"status":        "ok",
		"trajectory_id": stringVal(traj, "trajectory_id", ""),
		"score":         roundFloat(sc, 3),
		"rubric_type":   rubricType,
		"detail":        detail,
	})
}

func (s *ScorerActor) batchScore(p map[string]any) string {
	trajsRaw := sliceVal(p, "trajectories")
	rubric, _ := p["rubric"].(map[string]any)

	results := make([]any, 0, len(trajsRaw))
	scores := make([]float64, 0, len(trajsRaw))
	for _, t := range trajsRaw {
		tMap, _ := t.(map[string]any)
		if tMap == nil {
			continue
		}
		resp := parsePayload(s.score(map[string]any{
			"trajectory": tMap,
			"rubric":     rubric,
		}))
		results = append(results, resp)
		scores = append(scores, float64Val(resp, "score", 0.0))
	}

	meanScore := 0.0
	passRate := 0.0
	if len(scores) > 0 {
		sum := 0.0
		passed := 0
		for _, sc := range scores {
			sum += sc
			if sc >= 0.8 {
				passed++
			}
		}
		meanScore = sum / float64(len(scores))
		passRate = float64(passed) / float64(len(scores))
	}

	return marshal(map[string]any{
		"status":     "ok",
		"scores":     results,
		"mean_score": roundFloat(meanScore, 3),
		"pass_rate":  roundFloat(passRate, 3),
	})
}

func (s *ScorerActor) scoreTaskCompletion(traj, rubric map[string]any) (float64, string) {
	outcome := stringVal(traj, "outcome", "")
	steps := sliceVal(traj, "steps")
	expectedKeywordsRaw := sliceVal(rubric, "expected_keywords")
	expectedKeywords := extractStringSliceFromAny(expectedKeywordsRaw)

	baseScore := 0.1
	switch outcome {
	case "success", "completed":
		baseScore = 0.7
	case "budget_exceeded":
		baseScore = 0.3
	case "suspended":
		baseScore = 0.5
	}

	// Bonus for completing in fewer steps
	maxSteps := intVal(rubric, "max_steps", 20)
	stepCount := len(steps)
	if stepCount > 0 && stepCount <= maxSteps/2 {
		baseScore = minFloat(1.0, baseScore+0.15)
	}

	// Check for expected keywords in outputs
	allOutputs, _ := json.Marshal(steps)
	allOutputsStr := string(allOutputs)
	keywordMatches := 0
	for _, kw := range expectedKeywords {
		if containsAny(allOutputsStr, kw) {
			keywordMatches++
		}
	}
	if len(expectedKeywords) > 0 {
		keywordBonus := 0.15 * float64(keywordMatches) / float64(len(expectedKeywords))
		baseScore = minFloat(1.0, baseScore+keywordBonus)
	}

	detail := fmt.Sprintf("outcome=%s steps=%d keywords_matched=%d/%d", outcome, stepCount, keywordMatches, len(expectedKeywords))
	return baseScore, detail
}

func (s *ScorerActor) scoreToolUse(traj, rubric map[string]any) (float64, string) {
	steps := sliceVal(traj, "steps")
	expectedToolsRaw := sliceVal(rubric, "expected_tools")
	expectedTools := extractStringSliceFromAny(expectedToolsRaw)

	toolCalls := 0
	usedTools := map[string]bool{}
	for _, step := range steps {
		sm, _ := step.(map[string]any)
		if sm == nil {
			continue
		}
		if stringVal(sm, "kind", "") == "tool_call" {
			toolCalls++
			toolName := stringVal(sm, "tool_name", "")
			if toolName != "" {
				usedTools[toolName] = true
			}
		}
	}

	var score float64
	if len(expectedTools) == 0 {
		if toolCalls > 0 {
			score = 0.8
		} else {
			score = 0.4
		}
	} else {
		matches := 0
		for _, et := range expectedTools {
			if usedTools[et] {
				matches++
			}
		}
		score = float64(matches) / float64(len(expectedTools))
	}

	usedList := make([]string, 0, len(usedTools))
	for k := range usedTools {
		usedList = append(usedList, k)
	}
	detail := fmt.Sprintf("tool_calls=%d used_tools=%v expected=%v", toolCalls, usedList, expectedTools)
	return score, detail
}

func (s *ScorerActor) scoreEfficiency(traj, rubric map[string]any) (float64, string) {
	totalTokens := intVal(traj, "total_input_tokens", 0) + intVal(traj, "total_output_tokens", 0)
	budget := intVal(rubric, "token_budget", 4096)

	if totalTokens == 0 {
		return 0.5, "no token data"
	}
	efficiency := maxFloat(0.0, 1.0-float64(totalTokens)/float64(budget))
	outcome := stringVal(traj, "outcome", "")
	if outcome != "success" && outcome != "completed" {
		efficiency *= 0.5
	}
	detail := fmt.Sprintf("tokens=%d budget=%d outcome=%s", totalTokens, budget, outcome)
	return roundFloat(efficiency, 3), detail
}

func (s *ScorerActor) scoreLLMJudge(traj, rubric map[string]any) (float64, string) {
	llmID, err := registryFirst("llm_gateway", "svc:llm_gateway", "completion")
	if err != nil || llmID == "" {
		return s.scoreTaskCompletion(traj, rubric)
	}

	criteria := stringVal(rubric, "criteria", "Did the agent successfully complete the task?")
	trajSummary := map[string]any{
		"outcome":      stringVal(traj, "outcome", ""),
		"step_count":   len(sliceVal(traj, "steps")),
		"total_tokens": intVal(traj, "total_input_tokens", 0) + intVal(traj, "total_output_tokens", 0),
	}
	summaryJSON, _ := json.Marshal(trajSummary)

	prompt := fmt.Sprintf(`Rate this agent trajectory on a scale of 0.0 to 1.0.

Criteria: %s

Trajectory summary: %s

Respond with ONLY a JSON object: {"score": 0.0-1.0, "reasoning": "brief explanation"}`,
		criteria, string(summaryJSON))

	resp, askErr := askActor(llmID, "completion", map[string]any{
		"op":       "completion",
		"messages": []any{map[string]any{"role": "user", "content": prompt}},
	}, 15000)
	if askErr == nil && len(resp) > 0 {
		response, _ := resp["response"].(map[string]any)
		if response != nil {
			content := stringVal(response, "content", "")
			if content != "" {
				var parsed map[string]any
				if err2 := json.Unmarshal([]byte(content), &parsed); err2 == nil {
					return float64Val(parsed, "score", 0.5), stringVal(parsed, "reasoning", "")
				}
			}
		}
	}
	return s.scoreTaskCompletion(traj, rubric)
}

func minFloat(a, b float64) float64 {
	if a < b {
		return a
	}
	return b
}

func maxFloat(a, b float64) float64 {
	if a > b {
		return a
	}
	return b
}
