// SPDX-License-Identifier: LGPL-2.1-or-later
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
	h := NewHost()
	members, err := h.PG().Members("workers")
	if err != nil {
		t.Fatalf("PG.Members should not return error: %v", err)
	}
	if len(members) != 2 {
		t.Errorf("expected 2 members, got %d", len(members))
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

func TestActorRouterInitWithNamespace(t *testing.T) {
	router := NewActorRouter()
	router.Route("rate-limiter", func() Actor { return newCounterActor() })

	// Full actor ID format: name:namespace@node
	result := router.Init(`{"actor_id":"rate-limiter:default@test-node"}`)
	if result != "" {
		t.Errorf("expected success, got %q", result)
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
	err := json.Unmarshal([]byte(`{"actor_id":"test:ns@node","args":{"key":"value"}}`), &config)
	if err != nil {
		t.Fatalf("failed to parse config: %v", err)
	}
	if config.ActorID != "test:ns@node" {
		t.Errorf("expected test:ns@node, got %q", config.ActorID)
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
	h := NewHost()
	result := h.TSWrite(`["task","worker-1",123]`)
	if isHostError(result) {
		t.Errorf("TSWrite should succeed, got %q", result)
	}
	result = h.TSRead(`["task","*",null]`)
	// Stub returns empty
	result = h.TSTake(`["task","*",null]`)
	// Stub returns empty
	result = h.TSReadAll(`["task","*",null]`)
	if result != "[]" {
		t.Errorf("TSReadAll stub should return [], got %q", result)
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

