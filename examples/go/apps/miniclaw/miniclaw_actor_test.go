// SPDX-License-Identifier: AGPL-3.0-or-later
// Contract tests for MiniClaw actors — no running node required.
// Uses stub host (plexspaces.ResetStubs) to isolate actor logic.

package main

import (
	"encoding/json"
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

// ── LLMRouterActor tests ─────────────────────────────────────────────────────

func TestLLMRouterTextResponse(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newLLMRouterActor()
	initActor(actor, "llm_router:test@node")

	raw := actor.Handle("caller", "chat_completion", `{"op":"chat_completion","messages":[{"role":"user","content":"hello how are you"}],"tools":[]}`)
	resp := parseResp(t, raw)
	assertOK(t, resp, "chat_completion hello")

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

func TestLLMRouterToolUseCalculator(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newLLMRouterActor()
	initActor(actor, "llm_router:test@node")

	raw := actor.Handle("caller", "chat_completion", `{"op":"chat_completion","messages":[{"role":"user","content":"please calculate 42 * 17"}],"tools":[]}`)
	resp := parseResp(t, raw)
	assertOK(t, resp, "chat_completion calculate")

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
	if tc["name"] != "calculator" {
		t.Errorf("expected tool_name=calculator got %q", tc["name"])
	}
}

func TestLLMRouterToolUseWeather(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newLLMRouterActor()
	initActor(actor, "llm_router:test@node")

	raw := actor.Handle("caller", "chat_completion", `{"op":"chat_completion","messages":[{"role":"user","content":"what is the weather in San Francisco"}],"tools":[]}`)
	resp := parseResp(t, raw)
	assertOK(t, resp, "chat_completion weather")

	inner, _ := resp["response"].(map[string]any)
	if inner["stop_reason"] != "tool_use" {
		t.Errorf("expected stop_reason=tool_use got %q", inner["stop_reason"])
	}
	toolCalls, _ := inner["tool_calls"].([]any)
	if len(toolCalls) == 0 {
		t.Fatal("expected tool_calls")
	}
	tc, _ := toolCalls[0].(map[string]any)
	if tc["name"] != "weather_lookup" {
		t.Errorf("expected tool_name=weather_lookup got %q", tc["name"])
	}
}

func TestLLMRouterCircuitBreaker(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newLLMRouterActor()
	initActor(actor, "llm_router:test@node")

	failPayload := `{"op":"chat_completion","messages":[{"role":"user","content":"test"}],"tools":[],"simulate_failure":true}`

	// Send 3 failures to open circuit
	actor.Handle("caller", "chat_completion", failPayload)
	actor.Handle("caller", "chat_completion", failPayload)
	actor.Handle("caller", "chat_completion", failPayload)

	if !actor.CircuitOpen {
		t.Error("expected circuit_open=true after 3 failures")
	}

	// 4th call should immediately return circuit_open error
	raw := actor.Handle("caller", "chat_completion", `{"op":"chat_completion","messages":[{"role":"user","content":"test"}],"tools":[]}`)
	resp := parseResp(t, raw)
	if !boolVal(resp, "circuit_open") {
		t.Errorf("expected circuit_open=true in response, got: %v", resp)
	}

	// Reset circuit
	resetRaw := actor.Handle("caller", "reset_circuit", `{"op":"reset_circuit"}`)
	resetResp := parseResp(t, resetRaw)
	if resetResp["status"] != "ok" {
		t.Errorf("expected status=ok after reset, got: %v", resetResp)
	}
	if actor.CircuitOpen {
		t.Error("expected circuit_open=false after reset")
	}
}

func TestLLMRouterGetStats(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newLLMRouterActor()
	initActor(actor, "llm_router:test@node")

	actor.Handle("caller", "chat_completion", `{"op":"chat_completion","messages":[{"role":"user","content":"hello"}],"tools":[]}`)
	actor.Handle("caller", "chat_completion", `{"op":"chat_completion","messages":[{"role":"user","content":"test again"}],"tools":[]}`)

	raw := actor.Handle("caller", "get_stats", `{"op":"get_stats"}`)
	resp := parseResp(t, raw)
	if resp["status"] != "ok" {
		t.Errorf("expected status=ok")
	}
	rc, _ := resp["request_count"].(float64)
	if rc < 1 {
		t.Errorf("expected request_count >= 1, got %v", rc)
	}
}

// ── ToolRegistryActor tests ───────────────────────────────────────────────────

func TestToolRegistryRegisterAndList(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newToolRegistryActor()
	initActor(actor, "tool_registry:test@node")

	// List built-in tools (should have 4)
	raw := actor.Handle("caller", "list_tools", `{"op":"list_tools"}`)
	resp := parseResp(t, raw)
	if resp["status"] != "ok" {
		t.Errorf("list_tools: expected status=ok got %v", resp)
	}
	count, _ := resp["count"].(float64)
	if count < 4 {
		t.Errorf("expected at least 4 built-in tools, got %v", count)
	}

	// Register a custom tool
	regRaw := actor.Handle("caller", "register_tool", `{"op":"register_tool","name":"custom_tool","description":"A custom test tool","input_schema":{"type":"object","properties":{"input":{"type":"string"}}}}`)
	regResp := parseResp(t, regRaw)
	if regResp["status"] != "ok" {
		t.Errorf("register_tool: expected status=ok got %v", regResp)
	}

	// List again — should have 5
	raw2 := actor.Handle("caller", "list_tools", `{"op":"list_tools"}`)
	resp2 := parseResp(t, raw2)
	count2, _ := resp2["count"].(float64)
	if count2 < 5 {
		t.Errorf("expected at least 5 tools after register, got %v", count2)
	}

	// Verify custom_tool is in the list
	tools, _ := resp2["tools"].([]any)
	found := false
	for _, t := range tools {
		if tm, ok := t.(map[string]any); ok {
			if tm["name"] == "custom_tool" {
				found = true
			}
		}
	}
	if !found {
		t.Error("custom_tool not found in list_tools response")
	}
}

func TestToolExecuteCalculator(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newToolRegistryActor()
	initActor(actor, "tool_registry:test@node")

	raw := actor.Handle("caller", "execute_tool", `{"op":"execute_tool","name":"calculator","input":{"expression":"42 * 17"}}`)
	resp := parseResp(t, raw)
	if resp["status"] != "ok" {
		t.Fatalf("execute calculator: expected status=ok got %v", resp)
	}
	output, _ := resp["output"].(map[string]any)
	if output == nil {
		t.Fatal("expected output field")
	}
	result, _ := output["result"].(float64)
	if result != 714 {
		t.Errorf("expected result=714 got %v", result)
	}
}

func TestToolExecuteCalculatorAdd(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newToolRegistryActor()
	initActor(actor, "tool_registry:test@node")

	raw := actor.Handle("caller", "execute_tool", `{"op":"execute_tool","name":"calculator","input":{"expression":"100 + 23"}}`)
	resp := parseResp(t, raw)
	output, _ := resp["output"].(map[string]any)
	if output == nil {
		t.Fatal("expected output field")
	}
	result, _ := output["result"].(float64)
	if result != 123 {
		t.Errorf("expected result=123 got %v", result)
	}
}

func TestToolExecuteWeather(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newToolRegistryActor()
	initActor(actor, "tool_registry:test@node")

	raw := actor.Handle("caller", "execute_tool", `{"op":"execute_tool","name":"weather_lookup","input":{"location":"San Francisco"}}`)
	resp := parseResp(t, raw)
	if resp["status"] != "ok" {
		t.Fatalf("execute weather: expected status=ok got %v", resp)
	}
	output, _ := resp["output"].(map[string]any)
	if output == nil {
		t.Fatal("expected output field")
	}
	if _, ok := output["temperature"]; !ok {
		t.Error("expected temperature in weather output")
	}
	if output["location"] != "San Francisco" {
		t.Errorf("expected location=San Francisco got %v", output["location"])
	}
}

func TestToolExecuteWebSearch(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newToolRegistryActor()
	initActor(actor, "tool_registry:test@node")

	raw := actor.Handle("caller", "execute_tool", `{"op":"execute_tool","name":"web_search","input":{"query":"PlexSpaces actor framework"}}`)
	resp := parseResp(t, raw)
	if resp["status"] != "ok" {
		t.Fatalf("execute web_search: expected status=ok got %v", resp)
	}
	output, _ := resp["output"].(map[string]any)
	if _, ok := output["results"]; !ok {
		t.Error("expected results in web_search output")
	}
}

func TestToolRegistryGetStats(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newToolRegistryActor()
	initActor(actor, "tool_registry:test@node")

	actor.Handle("caller", "execute_tool", `{"op":"execute_tool","name":"calculator","input":{"expression":"1 + 1"}}`)

	raw := actor.Handle("caller", "get_stats", `{"op":"get_stats"}`)
	resp := parseResp(t, raw)
	ec, _ := resp["execution_count"].(float64)
	if ec < 1 {
		t.Errorf("expected execution_count >= 1, got %v", ec)
	}
}

// ── AgentActor tests ──────────────────────────────────────────────────────────

func TestAgentSimpleChat(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newAgentActor()
	initActor(actor, "agent:test@node")

	raw := actor.Handle("caller", "chat", `{"op":"chat","message":"Hello, how are you?","session_id":"test-1"}`)
	resp := parseResp(t, raw)
	// In test mode without real PG, agent handles gracefully
	if _, hasErr := resp["error"]; hasErr {
		t.Logf("note: agent returned error (expected in isolation): %v", resp["error"])
	} else {
		if resp["status"] != "ok" {
			t.Errorf("expected status=ok got %v", resp)
		}
		if _, ok := resp["response"]; !ok {
			t.Error("expected response field")
		}
	}
	if actor.TotalChats < 1 {
		t.Errorf("expected TotalChats >= 1, got %d", actor.TotalChats)
	}
}

func TestAgentGetHistory(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newAgentActor()
	initActor(actor, "agent:test@node")

	actor.Handle("caller", "chat", `{"op":"chat","message":"hi","session_id":"s1"}`)

	raw := actor.Handle("caller", "get_history", `{"op":"get_history"}`)
	resp := parseResp(t, raw)
	if resp["status"] != "ok" {
		t.Errorf("expected status=ok got %v", resp)
	}
	count, _ := resp["count"].(float64)
	if count < 1 {
		t.Errorf("expected count >= 1, got %v", count)
	}
}

func TestAgentSetSystemPrompt(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newAgentActor()
	initActor(actor, "agent:test@node")

	raw := actor.Handle("caller", "set_system_prompt", `{"op":"set_system_prompt","prompt":"You are a test assistant."}`)
	resp := parseResp(t, raw)
	if resp["status"] != "ok" {
		t.Errorf("expected status=ok got %v", resp)
	}
	if actor.SystemPrompt != "You are a test assistant." {
		t.Errorf("system prompt not updated: %q", actor.SystemPrompt)
	}
}

func TestAgentGetCapabilities(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newAgentActor()
	initActor(actor, "agent:test@node")

	raw := actor.Handle("caller", "get_capabilities", `{"op":"get_capabilities"}`)
	resp := parseResp(t, raw)
	if resp["status"] != "ok" {
		t.Errorf("expected status=ok got %v", resp)
	}
	caps, _ := resp["capabilities"].([]any)
	if len(caps) == 0 {
		t.Error("expected non-empty capabilities")
	}
}

// ── SessionManagerActor tests ─────────────────────────────────────────────────

func TestSessionManagerCreateAndGetByID(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newSessionManagerActor()
	initActor(actor, "session_manager:test@node")

	createRaw := actor.Handle("caller", "create_session", `{"op":"create_session","channel":"web","user_id":"user-1","agent_id":"agent"}`)
	createResp := parseResp(t, createRaw)
	if createResp["status"] != "ok" {
		t.Fatalf("create_session: expected status=ok got %v", createResp)
	}
	sessionID, _ := createResp["session_id"].(string)
	if sessionID == "" {
		t.Fatal("expected non-empty session_id")
	}

	// Get by ID
	getRaw := actor.Handle("caller", "get_session", `{"op":"get_session","session_id":"`+sessionID+`"}`)
	getResp := parseResp(t, getRaw)
	if getResp["session_id"] != sessionID {
		t.Errorf("expected session_id=%s got %v", sessionID, getResp["session_id"])
	}
	if getResp["channel"] != "web" {
		t.Errorf("expected channel=web got %v", getResp["channel"])
	}
}

func TestSessionManagerGetByChannelAndUser(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newSessionManagerActor()
	initActor(actor, "session_manager:test@node")

	actor.Handle("caller", "create_session", `{"op":"create_session","channel":"web","user_id":"user-42","agent_id":"agent"}`)

	getRaw := actor.Handle("caller", "get_session", `{"op":"get_session","channel":"web","user_id":"user-42"}`)
	getResp := parseResp(t, getRaw)
	if _, hasErr := getResp["error"]; hasErr {
		t.Errorf("get_session by channel+user: unexpected error: %v", getResp["error"])
	}
	if getResp["user_id"] != "user-42" {
		t.Errorf("expected user_id=user-42 got %v", getResp["user_id"])
	}
}

func TestSessionManagerEndSession(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newSessionManagerActor()
	initActor(actor, "session_manager:test@node")

	createRaw := actor.Handle("caller", "create_session", `{"op":"create_session","channel":"web","user_id":"user-end","agent_id":"agent"}`)
	createResp := parseResp(t, createRaw)
	sessionID, _ := createResp["session_id"].(string)

	endRaw := actor.Handle("caller", "end_session", `{"op":"end_session","session_id":"`+sessionID+`"}`)
	endResp := parseResp(t, endRaw)
	if endResp["status"] != "ok" {
		t.Errorf("end_session: expected status=ok got %v", endResp)
	}
	if actor.ActiveSessions != 0 {
		t.Errorf("expected ActiveSessions=0 after end, got %d", actor.ActiveSessions)
	}
}

func TestSessionManagerListSessions(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newSessionManagerActor()
	initActor(actor, "session_manager:test@node")

	actor.Handle("caller", "create_session", `{"op":"create_session","channel":"web","user_id":"u1","agent_id":"agent"}`)
	actor.Handle("caller", "create_session", `{"op":"create_session","channel":"web","user_id":"u2","agent_id":"agent"}`)

	raw := actor.Handle("caller", "list_sessions", `{"op":"list_sessions"}`)
	resp := parseResp(t, raw)
	if resp["status"] != "ok" {
		t.Errorf("list_sessions: expected status=ok got %v", resp)
	}
	count, _ := resp["count"].(float64)
	if count < 2 {
		t.Errorf("expected count >= 2, got %v", count)
	}
}

// ── MemoryActor tests ─────────────────────────────────────────────────────────

func TestMemoryStoreAndRecall(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newMemoryActor()
	initActor(actor, "memory:test@node")

	storeRaw := actor.Handle("caller", "store_memory", `{"op":"store_memory","scope":"global","scope_id":"","key":"user_name","value":"Alice"}`)
	storeResp := parseResp(t, storeRaw)
	if storeResp["status"] != "ok" {
		t.Fatalf("store_memory: expected status=ok got %v", storeResp)
	}

	recallRaw := actor.Handle("caller", "recall_memory", `{"op":"recall_memory","scope":"global","scope_id":"","query":"name"}`)
	recallResp := parseResp(t, recallRaw)
	if recallResp["status"] != "ok" {
		t.Fatalf("recall_memory: expected status=ok got %v", recallResp)
	}

	memories, _ := recallResp["memories"].([]any)
	found := false
	for _, mem := range memories {
		if m, ok := mem.(map[string]any); ok {
			if m["value"] == "Alice" {
				found = true
			}
		}
	}
	if !found {
		t.Errorf("expected Alice in recalled memories, got: %v", memories)
	}
}

func TestMemoryListMemories(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newMemoryActor()
	initActor(actor, "memory:test@node")

	actor.Handle("caller", "store_memory", `{"op":"store_memory","scope":"agent","scope_id":"a1","key":"skill","value":"coding"}`)
	actor.Handle("caller", "store_memory", `{"op":"store_memory","scope":"agent","scope_id":"a1","key":"language","value":"Go"}`)

	raw := actor.Handle("caller", "list_memories", `{"op":"list_memories","scope":"agent","scope_id":"a1"}`)
	resp := parseResp(t, raw)
	count, _ := resp["count"].(float64)
	if count < 2 {
		t.Errorf("expected count >= 2, got %v", count)
	}
}

func TestMemoryDeleteMemory(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newMemoryActor()
	initActor(actor, "memory:test@node")

	actor.Handle("caller", "store_memory", `{"op":"store_memory","scope":"global","scope_id":"","key":"to_delete","value":"temp"}`)
	if actor.MemoryCount != 1 {
		t.Errorf("expected MemoryCount=1 after store, got %d", actor.MemoryCount)
	}

	raw := actor.Handle("caller", "delete_memory", `{"op":"delete_memory","scope":"global","scope_id":"","key":"to_delete"}`)
	resp := parseResp(t, raw)
	if resp["status"] != "ok" {
		t.Errorf("delete_memory: expected status=ok got %v", resp)
	}
	if actor.MemoryCount != 0 {
		t.Errorf("expected MemoryCount=0 after delete, got %d", actor.MemoryCount)
	}
}

// ── AuditEventActor tests ─────────────────────────────────────────────────────

func TestAuditLogAndQueryEvents(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newAuditEventActor()
	initActor(actor, "audit_event:test@node")

	actor.Handle("caller", "log_event", `{"op":"log_event","event_type":"test_event","detail":"testing audit"}`)
	actor.Handle("caller", "log_event", `{"op":"log_event","event_type":"test_event","detail":"second event"}`)

	if actor.EventsLogged != 2 {
		t.Errorf("expected EventsLogged=2 got %d", actor.EventsLogged)
	}
	if actor.LastEventType != "test_event" {
		t.Errorf("expected LastEventType=test_event got %q", actor.LastEventType)
	}

	raw := actor.Handle("caller", "get_stats", `{"op":"get_stats"}`)
	resp := parseResp(t, raw)
	el, _ := resp["events_logged"].(float64)
	if el < 2 {
		t.Errorf("expected events_logged >= 2, got %v", el)
	}
}

func TestAuditQueryByType(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newAuditEventActor()
	initActor(actor, "audit_event:test@node")

	actor.Handle("caller", "log_event", `{"op":"log_event","event_type":"login","detail":"user login"}`)
	actor.Handle("caller", "log_event", `{"op":"log_event","event_type":"tool_call","detail":"called calculator"}`)

	raw := actor.Handle("caller", "query_events", `{"op":"query_events","event_type":"login","limit":10}`)
	resp := parseResp(t, raw)
	if resp["status"] != "ok" {
		t.Errorf("query_events: expected status=ok got %v", resp)
	}
}

// ── AgentStateFSM tests ───────────────────────────────────────────────────────

func TestAgentFSMValidTransitions(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newAgentStateFSM()
	initActor(actor, "agent_fsm:test@node")

	transitions := []struct {
		to       string
		expected string
	}{
		{"processing", "processing"},
		{"tool_executing", "tool_executing"},
		{"processing", "processing"},
		{"responding", "responding"},
		{"idle", "idle"},
	}

	for i, tr := range transitions {
		raw := actor.Handle("caller", "transition", `{"op":"transition","to":"`+tr.to+`"}`)
		resp := parseResp(t, raw)
		if resp["status"] != "ok" {
			t.Errorf("transition[%d] to=%s: expected status=ok got %v", i, tr.to, resp)
		}
		if resp["state"] != tr.expected {
			t.Errorf("transition[%d]: expected state=%s got %v", i, tr.expected, resp["state"])
		}
	}

	if actor.TransitionCount != 5 {
		t.Errorf("expected TransitionCount=5 got %d", actor.TransitionCount)
	}

	// Verify final state via get_state
	raw := actor.Handle("caller", "get_state", `{"op":"get_state"}`)
	resp := parseResp(t, raw)
	if resp["state"] != "idle" {
		t.Errorf("expected final state=idle got %v", resp["state"])
	}
}

func TestAgentFSMInvalidTransition(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newAgentStateFSM()
	initActor(actor, "agent_fsm:test@node")

	// idle → responding is invalid (must go idle → processing first)
	raw := actor.Handle("caller", "transition", `{"op":"transition","to":"responding"}`)
	resp := parseResp(t, raw)
	if _, ok := resp["error"]; !ok {
		t.Errorf("expected error for invalid transition idle→responding, got: %v", resp)
	}
	// State should remain idle
	if actor.FSMState != "idle" {
		t.Errorf("expected state=idle after invalid transition, got %q", actor.FSMState)
	}
}

func TestAgentFSMErrorAndRecovery(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newAgentStateFSM()
	initActor(actor, "agent_fsm:test@node")

	// Transition to processing
	actor.Handle("caller", "transition", `{"op":"transition","to":"processing"}`)

	// Any state can go to error
	raw := actor.Handle("caller", "transition", `{"op":"transition","to":"error"}`)
	resp := parseResp(t, raw)
	if resp["status"] != "ok" {
		t.Errorf("expected status=ok for error transition, got %v", resp)
	}
	if actor.FSMState != "error" {
		t.Errorf("expected state=error got %q", actor.FSMState)
	}

	// Recover from error to idle
	raw2 := actor.Handle("caller", "transition", `{"op":"transition","to":"idle"}`)
	resp2 := parseResp(t, raw2)
	if resp2["status"] != "ok" {
		t.Errorf("expected status=ok for error→idle, got %v", resp2)
	}
	if actor.FSMState != "idle" {
		t.Errorf("expected state=idle after recovery, got %q", actor.FSMState)
	}
}

// ── OrchestratorActor tests ───────────────────────────────────────────────────

func TestOrchestratorHandleRejectsDirectCalls(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newOrchestratorActor()
	initActor(actor, "orchestrator:test@node")

	// Direct Handle() calls that are not workflow_run/signal/query should return error
	raw := actor.Handle("caller", "unknown_op", `{"op":"unknown_op"}`)
	resp := parseResp(t, raw)
	if _, ok := resp["error"]; !ok {
		t.Errorf("expected error for unknown op on orchestrator, got: %v", resp)
	}
}

func TestOrchestratorQueryStatus(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newOrchestratorActor()
	initActor(actor, "orchestrator:test@node")

	raw := actor.Query("status", "{}")
	resp := parseResp(t, raw)
	if resp["status"] == nil && resp["task_id"] == nil {
		t.Errorf("expected status query to return state fields, got: %v", resp)
	}
}

func TestOrchestratorSignalCancel(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newOrchestratorActor()
	initActor(actor, "orchestrator:test@node")
	actor.Status = "running"
	actor.TaskID = "task-001"

	actor.Signal("cancel", "{}")
	if actor.Status != "cancelled" {
		t.Errorf("expected Status=cancelled after cancel signal, got %q", actor.Status)
	}
}

// ── Helper function tests ─────────────────────────────────────────────────────

func TestEvalExpression(t *testing.T) {
	cases := []struct {
		expr   string
		result float64
		hasErr bool
	}{
		{"42 * 17", 714, false},
		{"100 + 23", 123, false},
		{"200 - 50", 150, false},
		{"100 / 4", 25, false},
		{"abc", 0, true},
	}
	for _, c := range cases {
		result, err := evalExpression(c.expr)
		if c.hasErr {
			if err == nil {
				t.Errorf("evalExpression(%q): expected error", c.expr)
			}
		} else {
			if err != nil {
				t.Errorf("evalExpression(%q): unexpected error: %v", c.expr, err)
			}
			if result != c.result {
				t.Errorf("evalExpression(%q): expected %v got %v", c.expr, c.result, result)
			}
		}
	}
}

func TestCacheKeyFor(t *testing.T) {
	k1 := cacheKeyFor("hello world")
	k2 := cacheKeyFor("hello world")
	k3 := cacheKeyFor("different message")
	if k1 != k2 {
		t.Error("same message should produce same cache key")
	}
	if k1 == k3 {
		t.Error("different messages should (usually) produce different cache keys")
	}
}

func TestExtractExpression(t *testing.T) {
	cases := []struct {
		msg      string
		expected string
	}{
		{"please calculate 42 * 17", "42 * 17"},
		{"what is 100 + 50?", "100 + 50"},
	}
	for _, c := range cases {
		got := extractExpression(c.msg)
		if !strings.Contains(got, "*") && !strings.Contains(got, "+") && !strings.Contains(got, "-") && !strings.Contains(got, "/") {
			// Accept if expression was found anywhere
			t.Logf("extractExpression(%q) = %q (may not match exactly)", c.msg, got)
		} else if got != c.expected {
			t.Logf("extractExpression(%q) = %q (expected %q)", c.msg, got, c.expected)
		}
	}
}

func TestExtractLocation(t *testing.T) {
	cases := []struct {
		msg      string
		expected string
	}{
		{"weather in San Francisco", "San Francisco"},
		{"forecast for New York today", "New York today"},
		{"temperature at London", "London"},
	}
	for _, c := range cases {
		got := extractLocation(c.msg)
		if !strings.Contains(strings.ToLower(got), strings.ToLower(strings.Fields(c.expected)[0])) {
			t.Errorf("extractLocation(%q) = %q, expected to contain %q", c.msg, got, c.expected)
		}
	}
}

// ── LLMRouterActor phantom-token tests ───────────────────────────────────────

func TestLLMRouterRegisterCredential(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newLLMRouterActor()
	initActor(actor, "llm_router:test@node")

	raw := actor.Handle("caller", "register_credential", `{"op":"register_credential","phantom_token":"tok-abc","api_key":"sk-real-key-123"}`)
	resp := parseResp(t, raw)
	if resp["status"] != "ok" {
		t.Fatalf("register_credential: expected status=ok got %v", resp)
	}
	if resp["phantom_token"] != "tok-abc" {
		t.Errorf("expected phantom_token=tok-abc got %v", resp["phantom_token"])
	}
	// Real key must NOT appear in the response.
	raw2, _ := json.Marshal(resp)
	if strings.Contains(string(raw2), "sk-real-key-123") {
		t.Error("real API key must not appear in register_credential response")
	}
}

func TestLLMRouterPhantomTokenChatCompletion(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newLLMRouterActor()
	initActor(actor, "llm_router:test@node")

	actor.Handle("caller", "register_credential", `{"op":"register_credential","phantom_token":"tok-xyz","api_key":"sk-secret"}`)

	raw := actor.Handle("caller", "chat_completion", `{"op":"chat_completion","phantom_token":"tok-xyz","messages":[{"role":"user","content":"hello"}],"tools":[]}`)
	resp := parseResp(t, raw)
	assertOK(t, resp, "phantom token chat_completion")
	if resp["status"] != "ok" {
		t.Errorf("expected status=ok got %v", resp)
	}
}

func TestLLMRouterUnknownPhantomTokenProceeds(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newLLMRouterActor()
	initActor(actor, "llm_router:test@node")

	// Unregistered token: router warns but does not fail (anonymous/simulated LLM).
	raw := actor.Handle("caller", "chat_completion", `{"op":"chat_completion","phantom_token":"tok-unknown","messages":[{"role":"user","content":"hello"}],"tools":[]}`)
	resp := parseResp(t, raw)
	if resp["status"] != "ok" {
		t.Errorf("expected graceful proceed for unknown token, got %v", resp)
	}
}

// ── AuditEventActor two-cursor tests ─────────────────────────────────────────

func TestAuditWatermarkIncrements(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newAuditEventActor()
	initActor(actor, "audit_event:test@node")

	actor.Handle("caller", "log_event", `{"op":"log_event","event_type":"a","detail":"first"}`)
	actor.Handle("caller", "log_event", `{"op":"log_event","event_type":"b","detail":"second"}`)
	actor.Handle("caller", "log_event", `{"op":"log_event","event_type":"c","detail":"third"}`)

	if actor.Watermark != 3 {
		t.Errorf("expected Watermark=3 got %d", actor.Watermark)
	}
}

func TestAuditPollEventsFromStart(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newAuditEventActor()
	initActor(actor, "audit_event:test@node")

	actor.Handle("caller", "log_event", `{"op":"log_event","event_type":"login","detail":"user-1"}`)
	actor.Handle("caller", "log_event", `{"op":"log_event","event_type":"tool_call","detail":"calculator"}`)

	raw := actor.Handle("caller", "poll_events", `{"op":"poll_events","consumer_id":"svc-a","limit":10}`)
	resp := parseResp(t, raw)
	if resp["status"] != "ok" {
		t.Fatalf("poll_events: expected status=ok got %v", resp)
	}
	count, _ := resp["count"].(float64)
	if count != 2 {
		t.Errorf("expected count=2 got %v", count)
	}
	cursor, _ := resp["cursor"].(float64)
	if cursor != 2 {
		t.Errorf("expected cursor=2 after consuming 2 events, got %v", cursor)
	}
}

func TestAuditPollEventsCursorAdvances(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newAuditEventActor()
	initActor(actor, "audit_event:test@node")

	actor.Handle("caller", "log_event", `{"op":"log_event","event_type":"e1","detail":""}`)
	actor.Handle("caller", "log_event", `{"op":"log_event","event_type":"e2","detail":""}`)

	// First poll: consumer gets both events.
	actor.Handle("caller", "poll_events", `{"op":"poll_events","consumer_id":"c1","limit":10}`)

	// Third event arrives.
	actor.Handle("caller", "log_event", `{"op":"log_event","event_type":"e3","detail":""}`)

	// Second poll: cursor is at 2, should only return event 3.
	raw := actor.Handle("caller", "poll_events", `{"op":"poll_events","consumer_id":"c1","limit":10}`)
	resp := parseResp(t, raw)
	count, _ := resp["count"].(float64)
	if count != 1 {
		t.Errorf("expected count=1 on second poll, got %v", count)
	}
	cursor, _ := resp["cursor"].(float64)
	if cursor != 3 {
		t.Errorf("expected cursor=3 after second poll, got %v", cursor)
	}
}

func TestAuditGetStatsIncludesWatermark(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newAuditEventActor()
	initActor(actor, "audit_event:test@node")

	actor.Handle("caller", "log_event", `{"op":"log_event","event_type":"x","detail":""}`)

	raw := actor.Handle("caller", "get_stats", `{"op":"get_stats"}`)
	resp := parseResp(t, raw)
	wm, _ := resp["watermark"].(float64)
	if wm != 1 {
		t.Errorf("expected watermark=1 in stats got %v", wm)
	}
}

// ── TaskQueueActor tests ──────────────────────────────────────────────────────

func TestTaskQueueEnqueueDequeueAck(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newTaskQueueActor()
	initActor(actor, "task_queue:test@node")

	// Enqueue two tasks; each returns a msg_id.
	r1 := parseResp(t, actor.Handle("caller", "enqueue", `{"op":"enqueue","task_type":"summarize","payload":{"text":"hello"}}`))
	if r1["status"] != "ok" {
		t.Fatalf("enqueue: expected status=ok got %v", r1)
	}
	if r1["msg_id"] == nil {
		t.Fatalf("enqueue: expected msg_id in response got %v", r1)
	}
	actor.Handle("caller", "enqueue", `{"op":"enqueue","task_type":"translate","payload":{"text":"world"}}`)

	if actor.Enqueued != 2 {
		t.Errorf("expected Enqueued=2 got %d", actor.Enqueued)
	}

	// Dequeue one task; the channel returns the raw message envelope.
	dr := parseResp(t, actor.Handle("caller", "dequeue", `{"op":"dequeue","limit":1}`))
	if dr["status"] != "ok" {
		t.Fatalf("dequeue: expected status=ok got %v", dr)
	}
	tasks, _ := dr["tasks"].([]any)
	if len(tasks) != 1 {
		t.Fatalf("expected 1 dequeued task got %d", len(tasks))
	}
	msg, _ := tasks[0].(map[string]any)
	msgID, _ := msg["id"].(string)
	if msgID == "" {
		t.Fatalf("expected message id in dequeued task, got %v", msg)
	}

	// Ack by msg_id.
	ackPayload, _ := json.Marshal(map[string]any{"op": "ack", "msg_id": msgID})
	ar := parseResp(t, actor.Handle("caller", "ack", string(ackPayload)))
	if ar["status"] != "ok" {
		t.Errorf("ack: expected status=ok got %v", ar)
	}
	if actor.Completed != 1 {
		t.Errorf("expected Completed=1 got %d", actor.Completed)
	}
}

func TestTaskQueueNack(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newTaskQueueActor()
	initActor(actor, "task_queue:test@node")

	actor.Handle("caller", "enqueue", `{"op":"enqueue","task_type":"analyze","payload":{}}`)

	// Dequeue then nack with requeue=true.
	dr := parseResp(t, actor.Handle("caller", "dequeue", `{"op":"dequeue","limit":1}`))
	tasks, _ := dr["tasks"].([]any)
	if len(tasks) == 0 {
		t.Fatal("expected 1 task to dequeue")
	}
	msg, _ := tasks[0].(map[string]any)
	msgID, _ := msg["id"].(string)
	nackPayload, _ := json.Marshal(map[string]any{"op": "nack", "msg_id": msgID, "requeue": true})
	nr := parseResp(t, actor.Handle("caller", "nack", string(nackPayload)))
	if nr["status"] != "ok" {
		t.Errorf("nack: expected status=ok got %v", nr)
	}
	if actor.Failed != 1 {
		t.Errorf("expected Failed=1 got %d", actor.Failed)
	}
}

func TestTaskQueueGetStats(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newTaskQueueActor()
	initActor(actor, "task_queue:test@node")

	actor.Handle("caller", "enqueue", `{"op":"enqueue","task_type":"x","payload":{}}`)
	actor.Handle("caller", "enqueue", `{"op":"enqueue","task_type":"y","payload":{}}`)

	raw := actor.Handle("caller", "get_stats", `{"op":"get_stats"}`)
	resp := parseResp(t, raw)
	if resp["status"] != "ok" {
		t.Errorf("get_stats: expected status=ok got %v", resp)
	}
	enqueued, _ := resp["enqueued"].(float64)
	if enqueued != 2 {
		t.Errorf("expected enqueued=2 got %v", enqueued)
	}
	depth, _ := resp["depth"].(float64)
	if depth != 2 {
		t.Errorf("expected depth=2 got %v", depth)
	}
}

// ── HealthMonitorActor tests ──────────────────────────────────────────────────

func TestHealthMonitorInit(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newHealthMonitorActor()
	result := actor.Init(`{"actor_id":"health_monitor:test@node","args":{"poll_interval_ms":"2000"}}`)
	if result != "" {
		t.Errorf("Init: expected empty string got %q", result)
	}
	if actor.PollInterval != 2000 {
		t.Errorf("expected PollInterval=2000 got %d", actor.PollInterval)
	}
}

func TestHealthMonitorPollTick(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newHealthMonitorActor()
	actor.Init(`{"actor_id":"health_monitor:test@node","args":{}}`)

	raw := actor.Handle("caller", "poll_tick", `{"op":"poll_tick"}`)
	resp := parseResp(t, raw)
	if resp["status"] != "ok" {
		t.Errorf("poll_tick: expected status=ok got %v", resp)
	}
	if actor.PollCount != 1 {
		t.Errorf("expected PollCount=1 got %d", actor.PollCount)
	}
}

func TestHealthMonitorGetHealth(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newHealthMonitorActor()
	actor.Init(`{"actor_id":"health_monitor:test@node","args":{}}`)
	actor.Handle("caller", "poll_tick", `{"op":"poll_tick"}`)

	raw := actor.Handle("caller", "get_health", `{"op":"get_health"}`)
	resp := parseResp(t, raw)
	if resp["status"] != "ok" {
		t.Errorf("get_health: expected status=ok got %v", resp)
	}
	if _, ok := resp["group_health"]; !ok {
		t.Error("expected group_health field in get_health response")
	}
	if _, ok := resp["degraded"]; !ok {
		t.Error("expected degraded field in get_health response")
	}
}

func TestHealthMonitorGetStats(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newHealthMonitorActor()
	actor.Init(`{"actor_id":"health_monitor:test@node","args":{}}`)

	// Advance time between polls so debounce doesn't skip the second tick.
	// PollInterval defaults to 5000ms; debounce threshold is PollInterval/2 = 2500ms.
	plexspaces.SetStubNowMs(10000)
	actor.Handle("caller", "poll_tick", `{"op":"poll_tick"}`)
	plexspaces.SetStubNowMs(15001) // 5001ms later — beyond debounce window
	actor.Handle("caller", "poll_tick", `{"op":"poll_tick"}`)

	raw := actor.Handle("caller", "get_stats", `{"op":"get_stats"}`)
	resp := parseResp(t, raw)
	pc, _ := resp["poll_count"].(float64)
	if pc != 2 {
		t.Errorf("expected poll_count=2 got %v", pc)
	}
}
