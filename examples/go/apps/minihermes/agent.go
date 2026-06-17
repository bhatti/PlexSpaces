// SPDX-License-Identifier: AGPL-3.0-or-later
// AgentActor — core self-improving agent loop (message → LLM → tool_use → skills → repeat).
// Demonstrates: Ask (LLM, tools, skills, memory), KV (session history),
// TupleSpace (context tracking), PG discovery, skill injection, cron processing.
package main

import (
	"encoding/json"
	"fmt"
	"strconv"
	"strings"

	"github.com/bhatti/PlexSpaces/sdks/go/plexspaces"
)

const (
	defaultMaxIterations = 8
	defaultTokenBudget   = 8192
	compressionThreshold = 0.75 // compress when history tokens exceed 75% of budget
)

// AgentActor is the core self-improving loop. On each turn it:
//  1. Loads session history from KV
//  2. Checks context budget — compresses if >75% full
//  3. Injects matching skills into the system prompt
//  4. Runs the LLM → tool loop (max N iterations)
//  5. Persists history and evaluates whether new skills should be learned
type AgentActor struct {
	plexspaces.BaseActor
	SystemPrompt  string           `json:"system_prompt"`
	Messages      []map[string]any `json:"messages"`
	MaxHistory    int              `json:"max_history"`
	MaxIterations int              `json:"max_iterations"`
	TokenBudget   int              `json:"token_budget"`
	TotalChats    int              `json:"total_chats"`
	TotalToolCalls int             `json:"total_tool_calls"`
}

func NewAgentActor() plexspaces.Actor {
	a := &AgentActor{MaxHistory: 50, MaxIterations: defaultMaxIterations, TokenBudget: defaultTokenBudget}
	a.SetSelf(a)
	return a
}

func newAgentActor() *AgentActor {
	a := &AgentActor{MaxHistory: 50, MaxIterations: defaultMaxIterations, TokenBudget: defaultTokenBudget}
	a.SetSelf(a)
	return a
}

func (a *AgentActor) Init(configJSON string) string {
	var config struct {
		ActorID string            `json:"actor_id"`
		Args    map[string]string `json:"args"`
	}
	if err := json.Unmarshal([]byte(configJSON), &config); err != nil {
		return "ERROR: " + err.Error()
	}
	a.SetRuntimeMetadata(config.ActorID)
	if sp := config.Args["system_prompt"]; sp != "" {
		a.SystemPrompt = sp
	} else {
		a.SystemPrompt = "You are Hermes, a self-improving AI assistant. You learn from experience, create reusable skills, and automate recurring tasks."
	}
	if v := config.Args["max_iterations"]; v != "" {
		if n, err := strconv.Atoi(v); err == nil {
			a.MaxIterations = n
		}
	}
	if v := config.Args["token_budget"]; v != "" {
		if n, err := strconv.Atoi(v); err == nil {
			a.TokenBudget = n
		}
	}
	if v := config.Args["max_history"]; v != "" {
		if n, err := strconv.Atoi(v); err == nil {
			a.MaxHistory = n
		}
	}

	if err := host.PG().Join("svc:agent"); err != nil {
		host.Warn(fmt.Sprintf("AgentActor: failed to join svc:agent: %v", err))
	}
	// Register in object registry for richer discovery (capability-aware routing)
	_ = host.Registry().Register(plexspaces.ObjectRegistration{
		ObjectID:       config.ActorID,
		ObjectType:     "actor",
		ObjectCategory: "agent",
		Capabilities:   []string{"chat", "tool_use", "skill_learning", "cron_automation", "context_compression"},
	})
	writeActorInfo(config.ActorID, "hermes-agent",
		"Self-improving agent with skill learning, tiered memory, and cron automation",
		[]string{"chat", "tool_use", "skill_learning", "cron_automation", "context_compression"})

	host.Info(fmt.Sprintf("AgentActor Init actor_id=%s max_iter=%d budget=%d", config.ActorID, a.MaxIterations, a.TokenBudget))
	return ""
}

