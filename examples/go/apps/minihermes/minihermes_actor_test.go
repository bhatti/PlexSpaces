// SPDX-License-Identifier: AGPL-3.0-or-later
// Contract tests for MiniHermes actors — no running node required.
// Uses stub host (plexspaces.ResetStubs) to isolate actor logic.

package main

import (
	"encoding/json"
	"fmt"
	"strings"
	"testing"

	"github.com/bhatti/PlexSpaces/sdks/go/plexspaces"
)

// ── helpers ──────────────────────────────────────────────────────────────────

func initActor(actor interface {
	Init(string) string
}, actorID string) {
	actor.Init(`{"actor_id":"` + actorID + `","args":{}}`)
}

func initActorWithArgs(actor interface {
	Init(string) string
}, actorID string, args map[string]string) {
	argsJSON, _ := json.Marshal(args)
	actor.Init(`{"actor_id":"` + actorID + `","args":` + string(argsJSON) + `}`)
}

func parseResp(t *testing.T, raw string) map[string]any {
	t.Helper()
	var m map[string]any
	if err := json.Unmarshal([]byte(raw), &m); err != nil {
		t.Fatalf("invalid JSON: %v — raw: %s", err, raw)
	}
	return m
}

func assertOK(t *testing.T, resp map[string]any, label string) {
	t.Helper()
	if _, hasErr := resp["error"]; hasErr {
		t.Errorf("%s: unexpected error: %v", label, resp["error"])
	}
}

func assertError(t *testing.T, resp map[string]any, label string) {
	t.Helper()
	if _, hasErr := resp["error"]; !hasErr {
		t.Errorf("%s: expected error, got: %v", label, resp)
	}
}

// ── LLMGatewayActor tests ─────────────────────────────────────────────────────

func TestLLMGatewaySimulatedMode(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newLLMGatewayActor()
	initActor(actor, "llm_gateway:test@node")

	// General question → end_turn
	raw := actor.Handle("caller", "completion", `{"op":"completion","messages":[{"role":"user","content":"hello how are you"}],"tools":[]}`)
	resp := parseResp(t, raw)
	assertOK(t, resp, "completion hello")
	inner, _ := resp["response"].(map[string]any)
	if inner == nil {
		t.Fatalf("expected response field, got: %v", resp)
	}
	if sr := inner["stop_reason"].(string); sr != "end_turn" {
		t.Errorf("expected stop_reason=end_turn got %q", sr)
	}
	if actor.RequestCount != 1 {
		t.Errorf("expected RequestCount=1 got %d", actor.RequestCount)
	}
}

func TestLLMGatewayToolUseCalculator(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newLLMGatewayActor()
	initActor(actor, "llm_gateway:test@node")

	raw := actor.Handle("caller", "completion", `{"op":"completion","messages":[{"role":"user","content":"please calculate 42 * 17"}],"tools":[]}`)
	resp := parseResp(t, raw)
	assertOK(t, resp, "completion calculate")

	inner, _ := resp["response"].(map[string]any)
	if inner == nil {
		t.Fatalf("expected response field")
	}
	if sr := inner["stop_reason"].(string); sr != "tool_use" {
		t.Errorf("expected stop_reason=tool_use got %q", sr)
	}
	toolCalls, _ := inner["tool_calls"].([]any)
	if len(toolCalls) == 0 {
		t.Fatal("expected at least one tool_call")
	}
	tc, _ := toolCalls[0].(map[string]any)
	if name := tc["name"].(string); name != "calculator" {
		t.Errorf("expected tool_call.name=calculator got %q", name)
	}
}

func TestLLMGatewayMemoryToolUse(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newLLMGatewayActor()
	initActor(actor, "llm_gateway:test@node")

	raw := actor.Handle("caller", "completion", `{"op":"completion","messages":[{"role":"user","content":"remember this fact for me"}],"tools":[]}`)
	resp := parseResp(t, raw)
	inner, _ := resp["response"].(map[string]any)
	if inner == nil {
		t.Fatal("expected response field")
	}
	if sr := inner["stop_reason"].(string); sr != "tool_use" {
		t.Errorf("expected tool_use for memory keyword, got %q", sr)
	}
}

