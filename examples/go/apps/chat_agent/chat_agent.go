// SPDX-License-Identifier: AGPL-3.0-or-later
//
// ChatAgentActor — Cloudflare Agents SDK equivalent (Go WASM)
//
// Demonstrates conversation state in KV, LLM calls via service link, and
// durable alarm for periodic summarization.
//
// Cloudflare Agents SDK vs PlexSpaces Go:
//
//   Cloudflare Agents SDK                  | PlexSpaces Go
//   ---------------------------------------|--------------------------------------------
//   this.env.AI.run(model, {messages})     | plexspaces.NewServiceHTTPClient(host, link)
//   await this.storage.get('history')      | host.KV().GetJSON("history", &v)
//   await this.storage.put('history', v)   | host.KV().PutJSON("history", v)
//   storage.setAlarm(ts)                   | host.Alarm().Set(ts)
//   async onAlarm() { ... }                | Handle "__alarm__" message type
//   connection.send(reply)                 | return JSON reply string
//   env.AI binding in wrangler.toml        | [service_links.llm-link] in app-config.toml
//   Durable Object per-agent              | virtual_actor + reminder facets
//
// NOTE: LLM calls require ANTHROPIC_API_KEY configured in service_links.
// test.sh validates state and alarm logic only.
package main

import (
	"encoding/json"
	"fmt"

	"github.com/bhatti/PlexSpaces/sdks/go/plexspaces"
)

// alarmThreshold is the number of messages that triggers scheduling
// the periodic summarization alarm — equivalent to DO setAlarm() guard.
const alarmThreshold = 10

// alarmDelayMs is how far in the future (ms) to schedule the alarm: 5 minutes.
const alarmDelayMs = 300_000

// ChatMessage represents one turn in the conversation history.
type ChatMessage struct {
	Role      string `json:"role"`
	Content   string `json:"content"`
	Timestamp uint64 `json:"timestamp"`
}

// ChatAgentActor holds durable stats. Conversation history lives in KV,
// not in actor state, so it survives any actor checkpoint/restore cycle.
type ChatAgentActor struct {
	plexspaces.BaseActor
	ActorID            string `json:"actor_id"`
	TotalMessages      int    `json:"total_messages"`
	TotalSummarizations int   `json:"total_summarizations"`
}

func NewChatAgentActor() plexspaces.Actor {
	a := &ChatAgentActor{}
	a.SetSelf(a)
	return a
}

func (a *ChatAgentActor) Init(configJSON string) string {
	var config struct {
		ActorID string            `json:"actor_id"`
		Args    map[string]string `json:"args"`
	}
	if err := json.Unmarshal([]byte(configJSON), &config); err != nil {
		return "ERROR: " + err.Error()
	}
	a.ActorID = config.ActorID
	a.SetRuntimeMetadata(config.ActorID)
	host.Info(fmt.Sprintf("ChatAgentActor init actor_id=%s", a.ActorID))
	return ""
}

func (a *ChatAgentActor) Handle(fromActor, msgType, payloadJSON string) string {
	switch msgType {
	case "chat":
		return a.handleChat(payloadJSON)
	case "get_history":
		return a.handleGetHistory()
	case "clear":
		return a.handleClear()
	case "__alarm__":
		return a.handleAlarm()
	default:
		return errJSON(fmt.Sprintf("unknown op: %s", msgType))
	}
}

// handleChat — equivalent to Cloudflare Agents SDK onMessage():
//  1. Load history from KV (this.storage.get)
//  2. Append user message
//  3. Call LLM via service link (this.env.AI.run)
//  4. Append response, persist to KV (this.storage.put)
//  5. Schedule alarm after threshold (storage.setAlarm)
func (a *ChatAgentActor) handleChat(payloadJSON string) string {
	var req struct {
		Message string `json:"message"`
	}
	if err := json.Unmarshal([]byte(payloadJSON), &req); err != nil || req.Message == "" {
		return errJSON("message is required")
	}

	// Load history from KV — equivalent to: await this.storage.get('history')
	var history []ChatMessage
	if _, err := host.KV().GetJSON("history", &history); err != nil {
		history = nil
	}

	history = append(history, ChatMessage{
		Role:      "user",
		Content:   req.Message,
		Timestamp: host.NowMs(),
	})

	// Call LLM via service link — equivalent to: await this.env.AI.run(model, {messages})
	assistantReply := a.callLLM(history)

	history = append(history, ChatMessage{
		Role:      "assistant",
		Content:   assistantReply,
		Timestamp: host.NowMs(),
	})

	// Persist history — equivalent to: await this.storage.put('history', history)
	if err := host.KV().PutJSON("history", history); err != nil {
		host.Warn(fmt.Sprintf("ChatAgentActor: KV PutJSON failed: %v", err))
	}

	a.TotalMessages++

	// Schedule alarm after threshold — equivalent to: storage.setAlarm(ts)
	if len(history) > alarmThreshold {
		if alarmAt, _ := host.Alarm().Get(); alarmAt == 0 {
			if err := host.Alarm().Set(host.NowMs() + alarmDelayMs); err != nil {
				host.Warn(fmt.Sprintf("ChatAgentActor: Alarm.Set failed: %v", err))
			} else {
				host.Info("ChatAgentActor: alarm set for summarization in 5 minutes")
			}
		}
	}

	return okJSON(map[string]any{
		"reply":          assistantReply,
		"history_length": len(history),
	})
}

