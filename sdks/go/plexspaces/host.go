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
type Host struct {
	ts *TupleSpace
}

// NewHost creates a new Host instance.
func NewHost() *Host {
	h := &Host{}
	h.ts = &TupleSpace{host: h}
	return h
}

// TupleSpace provides list-in, list-out tuple space API. Use nil in patterns for wildcards.
// Consistent with Python host.ts and TypeScript host.ts.
type TupleSpace struct {
	host *Host
}

// TS returns the TupleSpace helper for list-in, list-out operations.
func (h *Host) TS() *TupleSpace { return h.ts }

// Write writes a tuple. Elements must be JSON-serializable. Returns empty on success, "ERROR:..." on failure.
func (ts *TupleSpace) Write(tuple []any) string {
	data, err := json.Marshal(tuple)
	if err != nil {
		return "ERROR: " + err.Error()
	}
	return ts.host.TSWrite(string(data))
}

// Take removes and returns one matching tuple. Returns (tuple, true) or (nil, false) if no match/error.
func (ts *TupleSpace) Take(pattern []any) ([]any, bool) {
	data, err := json.Marshal(pattern)
	if err != nil {
		return nil, false
	}
	raw := ts.host.TSTake(string(data))
	if raw == "" || strings.HasPrefix(raw, errorPrefix) {
		return nil, false
	}
	var tuple []any
	if err := json.Unmarshal([]byte(raw), &tuple); err != nil {
		return nil, false
	}
	return tuple, true
}

// Read returns one matching tuple (non-destructive). Returns (tuple, true) or (nil, false) if no match/error.
func (ts *TupleSpace) Read(pattern []any) ([]any, bool) {
	data, err := json.Marshal(pattern)
	if err != nil {
		return nil, false
	}
	raw := ts.host.TSRead(string(data))
	if raw == "" || strings.HasPrefix(raw, errorPrefix) {
		return nil, false
	}
	var tuple []any
	if err := json.Unmarshal([]byte(raw), &tuple); err != nil {
		return nil, false
	}
	return tuple, true
}

