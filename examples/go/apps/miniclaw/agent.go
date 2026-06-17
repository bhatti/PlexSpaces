// SPDX-License-Identifier: AGPL-3.0-or-later
// AgentActor — core agent loop (message → LLM → tool_use → repeat until end_turn).
// SessionManagerActor — session lifecycle management with KV persistence.
package main

import (
	"encoding/json"
	"fmt"
	"strconv"
	"strings"

	"github.com/bhatti/PlexSpaces/sdks/go/plexspaces"
)

// ========================================================================
// AgentActor
// ========================================================================

// AgentActor implements the core agent loop: receive user message → call LLM →
// if tool_use, execute tools and feed results back → repeat until end_turn.
// Discovers LLM and tools via process groups (location transparent).
type AgentActor struct {
	plexspaces.BaseActor
	SystemPrompt string           `json:"system_prompt"`
	Messages     []map[string]any `json:"messages"`
	MaxHistory   int              `json:"max_history"`
	LoopCount    int              `json:"loop_count"`
	TotalChats   int              `json:"total_chats"`
	AgentName    string           `json:"agent_name"`
	Capabilities []string         `json:"capabilities"`
}

func NewAgentActor() plexspaces.Actor {
	a := &AgentActor{MaxHistory: 50, AgentName: "general-assistant"}
	a.SetSelf(a)
	return a
}

func newAgentActor() *AgentActor {
	a := &AgentActor{MaxHistory: 50, AgentName: "general-assistant"}
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
	if name := config.Args["agent_name"]; name != "" {
		a.AgentName = name
	}
	if sp := config.Args["system_prompt"]; sp != "" {
		a.SystemPrompt = sp
	} else {
		a.SystemPrompt = "You are a helpful AI assistant with access to tools."
	}
	if mh := config.Args["max_history"]; mh != "" {
		if n, err := strconv.Atoi(mh); err == nil {
			a.MaxHistory = n
		}
	}
	a.Capabilities = []string{"chat", "tool_use", "memory"}

	if err := host.PG().Join("svc:agent"); err != nil {
		host.Warn(fmt.Sprintf("AgentActor: failed to join svc:agent: %v", err))
	}
	writeActorInfo(config.ActorID, a.AgentName,
		"Core agent loop with tool calling and session memory",
		a.Capabilities)

	host.Info(fmt.Sprintf("AgentActor Init actor_id=%s name=%s", config.ActorID, a.AgentName))
	return ""
}

func (a *AgentActor) Handle(fromActor, msgType, payloadJSON string) string {
	p := parsePayload(payloadJSON)
	op := stringVal(p, "op", msgType)
	switch op {
	case "chat":
		return a.chat(p)
	case "set_system_prompt":
		a.SystemPrompt = stringVal(p, "prompt", a.SystemPrompt)
		return marshal(map[string]any{"status": "ok"})
	case "get_history":
		return marshal(map[string]any{
			"status":   "ok",
			"messages": a.Messages,
			"count":    len(a.Messages),
		})
	case "compact_context":
		return a.compactContext(10)
	case "get_capabilities":
		return marshal(map[string]any{"status": "ok", "capabilities": a.Capabilities})
	case "get_stats":
		return marshal(map[string]any{
			"status":      "ok",
			"total_chats": a.TotalChats,
			"loop_count":  a.LoopCount,
			"history_len": len(a.Messages),
			"agent_name":  a.AgentName,
		})
	default:
		return marshal(map[string]any{"error": "unknown_op", "op": op})
	}
}