// handleGetHistory returns the stored conversation history from KV.
func (a *ChatAgentActor) handleGetHistory() string {
	var history []ChatMessage
	if _, err := host.KV().GetJSON("history", &history); err != nil || history == nil {
		history = []ChatMessage{}
	}
	return okJSON(map[string]any{
		"history": history,
		"count":   len(history),
	})
}

// handleClear clears history, summary, and the pending alarm.
// Equivalent to: storage.delete('history'); storage.deleteAlarm()
func (a *ChatAgentActor) handleClear() string {
	_ = host.KV().Delete("history")
	_ = host.KV().Delete("summary")
	_ = host.Alarm().Delete()
	return okJSON(map[string]any{"cleared": true})
}

// handleAlarm — durable alarm callback, equivalent to Cloudflare Agents SDK onAlarm().
// Summarizes history, stores a summary KV key, and clears history.
func (a *ChatAgentActor) handleAlarm() string {
	host.Info("ChatAgentActor: alarm fired — summarizing history")

	var history []ChatMessage
	if _, err := host.KV().GetJSON("history", &history); err != nil || len(history) == 0 {
		return okJSON(map[string]any{"action": "no_history_to_summarize"})
	}

	// Summarize via LLM
	summaryMsg := fmt.Sprintf("Summarize this conversation concisely (2-3 sentences): %s",
		mustJSON(history))
	summary := a.callLLMMessages([]ChatMessage{{Role: "user", Content: summaryMsg}})

	// Persist summary, clear history — equivalent to: storage.put('summary', s); storage.delete('history')
	_ = host.KV().Put("summary", summary)
	_ = host.KV().Delete("history")

	a.TotalSummarizations++

	host.Info(fmt.Sprintf("ChatAgentActor: summarized %d messages", len(history)))
	return okJSON(map[string]any{
		"action":             "summarized",
		"messages_summarized": len(history),
	})
}

// callLLM sends the conversation history to the LLM service link and returns the reply.
func (a *ChatAgentActor) callLLM(history []ChatMessage) string {
	return a.callLLMMessages(history)
}

func (a *ChatAgentActor) callLLMMessages(messages []ChatMessage) string {
	// Build Anthropic-format request body.
	type llmMessage struct {
		Role    string `json:"role"`
		Content string `json:"content"`
	}
	var llmMsgs []llmMessage
	for _, m := range messages {
		llmMsgs = append(llmMsgs, llmMessage{Role: m.Role, Content: m.Content})
	}
	reqBody, _ := json.Marshal(map[string]any{
		"model":      "claude-3-5-haiku-20241022",
		"max_tokens": 1024,
		"messages":   llmMsgs,
	})

	client := plexspaces.NewServiceHTTPClient(host, "llm-link")
	resp, err := client.Post("/v1/messages", reqBody, nil)
	if err != nil {
		host.Warn(fmt.Sprintf("ChatAgentActor: LLM call failed: %v", err))
		return "[LLM unavailable — message stored]"
	}

	// Parse Anthropic response: content[0].text
	if content, ok := resp["content"].([]any); ok && len(content) > 0 {
		if block, ok := content[0].(map[string]any); ok {
			if text, ok := block["text"].(string); ok {
				return text
			}
		}
	}
	// Fallback: OpenAI-compatible
	if choices, ok := resp["choices"].([]any); ok && len(choices) > 0 {
		if choice, ok := choices[0].(map[string]any); ok {
			if msg, ok := choice["message"].(map[string]any); ok {
				if text, ok := msg["content"].(string); ok {
					return text
				}
			}
		}
	}
	return "[no response]"
}

// ---- helpers ----

func okJSON(data map[string]any) string {
	data["status"] = "ok"
	b, _ := json.Marshal(data)
	return string(b)
}

func errJSON(msg string) string {
	b, _ := json.Marshal(map[string]any{"error": msg})
	return string(b)
}

func mustJSON(v any) string {
	b, _ := json.Marshal(v)
	return string(b)
}

// ---- WASM entrypoint ----

var host = plexspaces.NewHost()

func main() {}

func init() {
	router := plexspaces.NewActorRouter()
	router.Route("ChatAgentActor", func() plexspaces.Actor { return NewChatAgentActor() })
	plexspaces.Register(router)
}