// ReadAll returns all matching tuples (non-destructive). Returns slice of tuples (each tuple is []any).
func (ts *TupleSpace) ReadAll(pattern []any) [][]any {
	data, err := json.Marshal(pattern)
	if err != nil {
		return nil
	}
	raw := ts.host.TSReadAll(string(data))
	if raw == "" || strings.HasPrefix(raw, errorPrefix) {
		return nil
	}
	var out [][]any
	if err := json.Unmarshal([]byte(raw), &out); err != nil {
		return nil
	}
	return out
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
// msgType is used by the host for routing; payload can be data-only (consistent with Python/TypeScript).
func (pg *ProcessGroups) Broadcast(group, msgType string, payload any) error {
	payloadJSON := marshalPayload(payload)
	result := hostPGBroadcast(group, msgType, payloadJSON)
	return checkError(result)
}

// ========================================================================
// Elastic pool (checkout/checkin)
// ========================================================================

// PoolCheckout checks out an actor from a named pool.
// Returns a map with actor_id, pool_name, checkout_id on success, or nil on failure (pool not configured, timeout, empty).
func (h *Host) PoolCheckout(poolName string, timeoutMs uint64) map[string]any {
	result := hostPoolCheckout(poolName, timeoutMs)
	if result == "" || isHostError(result) {
		return nil
	}
	var out map[string]any
	if err := json.Unmarshal([]byte(result), &out); err != nil {
		return nil
	}
	return out
}

// PoolCheckin checks in an actor to the pool. actorID and checkoutID come from the handle returned by PoolCheckout.
// healthy should be true if the actor is healthy and can be reused.
func (h *Host) PoolCheckin(poolName, actorID, checkoutID string, healthy bool) error {
	result := hostPoolCheckin(poolName, actorID, checkoutID, healthy)
	return checkError(result)
}

// PoolGetMetrics returns pool metrics (total_actors, available_actors, busy_actors, current_load, etc.) or nil if not available.
func (h *Host) PoolGetMetrics(poolName string) map[string]any {
	result := hostPoolGetMetrics(poolName)
	if result == "" || isHostError(result) {
		return nil
	}
	var out map[string]any
	if err := json.Unmarshal([]byte(result), &out); err != nil {
		return nil
	}
	return out
}

// CreateShardGroup creates a shard group using proto field names in the JSON payload.
func (h *Host) CreateShardGroup(request any) (map[string]any, error) {
	result := hostCreateShardGroup(marshalPayload(request))
	if isHostError(result) {
		return nil, &HostError{result}
	}
	var out map[string]any
	if err := json.Unmarshal([]byte(result), &out); err != nil {
		return nil, err
	}
	return out, nil
}

// BulkUpdateShardGroup sends bulk updates to shards using proto field names in the JSON payload.
func (h *Host) BulkUpdateShardGroup(request any) (map[string]any, error) {
	result := hostBulkUpdateShardGroup(marshalPayload(request))
	if isHostError(result) {
		return nil, &HostError{result}
	}
	var out map[string]any
	if err := json.Unmarshal([]byte(result), &out); err != nil {
		return nil, err
	}
	return out, nil
}

// MapShardGroup maps a query across shards using proto field names in the JSON payload.
func (h *Host) MapShardGroup(request any) (map[string]any, error) {
	result := hostMapShardGroup(marshalPayload(request))
	if isHostError(result) {
		return nil, &HostError{result}
	}
	var out map[string]any
	if err := json.Unmarshal([]byte(result), &out); err != nil {
		return nil, err
	}
	return out, nil
}

// ScatterGather runs scatter/gather using proto field names in the JSON payload.
func (h *Host) ScatterGather(request any) (map[string]any, error) {
	result := hostScatterGather(marshalPayload(request))
	if isHostError(result) {
		return nil, &HostError{result}
	}
	var out map[string]any
	if err := json.Unmarshal([]byte(result), &out); err != nil {
		return nil, err
	}
	return out, nil
}

// BroadcastShardGroup broadcasts a message to every shard in a group.
func (h *Host) BroadcastShardGroup(request any) (map[string]any, error) {
	result := hostBroadcastShardGroup(marshalPayload(request))
	if isHostError(result) {
		return nil, &HostError{result}
	}
	var out map[string]any
	if err := json.Unmarshal([]byte(result), &out); err != nil {
		return nil, err
	}
	return out, nil
}

// ReduceShardGroup reduces values returned by a shard-group map operation.
func (h *Host) ReduceShardGroup(request any) (map[string]any, error) {
	result := hostReduceShardGroup(marshalPayload(request))
	if isHostError(result) {
		return nil, &HostError{result}
	}
	var out map[string]any
	if err := json.Unmarshal([]byte(result), &out); err != nil {
		return nil, err
	}
	return out, nil
}

// AllReduceShardGroup reduces values and broadcasts the reduced result back to all shards.
func (h *Host) AllReduceShardGroup(request any) (map[string]any, error) {
	result := hostAllReduceShardGroup(marshalPayload(request))
	if isHostError(result) {
		return nil, &HostError{result}
	}
	var out map[string]any
	if err := json.Unmarshal([]byte(result), &out); err != nil {
		return nil, err
	}
	return out, nil
}

// BarrierShardGroup synchronizes a shard group at a framework barrier round.
func (h *Host) BarrierShardGroup(request any) (map[string]any, error) {
	result := hostBarrierShardGroup(marshalPayload(request))
	if isHostError(result) {
		return nil, &HostError{result}
	}
	var out map[string]any
	if err := json.Unmarshal([]byte(result), &out); err != nil {
		return nil, err
	}
	return out, nil
}

// SpawnActors spawns multiple actors using the framework actor service.
func (h *Host) SpawnActors(request any) (map[string]any, error) {
	result := hostSpawnActors(marshalPayload(request))
	if isHostError(result) {
		return nil, &HostError{result}
	}
	var out map[string]any
	if err := json.Unmarshal([]byte(result), &out); err != nil {
		return nil, err
	}
	return out, nil
}

// ApplicationMetricsAdd merges a node-local application metrics delta.
func (h *Host) ApplicationMetricsAdd(applicationID string, metrics any) (map[string]any, error) {
	result := hostApplicationMetricsAdd(applicationID, marshalPayload(metrics))
	if isHostError(result) {
		return nil, &HostError{result}
	}
	var out map[string]any
	if err := json.Unmarshal([]byte(result), &out); err != nil {
		return nil, err
	}
	return out, nil
}

// ApplicationGetStatus returns application status for a participating node.
func (h *Host) ApplicationGetStatus(applicationID, nodeID string) (map[string]any, error) {
	result := hostApplicationGetStatus(applicationID, nodeID)
	if isHostError(result) {
		return nil, &HostError{result}
	}
	var out map[string]any
	if err := json.Unmarshal([]byte(result), &out); err != nil {
		return nil, err
	}
	return out, nil
}

// ========================================================================
// Helpers
// ========================================================================

// HostError represents an error from a host function call.
// The error message follows the WIT convention: "ERROR:<details>".
// When the host returns a structured JSON error body, use ParseErrorDetail()
// to extract the proto ErrorDetail fields (code, message, details).
type HostError struct {
	Message string
}

func (e *HostError) Error() string { return e.Message }

// ErrorDetail holds structured error information matching proto ErrorDetail
// (plexspaces.common.v1.ErrorDetail).  This is a local mirror so that host.go
// has no external proto dependency (TinyGo WASM compatibility).
// When sdks/go/plexspaces/proto/ is generated via `make proto-go`, callers
// can convert: protoDetail := &proto.ErrorDetail{Code: d.Code, Message: d.Message}
type ErrorDetail struct {
	// Code is the error code string (e.g. "NOT_FOUND", "INTERNAL").
	Code string `json:"code"`
	// Message is the human-readable error description.
	Message string `json:"message"`
}

// ParseErrorDetail attempts to parse a structured JSON error body from the HostError message.
// Host functions may embed a JSON ErrorDetail after the "ERROR:" prefix.
// Returns nil if the message is not structured JSON (plain text errors are common).
//
// Example host error: `ERROR:{"code":"NOT_FOUND","message":"actor not found"}`
func (e *HostError) ParseErrorDetail() *ErrorDetail {
	s := strings.TrimPrefix(e.Message, errorPrefix)
	s = strings.TrimSpace(s)
	var detail ErrorDetail
	if err := json.Unmarshal([]byte(s), &detail); err != nil {
		return nil
	}
	return &detail
}

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