func TestLLMGatewayProviderRegistration(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newLLMGatewayActor()
	initActor(actor, "llm_gateway:test@node")

	raw := actor.Handle("caller", "register_provider", `{"op":"register_provider","name":"openai","base_url":"https://api.openai.com","model":"gpt-4o"}`)
	resp := parseResp(t, raw)
	assertOK(t, resp, "register_provider openai")
	if resp["provider"] != "openai" {
		t.Errorf("expected provider=openai got %v", resp["provider"])
	}
}

func TestLLMGatewayProviderSwap(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newLLMGatewayActor()
	initActor(actor, "llm_gateway:test@node")

	raw := actor.Handle("caller", "switch_provider", `{"op":"switch_provider","provider":"openai","model":"gpt-4o"}`)
	resp := parseResp(t, raw)
	assertOK(t, resp, "switch_provider")
	if actor.ActiveProvider != "openai" {
		t.Errorf("expected ActiveProvider=openai got %q", actor.ActiveProvider)
	}
	if actor.DefaultModel != "gpt-4o" {
		t.Errorf("expected DefaultModel=gpt-4o got %q", actor.DefaultModel)
	}
}

func TestLLMGatewayCircuitBreaker(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newLLMGatewayActor()
	initActor(actor, "llm_gateway:test@node")

	// With stub host, HTTPFetch always fails → simulated mode activates, no circuit open
	// Test circuit reset
	actor.CircuitOpen = true
	actor.ConsecutiveFailures = 3
	raw := actor.Handle("caller", "reset_circuit", `{"op":"reset_circuit"}`)
	resp := parseResp(t, raw)
	if actor.CircuitOpen {
		t.Error("expected circuit to be closed after reset")
	}
	_ = resp
}

func TestLLMGatewayGetStats(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newLLMGatewayActor()
	initActor(actor, "llm_gateway:test@node")

	raw := actor.Handle("caller", "get_stats", `{"op":"get_stats"}`)
	resp := parseResp(t, raw)
	assertOK(t, resp, "get_stats")
	if resp["active_provider"] == nil {
		t.Error("expected active_provider in stats")
	}
}

// ── ToolExecutorActor tests ───────────────────────────────────────────────────

func TestToolExecutorBuiltinTools(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newToolExecutorActor()
	initActor(actor, "tools:test@node")

	raw := actor.Handle("caller", "list_tools", `{"op":"list_tools"}`)
	resp := parseResp(t, raw)
	assertOK(t, resp, "list_tools")
	count, _ := resp["count"].(float64)
	if int(count) < 6 {
		t.Errorf("expected at least 6 built-in tools, got %d", int(count))
	}
}

func TestToolExecutorCalculator(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newToolExecutorActor()
	initActor(actor, "tools:test@node")

	for _, tc := range []struct {
		expr   string
		expect float64
	}{
		{"42 * 17", 714},
		{"100 + 25", 125},
		{"200 - 50", 150},
		{"10 / 4", 2.5},
	} {
		raw := actor.Handle("caller", "execute", `{"op":"execute","name":"calculator","input":{"expression":"`+tc.expr+`"}}`)
		resp := parseResp(t, raw)
		assertOK(t, resp, "calculator "+tc.expr)
		out, _ := resp["output"].(map[string]any)
		result, _ := out["result"].(float64)
		if result != tc.expect {
			t.Errorf("calculator %q: expected %v got %v", tc.expr, tc.expect, result)
		}
	}
}