func (a *AgentActor) Handle(fromActor, msgType, payloadJSON string) string {
	p := parsePayload(payloadJSON)
	op := stringVal(p, "op", msgType)
	switch op {
	case "chat":
		return a.chat(p)
	case "process_cron":
		return a.processCron(p)
	case "get_history":
		return marshal(map[string]any{"status": "ok", "messages": a.Messages, "count": len(a.Messages)})
	case "clear_history":
		sessionID := stringVal(p, "session_id", "")
		a.Messages = []map[string]any{}
		if sessionID != "" {
			host.KVDelete("session_history:" + sessionID)
		}
		return marshal(map[string]any{"status": "ok"})
	case "get_stats":
		return marshal(map[string]any{
			"status":           "ok",
			"total_chats":      a.TotalChats,
			"total_tool_calls": a.TotalToolCalls,
			"history_len":      len(a.Messages),
			"max_iterations":   a.MaxIterations,
			"token_budget":     a.TokenBudget,
		})
	default:
		return marshal(map[string]any{"error": "unknown_op", "op": op})
	}
}

func (a *AgentActor) chat(p map[string]any) string {
	userMsg := stringVal(p, "message", "")
	sessionID := stringVal(p, "session_id", "default")
	if userMsg == "" {
		return marshal(map[string]any{"error": "message is required"})
	}

	// Restore session history from KV if available
	if hist := host.KVGet("session_history:" + sessionID); hist != "" && len(a.Messages) == 0 {
		var msgs []map[string]any
		if err := json.Unmarshal([]byte(hist), &msgs); err == nil {
			a.Messages = msgs
		}
	}

	// Auto-compress if context is too large
	estimatedTokens := estimateTokens(a.Messages)
	if float64(estimatedTokens) > float64(a.TokenBudget)*compressionThreshold {
		a.compressContext(sessionID)
	}

	// Inject relevant skills into system prompt
	skillContext := a.fetchSkillContext(userMsg)

	a.Messages = append(a.Messages, map[string]any{"role": "user", "content": userMsg})

	// Get available tools
	tools := a.fetchToolSchemas()

	llmID, _ := registryFirst("llm_gateway", "svc:llm_gateway", "completion")

	loopIter := 0
	finalResponse := ""
	toolCallsThisTurn := 0

	for loopIter < a.MaxIterations {
		loopIter++

		// Build messages with system prompt + skill context
		sysContent := a.SystemPrompt
		if skillContext != "" {
			sysContent = sysContent + "\n\n## Learned Skills\n" + skillContext
		}
		msgsWithSystem := append([]map[string]any{{"role": "system", "content": sysContent}}, a.Messages...)

		if llmID == "" {
			// No LLM available: short-circuit
			finalResponse = fmt.Sprintf("Agent processed: %s (LLM not available)", userMsg)
			break
		}

		raw, err := host.Ask(llmID, "completion", map[string]any{
			"op":       "completion",
			"messages": msgsToAny(msgsWithSystem),
			"tools":    tools,
		}, 15000)
		if err != nil {
			finalResponse = fmt.Sprintf("LLM error: %v", err)
			break
		}

		llmMap, ok := raw.(map[string]any)
		if !ok {
			finalResponse = fmt.Sprintf("Agent processed: %s", userMsg)
			break
		}
		if _, hasErr := llmMap["error"]; hasErr {
			finalResponse = fmt.Sprintf("LLM unavailable: %v", llmMap["error"])
			break
		}

		resp, _ := llmMap["response"].(map[string]any)
		if resp == nil {
			finalResponse = fmt.Sprintf("Agent processed: %s", userMsg)
			break
		}

		stopReason := stringVal(resp, "stop_reason", "end_turn")
		content := stringVal(resp, "content", "")

		assistantMsg := map[string]any{"role": "assistant", "content": content, "stop_reason": stopReason}
		if tcs := resp["tool_calls"]; tcs != nil {
			assistantMsg["tool_calls"] = tcs
		}
		a.Messages = append(a.Messages, assistantMsg)

		if stopReason == "end_turn" {
			finalResponse = content
			break
		}

		if stopReason == "tool_use" {
			toolCalls, _ := resp["tool_calls"].([]any)
			toolResults := []string{}

			for _, tcRaw := range toolCalls {
				tc, ok := tcRaw.(map[string]any)
				if !ok {
					continue
				}
				tcID := stringVal(tc, "id", "tc_unknown")
				tcName := stringVal(tc, "name", "")
				tcInput, _ := tc["input"].(map[string]any)
				if tcInput == nil {
					tcInput = map[string]any{}
				}

				// Check guardrails before execution
				if a.requiresApproval(tcName) {
					a.Messages = append(a.Messages, map[string]any{
						"role":         "tool",
						"tool_call_id": tcID,
						"content":      `{"error":"tool requires approval","tool":"` + tcName + `"}`,
					})
					toolResults = append(toolResults, fmt.Sprintf("%s: requires_approval", tcName))
					continue
				}

				// Dispatch to ToolExecutorActor
				toolID, toolErr := registryFirst("tool_executor", "svc:tools")
				var toolOutput map[string]any
				if toolErr == nil {
					toolResp, askErr := host.Ask(toolID, "execute", map[string]any{
						"op":    "execute",
						"name":  tcName,
						"input": tcInput,
					}, 10000)
					if askErr == nil {
						if tr, ok := toolResp.(map[string]any); ok {
							toolOutput = tr
						}
					}
				}
				if toolOutput == nil {
					toolOutput = map[string]any{"error": "tool execution failed", "tool": tcName}
				}

				outJSON, _ := json.Marshal(toolOutput)
				a.Messages = append(a.Messages, map[string]any{
					"role":         "tool",
					"tool_call_id": tcID,
					"content":      string(outJSON),
				})
				toolResults = append(toolResults, fmt.Sprintf("%s: %s", tcName, string(outJSON)))
				toolCallsThisTurn++
				fireAudit("tool_called", fmt.Sprintf("tool=%s session=%s", tcName, sessionID))
			}

			if len(toolResults) > 0 {
				finalResponse = strings.Join(toolResults, "; ")
			}
			continue
		}

		finalResponse = content
		break
	}

	// Trim history if too long
	if len(a.Messages) > a.MaxHistory {
		keep := a.MaxHistory / 2
		a.Messages = a.Messages[len(a.Messages)-keep:]
	}

	// Persist session history
	if msgsJSON, err := json.Marshal(a.Messages); err == nil {
		host.KVPut("session_history:"+sessionID, string(msgsJSON))
	}

	// Track multi-step patterns for skill learning
	a.TotalToolCalls += toolCallsThisTurn
	if toolCallsThisTurn >= 3 {
		a.maybeLearnSkill(sessionID, toolCallsThisTurn)
	}

	a.TotalChats++
	a.IncrCounter(host, "agent_chats")
	fireAudit("agent_chat", fmt.Sprintf("session=%s iterations=%d tools=%d", sessionID, loopIter, toolCallsThisTurn))

	return marshal(map[string]any{
		"status":          "ok",
		"response":        finalResponse,
		"session_id":      sessionID,
		"loop_iterations": loopIter,
		"tool_calls":      toolCallsThisTurn,
		"messages_count":  len(a.Messages),
	})
}

