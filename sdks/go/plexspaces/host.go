// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// PlexSpaces Go SDK - Host Functions
//
// Provides Go wrappers for WIT host imports. When compiled with TinyGo
// to WASM, the //go:wasmimport directives link to the actual host functions.
// Outside WASM, stub implementations are used.

package plexspaces

import (
	"encoding/json"
)

// Host provides access to PlexSpaces host functions from within a WASM actor.
//
// Usage:
//
//	host := plexspaces.NewHost()
//	host.Send("other-actor", "ping", map[string]any{"data": "hello"})
//	response := host.Ask("other-actor", "get_balance", nil, 5000)
//	myID := host.SelfID()
type Host struct{}

// NewHost creates a new Host instance.
func NewHost() *Host {
	return &Host{}
}

// ========================================================================
// Messaging
// ========================================================================

// Send sends a message to another actor (fire-and-forget).
func (h *Host) Send(to, msgType string, payload any) string {
	payloadJSON := marshalPayload(payload)
	return hostSend(to, msgType, payloadJSON)
}

// Ask sends a request and waits for a response (request-reply pattern).
// timeoutMs is the maximum wait time in milliseconds (0 = default timeout).
func (h *Host) Ask(to, msgType string, payload any, timeoutMs uint64) (any, error) {
	payloadJSON := marshalPayload(payload)
	result := hostAsk(to, msgType, payloadJSON, timeoutMs)
	if len(result) > 6 && result[:6] == "ERROR:" {
		return nil, &HostError{result}
	}
	var parsed any
	if err := json.Unmarshal([]byte(result), &parsed); err != nil {
		return result, nil
	}
	return parsed, nil
}

// ========================================================================
// Actor Identity
// ========================================================================

// SelfID returns the actor's own ID.
func (h *Host) SelfID() string {
	return hostSelfID()
}

// ========================================================================
// Actor Lifecycle
// ========================================================================

// Spawn creates a new actor. Delegates to ActorFactory::spawn_actor() via the host.
// moduleRef is the actor type/module reference (must be a deployed WASM module or registered behavior).
// actorID is the unique ID for the new actor (empty = auto-generated ULID).
// Returns the spawned actor ID (may be auto-generated if actorID was empty).
func (h *Host) Spawn(moduleRef, actorID string, initConfig any) (string, error) {
	configJSON := marshalPayload(initConfig)
	result := hostSpawn(moduleRef, actorID, configJSON)
	if len(result) > 6 && result[:6] == "ERROR:" {
		return "", &HostError{result}
	}
	return result, nil
}

// Stop gracefully stops an actor.
func (h *Host) Stop(actorID string) error {
	result := hostStop(actorID)
	return checkError(result)
}

// ========================================================================
// Actor Linking & Monitoring (Erlang/OTP patterns)
// ========================================================================

// Link creates a bidirectional link with another actor.
func (h *Host) Link(actorID string) error {
	result := hostLink(actorID)
	return checkError(result)
}

// Unlink removes a bidirectional link.
func (h *Host) Unlink(actorID string) error {
	result := hostUnlink(actorID)
	return checkError(result)
}

// Monitor creates a unidirectional monitor. Returns a monitor reference.
func (h *Host) Monitor(actorID string) (string, error) {
	result := hostMonitor(actorID)
	if len(result) > 6 && result[:6] == "ERROR:" {
		return "", &HostError{result}
	}
	return result, nil
}

// Demonitor cancels a monitor.
func (h *Host) Demonitor(monitorRef string) error {
	result := hostDemonitor(monitorRef)
	return checkError(result)
}

// ========================================================================
// Timers
// ========================================================================

// SendAfter sends a message to self after a delay. Returns a timer ID for tracking.
// Timer cancellation is managed by the framework's TimerFacet/ReminderFacet.
// Stop the actor to cancel pending timers.
func (h *Host) SendAfter(delayMs uint64, msgType string, payload any) string {
	payloadJSON := marshalPayload(payload)
	return hostSendAfter(delayMs, msgType, payloadJSON)
}

// ========================================================================
// Logging & Time
// ========================================================================

// Log logs a message at the specified level.
func (h *Host) Log(level, message string) {
	hostLog(level, message)
}

// Debug logs a debug message.
func (h *Host) Debug(message string) { h.Log("debug", message) }

// Info logs an info message.
func (h *Host) Info(message string) { h.Log("info", message) }

// Warn logs a warning message.
func (h *Host) Warn(message string) { h.Log("warn", message) }

// Error logs an error message.
func (h *Host) Error(message string) { h.Log("error", message) }

// NowMs returns the current timestamp in milliseconds.
func (h *Host) NowMs() uint64 {
	return hostNowMs()
}

// ========================================================================
// Key-Value Store
// ========================================================================

// KVGet retrieves a value by key.
func (h *Host) KVGet(key string) string { return hostKVGet(key) }

// KVPut stores a value.
func (h *Host) KVPut(key, value string) string { return hostKVPut(key, value) }

// KVDelete removes a key.
func (h *Host) KVDelete(key string) string { return hostKVDelete(key) }

// KVList lists keys with a prefix.
func (h *Host) KVList(prefix string) string { return hostKVList(prefix) }

// ========================================================================
// Process Groups
// ========================================================================

// ProcessGroups provides access to process group operations.
type ProcessGroups struct{}

// PG returns the ProcessGroups sub-API.
func (h *Host) PG() *ProcessGroups { return &ProcessGroups{} }

// Join joins a named process group.
func (pg *ProcessGroups) Join(group string) error {
	result := hostPGJoin(group)
	return checkError(result)
}

// Leave leaves a named process group.
func (pg *ProcessGroups) Leave(group string) error {
	result := hostPGLeave(group)
	return checkError(result)
}

// Members returns the members of a process group.
func (pg *ProcessGroups) Members(group string) ([]string, error) {
	result := hostPGMembers(group)
	if len(result) > 6 && result[:6] == "ERROR:" {
		return nil, &HostError{result}
	}
	var members []string
	if err := json.Unmarshal([]byte(result), &members); err != nil {
		return nil, err
	}
	return members, nil
}

// Broadcast sends a message to all members of a process group.
func (pg *ProcessGroups) Broadcast(group, msgType string, payload any) error {
	payloadJSON := marshalPayload(payload)
	result := hostPGBroadcast(group, msgType, payloadJSON)
	return checkError(result)
}

// ========================================================================
// Helpers
// ========================================================================

// HostError represents an error from a host function call.
type HostError struct {
	Message string
}

func (e *HostError) Error() string { return e.Message }

func marshalPayload(payload any) string {
	if payload == nil {
		return "{}"
	}
	if s, ok := payload.(string); ok {
		return s
	}
	data, err := json.Marshal(payload)
	if err != nil {
		return "{}"
	}
	return string(data)
}

func checkError(result string) error {
	if len(result) > 6 && result[:6] == "ERROR:" {
		return &HostError{result}
	}
	return nil
}
