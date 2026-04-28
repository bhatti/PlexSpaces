// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Unit tests for the PlexSpaces Go SDK.
// Tests cover: Actor interface, BaseActor, Host functions, ActorRouter,
// marshalPayload, error handling, and actor registration.

package plexspaces

import (
	"encoding/json"
	"strings"
	"testing"
)

// ========================================================================
// Test Actor Implementations
// ========================================================================

// CounterActor is a simple actor for testing.
type CounterActor struct {
	BaseActor
	Value int    `json:"value"`
	Name  string `json:"name"`
}

func newCounterActor() *CounterActor {
	a := &CounterActor{Name: "counter"}
	a.SetSelf(a)
	return a
}

func (c *CounterActor) Handle(from, msgType, payloadJSON string) string {
	switch msgType {
	case "increment":
		c.Value++
		data, _ := json.Marshal(map[string]any{"value": c.Value})
		return string(data)
	case "get":
		data, _ := json.Marshal(map[string]any{"value": c.Value})
		return string(data)
	case "echo":
		return payloadJSON
	default:
		return `{"error":"unknown"}`
	}
}

// EchoActor simply echoes messages back.
type EchoActor struct {
	BaseActor
	LastMsg string `json:"last_msg"`
}

func newEchoActor() *EchoActor {
	a := &EchoActor{}
	a.SetSelf(a)
	return a
}

func (e *EchoActor) Init(configJSON string) string {
	e.LastMsg = "initialized"
	return ""
}

func (e *EchoActor) Handle(from, msgType, payloadJSON string) string {
	e.LastMsg = msgType
	return payloadJSON
}

type WorkflowTestActor struct {
	BaseActor
	Status  string   `json:"status"`
	Signals []string `json:"signals"`
}

func newWorkflowTestActor() *WorkflowTestActor {
	a := &WorkflowTestActor{Status: "pending", Signals: []string{}}
	a.SetSelf(a)
	return a
}

func (w *WorkflowTestActor) Handle(from, msgType, payloadJSON string) string {
	return `{"error":"unexpected"}`
}

func (w *WorkflowTestActor) Run(payloadJSON string) string {
	w.Status = "running:o-1"
	return `{"status":"running:o-1"}`
}

func (w *WorkflowTestActor) Signal(name, payloadJSON string) {
	w.Signals = append(w.Signals, "cancel:user")
	w.Status = "cancelled"
}

func (w *WorkflowTestActor) Query(name, payloadJSON string) string {
	return `{"status":"cancelled","signals":["cancel:user"]}`
}

// ========================================================================
// Actor Interface Tests
// ========================================================================

func TestActorInterface(t *testing.T) {
	counter := newCounterActor()

	// Test that CounterActor implements Actor
	var _ Actor = counter

	// Test default Init (from BaseActor)
	result := counter.Init("{}")
	if result != "" {
		t.Errorf("Init should return empty string, got %q", result)
	}
}

func TestBaseActorGetState(t *testing.T) {
	counter := newCounterActor()
	counter.Value = 42
	counter.Name = "test"

	state := counter.GetState()
	var parsed map[string]any
	if err := json.Unmarshal([]byte(state), &parsed); err != nil {
		t.Fatalf("GetState should return valid JSON: %v", err)
	}

	if parsed["value"].(float64) != 42 {
		t.Errorf("expected value=42, got %v", parsed["value"])
	}
	if parsed["name"].(string) != "test" {
		t.Errorf("expected name=test, got %v", parsed["name"])
	}
}

func TestBaseActorSetState(t *testing.T) {
	counter := newCounterActor()
	err := counter.SetState(`{"value":99,"name":"restored"}`)
	if err != "" {
		t.Errorf("SetState should return empty string on success, got %q", err)
	}

	if counter.Value != 99 {
		t.Errorf("expected value=99 after SetState, got %d", counter.Value)
	}
	if counter.Name != "restored" {
		t.Errorf("expected name=restored after SetState, got %q", counter.Name)
	}
}

func TestBaseActorSetStateInvalidJSON(t *testing.T) {
	counter := newCounterActor()
	result := counter.SetState("not json")
	if !strings.HasPrefix(result, "ERROR:") {
		t.Errorf("SetState with invalid JSON should return ERROR:, got %q", result)
	}
}

func TestBaseActorGetStateWithoutSelf(t *testing.T) {
	// BaseActor without SetSelf should return "{}"
	actor := &BaseActor{}
	state := actor.GetState()
	if state != "{}" {
		t.Errorf("GetState without self should return {}, got %q", state)
	}
}

func TestBaseActorRuntimeMetadata(t *testing.T) {
	actor := &BaseActor{}
	actor.SetRuntimeMetadata("01KM1SX3YM67ZK3PCRGTSNRAYZ//worker::parameter-server-go@test-node-8093")

	if got := actor.ActorID(); got != "01KM1SX3YM67ZK3PCRGTSNRAYZ//worker::parameter-server-go@test-node-8093" {
		t.Fatalf("ActorID() = %q", got)
	}
	if got := actor.ApplicationID(); got != "parameter-server-go" {
		t.Fatalf("ApplicationID() = %q, want parameter-server-go", got)
	}
}

func TestHandle(t *testing.T) {
	counter := newCounterActor()

	result := counter.Handle("sender", "increment", "{}")
	var parsed map[string]any
	json.Unmarshal([]byte(result), &parsed)
	if parsed["value"].(float64) != 1 {
		t.Errorf("expected value=1 after increment, got %v", parsed["value"])
	}

	result = counter.Handle("sender", "increment", "{}")
	json.Unmarshal([]byte(result), &parsed)
	if parsed["value"].(float64) != 2 {
		t.Errorf("expected value=2 after second increment, got %v", parsed["value"])
	}

	result = counter.Handle("sender", "get", "{}")
	json.Unmarshal([]byte(result), &parsed)
	if parsed["value"].(float64) != 2 {
		t.Errorf("expected value=2 on get, got %v", parsed["value"])
	}
}

// ========================================================================
// Actor Registration Tests
// ========================================================================

func TestRegisterAndGetActor(t *testing.T) {
	ResetStubs()

	counter := newCounterActor()
	Register(counter)

	got := GetRegisteredActor()
	if got != counter {
		t.Error("GetRegisteredActor should return the registered actor")
	}

	// Clean up
	registeredActor = nil
}

func TestGetRegisteredActorNil(t *testing.T) {
	registeredActor = nil
	got := GetRegisteredActor()
	if got != nil {
		t.Error("GetRegisteredActor should return nil when nothing is registered")
	}
}