func TestToolExecutorRegisterCustomTool(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newToolExecutorActor()
	initActor(actor, "tools:test@node")

	raw := actor.Handle("caller", "register_tool", `{"op":"register_tool","name":"my_tool","description":"A test tool","input_schema":{"type":"object"}}`)
	resp := parseResp(t, raw)
	assertOK(t, resp, "register_tool")

	// Verify it appears in list
	listRaw := actor.Handle("caller", "list_tools", `{"op":"list_tools"}`)
	listResp := parseResp(t, listRaw)
	tools, _ := listResp["tools"].([]any)
	found := false
	for _, t := range tools {
		if tm, ok := t.(map[string]any); ok {
			if tm["name"] == "my_tool" {
				found = true
				break
			}
		}
	}
	if !found {
		t.Error("expected my_tool in tool list after registration")
	}
}

func TestToolExecutorUnknownTool(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newToolExecutorActor()
	initActor(actor, "tools:test@node")

	raw := actor.Handle("caller", "execute", `{"op":"execute","name":"nonexistent_tool","input":{}}`)
	resp := parseResp(t, raw)
	assertError(t, resp, "execute nonexistent_tool")
}

// ── AgentActor tests ─────────────────────────────────────────────────────────

func TestAgentSimpleChat(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newAgentActor()
	initActor(actor, "agent:test@node")

	raw := actor.Handle("caller", "chat", `{"op":"chat","message":"hello world","session_id":"sess1"}`)
	resp := parseResp(t, raw)
	assertOK(t, resp, "simple chat")
	if resp["response"] == nil {
		t.Error("expected response field")
	}
	if actor.TotalChats != 1 {
		t.Errorf("expected TotalChats=1 got %d", actor.TotalChats)
	}
}

func TestAgentHistoryPersistence(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newAgentActor()
	initActor(actor, "agent:test@node")

	actor.Handle("caller", "chat", `{"op":"chat","message":"first message","session_id":"sess1"}`)
	actor.Handle("caller", "chat", `{"op":"chat","message":"second message","session_id":"sess1"}`)

	raw := actor.Handle("caller", "get_history", `{"op":"get_history"}`)
	resp := parseResp(t, raw)
	count, _ := resp["count"].(float64)
	if int(count) < 2 {
		t.Errorf("expected at least 2 messages in history, got %d", int(count))
	}
}

func TestAgentClearHistory(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newAgentActor()
	initActor(actor, "agent:test@node")

	actor.Handle("caller", "chat", `{"op":"chat","message":"test message","session_id":"sess1"}`)
	actor.Handle("caller", "clear_history", `{"op":"clear_history","session_id":"sess1"}`)

	if len(actor.Messages) != 0 {
		t.Errorf("expected empty history after clear, got %d messages", len(actor.Messages))
	}
}

func TestAgentGetStats(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newAgentActor()
	initActor(actor, "agent:test@node")

	actor.Handle("caller", "chat", `{"op":"chat","message":"test","session_id":"s1"}`)
	raw := actor.Handle("caller", "get_stats", `{"op":"get_stats"}`)
	resp := parseResp(t, raw)
	assertOK(t, resp, "get_stats")
	if resp["total_chats"] == nil {
		t.Error("expected total_chats in stats")
	}
}

func TestAgentProcessCron(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newAgentActor()
	initActor(actor, "agent:test@node")

	// Process a cron job
	raw := actor.Handle("caller", "process_cron", `{"op":"process_cron","job_id":"test-job","prompt":"What is 2+2?"}`)
	resp := parseResp(t, raw)
	assertOK(t, resp, "process_cron")
	if resp["run_id"] == nil {
		t.Error("expected run_id in cron response")
	}
	if resp["job_id"] != "test-job" {
		t.Errorf("expected job_id=test-job, got %v", resp["job_id"])
	}
}

// ── SkillStoreActor tests ─────────────────────────────────────────────────────

func TestSkillStorePropose(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newSkillStoreActor()
	initActor(actor, "skills:test@node")

	raw := actor.Handle("caller", "propose_skill", `{
		"op":"propose_skill",
		"name":"Calculate ROI",
		"description":"Steps to calculate return on investment",
		"procedure":"1. Get initial investment amount\n2. Get final value\n3. Calculate (final-initial)/initial*100",
		"tags":"finance,math",
		"trigger_patterns":"roi,return on investment,calculate roi"
	}`)
	resp := parseResp(t, raw)
	assertOK(t, resp, "propose_skill")
	if resp["skill_id"] == nil {
		t.Error("expected skill_id in response")
	}
	if actor.SkillCount != 1 {
		t.Errorf("expected SkillCount=1, got %d", actor.SkillCount)
	}
}

