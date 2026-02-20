// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// PlexSpaces Go SDK - Host Functions
//
// Provides Go wrappers for WIT host imports. When compiled with TinyGo
// to WASM, the //go:wasmimport directives link to the actual host functions.
// Outside WASM, stub implementations are used.
//
// All communication uses JSON strings over the WIT interface boundary.
// Payload parameters accept any JSON-serializable value (consistent with
// Python SDK's Any and TypeScript SDK's unknown).

package plexspaces

import (
	"encoding/json"
	"strings"
)

// errorPrefix is the convention used by WIT host functions to signal errors.
const errorPrefix = "ERROR:"

// Host provides access to PlexSpaces host functions from within a WASM actor.
//
// Usage:
//
//	host := plexspaces.NewHost()
//	host.Send("other-actor", "ping", map[string]any{"data": "hello"})
//	response, err := host.Ask("other-actor", "get_balance", nil, 5000)
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
// payload is JSON-serialized before sending. Accepts any JSON-serializable value,
// a pre-serialized JSON string, or nil (sent as "{}").
func (h *Host) Send(to, msgType string, payload any) string {
	payloadJSON := marshalPayload(payload)
	return hostSend(to, msgType, payloadJSON)
}

// Ask sends a request and waits for a response (request-reply pattern).
// timeoutMs is the maximum wait time in milliseconds (0 = default timeout).
// Returns the parsed JSON response, or an error if the host returned an error.
func (h *Host) Ask(to, msgType string, payload any, timeoutMs uint64) (any, error) {
	payloadJSON := marshalPayload(payload)
	result := hostAsk(to, msgType, payloadJSON, timeoutMs)
	if isHostError(result) {
		return nil, &HostError{result}
	}
	var parsed any
	if err := json.Unmarshal([]byte(result), &parsed); err != nil {
		// If not valid JSON, return as raw string (some host functions return plain strings)
		return result, nil
	}
	return parsed, nil
}

// ========================================================================
// Actor Identity
// ========================================================================

// SelfID returns the actor's own ID (format: name:namespace@node_id).
func (h *Host) SelfID() string {
	return hostSelfID()
}

// ========================================================================
// Actor Lifecycle
// ========================================================================