func TestActorDefinitionHelpers(t *testing.T) {
	definition := WorkflowActorDefinition(func() Actor {
		return newCounterActor()
	}, "virtual_actor", "durability")

	if definition.BehaviorType != BehaviorWorkflowActor {
		t.Fatalf("BehaviorType = %q", definition.BehaviorType)
	}
	if len(definition.Facets) != 2 || definition.Facets[0] != "virtual_actor" || definition.Facets[1] != "durability" {
		t.Fatalf("Facets = %#v", definition.Facets)
	}
	if definition.Factory == nil {
		t.Fatal("Factory should be set")
	}
}

func TestActorRouterRouteDefinition(t *testing.T) {
	router := NewActorRouter()
	definition := GenServerActor(func() Actor { return newCounterActor() }, "virtual_actor")
	router.RouteDefinition("counter", definition)

	got, ok := router.Definition("counter")
	if !ok {
		t.Fatal("Definition should be registered")
	}
	if got.BehaviorType != BehaviorGenServer {
		t.Fatalf("BehaviorType = %q", got.BehaviorType)
	}
	if len(got.Facets) != 1 || got.Facets[0] != "virtual_actor" {
		t.Fatalf("Facets = %#v", got.Facets)
	}
}

// ========================================================================
// marshalPayload Tests
// ========================================================================

func TestMarshalPayloadNil(t *testing.T) {
	result := marshalPayload(nil)
	if result != "{}" {
		t.Errorf("marshalPayload(nil) should return {}, got %q", result)
	}
}

func TestMarshalPayloadString(t *testing.T) {
	result := marshalPayload(`{"key":"value"}`)
	if result != `{"key":"value"}` {
		t.Errorf("marshalPayload(string) should pass through, got %q", result)
	}
}

func TestMarshalPayloadMap(t *testing.T) {
	result := marshalPayload(map[string]any{"count": 42})
	var parsed map[string]any
	if err := json.Unmarshal([]byte(result), &parsed); err != nil {
		t.Fatalf("marshalPayload should return valid JSON: %v", err)
	}
	if parsed["count"].(float64) != 42 {
		t.Errorf("expected count=42, got %v", parsed["count"])
	}
}

func TestMarshalPayloadStruct(t *testing.T) {
	type payload struct {
		Name  string `json:"name"`
		Value int    `json:"value"`
	}
	result := marshalPayload(payload{Name: "test", Value: 10})
	if !strings.Contains(result, `"name":"test"`) {
		t.Errorf("expected name:test in result, got %q", result)
	}
}

func TestMarshalPayloadUnmarshalableReturnsError(t *testing.T) {
	// Channels cannot be JSON-marshaled
	result := marshalPayload(make(chan int))
	if !strings.Contains(result, "error") {
		t.Errorf("marshalPayload of chan should return error JSON, got %q", result)
	}
}

// ========================================================================
// Error Handling Tests
// ========================================================================

func TestIsHostError(t *testing.T) {
	tests := []struct {
		input    string
		expected bool
	}{
		{"ERROR: something went wrong", true},
		{"ERROR:", true},
		{"ERROR:timeout", true},
		{"", false},
		{"success", false},
		{"ERRORS are not errors", false},
	}

	for _, tt := range tests {
		got := isHostError(tt.input)
		if got != tt.expected {
			t.Errorf("isHostError(%q) = %v, want %v", tt.input, got, tt.expected)
		}
	}
}

func TestHostCreateShardGroup(t *testing.T) {
	ResetStubs()
	host := NewHost()
	out, err := host.CreateShardGroup(map[string]any{
		"group_id":    "group-a",
		"actor_type":  "worker",
		"shard_count": 1,
	})
	if err != nil {
		t.Fatalf("CreateShardGroup returned error: %v", err)
	}
	if out["group_id"] != "mock-group" {
		t.Fatalf("expected mock-group, got %v", out["group_id"])
	}
}

func TestHostApplicationGetStatus(t *testing.T) {
	ResetStubs()
	host := NewHost()
	out, err := host.ApplicationGetStatus("app-a", "node-a")
	if err != nil {
		t.Fatalf("ApplicationGetStatus returned error: %v", err)
	}
	if out["node_id"] != "node-a" {
		t.Fatalf("expected node-a, got %v", out["node_id"])
	}
}

func TestHostApplicationMetricsAdd(t *testing.T) {
	ResetStubs()
	h := NewHost()
	out, err := h.ApplicationMetricsAdd("app-a", map[string]any{
		"message_count": float64(7),
		"counter_metrics": map[string]any{
			"tuple_operations": float64(42),
		},
	})
	if err != nil {
		t.Fatalf("ApplicationMetricsAdd returned error: %v", err)
	}
	mc, ok := out["message_count"].(float64)
	if !ok || mc != 7 {
		t.Fatalf("expected message_count 7, got %v", out["message_count"])
	}
}

func TestHostApplicationGetMetrics(t *testing.T) {
	ResetStubs()
	h := NewHost()
	out, err := h.ApplicationGetMetrics("app-a", "node-a")
	if err != nil {
		t.Fatalf("ApplicationGetMetrics returned error: %v", err)
	}
	mc, ok := out["message_count"].(float64)
	if !ok || mc != 0 {
		t.Fatalf("expected message_count 0, got %v", out["message_count"])
	}
}

func TestCheckError(t *testing.T) {
	err := checkError("")
	if err != nil {
		t.Error("checkError('') should return nil")
	}

	err = checkError("success")
	if err != nil {
		t.Error("checkError('success') should return nil")
	}

	err = checkError("ERROR: timeout")
	if err == nil {
		t.Error("checkError('ERROR: timeout') should return error")
	}
	if err.Error() != "ERROR: timeout" {
		t.Errorf("expected 'ERROR: timeout', got %q", err.Error())
	}
}

func TestHostError(t *testing.T) {
	err := &HostError{Message: "ERROR: test"}
	if err.Error() != "ERROR: test" {
		t.Errorf("HostError.Error() should return message, got %q", err.Error())
	}
}

// ========================================================================
// Host Function Tests (using stubs)
// ========================================================================

func TestHostSend(t *testing.T) {
	ResetStubs()
	h := NewHost()
	h.Send("target-actor", "ping", map[string]any{"data": "hello"})

	msgs := GetStubSentMessages()
	if len(msgs) != 1 {
		t.Fatalf("expected 1 sent message, got %d", len(msgs))
	}
	if msgs[0].To != "target-actor" {
		t.Errorf("expected to=target-actor, got %q", msgs[0].To)
	}
	if msgs[0].MsgType != "ping" {
		t.Errorf("expected msgType=ping, got %q", msgs[0].MsgType)
	}
}

func TestHostAsk(t *testing.T) {
	h := NewHost()
	result, err := h.Ask("target", "query", nil, 5000)
	if err != nil {
		t.Fatalf("Ask should not return error: %v", err)
	}
	if result == nil {
		t.Error("Ask should return non-nil result")
	}
}

func TestHostSelfID(t *testing.T) {
	ResetStubs()
	SetStubSelfID("my-actor:ns@node")
	h := NewHost()
	id := h.SelfID()
	if id != "my-actor:ns@node" {
		t.Errorf("expected my-actor:ns@node, got %q", id)
	}
}