func TestSkillStoreMatchByTrigger(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newSkillStoreActor()
	initActor(actor, "skills:test@node")

	// Create a skill
	actor.Handle("caller", "propose_skill", `{
		"op":"propose_skill",
		"name":"Database backup",
		"description":"Steps to backup a database",
		"procedure":"1. Connect to DB\n2. Run pg_dump\n3. Upload to S3",
		"tags":"database,backup",
		"trigger_patterns":"backup,database,pg_dump"
	}`)

	raw := actor.Handle("caller", "match_skills", `{"op":"match_skills","query":"I need to backup my database","limit":3}`)
	resp := parseResp(t, raw)
	assertOK(t, resp, "match_skills")
	count, _ := resp["count"].(float64)
	if int(count) == 0 {
		t.Error("expected to find at least 1 matching skill")
	}
}

func TestSkillStoreLifecycle(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newSkillStoreActor()
	initActor(actor, "skills:test@node")

	// Create a skill
	propRaw := actor.Handle("caller", "propose_skill", `{
		"op":"propose_skill",
		"name":"Test skill",
		"description":"Test",
		"procedure":"Do X then Y",
		"tags":"test"
	}`)
	propResp := parseResp(t, propRaw)
	skillID, _ := propResp["skill_id"].(string)

	// Get the skill
	getResp := parseResp(t, actor.Handle("caller", "get_skill", `{"op":"get_skill","skill_id":"`+skillID+`"}`))
	if getResp["name"] != "Test skill" {
		t.Errorf("expected skill name=Test skill, got %v", getResp["name"])
	}

	// Record usage
	useResp := parseResp(t, actor.Handle("caller", "record_usage", `{"op":"record_usage","skill_id":"`+skillID+`"}`))
	assertOK(t, useResp, "record_usage")

	// Delete
	delResp := parseResp(t, actor.Handle("caller", "delete_skill", `{"op":"delete_skill","skill_id":"`+skillID+`"}`))
	assertOK(t, delResp, "delete_skill")
	if actor.SkillCount != 0 {
		t.Errorf("expected SkillCount=0 after delete, got %d", actor.SkillCount)
	}
}

func TestSkillStoreEvaluateForLearning(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newSkillStoreActor()
	initActor(actor, "skills:test@node")

	// Too few tool calls → no learning
	raw := actor.Handle("caller", "evaluate_for_learning", `{
		"op":"evaluate_for_learning",
		"session_id":"s1",
		"tool_call_count":2,
		"messages":"[]"
	}`)
	resp := parseResp(t, raw)
	if resp["action"] != "no_learning" {
		t.Errorf("expected no_learning for 2 tool calls, got %v", resp["action"])
	}

	// Enough tool calls with message history
	rawLearn := actor.Handle("caller", "evaluate_for_learning", `{
		"op":"evaluate_for_learning",
		"session_id":"s2",
		"tool_call_count":4,
		"messages":"[{\"role\":\"user\",\"content\":\"do a complex task\"},{\"role\":\"assistant\",\"content\":\"\",\"tool_calls\":[{\"id\":\"1\",\"name\":\"calculator\"},{\"id\":\"2\",\"name\":\"memory_store\"},{\"id\":\"3\",\"name\":\"http_request\"}]}]"
	}`)
	// Should either learn or detect pattern too few words
	_ = parseResp(t, rawLearn) // just verify no panic
}

// ── MemoryActor tests ─────────────────────────────────────────────────────────