// processCron dequeues and executes a cron job in an isolated session context.
func (a *AgentActor) processCron(p map[string]any) string {
	jobID := stringVal(p, "job_id", "")
	prompt := stringVal(p, "prompt", "")
	runID := fmt.Sprintf("cron-%s-%d", jobID, host.NowMs())

	if prompt == "" {
		return marshal(map[string]any{"error": "prompt is required for cron execution"})
	}

	// Execute in isolated session: clear messages first
	savedMessages := a.Messages
	a.Messages = []map[string]any{}

	cronResp := a.chat(map[string]any{
		"message":    prompt,
		"session_id": runID,
	})

	// Restore main messages; cron runs in isolation
	a.Messages = savedMessages

	// Persist cron result
	host.KVPut("cron_result:"+jobID+":"+runID, cronResp)
	fireAudit("cron_executed", fmt.Sprintf("job_id=%s run_id=%s", jobID, runID))
	host.Info(fmt.Sprintf("AgentActor: cron job executed job_id=%s run_id=%s", jobID, runID))

	var cronRespMap map[string]any
	if err := json.Unmarshal([]byte(cronResp), &cronRespMap); err == nil {
		cronRespMap["run_id"] = runID
		cronRespMap["job_id"] = jobID
		return marshal(cronRespMap)
	}
	return marshal(map[string]any{"status": "ok", "run_id": runID, "job_id": jobID})
}