func TestHostSpawn(t *testing.T) {
	h := NewHost()
	id, err := h.Spawn("counter-module", "counter-1", nil)
	if err != nil {
		t.Fatalf("Spawn should not return error: %v", err)
	}
	if id != "counter-1" {
		t.Errorf("expected counter-1, got %q", id)
	}
}

func TestHostSpawnAutoID(t *testing.T) {
	h := NewHost()
	id, err := h.Spawn("counter-module", "", nil)
	if err != nil {
		t.Fatalf("Spawn should not return error: %v", err)
	}
	if !strings.Contains(id, "counter-module") {
		t.Errorf("auto-generated ID should contain module name, got %q", id)
	}
}

func TestHostLog(t *testing.T) {
	ResetStubs()
	h := NewHost()
	h.Info("test info message")
	h.Debug("test debug message")
	h.Warn("test warn")
	h.Error("test error")

	logs := GetStubLogs()
	if len(logs) != 4 {
		t.Fatalf("expected 4 log messages, got %d", len(logs))
	}
	if logs[0].Level != "info" || logs[0].Message != "test info message" {
		t.Errorf("expected info/test info message, got %s/%s", logs[0].Level, logs[0].Message)
	}
}

func TestHostNowMs(t *testing.T) {
	ResetStubs()
	h := NewHost()
	now := h.NowMs()
	if now == 0 {
		t.Error("NowMs should return non-zero timestamp")
	}

	SetStubNowMs(12345)
	now = h.NowMs()
	if now != 12345 {
		t.Errorf("expected 12345, got %d", now)
	}
}

func TestHostKV(t *testing.T) {
	ResetStubs()
	h := NewHost()

	h.KVPut("key1", "value1")
	got := h.KVGet("key1")
	if got != "value1" {
		t.Errorf("expected value1, got %q", got)
	}

	h.KVDelete("key1")
	got = h.KVGet("key1")
	if got != "" {
		t.Errorf("expected empty after delete, got %q", got)
	}
}

func TestHostPGMembers(t *testing.T) {
	ResetStubs()
	h := NewHost()
	if err := h.PG().Join("workers"); err != nil {
		t.Fatalf("PG.Join should not return error: %v", err)
	}
	members, err := h.PG().Members("workers")
	if err != nil {
		t.Fatalf("PG.Members should not return error: %v", err)
	}
	if len(members) != 1 {
		t.Errorf("expected 1 member, got %d", len(members))
	}
}

func TestHostMonitor(t *testing.T) {
	h := NewHost()
	ref, err := h.Monitor("target-actor")
	if err != nil {
		t.Fatalf("Monitor should not return error: %v", err)
	}
	if ref == "" {
		t.Error("Monitor should return non-empty reference")
	}
}

func TestHostSendAfter(t *testing.T) {
	h := NewHost()
	timerID := h.SendAfter(1000, "tick", nil)
	if timerID == "" {
		t.Error("SendAfter should return non-empty timer ID")
	}
}

// ========================================================================
// ActorRouter Tests
// ========================================================================

func TestActorRouterInit(t *testing.T) {
	router := NewActorRouter()
	router.Route("counter", func() Actor { return newCounterActor() })
	router.Route("echo", func() Actor { return newEchoActor() })

	// Init with counter actor ID
	result := router.Init(`{"actor_id":"counter:test@node","args":{}}`)
	if result != "" {
		t.Errorf("router Init should return empty on success, got %q", result)
	}
	if router.active == nil {
		t.Fatal("router should have active actor after Init")
	}
}

func TestActorRouterInitPrefersDeclarationNameOverActorType(t *testing.T) {
	router := NewActorRouter()
	counterCreated := false

	router.Route("counter", func() Actor {
		counterCreated = true
		return newCounterActor()
	})

	result := router.Init(`{"actor_id":"counter//shared_wasm::app@test-node","actor_type":"shared_wasm","declaration_name":"counter","args":{}}`)
	if result != "" {
		t.Fatalf("router Init should succeed with declaration_name dispatch, got %q", result)
	}
	if !counterCreated {
		t.Fatal("declaration_name should select the registered actor factory")
	}
}

func TestActorRouterInitWithNamespace(t *testing.T) {
	router := NewActorRouter()
	router.Route("rate-limiter", func() Actor { return newCounterActor() })

	// Full actor ID format: name:namespace@node
	result := router.Init(`{"actor_id":"rate-limiter:default@test-node"}`)
	if result != "" {
		t.Errorf("expected success, got %q", result)
	}
}

func TestActorRouterInitWithCanonicalActorID(t *testing.T) {
	router := NewActorRouter()
	router.Route("leader", func() Actor { return newEchoActor() })

	result := router.Init(`{"actor_id":"01KM1SX3YM67ZK3PCRGTSNRAYZ//leader::parameter-server-go@test-node-8091"}`)
	if result != "" {
		t.Errorf("expected success for canonical actor id, got %q", result)
	}
}

func TestActorRouterPrefixMatching(t *testing.T) {
	router := NewActorRouter()
	router.Route("counter", func() Actor { return newCounterActor() })

	// "counter-1" should match "counter" prefix
	result := router.Init(`{"actor_id":"counter-1:ns@node"}`)
	if result != "" {
		t.Errorf("prefix matching should work, got %q", result)
	}
}

func TestActorRouterCanonicalPrefixMatching(t *testing.T) {
	router := NewActorRouter()
	router.Route("worker", func() Actor { return newCounterActor() })

	result := router.Init(`{"actor_id":"01KM1SX3YM67ZK3PCRGTSNRAYZ//worker-3::parameter-server-go@test-node-8093"}`)
	if result != "" {
		t.Errorf("canonical prefix matching should work, got %q", result)
	}
}

func TestActorRouterLongestPrefixWins(t *testing.T) {
	router := NewActorRouter()
	counterCreated := false
	longCounterCreated := false

	router.Route("count", func() Actor {
		counterCreated = true
		return newCounterActor()
	})
	router.Route("counter", func() Actor {
		longCounterCreated = true
		return newCounterActor()
	})

	router.Init(`{"actor_id":"counter:ns@node"}`)
	if counterCreated {
		t.Error("shorter prefix 'count' should not win over 'counter'")
	}
	if !longCounterCreated {
		t.Error("longer prefix 'counter' should win")
	}
}

func TestActorRouterNoMatch(t *testing.T) {
	router := NewActorRouter()
	router.Route("counter", func() Actor { return newCounterActor() })

	result := router.Init(`{"actor_id":"unknown:ns@node"}`)
	if !strings.HasPrefix(result, "ERROR:") {
		t.Errorf("expected ERROR for unknown prefix, got %q", result)
	}
}