func TestMemoryTieredStorage(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newMemoryActor()
	initActor(actor, "memory:test@node")

	// Core tier
	coreResp := parseResp(t, actor.Handle("caller", "store_memory", `{"op":"store_memory","tier":"core","key":"user_name","value":"Alice","scope":"global"}`))
	assertOK(t, coreResp, "store core")
	if actor.CoreCount != 1 {
		t.Errorf("expected CoreCount=1, got %d", actor.CoreCount)
	}

	// Reachable tier
	reachResp := parseResp(t, actor.Handle("caller", "store_memory", `{"op":"store_memory","tier":"reachable","key":"last_topic","value":"AI","scope":"global"}`))
	assertOK(t, reachResp, "store reachable")
	if actor.ReachableCount != 1 {
		t.Errorf("expected ReachableCount=1, got %d", actor.ReachableCount)
	}

	// Recall
	recallResp := parseResp(t, actor.Handle("caller", "recall_memory", `{"op":"recall_memory","query":"user","scope":"global"}`))
	assertOK(t, recallResp, "recall")
	count, _ := recallResp["count"].(float64)
	if int(count) == 0 {
		t.Error("expected at least 1 recalled memory")
	}
}

func TestMemoryDelete(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newMemoryActor()
	initActor(actor, "memory:test@node")

	actor.Handle("caller", "store_memory", `{"op":"store_memory","tier":"core","key":"temp","value":"temporary","scope":"global"}`)
	delResp := parseResp(t, actor.Handle("caller", "delete_memory", `{"op":"delete_memory","tier":"core","key":"temp","scope":"global"}`))
	assertOK(t, delResp, "delete_memory")
}

// ── ContextCompressorActor tests ──────────────────────────────────────────────

func TestContextCompressorSmallHistory(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newContextCompressorActor()
	initActor(actor, "compressor:test@node")

	// Too few messages → no compression
	raw := actor.Handle("caller", "compress", `{
		"op":"compress",
		"session_id":"s1",
		"messages":"[{\"role\":\"user\",\"content\":\"hi\"},{\"role\":\"assistant\",\"content\":\"hello\"}]",
		"keep_last":4
	}`)
	resp := parseResp(t, raw)
	assertOK(t, resp, "compress small")
	if resp["action"] != "no_compression_needed" {
		t.Errorf("expected no_compression_needed, got %v", resp["action"])
	}
}

func TestContextCompressorLargeHistory(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newContextCompressorActor()
	initActor(actor, "compressor:test@node")

	// Build 12 messages (> keep_last+2=6 to trigger real compression)
	msgs := `[`
	for i := 0; i < 12; i++ {
		if i > 0 {
			msgs += ","
		}
		role := "user"
		if i%2 == 1 {
			role = "assistant"
		}
		msgs += `{"role":"` + role + `","content":"message ` + fmt.Sprintf("%d", i) + `"}`
	}
	msgs += `]`
	msgsEscaped, _ := json.Marshal(msgs)

	raw := actor.Handle("caller", "compress", `{
		"op":"compress",
		"session_id":"s2",
		"messages":` + string(msgsEscaped) + `,
		"keep_last":4
	}`)
	resp := parseResp(t, raw)
	assertOK(t, resp, "compress large")
	before, _ := resp["before_messages"].(float64)
	after, _ := resp["after_messages"].(float64)
	if int(after) >= int(before) {
		t.Errorf("expected compression: after(%d) < before(%d)", int(after), int(before))
	}
	if actor.CompressCount != 1 {
		t.Errorf("expected CompressCount=1, got %d", actor.CompressCount)
	}
}

// ── CronSchedulerActor tests ──────────────────────────────────────────────────

func TestCronSchedulerCreateJob(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newCronSchedulerActor()
	initActorWithArgs(actor, "cron:test@node", map[string]string{"tick_interval_ms": "60000"})

	raw := actor.Handle("caller", "create_job", `{
		"op":"create_job",
		"job_id":"test-job",
		"prompt":"Do something useful",
		"schedule":"every_1h"
	}`)
	resp := parseResp(t, raw)
	assertOK(t, resp, "create_job")
	if resp["job_id"] != "test-job" {
		t.Errorf("expected job_id=test-job, got %v", resp["job_id"])
	}
	intervalMs, _ := resp["interval_ms"].(float64)
	if uint64(intervalMs) != scheduleToMs("every_1h") {
		t.Errorf("unexpected interval_ms: %v", intervalMs)
	}
	if actor.JobCount != 1 {
		t.Errorf("expected JobCount=1, got %d", actor.JobCount)
	}
}