// Spawn creates a new actor via the node's actor management.
// moduleRef is the actor type/module reference (must be a deployed WASM module).
// actorID is the unique ID for the new actor (empty = auto-generated ULID).
// initConfig is JSON-serialized and passed to the new actor's Init().
// Returns the spawned actor ID (may be auto-generated if actorID was empty).
func (h *Host) Spawn(moduleRef, actorID string, initConfig any) (string, error) {
	configJSON := marshalPayload(initConfig)
	result := hostSpawn(moduleRef, actorID, configJSON)
	if isHostError(result) {
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
// If either linked actor crashes, the other receives a DOWN notification.
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
// The monitoring actor receives DOWN notifications if the monitored actor stops.
func (h *Host) Monitor(actorID string) (string, error) {
	result := hostMonitor(actorID)
	if isHostError(result) {
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

// Log logs a message at the specified level (debug, info, warn, error).
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

// KVGet retrieves a value by key. Returns value or empty if not found.
func (h *Host) KVGet(key string) string { return hostKVGet(key) }

// KVPut stores a value. Returns empty on success, "ERROR:message" on failure.
func (h *Host) KVPut(key, value string) string { return hostKVPut(key, value) }

// KVDelete removes a key. Returns empty on success, "ERROR:message" on failure.
func (h *Host) KVDelete(key string) string { return hostKVDelete(key) }

// KVList lists keys with a prefix. Returns JSON array of keys.
func (h *Host) KVList(prefix string) string { return hostKVList(prefix) }

// ========================================================================
// TupleSpace (Linda-style coordination)
// ========================================================================

// TSWrite writes a tuple to the TupleSpace.
// tupleJSON: JSON array of strings/numbers, e.g. ["task","worker-1",123].
// Returns empty on success, "ERROR:message" on failure.
func (h *Host) TSWrite(tupleJSON string) string { return hostTSWrite(tupleJSON) }

// TSRead performs a non-destructive read from the TupleSpace.
// patternJSON: JSON array with wildcards (null or "*" matches any).
// Returns matched tuple as JSON array, or empty if not found.
func (h *Host) TSRead(patternJSON string) string { return hostTSRead(patternJSON) }

// TSTake performs a destructive read (removes the matched tuple).
// patternJSON: JSON array with wildcards.
// Returns matched tuple as JSON array and removes it, or empty if not found.
func (h *Host) TSTake(patternJSON string) string { return hostTSTake(patternJSON) }

// TSReadAll reads all matching tuples (non-destructive).
// patternJSON: JSON array with wildcards.
// Returns JSON array of matched tuples, e.g. [["task","w1",1],["task","w2",2]].
func (h *Host) TSReadAll(patternJSON string) string { return hostTSReadAll(patternJSON) }

// ========================================================================
// Distributed Locks
// ========================================================================

// LockAcquire acquires a distributed lock.
// Returns JSON on success with lock details, or "ERROR:message" on failure/timeout.
func (h *Host) LockAcquire(tenantID, namespace, holderID, lockName string, leaseDurationSecs uint32, timeoutMs uint64) string {
	return hostLockAcquire(tenantID, namespace, holderID, lockName, leaseDurationSecs, timeoutMs)
}

// LockRelease releases a distributed lock.
// Returns empty on success, "ERROR:message" on failure.
func (h *Host) LockRelease(lockID, tenantID, namespace, holderID, lockVersion string) string {
	return hostLockRelease(lockID, tenantID, namespace, holderID, lockVersion)
}

// LockRenew renews the lease on a held lock (heartbeat).
// Returns new lock version on success, or "ERROR:message" on failure.
func (h *Host) LockRenew(lockID, tenantID, namespace, holderID, lockVersion string, leaseDurationSecs uint32) string {
	return hostLockRenew(lockID, tenantID, namespace, holderID, lockVersion, leaseDurationSecs)
}

// ========================================================================
// Blob Storage
// ========================================================================

// BlobUpload uploads blob data (base64-encoded).
// Returns empty on success, "ERROR:message" on failure.
func (h *Host) BlobUpload(blobID, data, contentType string) string {
	return hostBlobUpload(blobID, data, contentType)
}

// BlobDownload downloads blob data.
// Returns base64-encoded content on success, empty if not found.
func (h *Host) BlobDownload(blobID string) string { return hostBlobDownload(blobID) }

// BlobDelete deletes a blob. Returns empty on success, "ERROR:message" on failure.
func (h *Host) BlobDelete(blobID string) string { return hostBlobDelete(blobID) }

// BlobList lists blobs with a prefix. Returns JSON array of blob IDs.
func (h *Host) BlobList(prefix string) string { return hostBlobList(prefix) }

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

// Members returns the members of a process group as a list of actor IDs.
func (pg *ProcessGroups) Members(group string) ([]string, error) {
	result := hostPGMembers(group)
	if isHostError(result) {
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
// The error message follows the WIT convention: "ERROR:<details>".
type HostError struct {
	Message string
}

func (e *HostError) Error() string { return e.Message }

// isHostError checks if a host function result is an error response.
func isHostError(result string) bool {
	return strings.HasPrefix(result, errorPrefix)
}

// checkError converts a host function result to an error if it's an error response.
func checkError(result string) error {
	if isHostError(result) {
		return &HostError{result}
	}
	return nil
}

// marshalPayload serializes a payload to JSON for WIT communication.
// Accepts:
//   - nil: returns "{}"
//   - string: returned as-is (assumed to be pre-serialized JSON)
//   - any other type: JSON-marshaled
//
// On marshal failure, returns the error as a JSON error object.
func marshalPayload(payload any) string {
	if payload == nil {
		return "{}"
	}
	if s, ok := payload.(string); ok {
		return s
	}
	data, err := json.Marshal(payload)
	if err != nil {
		return `{"error":"marshal failed: ` + err.Error() + `"}`
	}
	return string(data)
}