func TestActorRouterHandleDelegates(t *testing.T) {
	router := NewActorRouter()
	router.Route("counter", func() Actor { return newCounterActor() })
	router.Init(`{"actor_id":"counter:ns@node"}`)

	result := router.Handle("sender", "increment", "{}")
	var parsed map[string]any
	json.Unmarshal([]byte(result), &parsed)
	if parsed["value"].(float64) != 1 {
		t.Errorf("expected value=1, got %v", parsed["value"])
	}
}

func TestActorRouterHandleWithoutInit(t *testing.T) {
	router := NewActorRouter()
	result := router.Handle("sender", "test", "{}")
	if !strings.Contains(result, "no active actor") {
		t.Errorf("expected no active actor error, got %q", result)
	}
}

func TestActorRouterGetStateDelegates(t *testing.T) {
	router := NewActorRouter()
	router.Route("counter", func() Actor { return newCounterActor() })
	router.Init(`{"actor_id":"counter:ns@node"}`)

	// Increment to change state
	router.Handle("sender", "increment", "{}")

	state := router.GetState()
	var parsed map[string]any
	json.Unmarshal([]byte(state), &parsed)
	if parsed["value"].(float64) != 1 {
		t.Errorf("expected value=1 in state, got %v", parsed["value"])
	}
}

func TestActorRouterSetStateDelegates(t *testing.T) {
	router := NewActorRouter()
	router.Route("counter", func() Actor { return newCounterActor() })
	router.Init(`{"actor_id":"counter:ns@node"}`)

	result := router.SetState(`{"value":99,"name":"restored"}`)
	if result != "" {
		t.Errorf("SetState should return empty on success, got %q", result)
	}

	// Verify state was restored
	state := router.GetState()
	if !strings.Contains(state, `"value":99`) {
		t.Errorf("expected value=99 in state, got %q", state)
	}
}

func TestActorRouterWorkflowDelegates(t *testing.T) {
	router := NewActorRouter()
	router.Route("workflow", func() Actor { return newWorkflowTestActor() })
	result := router.Init(`{"actor_id":"workflow:ns@node"}`)
	if result != "" {
		t.Fatalf("Init() = %q", result)
	}

	runResult := router.Run(`{"order_id":"o-1"}`)
	if runResult != `{"status":"running:o-1"}` {
		t.Fatalf("Run() = %q", runResult)
	}

	router.Signal("cancel", `{"reason":"user"}`)
	queryResult := router.Query("status", `{}`)
	if queryResult != `{"status":"cancelled","signals":["cancel:user"]}` {
		t.Fatalf("Query() = %q", queryResult)
	}
}

func TestActorRouterWorkflowWithoutWorkflowBehavior(t *testing.T) {
	router := NewActorRouter()
	router.Route("counter", func() Actor { return newCounterActor() })
	result := router.Init(`{"actor_id":"counter:ns@node"}`)
	if result != "" {
		t.Fatalf("Init() = %q", result)
	}

	if got := router.Run(`{}`); !strings.Contains(got, "does not implement workflow behavior") {
		t.Fatalf("Run() = %q", got)
	}
	if got := router.Query("status", `{}`); !strings.Contains(got, "does not implement workflow behavior") {
		t.Fatalf("Query() = %q", got)
	}
}

func TestActorRouterInvalidConfigJSON(t *testing.T) {
	router := NewActorRouter()
	router.Route("counter", func() Actor { return newCounterActor() })

	result := router.Init("not json")
	if !strings.HasPrefix(result, "ERROR:") {
		t.Errorf("expected ERROR for invalid JSON, got %q", result)
	}
}

// ========================================================================
// initConfig Tests
// ========================================================================

func TestInitConfigParsing(t *testing.T) {
	var config initConfig
	err := json.Unmarshal([]byte(`{"actor_id":"test:ns@node","actor_type":"shared_wasm","declaration_name":"counter","role":"counter","args":{"key":"value"}}`), &config)
	if err != nil {
		t.Fatalf("failed to parse config: %v", err)
	}
	if config.ActorID != "test:ns@node" {
		t.Errorf("expected test:ns@node, got %q", config.ActorID)
	}
	if config.ActorType != "shared_wasm" {
		t.Errorf("expected shared_wasm, got %q", config.ActorType)
	}
	if config.DeclarationName != "counter" {
		t.Errorf("expected counter declaration_name, got %q", config.DeclarationName)
	}
	if config.Role != "counter" {
		t.Errorf("expected counter role, got %q", config.Role)
	}
	if config.Args == nil {
		t.Error("Args should not be nil")
	}
}

func TestInitConfigWithoutArgs(t *testing.T) {
	var config initConfig
	err := json.Unmarshal([]byte(`{"actor_id":"test:ns@node"}`), &config)
	if err != nil {
		t.Fatalf("failed to parse config: %v", err)
	}
	if config.ActorID != "test:ns@node" {
		t.Errorf("expected test:ns@node, got %q", config.ActorID)
	}
}

// ========================================================================
// State Round-Trip Tests
// ========================================================================

func TestStateRoundTrip(t *testing.T) {
	original := newCounterActor()
	original.Value = 42
	original.Name = "round-trip"

	state := original.GetState()

	restored := newCounterActor()
	result := restored.SetState(state)
	if result != "" {
		t.Fatalf("SetState failed: %s", result)
	}

	if restored.Value != 42 || restored.Name != "round-trip" {
		t.Errorf("state round-trip failed: value=%d, name=%q", restored.Value, restored.Name)
	}
}

// ========================================================================
// Additional Host Function Coverage Tests
// ========================================================================

func TestHostStop(t *testing.T) {
	h := NewHost()
	err := h.Stop("some-actor")
	if err != nil {
		t.Errorf("Stop should not return error: %v", err)
	}
}

func TestHostLink(t *testing.T) {
	h := NewHost()
	err := h.Link("other-actor")
	if err != nil {
		t.Errorf("Link should not return error: %v", err)
	}
}

func TestHostUnlink(t *testing.T) {
	h := NewHost()
	err := h.Unlink("other-actor")
	if err != nil {
		t.Errorf("Unlink should not return error: %v", err)
	}
}

func TestHostDemonitor(t *testing.T) {
	h := NewHost()
	err := h.Demonitor("monitor-ref")
	if err != nil {
		t.Errorf("Demonitor should not return error: %v", err)
	}
}

func TestHostTupleSpace(t *testing.T) {
	ResetStubs()
	h := NewHost()
	result := h.TSWrite(`["task","worker-1",123]`)
	if isHostError(result) {
		t.Errorf("TSWrite should succeed, got %q", result)
	}
	result = h.TSRead(`["task","*",null]`)
	if result != `["task","worker-1",123]` {
		t.Errorf("TSRead should return stored tuple, got %q", result)
	}
	result = h.TSTake(`["task","*",null]`)
	if result != `["task","worker-1",123]` {
		t.Errorf("TSTake should return stored tuple, got %q", result)
	}
	result = h.TSReadAll(`["task","*",null]`)
	if result != "[]" {
		t.Errorf("TSReadAll should return [], got %q", result)
	}
}