func TestCronSchedulerListJobs(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newCronSchedulerActor()
	initActor(actor, "cron:test@node")

	actor.Handle("caller", "create_job", `{"op":"create_job","job_id":"j1","prompt":"Task 1","schedule":"every_5m"}`)
	actor.Handle("caller", "create_job", `{"op":"create_job","job_id":"j2","prompt":"Task 2","schedule":"every_1h"}`)

	raw := actor.Handle("caller", "list_jobs", `{"op":"list_jobs"}`)
	resp := parseResp(t, raw)
	assertOK(t, resp, "list_jobs")
	count, _ := resp["count"].(float64)
	if int(count) != 2 {
		t.Errorf("expected 2 jobs, got %d", int(count))
	}
}

func TestCronSchedulerDeleteJob(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newCronSchedulerActor()
	initActor(actor, "cron:test@node")

	actor.Handle("caller", "create_job", `{"op":"create_job","job_id":"to-delete","prompt":"Delete me","schedule":"every_1h"}`)
	delResp := parseResp(t, actor.Handle("caller", "delete_job", `{"op":"delete_job","job_id":"to-delete"}`))
	assertOK(t, delResp, "delete_job")
}

func TestCronSchedulerTick(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newCronSchedulerActor()
	initActorWithArgs(actor, "cron:test@node", map[string]string{"tick_interval_ms": "1000"})

	actor.Handle("caller", "create_job", `{"op":"create_job","job_id":"ticktest","prompt":"Tick job","schedule":"every_1m"}`)

	// Force tick — with stub host, lock acquisition succeeds
	raw := actor.Handle("caller", "trigger_tick", `{"op":"trigger_tick"}`)
	resp := parseResp(t, raw)
	assertOK(t, resp, "trigger_tick")
	if actor.TickCount != 1 {
		t.Errorf("expected TickCount=1, got %d", actor.TickCount)
	}
}

func TestScheduleToMs(t *testing.T) {
	tests := map[string]uint64{
		"every_1m":  60 * 1000,
		"every_5m":  5 * 60 * 1000,
		"every_1h":  3600 * 1000,
		"every_24h": 24 * 3600 * 1000,
		"unknown":   3600 * 1000, // default
	}
	for schedule, expected := range tests {
		got := scheduleToMs(schedule)
		if got != expected {
			t.Errorf("scheduleToMs(%q): expected %d got %d", schedule, expected, got)
		}
	}
}

// ── GuardrailsGateActor tests ─────────────────────────────────────────────────

func TestGuardrailsAllowSafeTool(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newGuardrailsGateActor()
	initActor(actor, "guardrails:test@node")

	raw := actor.Handle("caller", "check", `{"op":"check","tool":"calculator"}`)
	resp := parseResp(t, raw)
	if resp["decision"] != "allow" {
		t.Errorf("expected decision=allow for calculator, got %v", resp["decision"])
	}
}

func TestGuardrailsReviewHttpRequest(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newGuardrailsGateActor()
	initActor(actor, "guardrails:test@node")

	raw := actor.Handle("caller", "check", `{"op":"check","tool":"http_request","input":{"url":"https://api.example.com/data"}}`)
	resp := parseResp(t, raw)
	if resp["decision"] != "requires_approval" {
		t.Errorf("expected requires_approval for http_request, got %v", resp["decision"])
	}
	if resp["approval_id"] == nil {
		t.Error("expected approval_id in review response")
	}
}

func TestGuardrailsDenyDestructive(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newGuardrailsGateActor()
	initActor(actor, "guardrails:test@node")

	raw := actor.Handle("caller", "check", `{"op":"check","tool":"delete_file"}`)
	resp := parseResp(t, raw)
	if resp["decision"] != "deny" {
		t.Errorf("expected deny for delete_file, got %v", resp["decision"])
	}
}

