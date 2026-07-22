// SPDX-License-Identifier: AGPL-3.0-or-later
// ContextCompressorActor — auto-compresses long conversation histories.
// Demonstrates: Ask (LLM for summarization), KV (original checkpoints), Metrics.
package main

import (
	"encoding/json"
	"fmt"

	"github.com/bhatti/PlexSpaces/sdks/go/plexspaces"
)

// ContextCompressorActor compresses long message histories by summarizing
// the middle section while preserving the first system message and last N turns.
// Original history is checkpointed in KV for audit/recovery.
type ContextCompressorActor struct {
	plexspaces.BaseActor
	CompressCount int `json:"compress_count"`
	TokensSaved   int `json:"tokens_saved"`
}

func NewContextCompressorActor() plexspaces.Actor {
	a := &ContextCompressorActor{}
	a.SetSelf(a)
	return a
}

func newContextCompressorActor() *ContextCompressorActor {
	a := &ContextCompressorActor{}
	a.SetSelf(a)
	return a
}

func (c *ContextCompressorActor) Init(configJSON string) string {
	var config struct {
		ActorID string `json:"actor_id"`
	}
	if err := json.Unmarshal([]byte(configJSON), &config); err != nil {
		return "ERROR: " + err.Error()
	}
	c.SetRuntimeMetadata(config.ActorID)
	if err := host.PG().Join("svc:compressor"); err != nil {
		host.Warn(fmt.Sprintf("ContextCompressorActor: failed to join svc:compressor: %v", err))
	}
	host.Info(fmt.Sprintf("ContextCompressorActor Init actor_id=%s", config.ActorID))
	return ""
}

func (c *ContextCompressorActor) Handle(fromActor, msgType, payloadJSON string) string {
	p := parsePayload(payloadJSON)
	op := stringVal(p, "op", msgType)
	switch op {
	case "compress":
		return c.compress(p)
	case "get_checkpoint":
		return c.getCheckpoint(p)
	case "get_stats":
		return marshal(map[string]any{
			"status":         "ok",
			"compress_count": c.CompressCount,
			"tokens_saved":   c.TokensSaved,
		})
	default:
		return marshal(map[string]any{"error": "unknown_op", "op": op})
	}
}

func (c *ContextCompressorActor) compress(p map[string]any) string {
	sessionID := stringVal(p, "session_id", "default")
	keepLast := intVal(p, "keep_last", 4)
	msgsRaw := stringVal(p, "messages", "")

	var messages []map[string]any
	if msgsRaw != "" {
		if err := json.Unmarshal([]byte(msgsRaw), &messages); err != nil {
			return marshal(map[string]any{"error": "invalid messages JSON"})
		}
	}

	if len(messages) <= keepLast+2 {
		msgsJSON, _ := json.Marshal(messages)
		return marshal(map[string]any{
			"status":   "ok",
			"messages": string(msgsJSON),
			"action":   "no_compression_needed",
		})
	}

	// Checkpoint original history in KV
	originalJSON, _ := json.Marshal(messages)
	checkpointKey := fmt.Sprintf("compression_checkpoint:%s:%d", sessionID, host.NowMs())
	host.KV().Put(checkpointKey, string(originalJSON))

	// Separate: system messages stay, last keepLast messages stay,
	// middle section gets summarized
	systemMsgs := []map[string]any{}
	middleMsgs := []map[string]any{}
	recentMsgs := messages[len(messages)-keepLast:]

	for _, m := range messages[:len(messages)-keepLast] {
		if stringVal(m, "role", "") == "system" {
			systemMsgs = append(systemMsgs, m)
		} else {
			middleMsgs = append(middleMsgs, m)
		}
	}

	if len(middleMsgs) == 0 {
		msgsJSON, _ := json.Marshal(messages)
		return marshal(map[string]any{
			"status":   "ok",
			"messages": string(msgsJSON),
			"action":   "nothing_to_compress",
		})
	}

	// Build the summary by calling LLM (or fallback to simple truncation)
	summary := c.summarizeMessages(middleMsgs)

	// Reassemble: system + summary placeholder + recent
	compressed := append(systemMsgs, map[string]any{
		"role":    "system",
		"content": "[Conversation summary: " + summary + "]",
	})
	compressed = append(compressed, recentMsgs...)

	beforeTokens := estimateTokens(messages)
	afterTokens := estimateTokens(compressed)
	saved := beforeTokens - afterTokens
	if saved < 0 {
		saved = 0
	}
	c.TokensSaved += saved
	c.CompressCount++
	c.IncrCounter(host, "compressions")

	compressedJSON, _ := json.Marshal(compressed)
	fireAudit("context_compressed", fmt.Sprintf("session=%s before=%d after=%d saved=%d tokens", sessionID, beforeTokens, afterTokens, saved))
	host.Info(fmt.Sprintf("ContextCompressor: compressed session=%s msgs=%d→%d tokens=%d→%d", sessionID, len(messages), len(compressed), beforeTokens, afterTokens))

	return marshal(map[string]any{
		"status":         "ok",
		"messages":       string(compressedJSON),
		"before_messages": len(messages),
		"after_messages": len(compressed),
		"tokens_saved":   saved,
		"checkpoint_key": checkpointKey,
	})
}