func TestHostTupleSpaceHelper(t *testing.T) {
	ResetStubs()
	h := NewHost()
	ts := h.TS()
	if ts == nil {
		t.Fatal("TS() should return non-nil")
	}
	// Write: list-in (stub returns success)
	errStr := ts.Write([]any{"job", "j1", "task", "t0", 1})
	if isHostError(errStr) {
		t.Errorf("TS().Write should succeed, got %q", errStr)
	}
	// Take should return the stored tuple
	tuple, ok := ts.Take([]any{"job", "j1", "task", nil, nil})
	if !ok || len(tuple) != 5 {
		t.Errorf("TS().Take should return stored tuple, got (%v, %v)", tuple, ok)
	}
	// ReadAll: tuple was taken, so the collection is empty
	all := ts.ReadAll([]any{"job", nil, nil, nil, nil})
	if all == nil || len(all) != 0 {
		t.Errorf("TS().ReadAll should return empty slice, got %v", all)
	}
}

func TestHostLocks(t *testing.T) {
	h := NewHost()
	result := h.LockAcquire("tenant", "ns", "holder", "lock-1", 30, 5000)
	if isHostError(result) {
		t.Errorf("LockAcquire should succeed, got %q", result)
	}
	if !strings.Contains(result, "lock_key") {
		t.Errorf("LockAcquire should return lock details, got %q", result)
	}

	releaseResult := h.LockRelease("test-lock", "tenant", "ns", "holder", "v1")
	if isHostError(releaseResult) {
		t.Errorf("LockRelease should succeed, got %q", releaseResult)
	}

	renewResult := h.LockRenew("test-lock", "tenant", "ns", "holder", "v1", 30)
	if isHostError(renewResult) {
		t.Errorf("LockRenew should succeed, got %q", renewResult)
	}
}

func TestHostBlobs(t *testing.T) {
	h := NewHost()
	result := h.BlobUpload("blob-1", "aGVsbG8=", "text/plain")
	if isHostError(result) {
		t.Errorf("BlobUpload should succeed, got %q", result)
	}

	h.BlobDownload("blob-1")
	h.BlobDelete("blob-1")

	list := h.BlobList("blob")
	if list != "[]" {
		t.Errorf("BlobList stub should return [], got %q", list)
	}
}

func TestHostPGJoinLeave(t *testing.T) {
	h := NewHost()
	pg := h.PG()

	if err := pg.Join("workers"); err != nil {
		t.Errorf("PG.Join should not return error: %v", err)
	}
	if err := pg.Leave("workers"); err != nil {
		t.Errorf("PG.Leave should not return error: %v", err)
	}
}

func TestHostPGBroadcast(t *testing.T) {
	h := NewHost()
	err := h.PG().Broadcast("workers", "status", map[string]any{"active": true})
	if err != nil {
		t.Errorf("PG.Broadcast should not return error: %v", err)
	}
}

func TestHostKVList(t *testing.T) {
	ResetStubs()
	h := NewHost()
	h.KVPut("user:1", "alice")
	h.KVPut("user:2", "bob")
	h.KVPut("order:1", "item")

	result := h.KVList("user:")
	var keys []string
	json.Unmarshal([]byte(result), &keys)
	if len(keys) != 2 {
		t.Errorf("expected 2 keys with prefix 'user:', got %d from %q", len(keys), result)
	}
}

func TestHostSendWithNilPayload(t *testing.T) {
	ResetStubs()
	h := NewHost()
	h.Send("target", "ping", nil)

	msgs := GetStubSentMessages()
	if len(msgs) != 1 {
		t.Fatalf("expected 1 message, got %d", len(msgs))
	}
	if msgs[0].Payload != "{}" {
		t.Errorf("nil payload should be sent as {}, got %q", msgs[0].Payload)
	}
}

func TestHostSendWithStringPayload(t *testing.T) {
	ResetStubs()
	h := NewHost()
	h.Send("target", "data", `{"custom":"json"}`)

	msgs := GetStubSentMessages()
	if msgs[0].Payload != `{"custom":"json"}` {
		t.Errorf("string payload should pass through, got %q", msgs[0].Payload)
	}
}

// ========================================================================
// Router Edge Cases
// ========================================================================

func TestActorRouterGetStateWithoutInit(t *testing.T) {
	router := NewActorRouter()
	state := router.GetState()
	if state != "{}" {
		t.Errorf("GetState without init should return {}, got %q", state)
	}
}

func TestActorRouterSetStateWithoutInit(t *testing.T) {
	router := NewActorRouter()
	result := router.SetState(`{"value":1}`)
	if !strings.HasPrefix(result, "ERROR:") {
		t.Errorf("SetState without init should return ERROR, got %q", result)
	}
}

func TestActorRouterInitWithEmptyActorID(t *testing.T) {
	router := NewActorRouter()
	router.Route("counter", func() Actor { return newCounterActor() })

	// Empty actor_id should fail to find a match
	result := router.Init(`{"actor_id":""}`)
	if !strings.HasPrefix(result, "ERROR:") {
		t.Errorf("expected ERROR for empty actor_id, got %q", result)
	}
}

func TestNormalizeRoleActorID(t *testing.T) {
	tests := []struct {
		name  string
		input string
		want  string
	}{
		{
			name:  "bare",
			input: "worker",
			want:  "worker",
		},
		{
			name:  "child style",
			input: "worker-0:parameter-server-go@test-node-8093",
			want:  "worker-0",
		},
		{
			name:  "canonical",
			input: "01KM1SX3YM67ZK3PCRGTSNRAYZ//leader::parameter-server-go@test-node-8091",
			want:  "leader",
		},
		{
			name:  "canonical prefix",
			input: "01KM1SX3YM67ZK3PCRGTSNRAYZ//worker-3::parameter-server-go@test-node-8093",
			want:  "worker-3",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if got := normalizeRoleActorID(tt.input); got != tt.want {
				t.Fatalf("normalizeRoleActorID(%q) = %q, want %q", tt.input, got, tt.want)
			}
		})
	}
}

// ========================================================================
// Collective / Parallel Host Operation Tests
// ========================================================================

func TestHostBroadcastShardGroup(t *testing.T) {
	ResetStubs()
	h := NewHost()
	out, err := h.BroadcastShardGroup(map[string]any{
		"group_id": "workers",
		"message":  map[string]any{"op": "reset"},
		"min_acks": 1,
	})
	if err != nil {
		t.Fatalf("BroadcastShardGroup returned error: %v", err)
	}
	stats, ok := out["stats"].(map[string]any)
	if !ok {
		t.Fatal("expected stats in response")
	}
	if _, exists := stats["shards_queried"]; !exists {
		t.Error("expected shards_queried in stats")
	}
}