func (a *AgentActor) chat(p map[string]any) string {
	userMsg := stringVal(p, "message", "")
	sessionID := stringVal(p, "session_id", "")

	if userMsg == "" {
		return marshal(map[string]any{"error": "message is required"})
	}

	a.Messages = append(a.Messages, map[string]any{"role": "user", "content": userMsg})

	toolRegID, toolRegErr := pgFirst("svc:tool_registry")
	tools := []any{}
	if toolRegErr != nil {
		host.Debug(fmt.Sprintf("AgentActor: tool_registry not available: %v", toolRegErr))
	} else {
		resp, err := host.Ask(toolRegID, "list_tools", map[string]any{"op": "list_tools"}, 5000)
		if err == nil {
			if respMap, ok := resp.(map[string]any); ok {
				if ts, ok := respMap["tools"].([]any); ok {
					tools = ts
				}
			}
		}
	}

	fsmID, fsmErr := pgFirst("svc:agent_fsm")
	if fsmErr != nil {
		host.Debug(fmt.Sprintf("AgentActor: agent_fsm not available: %v", fsmErr))
	}
	if fsmID != "" {
		_ = host.Send(fsmID, "transition", map[string]any{"op": "transition", "to": "processing"})
	}

	const maxIter = 5
	loopIter := 0
	finalResponse := ""

	for loopIter < maxIter {
		loopIter++

		llmID, _ := pgFirst("svc:llm_router")
		if llmID == "" {
			finalResponse = fmt.Sprintf("Agent processed: %s (LLM not available in test mode)", userMsg)
			break
		}

		llmResp, err := host.Ask(llmID, "chat_completion", map[string]any{
			"op":       "chat_completion",
			"messages": a.Messages,
			"tools":    tools,
		}, 10000)
		if err != nil {
			finalResponse = fmt.Sprintf("LLM error: %v", err)
			break
		}

		llmMap, ok := llmResp.(map[string]any)
		if !ok {
			finalResponse = fmt.Sprintf("Agent processed: %s", userMsg)
			break
		}
		if _, hasErr := llmMap["error"]; hasErr {
			finalResponse = fmt.Sprintf("LLM unavailable: %v", llmMap["error"])
			break
		}

		respInner, _ := llmMap["response"].(map[string]any)
		if respInner == nil {
			finalResponse = fmt.Sprintf("Agent processed: %s", userMsg)
			break
		}

		stopReason := stringVal(respInner, "stop_reason", "end_turn")
		content := stringVal(respInner, "content", "")

		assistantMsg := map[string]any{
			"role":        "assistant",
			"content":     content,
			"stop_reason": stopReason,
		}
		if tcs := respInner["tool_calls"]; tcs != nil {
			assistantMsg["tool_calls"] = tcs
		}
		a.Messages = append(a.Messages, assistantMsg)

		if stopReason == "end_turn" {
			finalResponse = content
			break
		}

		if stopReason == "tool_use" {
			toolCalls, _ := respInner["tool_calls"].([]any)
			allToolResults := []string{}

			if fsmID != "" {
				_ = host.Send(fsmID, "transition", map[string]any{"op": "transition", "to": "tool_executing"})
			}

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

				var toolOutput map[string]any
				if toolRegID != "" {
					toolResp, err := host.Ask(toolRegID, "execute_tool", map[string]any{
						"op":    "execute_tool",
						"name":  tcName,
						"input": tcInput,
					}, 5000)
					if err == nil {
						if tr, ok := toolResp.(map[string]any); ok {
							toolOutput = tr
						}
					}
				}
				if toolOutput == nil {
					toolOutput = map[string]any{"error": "tool execution failed", "tool": tcName}
				}

				toolOutputJSON, _ := json.Marshal(toolOutput)
				a.Messages = append(a.Messages, map[string]any{
					"role":         "tool",
					"tool_call_id": tcID,
					"content":      string(toolOutputJSON),
				})
				allToolResults = append(allToolResults, fmt.Sprintf("%s: %s", tcName, string(toolOutputJSON)))
				fireAudit("tool_called", fmt.Sprintf("tool=%s session=%s", tcName, sessionID))
			}

			if fsmID != "" {
				_ = host.Send(fsmID, "transition", map[string]any{"op": "transition", "to": "processing"})
			}
			if len(allToolResults) > 0 {
				finalResponse = fmt.Sprintf("Tool results: %s", strings.Join(allToolResults, "; "))
			}
			continue
		}

		finalResponse = content
		break
	}

	a.LoopCount += loopIter

	if fsmID != "" {
		_ = host.Send(fsmID, "transition", map[string]any{"op": "transition", "to": "responding"})
		_ = host.Send(fsmID, "transition", map[string]any{"op": "transition", "to": "idle"})
	}

	if len(a.Messages) > a.MaxHistory {
		_ = a.compactContext(a.MaxHistory / 2)
	}
	if sessionID != "" {
		msgsJSON, _ := json.Marshal(a.Messages)
		host.KVPut("session_history:"+sessionID, string(msgsJSON))
	}

	a.TotalChats++
	a.IncrCounter(host, "agent_chats")
	fireAudit("agent_chat", fmt.Sprintf("session=%s iterations=%d", sessionID, loopIter))

	return marshal(map[string]any{
		"status":          "ok",
		"response":        finalResponse,
		"session_id":      sessionID,
		"loop_iterations": loopIter,
		"messages_count":  len(a.Messages),
	})
}

func (a *AgentActor) compactContext(keep int) string {
	if len(a.Messages) <= keep {
		return marshal(map[string]any{"status": "ok", "messages_count": len(a.Messages)})
	}
	compacted := make([]map[string]any, 0, keep+1)
	if len(a.Messages) > 0 {
		compacted = append(compacted, a.Messages[0])
	}
	start := len(a.Messages) - keep
	if start < 1 {
		start = 1
	}
	compacted = append(compacted, a.Messages[start:]...)
	a.Messages = compacted
	return marshal(map[string]any{"status": "ok", "messages_count": len(a.Messages)})
}

// ========================================================================
// SessionManagerActor
// ========================================================================

// SessionManagerActor manages agent session lifecycle: create, get, end, list.
// Sessions are persisted in KV; a mapping by channel+user enables lookup.
type SessionManagerActor struct {
	plexspaces.BaseActor
	ActiveSessions int      `json:"active_sessions"`
	TotalCreated   int      `json:"total_created"`
	SessionIDs     []string `json:"session_ids"`
}