// fetchSkillContext queries SkillStoreActor for relevant skills and formats them
// as a text block to inject into the system prompt.
func (a *AgentActor) fetchSkillContext(userMsg string) string {
	skillID, err := registryFirst("skill_store", "svc:skills")
	if err != nil {
		return ""
	}
	resp, askErr := host.Ask(skillID, "match_skills", map[string]any{
		"op":    "match_skills",
		"query": userMsg,
		"limit": 3,
	}, 3000)
	if askErr != nil {
		return ""
	}
	respMap, ok := resp.(map[string]any)
	if !ok {
		return ""
	}
	skills, _ := respMap["skills"].([]any)
	if len(skills) == 0 {
		return ""
	}
	lines := []string{}
	for _, s := range skills {
		sk, ok := s.(map[string]any)
		if !ok {
			continue
		}
		name := stringVal(sk, "name", "")
		procedure := stringVal(sk, "procedure", "")
		if name != "" && procedure != "" {
			lines = append(lines, fmt.Sprintf("**%s**: %s", name, procedure))
		}
	}
	return strings.Join(lines, "\n\n")
}

// fetchToolSchemas returns tool definitions from ToolExecutorActor.
func (a *AgentActor) fetchToolSchemas() []any {
	toolID, err := registryFirst("tool_executor", "svc:tools")
	if err != nil {
		return []any{}
	}
	resp, askErr := host.Ask(toolID, "list_tools", map[string]any{"op": "list_tools"}, 5000)
	if askErr != nil {
		return []any{}
	}
	respMap, ok := resp.(map[string]any)
	if !ok {
		return []any{}
	}
	tools, _ := respMap["tools"].([]any)
	return tools
}

// compressContext delegates context compression to ContextCompressorActor.
func (a *AgentActor) compressContext(sessionID string) {
	compID, err := registryFirst("context_compressor", "svc:compressor")
	if err != nil {
		// Fallback: simple truncation
		if len(a.Messages) > 10 {
			a.Messages = a.Messages[len(a.Messages)-10:]
		}
		return
	}
	msgsJSON, _ := json.Marshal(a.Messages)
	resp, askErr := host.Ask(compID, "compress", map[string]any{
		"op":         "compress",
		"messages":   string(msgsJSON),
		"session_id": sessionID,
		"keep_last":  4,
	}, 15000)
	if askErr != nil {
		host.Debug(fmt.Sprintf("AgentActor: compression failed: %v", askErr))
		return
	}
	respMap, ok := resp.(map[string]any)
	if !ok {
		return
	}
	if compressed, ok := respMap["messages"].(string); ok {
		var newMsgs []map[string]any
		if err := json.Unmarshal([]byte(compressed), &newMsgs); err == nil {
			a.Messages = newMsgs
			host.Info(fmt.Sprintf("AgentActor: context compressed session=%s new_len=%d", sessionID, len(a.Messages)))
		}
	}
}

// maybeLearnSkill signals SkillStoreActor to propose a skill from recent conversation.
func (a *AgentActor) maybeLearnSkill(sessionID string, toolCallCount int) {
	skillID, err := registryFirst("skill_store", "svc:skills")
	if err != nil {
		return
	}
	msgsJSON, _ := json.Marshal(a.Messages)
	// Fire-and-forget: skill learning is async
	_ = host.Send(skillID, "evaluate_for_learning", map[string]any{
		"op":              "evaluate_for_learning",
		"session_id":      sessionID,
		"messages":        string(msgsJSON),
		"tool_call_count": toolCallCount,
	})
}

// requiresApproval checks whether a tool requires guardrails approval.
func (a *AgentActor) requiresApproval(toolName string) bool {
	guardID, err := registryFirst("guardrails", "svc:guardrails")
	if err != nil {
		return false
	}
	resp, askErr := host.Ask(guardID, "check", map[string]any{
		"op":   "check",
		"tool": toolName,
	}, 2000)
	if askErr != nil {
		return false
	}
	respMap, ok := resp.(map[string]any)
	if !ok {
		return false
	}
	return stringVal(respMap, "decision", "allow") == "requires_approval"
}

func estimateTokens(messages []map[string]any) int {
	total := 0
	for _, m := range messages {
		if content, ok := m["content"].(string); ok {
			total += len(content) / 4
		}
	}
	return total
}

func msgsToAny(msgs []map[string]any) []any {
	result := make([]any, len(msgs))
	for i, m := range msgs {
		result[i] = m
	}
	return result
}