func TestHostReduceShardGroup(t *testing.T) {
	ResetStubs()
	h := NewHost()
	out, err := h.ReduceShardGroup(map[string]any{
		"group_id":      "workers",
		"query":         map[string]any{"action": "get_count"},
		"reduction":     1,
		"min_responses": 1,
	})
	if err != nil {
		t.Fatalf("ReduceShardGroup returned error: %v", err)
	}
	if _, exists := out["stats"]; !exists {
		t.Error("expected stats in response")
	}
	if _, exists := out["shard_responses"]; !exists {
		t.Error("expected shard_responses in response")
	}
}

func TestHostAllReduceShardGroup(t *testing.T) {
	ResetStubs()
	h := NewHost()
	out, err := h.AllReduceShardGroup(map[string]any{
		"group_id":      "workers",
		"query":         map[string]any{"action": "sum"},
		"reduction":     1,
		"min_responses": 1,
	})
	if err != nil {
		t.Fatalf("AllReduceShardGroup returned error: %v", err)
	}
	if _, exists := out["stats"]; !exists {
		t.Error("expected stats in response")
	}
}

func TestHostBarrierShardGroup(t *testing.T) {
	ResetStubs()
	h := NewHost()
	out, err := h.BarrierShardGroup(map[string]any{
		"group_id":   "workers",
		"barrier_id": "round-1",
		"round":      1,
		"min_acks":   1,
	})
	if err != nil {
		t.Fatalf("BarrierShardGroup returned error: %v", err)
	}
	if _, exists := out["stats"]; !exists {
		t.Error("expected stats in response")
	}
}

func TestHostSpawnActors(t *testing.T) {
	ResetStubs()
	h := NewHost()
	out, err := h.SpawnActors(map[string]any{
		"requests": []any{
			map[string]any{"actor_type": "counter", "actor_id": "c-0"},
			map[string]any{"actor_type": "counter", "actor_id": "c-1"},
		},
	})
	if err != nil {
		t.Fatalf("SpawnActors returned error: %v", err)
	}
	results, ok := out["results"].([]any)
	if !ok {
		t.Fatal("expected results array in response")
	}
	if len(results) != 2 {
		t.Fatalf("expected 2 results, got %d", len(results))
	}
	for i, r := range results {
		result := r.(map[string]any)
		if result["success"] != true {
			t.Errorf("result[%d] should be success", i)
		}
	}
}

func TestHostSpawnActorsEmpty(t *testing.T) {
	ResetStubs()
	h := NewHost()
	out, err := h.SpawnActors(map[string]any{
		"requests": []any{},
	})
	if err != nil {
		t.Fatalf("SpawnActors returned error: %v", err)
	}
	results, ok := out["results"].([]any)
	if !ok {
		t.Fatal("expected results array in response")
	}
	if len(results) != 0 {
		t.Fatalf("expected 0 results for empty request, got %d", len(results))
	}
}

func TestHostSpawnActorsWithInstancesCount(t *testing.T) {
	ResetStubs()
	h := NewHost()
	out, err := h.SpawnActors(map[string]any{
		"requests": []any{
			map[string]any{
				"actor_type":      "worker",
				"actor_id":        "w",
				"instances_count": 3,
			},
		},
	})
	if err != nil {
		t.Fatalf("SpawnActors returned error: %v", err)
	}
	if _, exists := out["results"]; !exists {
		t.Error("expected results in response")
	}
}

func TestHostBulkUpdateShardGroup(t *testing.T) {
	ResetStubs()
	h := NewHost()
	out, err := h.BulkUpdateShardGroup(map[string]any{
		"group_id": "workers",
		"updates":  map[string]any{"key1": map[string]any{"payload": "data"}},
	})
	if err != nil {
		t.Fatalf("BulkUpdateShardGroup returned error: %v", err)
	}
	if _, exists := out["updates_sent"]; !exists {
		t.Error("expected updates_sent in response")
	}
}

func TestHostMapShardGroup(t *testing.T) {
	ResetStubs()
	h := NewHost()
	out, err := h.MapShardGroup(map[string]any{
		"group_id": "workers",
		"query":    map[string]any{"action": "status"},
	})
	if err != nil {
		t.Fatalf("MapShardGroup returned error: %v", err)
	}
	if _, exists := out["results"]; !exists {
		t.Error("expected results in response")
	}
}

func TestHostScatterGather(t *testing.T) {
	ResetStubs()
	h := NewHost()
	out, err := h.ScatterGather(map[string]any{
		"group_id": "workers",
		"query":    map[string]any{"action": "get_all"},
	})
	if err != nil {
		t.Fatalf("ScatterGather returned error: %v", err)
	}
	if _, exists := out["stats"]; !exists {
		t.Error("expected stats in response")
	}
	if _, exists := out["shard_responses"]; !exists {
		t.Error("expected shard_responses in response")
	}
}

func TestHostHTTPFetch(t *testing.T) {
	h := NewHost()
	resp, err := h.HTTPFetch("test-link", "GET", "/v1/items", nil, nil)
	if err != nil {
		t.Fatalf("HTTPFetch failed: %v", err)
	}
	status, ok := resp["status"].(float64)
	if !ok || int(status) != 200 {
		t.Errorf("expected status 200, got %v", resp["status"])
	}
}

func TestHostHTTPFetchWithHeaders(t *testing.T) {
	h := NewHost()
	headers := map[string]string{"Authorization": "Bearer test-token"}
	resp, err := h.HTTPFetch("test-link", "POST", "/v1/items", headers, []byte(`{"name":"test"}`))
	if err != nil {
		t.Fatalf("HTTPFetch failed: %v", err)
	}
	if resp["status"] == nil {
		t.Error("expected status in response")
	}
}

func TestServiceHTTPClientGet(t *testing.T) {
	h := NewHost()
	client := NewServiceHTTPClient(h, "test-api")
	resp, err := client.Get("/v1/items", nil)
	if err != nil {
		t.Fatalf("Get failed: %v", err)
	}
	if resp["status"] == nil {
		t.Error("expected status in response")
	}
}

func TestServiceHTTPClientPost(t *testing.T) {
	h := NewHost()
	client := NewServiceHTTPClient(h, "test-api")
	body, _ := json.Marshal(map[string]string{"name": "test"})
	resp, err := client.Post("/v1/items", body, nil)
	if err != nil {
		t.Fatalf("Post failed: %v", err)
	}
	if resp["status"] == nil {
		t.Error("expected status in response")
	}
}

func TestServiceHTTPClientDelete(t *testing.T) {
	h := NewHost()
	client := NewServiceHTTPClient(h, "test-api")
	resp, err := client.Delete("/v1/items/1", nil)
	if err != nil {
		t.Fatalf("Delete failed: %v", err)
	}
	if resp["status"] == nil {
		t.Error("expected status in response")
	}
}