func NewSessionManagerActor() plexspaces.Actor {
	a := &SessionManagerActor{}
	a.SetSelf(a)
	return a
}

func newSessionManagerActor() *SessionManagerActor {
	a := &SessionManagerActor{}
	a.SetSelf(a)
	return a
}

func (s *SessionManagerActor) Init(configJSON string) string {
	var config struct {
		ActorID string `json:"actor_id"`
	}
	if err := json.Unmarshal([]byte(configJSON), &config); err != nil {
		return "ERROR: " + err.Error()
	}
	s.SetRuntimeMetadata(config.ActorID)
	if err := host.PG().Join("svc:session_manager"); err != nil {
		host.Warn(fmt.Sprintf("SessionManagerActor: failed to join svc:session_manager: %v", err))
	}
	host.Info(fmt.Sprintf("SessionManagerActor Init actor_id=%s", config.ActorID))
	return ""
}

func (s *SessionManagerActor) Handle(fromActor, msgType, payloadJSON string) string {
	p := parsePayload(payloadJSON)
	op := stringVal(p, "op", msgType)
	switch op {
	case "create_session":
		return s.createSession(p)
	case "get_session":
		return s.getSession(p)
	case "end_session":
		return s.endSession(p)
	case "list_sessions":
		return s.listSessions()
	case "get_stats":
		return marshal(map[string]any{
			"status":          "ok",
			"active_sessions": s.ActiveSessions,
			"total_created":   s.TotalCreated,
		})
	default:
		return marshal(map[string]any{"error": "unknown_op", "op": op})
	}
}

func (s *SessionManagerActor) createSession(p map[string]any) string {
	channel := stringVal(p, "channel", "web")
	userID := stringVal(p, "user_id", "anonymous")
	agentID := stringVal(p, "agent_id", "agent")

	sessionID := fmt.Sprintf("sess-%s-%s-%d", channel, userID, host.NowMs())
	meta := map[string]any{
		"session_id": sessionID,
		"channel":    channel,
		"user_id":    userID,
		"agent_id":   agentID,
		"created_at": host.NowMs(),
		"status":     "active",
	}
	metaJSON, _ := json.Marshal(meta)
	host.KVPut("session:"+sessionID, string(metaJSON))
	host.KVPut("session_map:"+channel+":"+userID, sessionID)

	s.SessionIDs = append(s.SessionIDs, sessionID)
	s.ActiveSessions++
	s.TotalCreated++

	s.IncrCounter(host, "sessions_created")
	fireAudit("session_created", fmt.Sprintf("session_id=%s channel=%s user_id=%s", sessionID, channel, userID))
	host.Info(fmt.Sprintf("SessionManager: created session_id=%s", sessionID))
	return marshal(map[string]any{"status": "ok", "session_id": sessionID})
}

func (s *SessionManagerActor) getSession(p map[string]any) string {
	sessionID := stringVal(p, "session_id", "")
	if sessionID == "" {
		channel := stringVal(p, "channel", "")
		userID := stringVal(p, "user_id", "")
		if channel != "" && userID != "" {
			sessionID = host.KVGet("session_map:" + channel + ":" + userID)
		}
	}
	if sessionID == "" {
		return marshal(map[string]any{"error": "session not found"})
	}
	raw := host.KVGet("session:" + sessionID)
	if raw == "" {
		return marshal(map[string]any{"error": "session not found", "session_id": sessionID})
	}
	var meta map[string]any
	if err := json.Unmarshal([]byte(raw), &meta); err != nil {
		return marshal(map[string]any{"error": "corrupt session data"})
	}
	meta["status"] = "ok"
	return marshal(meta)
}

func (s *SessionManagerActor) endSession(p map[string]any) string {
	sessionID := stringVal(p, "session_id", "")
	if sessionID == "" {
		return marshal(map[string]any{"error": "session_id is required"})
	}
	host.KVDelete("session:" + sessionID)

	newIDs := make([]string, 0, len(s.SessionIDs))
	for _, id := range s.SessionIDs {
		if id != sessionID {
			newIDs = append(newIDs, id)
		}
	}
	s.SessionIDs = newIDs
	if s.ActiveSessions > 0 {
		s.ActiveSessions--
	}
	fireAudit("session_ended", fmt.Sprintf("session_id=%s", sessionID))
	return marshal(map[string]any{"status": "ok", "session_id": sessionID})
}

func (s *SessionManagerActor) listSessions() string {
	sessions := make([]any, 0, len(s.SessionIDs))
	for _, id := range s.SessionIDs {
		raw := host.KVGet("session:" + id)
		if raw == "" {
			continue
		}
		var meta map[string]any
		if err := json.Unmarshal([]byte(raw), &meta); err == nil {
			sessions = append(sessions, meta)
		}
	}
	return marshal(map[string]any{"status": "ok", "sessions": sessions, "count": len(sessions)})
}