func TestGuardrailsApproveFlow(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newGuardrailsGateActor()
	initActor(actor, "guardrails:test@node")

	checkResp := parseResp(t, actor.Handle("caller", "check", `{"op":"check","tool":"http_request"}`))
	approvalID, _ := checkResp["approval_id"].(string)
	if approvalID == "" {
		t.Skip("no approval_id returned (tool may not require review)")
	}

	approveResp := parseResp(t, actor.Handle("caller", "approve", `{"op":"approve","approval_id":"`+approvalID+`"}`))
	assertOK(t, approveResp, "approve")
	if approveResp["decision"] != "approved" {
		t.Errorf("expected decision=approved, got %v", approveResp["decision"])
	}
	if actor.ApprovalCount != 1 {
		t.Errorf("expected ApprovalCount=1, got %d", actor.ApprovalCount)
	}
}

func TestGuardrailsSetPolicy(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newGuardrailsGateActor()
	initActor(actor, "guardrails:test@node")

	// Set custom tool to deny
	actor.Handle("caller", "set_policy", `{"op":"set_policy","tool":"custom_danger_tool","policy":"deny"}`)

	raw := actor.Handle("caller", "check", `{"op":"check","tool":"custom_danger_tool"}`)
	resp := parseResp(t, raw)
	if resp["decision"] != "deny" {
		t.Errorf("expected deny after set_policy, got %v", resp["decision"])
	}
}

// ── SessionManagerActor tests ─────────────────────────────────────────────────

func TestSessionManagerCreateAndGet(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newSessionManagerActor()
	initActor(actor, "sessions:test@node")

	createResp := parseResp(t, actor.Handle("caller", "create_session", `{"op":"create_session","channel":"web","user_id":"alice"}`))
	assertOK(t, createResp, "create_session")
	sessionID, _ := createResp["session_id"].(string)
	if sessionID == "" {
		t.Fatal("expected non-empty session_id")
	}

	getResp := parseResp(t, actor.Handle("caller", "get_session", `{"op":"get_session","session_id":"`+sessionID+`"}`))
	if getResp["session_id"] != sessionID {
		t.Errorf("expected session_id=%s, got %v", sessionID, getResp["session_id"])
	}
}

func TestSessionManagerEndSession(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newSessionManagerActor()
	initActor(actor, "sessions:test@node")

	createResp := parseResp(t, actor.Handle("caller", "create_session", `{"op":"create_session","channel":"cli","user_id":"bob"}`))
	sessionID, _ := createResp["session_id"].(string)

	endResp := parseResp(t, actor.Handle("caller", "end_session", `{"op":"end_session","session_id":"`+sessionID+`"}`))
	assertOK(t, endResp, "end_session")
	if actor.ActiveSessions != 0 {
		t.Errorf("expected ActiveSessions=0, got %d", actor.ActiveSessions)
	}
}

// ── AuditEventActor tests ─────────────────────────────────────────────────────

func TestAuditEventLogAndPoll(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newAuditEventActor()
	initActor(actor, "audit:test@node")

	actor.Handle("caller", "log_event", `{"op":"log_event","event_type":"test_event","detail":"test detail"}`)
	actor.Handle("caller", "log_event", `{"op":"log_event","event_type":"another_event","detail":"more detail"}`)

	raw := actor.Handle("caller", "poll_events", `{"op":"poll_events","consumer_id":"test","limit":10}`)
	resp := parseResp(t, raw)
	assertOK(t, resp, "poll_events")
	count, _ := resp["count"].(float64)
	if int(count) != 2 {
		t.Errorf("expected 2 events, got %d", int(count))
	}
	if actor.Watermark != 2 {
		t.Errorf("expected Watermark=2, got %d", actor.Watermark)
	}
}