func TestActorRouterEchoActorInit(t *testing.T) {
	router := NewActorRouter()
	router.Route("echo", func() Actor { return newEchoActor() })

	// EchoActor has custom Init that sets LastMsg
	result := router.Init(`{"actor_id":"echo:ns@node"}`)
	if result != "" {
		t.Errorf("expected success, got %q", result)
	}

	// Verify the echo actor was properly initialized
	state := router.GetState()
	if !strings.Contains(state, "initialized") {
		t.Errorf("expected state to contain 'initialized', got %q", state)
	}
}

// ========================================================================
// EventLog
// ========================================================================

func TestEventLogAppendAndPoll(t *testing.T) {
	ResetStubs()
	h := NewHost()
	var el EventLog

	seq, err := el.Append(h, "audit:", map[string]any{"action": "login"})
	if err != nil || seq != 1 {
		t.Fatalf("Append: seq=%d err=%v", seq, err)
	}
	seq, err = el.Append(h, "audit:", map[string]any{"action": "logout"})
	if err != nil || seq != 2 {
		t.Fatalf("Append: seq=%d err=%v", seq, err)
	}

	events, cursor, err := el.Poll(h, "audit:", "consumer-1", 10)
	if err != nil {
		t.Fatalf("Poll: %v", err)
	}
	if len(events) != 2 || cursor != 2 {
		t.Errorf("expected 2 events cursor=2, got %d cursor=%d", len(events), cursor)
	}
}

func TestEventLogPollIdempotent(t *testing.T) {
	ResetStubs()
	h := NewHost()
	var el EventLog
	_, _ = el.Append(h, "ev:", map[string]any{"x": 1})

	events, cursor, _ := el.Poll(h, "ev:", "c1", 10)
	if len(events) != 1 || cursor != 1 {
		t.Fatalf("first poll: events=%d cursor=%d", len(events), cursor)
	}
	// second poll: cursor is now at watermark, returns nothing new
	events, cursor, _ = el.Poll(h, "ev:", "c1", 10)
	if len(events) != 0 {
		t.Errorf("second poll should return 0 events, got %d", len(events))
	}
	if cursor != 1 {
		t.Errorf("cursor should remain 1, got %d", cursor)
	}
}

func TestEventLogTwoIndependentConsumers(t *testing.T) {
	ResetStubs()
	h := NewHost()
	var el EventLog
	for i := 0; i < 3; i++ {
		_, _ = el.Append(h, "ev:", map[string]any{"i": i})
	}

	evA, curA, _ := el.Poll(h, "ev:", "consumer-A", 10)
	evB, curB, _ := el.Poll(h, "ev:", "consumer-B", 2)
	if len(evA) != 3 || curA != 3 {
		t.Errorf("consumer-A: events=%d cursor=%d", len(evA), curA)
	}
	if len(evB) != 2 || curB != 2 {
		t.Errorf("consumer-B: events=%d cursor=%d", len(evB), curB)
	}
}

func TestEventLogAppendRollsBackOnError(t *testing.T) {
	ResetStubs()
	h := NewHost()
	var el EventLog
	// chan is unmarshalable — Append should fail and Watermark should stay 0
	err := func() error {
		_, e := el.Append(h, "ev:", make(chan int))
		return e
	}()
	if err == nil {
		t.Fatal("expected error for unmarshalable value")
	}
	if el.Watermark != 0 {
		t.Errorf("watermark should be rolled back to 0, got %d", el.Watermark)
	}
}

// ========================================================================
// PG.First
// ========================================================================

func TestPGFirstReturnsMember(t *testing.T) {
	ResetStubs()
	h := NewHost()
	_ = h.PG().Join("svc:test")
	id, err := h.PG().First("svc:test")
	if err != nil {
		t.Fatalf("expected member, got error: %v", err)
	}
	if id == "" {
		t.Fatal("expected non-empty actor ID")
	}
}

func TestPGFirstErrorWhenEmpty(t *testing.T) {
	ResetStubs()
	h := NewHost()
	_, err := h.PG().First("svc:empty")
	if err == nil {
		t.Fatal("expected error for empty process group")
	}
}

// ========================================================================
// KVGetJSON / KVPutJSON
// ========================================================================

func TestKVPutAndGetJSONRoundTrip(t *testing.T) {
	ResetStubs()
	h := NewHost()

	type task struct {
		Seq  int    `json:"seq"`
		Type string `json:"task_type"`
	}
	original := task{Seq: 42, Type: "summarize"}
	if err := h.KVPutJSON("test:task:42", original); err != nil {
		t.Fatalf("KVPutJSON: %v", err)
	}

	var restored task
	found, err := h.KVGetJSON("test:task:42", &restored)
	if err != nil {
		t.Fatalf("KVGetJSON: %v", err)
	}
	if !found {
		t.Fatal("KVGetJSON: key not found after put")
	}
	if restored.Seq != 42 || restored.Type != "summarize" {
		t.Errorf("round-trip mismatch: got %+v", restored)
	}
}

func TestKVGetJSONMissingKey(t *testing.T) {
	ResetStubs()
	h := NewHost()
	var dest map[string]any
	found, err := h.KVGetJSON("nonexistent:key", &dest)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if found {
		t.Fatal("expected found=false for missing key")
	}
}

func TestKVGetJSONBadJSON(t *testing.T) {
	ResetStubs()
	h := NewHost()
	_ = h.KVPut("bad:json", "not-json{")
	var dest map[string]any
	found, err := h.KVGetJSON("bad:json", &dest)
	if err == nil {
		t.Fatal("expected unmarshal error for bad JSON")
	}
	if found {
		t.Fatal("found should be false on unmarshal error")
	}
}

func TestKVPutJSONMarshalError(t *testing.T) {
	ResetStubs()
	h := NewHost()
	ch := make(chan int)
	err := h.KVPutJSON("bad:val", ch)
	if err == nil {
		t.Fatal("expected marshal error for un-marshalable value")
	}
}

// ========================================================================
// BaseActor.IncrCounter / IncrCounters
// ========================================================================

func TestIncrCounterNoError(t *testing.T) {
	ResetStubs()
	a := &BaseActor{}
	a.SetRuntimeMetadata("counter_test:test@node")
	h := NewHost()
	a.IncrCounter(h, "my_op")
}

func TestIncrCountersMultiple(t *testing.T) {
	ResetStubs()
	a := &BaseActor{}
	a.SetRuntimeMetadata("counter_test:test@node")
	h := NewHost()
	a.IncrCounters(h, map[string]int{
		"cache_hits":   5,
		"cache_misses": 2,
	})
}

// ========================================================================
// Composed: PG.First + KVPutJSON/GetJSON
// ========================================================================