// summarizeMessages calls LLMGateway to summarize middle messages.
// Falls back to a simple description when LLM is unavailable.
func (c *ContextCompressorActor) summarizeMessages(msgs []map[string]any) string {
	if len(msgs) == 0 {
		return ""
	}

	llmID, err := registryFirst("llm_gateway", "svc:llm_gateway", "completion")
	if err != nil {
		return c.simpleSummary(msgs)
	}

	// Build a single text block from the messages
	var lines []string
	for _, m := range msgs {
		role := stringVal(m, "role", "unknown")
		content := stringVal(m, "content", "")
		if content == "" {
			continue
		}
		lines = append(lines, fmt.Sprintf("%s: %s", role, content))
	}
	if len(lines) == 0 {
		return "Previous conversation"
	}
	conversationText := ""
	for _, l := range lines {
		conversationText += l + "\n"
	}

	resp, askErr := host.Ask(llmID, "completion", map[string]any{
		"op": "completion",
		"messages": []any{
			map[string]any{"role": "user", "content": "Summarize this conversation concisely in 2-3 sentences, preserving key facts and decisions:\n\n" + conversationText},
		},
		"tools": []any{},
	}, 15000)
	if askErr != nil {
		return c.simpleSummary(msgs)
	}

	respMap, ok := resp.(map[string]any)
	if !ok {
		return c.simpleSummary(msgs)
	}
	inner, _ := respMap["response"].(map[string]any)
	if inner == nil {
		return c.simpleSummary(msgs)
	}
	summary := stringVal(inner, "content", "")
	if summary == "" {
		return c.simpleSummary(msgs)
	}
	return summary
}

func (c *ContextCompressorActor) simpleSummary(msgs []map[string]any) string {
	userCount, toolCount := 0, 0
	for _, m := range msgs {
		switch stringVal(m, "role", "") {
		case "user":
			userCount++
		case "tool":
			toolCount++
		}
	}
	return fmt.Sprintf("%d previous messages including %d user turns and %d tool calls", len(msgs), userCount, toolCount)
}

func (c *ContextCompressorActor) getCheckpoint(p map[string]any) string {
	key := stringVal(p, "checkpoint_key", "")
	if key == "" {
		return marshal(map[string]any{"error": "checkpoint_key is required"})
	}
	raw, _ := host.KV().Get(key)
	if raw == "" {
		return marshal(map[string]any{"error": "checkpoint not found", "key": key})
	}
	return marshal(map[string]any{"status": "ok", "messages": raw})
}