func TestAuditEventWatermarkCursor(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newAuditEventActor()
	initActor(actor, "audit:test@node")

	actor.Handle("caller", "log_event", `{"op":"log_event","event_type":"e1","detail":"d1"}`)
	actor.Handle("caller", "log_event", `{"op":"log_event","event_type":"e2","detail":"d2"}`)
	actor.Handle("caller", "log_event", `{"op":"log_event","event_type":"e3","detail":"d3"}`)

	// First poll: should get all 3
	first := parseResp(t, actor.Handle("caller", "poll_events", `{"op":"poll_events","consumer_id":"c1","limit":10}`))
	firstCount, _ := first["count"].(float64)
	if int(firstCount) != 3 {
		t.Errorf("expected 3 on first poll, got %d", int(firstCount))
	}

	// Second poll: no new events, cursor at 3
	second := parseResp(t, actor.Handle("caller", "poll_events", `{"op":"poll_events","consumer_id":"c1","limit":10}`))
	secondCount, _ := second["count"].(float64)
	if int(secondCount) != 0 {
		t.Errorf("expected 0 on second poll (cursor up to date), got %d", int(secondCount))
	}
}

// ── HealthMonitorActor tests ──────────────────────────────────────────────────

func TestHealthMonitorPoll(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newHealthMonitorActor()
	initActorWithArgs(actor, "health:test@node", map[string]string{"poll_interval_ms": "10000"})

	raw := actor.Handle("caller", "poll_tick", `{"op":"poll_tick"}`)
	resp := parseResp(t, raw)
	assertOK(t, resp, "poll_tick")
	if actor.PollCount != 1 {
		t.Errorf("expected PollCount=1, got %d", actor.PollCount)
	}
}

func TestHealthMonitorGetHealth(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newHealthMonitorActor()
	initActor(actor, "health:test@node")

	actor.Handle("caller", "poll_tick", `{"op":"poll_tick"}`)
	raw := actor.Handle("caller", "get_health", `{"op":"get_health"}`)
	resp := parseResp(t, raw)
	assertOK(t, resp, "get_health")
	if resp["group_health"] == nil {
		t.Error("expected group_health in response")
	}
}

// ── Helper function tests ─────────────────────────────────────────────────────

func TestEvalExpression(t *testing.T) {
	tests := []struct {
		expr   string
		expect float64
		hasErr bool
	}{
		{"42 * 17", 714, false},
		{"10 + 5", 15, false},
		{"20 - 8", 12, false},
		{"100 / 4", 25, false},
		{"0", 0, false},
		{"abc", 0, true},
		{"10 / 0", 0, true},
	}
	for _, tc := range tests {
		result, err := evalExpression(tc.expr)
		if tc.hasErr {
			if err == nil {
				t.Errorf("evalExpression(%q): expected error", tc.expr)
			}
		} else {
			if err != nil {
				t.Errorf("evalExpression(%q): unexpected error: %v", tc.expr, err)
			}
			if result != tc.expect {
				t.Errorf("evalExpression(%q): expected %v got %v", tc.expr, tc.expect, result)
			}
		}
	}
}

func TestCacheKeyFor(t *testing.T) {
	k1 := llmCacheKeyFor("hello world")
	k2 := llmCacheKeyFor("hello world")
	k3 := llmCacheKeyFor("different message")
	if k1 != k2 {
		t.Error("same message should produce same cache key")
	}
	if k1 == k3 {
		t.Error("different messages should (likely) produce different cache keys")
	}
}

func TestContainsMathPattern(t *testing.T) {
	if !containsMathPattern("calculate 42 * 17") {
		t.Error("should detect math pattern")
	}
	if containsMathPattern("just a normal sentence") {
		t.Error("should not detect math in normal text")
	}
}

func TestTruncateStr(t *testing.T) {
	s := truncateStr("hello world", 5)
	if !strings.HasPrefix(s, "hello") {
		t.Errorf("expected truncated string to start with 'hello', got %q", s)
	}
	s2 := truncateStr("short", 100)
	if s2 != "short" {
		t.Errorf("should not truncate short string, got %q", s2)
	}
}