func TestComposedPGFirstAndKVJSON(t *testing.T) {
	ResetStubs()
	h := NewHost()
	_ = h.PG().Join("svc:llm_router")

	routerID, err := h.PG().First("svc:llm_router")
	if err != nil {
		t.Fatalf("PG.First: %v", err)
	}

	entry := map[string]any{"router_id": routerID, "model": "miniclaw-v1"}
	if err := h.KVPutJSON("routers:first", entry); err != nil {
		t.Fatalf("KVPutJSON: %v", err)
	}

	var restored map[string]any
	found, err := h.KVGetJSON("routers:first", &restored)
	if err != nil || !found {
		t.Fatalf("KVGetJSON after compose: found=%v err=%v", found, err)
	}
	data, _ := json.Marshal(restored)
	if string(data) == "" {
		t.Fatal("empty restored entry")
	}
}

// ── Channel tests ────────────────────────────────────────────────────────────

func TestChannelSendReceive(t *testing.T) {
	ResetStubs()
	h := NewHost()

	msgID, err := h.Ch().Send("", "tasks:work", "process", map[string]any{"doc": "d1"})
	if err != nil {
		t.Fatalf("Send: %v", err)
	}
	if msgID == "" {
		t.Fatal("Send: expected non-empty message ID")
	}

	msg, ok, err := h.Ch().Receive("", "tasks:work", 0)
	if err != nil {
		t.Fatalf("Receive: %v", err)
	}
	if !ok {
		t.Fatal("Receive: expected message, got empty")
	}
	if msg["id"] != msgID {
		t.Errorf("Receive: expected id=%s got %v", msgID, msg["id"])
	}
}

func TestChannelReceiveEmptyReturnsNotOk(t *testing.T) {
	ResetStubs()
	h := NewHost()

	msg, ok, err := h.Ch().Receive("", "empty:channel", 0)
	if err != nil {
		t.Fatalf("Receive on empty: unexpected error: %v", err)
	}
	if ok || msg != nil {
		t.Fatalf("Receive on empty: expected (nil, false) got (%v, %v)", msg, ok)
	}
}

func TestChannelAckTracked(t *testing.T) {
	ResetStubs()
	h := NewHost()

	msgID, _ := h.Ch().Send("", "q", "x", nil)
	msg, _, _ := h.Ch().Receive("", "q", 0)
	id := msg["id"].(string)

	if err := h.Ch().Ack("", "q", id); err != nil {
		t.Fatalf("Ack: %v", err)
	}
	acked := GetStubChannelAcked()
	if !acked[msgID] {
		t.Errorf("expected %s to be acked, got %v", msgID, acked)
	}
}

func TestChannelNackTracked(t *testing.T) {
	ResetStubs()
	h := NewHost()

	msgID, _ := h.Ch().Send("", "q", "x", nil)
	msg, _, _ := h.Ch().Receive("", "q", 0)
	id := msg["id"].(string)

	if err := h.Ch().Nack("", "q", id, false); err != nil {
		t.Fatalf("Nack: %v", err)
	}
	nacked := GetStubChannelNacked()
	if !nacked[msgID] {
		t.Errorf("expected %s to be nacked, got %v", msgID, nacked)
	}
}

func TestChannelSubscribeUnsubscribe(t *testing.T) {
	ResetStubs()
	h := NewHost()

	subID, err := h.Ch().Subscribe("", "events:login", "")
	if err != nil {
		t.Fatalf("Subscribe: %v", err)
	}
	if subID == "" {
		t.Fatal("Subscribe: expected non-empty subscription ID")
	}

	subs := GetStubChannelSubscriptions()
	if subs[subID] != "events:login" {
		t.Errorf("expected sub %s → events:login, got %v", subID, subs)
	}

	if err := h.Ch().Unsubscribe(subID); err != nil {
		t.Fatalf("Unsubscribe: %v", err)
	}
	subs = GetStubChannelSubscriptions()
	if _, exists := subs[subID]; exists {
		t.Errorf("expected sub %s to be removed after Unsubscribe", subID)
	}
}

func TestChannelSubscribeUniqueIDs(t *testing.T) {
	ResetStubs()
	h := NewHost()

	id1, _ := h.Ch().Subscribe("", "events:a", "")
	id2, _ := h.Ch().Subscribe("", "events:b", "")
	if id1 == id2 {
		t.Errorf("expected unique subscription IDs, got identical: %s", id1)
	}
}

func TestChannelPublish(t *testing.T) {
	ResetStubs()
	h := NewHost()

	msgID, err := h.Ch().Publish("", "events:login", "user_login", map[string]any{"user": "alice"})
	if err != nil {
		t.Fatalf("Publish: %v", err)
	}
	if msgID == "" {
		t.Fatal("Publish: expected non-empty message ID")
	}
}

func TestChannelDepth(t *testing.T) {
	ResetStubs()
	h := NewHost()

	h.Ch().Send("", "tasks:depth", "t", nil)
	h.Ch().Send("", "tasks:depth", "t", nil)

	depth, err := h.Ch().Depth("", "tasks:depth")
	if err != nil {
		t.Fatalf("Depth: %v", err)
	}
	if depth != 2 {
		t.Errorf("expected depth=2 got %d", depth)
	}
}

func TestChannelDepthAfterReceive(t *testing.T) {
	ResetStubs()
	h := NewHost()

	h.Ch().Send("", "tasks:dr", "t", nil)
	h.Ch().Send("", "tasks:dr", "t", nil)
	h.Ch().Receive("", "tasks:dr", 0)

	depth, err := h.Ch().Depth("", "tasks:dr")
	if err != nil {
		t.Fatalf("Depth after receive: %v", err)
	}
	if depth != 1 {
		t.Errorf("expected depth=1 after receive, got %d", depth)
	}
}

func TestChannelCreateDelete(t *testing.T) {
	ResetStubs()
	h := NewHost()

	if err := h.Ch().Create("", "managed:q", 100, 60000); err != nil {
		t.Fatalf("Create: %v", err)
	}
	h.Ch().Send("", "managed:q", "x", nil)

	if err := h.Ch().Delete("", "managed:q"); err != nil {
		t.Fatalf("Delete: %v", err)
	}
	depth, _ := h.Ch().Depth("", "managed:q")
	if depth != 0 {
		t.Errorf("expected depth=0 after delete, got %d", depth)
	}
}

func TestChannelSendWithOptions(t *testing.T) {
	ResetStubs()
	h := NewHost()

	msgID, err := h.Ch().SendWithOptions("", "delayed:q", "work", map[string]any{"n": 1}, 500, 30000, map[string]string{"x-priority": "high"})
	if err != nil {
		t.Fatalf("SendWithOptions: %v", err)
	}
	if msgID == "" {
		t.Fatal("SendWithOptions: expected non-empty message ID")
	}
	depth, _ := h.Ch().Depth("", "delayed:q")
	if depth != 1 {
		t.Errorf("expected depth=1 after SendWithOptions, got %d", depth)
	}
}
