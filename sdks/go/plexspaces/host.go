// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// PlexSpaces Go SDK - Host Functions
//
// Provides Go wrappers for WIT host imports. When compiled with TinyGo
// to WASM, the //go:wasmimport directives link to the actual host functions.
// Outside WASM, stub implementations are used.
//
// Actor message payloads and several host APIs use JSON strings at the WIT boundary.
// TupleSpace host imports (ts-write, ts-read, …) use protobuf wire bytes (WriteRequest,
// ReadRequest, ReadResponse) per wit/plexspaces-actor/host.wit; the SDK maps []any patterns
// to those messages (WASM) or JSON for native test stubs.
// Shard-group and application-metrics/status host calls use protobuf wire on WASM
// (see host_actor_api_wire_wasm.go) and JSON for native stubs (host_actor_api_wire_native.go).

package plexspaces

import (
	"encoding/json"
	"fmt"
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
	ts  *TupleSpace
	ch  *Channel
	reg *Registry
}

// NewHost creates a new Host instance.
func NewHost() *Host {
	h := &Host{}
	h.ts = &TupleSpace{host: h}
	h.ch = &Channel{host: h}
	h.reg = &Registry{}
	return h
}

// Registry returns the Object Registry sub-API.
func (h *Host) Registry() *Registry { return h.reg }

// TupleSpace provides list-in, list-out tuple space API. Use nil in patterns for wildcards.
// Consistent with Python host.ts and TypeScript host.ts.
type TupleSpace struct {
	host *Host
}

// TS returns the TupleSpace helper for list-in, list-out operations.
func (h *Host) TS() *TupleSpace { return h.ts }

// Write writes a tuple. Elements must be JSON-serializable (WASM: encoded as tuplespace WriteRequest protobuf).
func (ts *TupleSpace) Write(tuple []any) string {
	data, err := tsWriteWire(tuple)
	if err != nil {
		return "ERROR: " + err.Error()
	}
	return ts.host.TSWrite(string(data))
}

// Take removes and returns one matching tuple. Returns (tuple, true) or (nil, false) if no match/error.
func (ts *TupleSpace) Take(pattern []any) ([]any, bool) {
	data, err := tsReadRequestWire(pattern, true, 1)
	if err != nil {
		return nil, false
	}
	raw := ts.host.TSTake(string(data))
	return tsDecodeReadResponseFirstTuple(raw)
}

// Read returns one matching tuple (non-destructive). Returns (tuple, true) or (nil, false) if no match/error.
func (ts *TupleSpace) Read(pattern []any) ([]any, bool) {
	data, err := tsReadRequestWire(pattern, false, 1)
	if err != nil {
		return nil, false
	}
	raw := ts.host.TSRead(string(data))
	return tsDecodeReadResponseFirstTuple(raw)
}

// ReadAll returns all matching tuples (non-destructive). Returns slice of tuples (each tuple is []any).
func (ts *TupleSpace) ReadAll(pattern []any) [][]any {
	data, err := tsReadRequestWire(pattern, false, 1024)
	if err != nil {
		return nil
	}
	raw := ts.host.TSReadAll(string(data))
	return tsDecodeReadResponseAllTuples(raw)
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

// KVGetJSON retrieves a value by key and JSON-unmarshals it into dest.
// Returns (true, nil) on success, (false, nil) if the key does not exist,
// or (false, err) if unmarshalling fails.
//
//	var task map[string]any
//	found, err := host.KVGetJSON("queue:pending:1", &task)
func (h *Host) KVGetJSON(key string, dest any) (bool, error) {
	raw := h.KVGet(key)
	if raw == "" {
		return false, nil
	}
	if err := json.Unmarshal([]byte(raw), dest); err != nil {
		return false, fmt.Errorf("KVGetJSON(%q): %w", key, err)
	}
	return true, nil
}

// KVPutJSON marshals src to JSON and stores it under key.
// Returns an error if marshalling fails or the KV write fails.
//
//	if err := host.KVPutJSON("queue:pending:1", task); err != nil {
//	    host.Warn(fmt.Sprintf("KVPutJSON failed: %v", err))
//	}
func (h *Host) KVPutJSON(key string, src any) error {
	data, err := json.Marshal(src)
	if err != nil {
		return fmt.Errorf("KVPutJSON(%q): marshal: %w", key, err)
	}
	if result := h.KVPut(key, string(data)); isHostError(result) {
		return fmt.Errorf("KVPutJSON(%q): %s", key, result)
	}
	return nil
}

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
// On success returns the server-assigned blob id (ULID on WASM); native stubs return "".
// On failure returns "ERROR:message". Use IsHostError to tell error from success.
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
// EventLog — two-cursor watermark (embed in actor state)
// ========================================================================

// EventLog is a monotonic append-only log backed by KV.
// Embed it in actor state (it serializes with the actor via JSON tags).
// Each consumer tracks its own read cursor so they advance independently.
//
// Usage:
//
//	type MyActor struct {
//	    plexspaces.BaseActor
//	    Log plexspaces.EventLog `json:"log"`
//	}
//
//	// append
//	_ = a.Log.Append(h, "audit:", map[string]any{"action": "login"})
//
//	// poll
//	events, newCursor, _ := a.Log.Poll(h, "audit:", "consumer-1", 20)
type EventLog struct {
	Watermark int64 `json:"watermark"`
}

// Append writes entry to KV at <prefix>seq:<watermark+1> and increments the watermark.
// Returns the sequence number assigned.
func (el *EventLog) Append(h *Host, prefix string, entry any) (int64, error) {
	el.Watermark++
	key := fmt.Sprintf("%sseq:%d", prefix, el.Watermark)
	if err := h.KVPutJSON(key, entry); err != nil {
		el.Watermark-- // roll back on failure
		return 0, err
	}
	return el.Watermark, nil
}

// Poll reads up to limit events for consumerID starting after its stored cursor.
// Returns the events, the new cursor value, and any error.
// The caller is responsible for persisting updated actor state (which contains el.Watermark).
func (el *EventLog) Poll(h *Host, prefix, consumerID string, limit int) ([]any, int64, error) {
	cursorKey := prefix + "cursor:" + consumerID
	var cursor int64
	if raw := h.KVGet(cursorKey); raw != "" {
		fmt.Sscanf(raw, "%d", &cursor)
	}

	var events []any
	newCursor := cursor
	for seq := cursor + 1; seq <= el.Watermark && len(events) < limit; seq++ {
		key := fmt.Sprintf("%sseq:%d", prefix, seq)
		var entry any
		found, err := h.KVGetJSON(key, &entry)
		if err != nil {
			return events, newCursor, err
		}
		if found {
			events = append(events, entry)
			newCursor = seq
		}
	}

	if newCursor != cursor {
		h.KVPut(cursorKey, fmt.Sprintf("%d", newCursor))
	}
	return events, newCursor, nil
}

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

// First returns the first member of a named process group.
// Returns an error if the group is empty or the PG call fails.
// Use this for location-transparent service discovery.
//
//	agentID, err := host.PG().First("svc:agent")
func (pg *ProcessGroups) First(group string) (string, error) {
	members, err := pg.Members(group)
	if err != nil {
		return "", fmt.Errorf("pg.Members(%q): %w", group, err)
	}
	if len(members) == 0 {
		return "", fmt.Errorf("no members in process group %q", group)
	}
	return members[0], nil
}

// ========================================================================
// Object Registry
// ========================================================================

// ObjectRegistration holds metadata for a registered object.
type ObjectRegistration struct {
	ObjectID       string   `json:"object_id"`
	ObjectType     string   `json:"object_type"`
	GRPCAddress    string   `json:"grpc_address"`
	ObjectCategory string   `json:"object_category"`
	TenantID       string   `json:"tenant_id,omitempty"`
	Namespace      string   `json:"namespace,omitempty"`
	Capabilities   []string `json:"capabilities"`
	Labels         []string `json:"labels"`
	HealthStatus   string   `json:"health_status"`
	CreatedAt      uint64   `json:"created_at"`
	UpdatedAt      uint64   `json:"updated_at"`
	LastHeartbeat  *uint64  `json:"last_heartbeat,omitempty"`
	Alias          *string  `json:"alias,omitempty"`
}

// Registry provides access to object registry operations.
type Registry struct{}

// Register registers an object in the registry.
func (r *Registry) Register(reg ObjectRegistration) error {
	reqBytes := encodeRegisterRequest(reg)
	result := hostRegistryRegister(string(reqBytes))
	return checkError(result)
}

// Unregister removes an object from the registry.
func (r *Registry) Unregister(objectID string, objectType int32, tenantID, namespace string) error {
	reqBytes := encodeUnregisterRequest(objectID, objectType, tenantID, namespace)
	result := hostRegistryUnregister(string(reqBytes))
	return checkError(result)
}

// Lookup finds an object by ID. Returns nil if not found.
func (r *Registry) Lookup(objectID string, objectType int32, tenantID, namespace string) (*ObjectRegistration, error) {
	reqBytes := encodeLookupRequest(objectID, objectType, tenantID, namespace, "")
	result := hostRegistryLookup(string(reqBytes))
	if isHostError(result) {
		return nil, &HostError{result}
	}
	if result == "" {
		return nil, nil
	}
	reg, found := decodeLookupResponse([]byte(result))
	if !found {
		return nil, nil
	}
	return reg, nil
}

// LookupByAlias finds an object by alias. Returns nil if not found.
// Alias format: "{actor_type}:{name}:{namespace}:{tenant_id}"
func (r *Registry) LookupByAlias(alias string) (*ObjectRegistration, error) {
	result := hostRegistryLookupByAlias(alias)
	if isHostError(result) {
		return nil, &HostError{result}
	}
	if result == "" {
		return nil, nil
	}
	reg, found := decodeLookupResponse([]byte(result))
	if !found {
		return nil, nil
	}
	return reg, nil
}

// DiscoverOptions holds filter criteria for Discover.
type DiscoverOptions struct {
	ObjectType     int32
	ObjectCategory string
	TenantID       string
	Namespace      string
	Capabilities   []string
	Labels         []string
	PageSize       int32
}

// Discover finds objects matching the given criteria.
func (r *Registry) Discover(opts DiscoverOptions) ([]ObjectRegistration, error) {
	pageSize := opts.PageSize
	if pageSize == 0 {
		pageSize = 100
	}
	reqBytes := encodeDiscoverRequest(opts.ObjectType, opts.ObjectCategory,
		opts.TenantID, opts.Namespace, opts.Capabilities, opts.Labels, pageSize)
	result := hostRegistryDiscover(string(reqBytes))
	if isHostError(result) {
		return nil, &HostError{result}
	}
	if result == "" {
		return nil, nil
	}
	regs := decodeDiscoverResponse([]byte(result))
	return regs, nil
}

// Heartbeat updates the heartbeat for a registered object.
func (r *Registry) Heartbeat(objectID string, objectType int32, tenantID, namespace string) error {
	reqBytes := encodeHeartbeatRequest(objectID, objectType, tenantID, namespace)
	result := hostRegistryHeartbeat(string(reqBytes))
	return checkError(result)
}

// ========================================================================
// Channel (queue + pub/sub)
// ========================================================================

// Channel provides access to channel/queue operations (queue and pub/sub patterns).
// Uses ctx as a JSON string encoding the tenant/namespace context.
type Channel struct {
	host *Host
}

// Ch returns the Channel sub-API.
func (h *Host) Ch() *Channel { return h.ch }

// Send sends a message to a channel (queue semantics — one consumer receives it).
// Returns the message ID on success.
func (ch *Channel) Send(ctx, channelName, msgType string, payload any) (string, error) {
	payloadJSON := marshalPayload(payload)
	ctxJSON := marshalPayload(ctx)
	result := hostChannelSend(ctxJSON, channelName, msgType, payloadJSON)
	if isHostError(result) {
		return "", &HostError{result}
	}
	return result, nil
}

// SendWithOptions sends a message with delay, TTL, and custom headers.
// delayMs: delay before message becomes visible (0 = immediate).
// ttlMs: time-to-live (0 = no expiry).
// headers: additional metadata key-value pairs.
func (ch *Channel) SendWithOptions(ctx, channelName, msgType string, payload any, delayMs, ttlMs uint64, headers map[string]string) (string, error) {
	payloadJSON := marshalPayload(payload)
	ctxJSON := marshalPayload(ctx)
	headersJSON := marshalPayload(headers)
	result := hostChannelSendWithOptions(ctxJSON, channelName, msgType, payloadJSON, delayMs, ttlMs, headersJSON)
	if isHostError(result) {
		return "", &HostError{result}
	}
	return result, nil
}

// Receive receives one message from a channel (blocking up to timeoutMs).
// Returns (message map, true) on receipt; (nil, false) on timeout/empty channel.
// The message map contains: id, msg_type, payload, timestamp, delivery_count, headers.
// Call Ack after successful processing to prevent redelivery.
func (ch *Channel) Receive(ctx, channelName string, timeoutMs uint64) (map[string]any, bool, error) {
	ctxJSON := marshalPayload(ctx)
	result := hostChannelReceive(ctxJSON, channelName, timeoutMs)
	if isHostError(result) {
		return nil, false, &HostError{result}
	}
	if result == "" {
		return nil, false, nil
	}
	var msg map[string]any
	if err := json.Unmarshal([]byte(result), &msg); err != nil {
		return nil, false, fmt.Errorf("channel receive decode: %w", err)
	}
	return msg, true, nil
}

// Publish publishes a message to a channel (pub/sub — all subscribers receive it).
// Returns the message ID on success.
func (ch *Channel) Publish(ctx, channelName, msgType string, payload any) (string, error) {
	payloadJSON := marshalPayload(payload)
	ctxJSON := marshalPayload(ctx)
	result := hostChannelPublish(ctxJSON, channelName, msgType, payloadJSON)
	if isHostError(result) {
		return "", &HostError{result}
	}
	return result, nil
}

// Subscribe subscribes to a channel (pub/sub pattern).
// filter: optional message-type filter (empty string = all messages).
// Returns a subscription ID for use with Unsubscribe.
func (ch *Channel) Subscribe(ctx, channelName, filter string) (string, error) {
	ctxJSON := marshalPayload(ctx)
	result := hostChannelSubscribe(ctxJSON, channelName, filter)
	if isHostError(result) {
		return "", &HostError{result}
	}
	return result, nil
}

// Unsubscribe cancels a subscription by its ID.
func (ch *Channel) Unsubscribe(subscriptionID string) error {
	return checkError(hostChannelUnsubscribe(subscriptionID))
}

// Ack acknowledges successful message processing (prevents redelivery).
func (ch *Channel) Ack(ctx, channelName, messageID string) error {
	ctxJSON := marshalPayload(ctx)
	return checkError(hostChannelAck(ctxJSON, channelName, messageID))
}

// Nack negative-acknowledges a message.
// requeue=true returns the message to the channel for retry; false sends it to the dead-letter channel.
func (ch *Channel) Nack(ctx, channelName, messageID string, requeue bool) error {
	ctxJSON := marshalPayload(ctx)
	return checkError(hostChannelNack(ctxJSON, channelName, messageID, requeue))
}

// Create creates a channel if it does not exist.
// maxSize: capacity limit (0 = unbounded).
// messageTTLMs: default TTL for messages (0 = no expiry).
func (ch *Channel) Create(ctx, channelName string, maxSize uint32, messageTTLMs uint64) error {
	ctxJSON := marshalPayload(ctx)
	return checkError(hostChannelCreate(ctxJSON, channelName, maxSize, messageTTLMs))
}

// Delete deletes a channel and all its pending messages.
func (ch *Channel) Delete(ctx, channelName string) error {
	ctxJSON := marshalPayload(ctx)
	return checkError(hostChannelDelete(ctxJSON, channelName))
}

// Depth returns the number of pending (unacked) messages in a channel.
func (ch *Channel) Depth(ctx, channelName string) (uint64, error) {
	ctxJSON := marshalPayload(ctx)
	result := hostChannelDepth(ctxJSON, channelName)
	if isHostError(result) {
		return 0, &HostError{result}
	}
	var depth uint64
	if _, err := fmt.Sscanf(result, "%d", &depth); err != nil {
		return 0, fmt.Errorf("channel depth: malformed response %q: %w", result, err)
	}
	return depth, nil
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

// CreateShardGroup creates a shard group. Native builds send JSON to stubs; WASM sends
// CreateShardGroupRequest protobuf wire per the actor host WIT contract.
func (h *Host) CreateShardGroup(request any) (map[string]any, error) {
	wire, err := hostWireCreateShardGroupRequest(request)
	if err != nil {
		return nil, err
	}
	result := hostCreateShardGroup(wire)
	if isHostError(result) {
		return nil, &HostError{result}
	}
	return hostDecodeCreateShardGroupResponse(result)
}

// BulkUpdateShardGroup sends bulk updates to shards.
func (h *Host) BulkUpdateShardGroup(request any) (map[string]any, error) {
	wire, err := hostWireBulkUpdateShardGroupRequest(request)
	if err != nil {
		return nil, err
	}
	result := hostBulkUpdateShardGroup(wire)
	if isHostError(result) {
		return nil, &HostError{result}
	}
	return hostDecodeBulkUpdateShardGroupResponse(result)
}

// MapShardGroup maps a query across shards.
func (h *Host) MapShardGroup(request any) (map[string]any, error) {
	wire, err := hostWireMapShardGroupRequest(request)
	if err != nil {
		return nil, err
	}
	result := hostMapShardGroup(wire)
	if isHostError(result) {
		return nil, &HostError{result}
	}
	return hostDecodeMapShardGroupResponse(result)
}

// ScatterGather runs scatter/gather across a shard group.
func (h *Host) ScatterGather(request any) (map[string]any, error) {
	wire, err := hostWireScatterGatherRequest(request)
	if err != nil {
		return nil, err
	}
	result := hostScatterGather(wire)
	if isHostError(result) {
		return nil, &HostError{result}
	}
	return hostDecodeScatterGatherResponse(result)
}

// BroadcastShardGroup broadcasts a message to every shard in a group.
func (h *Host) BroadcastShardGroup(request any) (map[string]any, error) {
	wire, err := hostWireBroadcastShardGroupRequest(request)
	if err != nil {
		return nil, err
	}
	result := hostBroadcastShardGroup(wire)
	if isHostError(result) {
		return nil, &HostError{result}
	}
	return hostDecodeBroadcastShardGroupResponse(result)
}

// ReduceShardGroup reduces values returned by a shard-group map operation.
func (h *Host) ReduceShardGroup(request any) (map[string]any, error) {
	wire, err := hostWireReduceShardGroupRequest(request)
	if err != nil {
		return nil, err
	}
	result := hostReduceShardGroup(wire)
	if isHostError(result) {
		return nil, &HostError{result}
	}
	return hostDecodeReduceShardGroupResponse(result)
}

// AllReduceShardGroup reduces values and broadcasts the reduced result back to all shards.
func (h *Host) AllReduceShardGroup(request any) (map[string]any, error) {
	wire, err := hostWireAllReduceShardGroupRequest(request)
	if err != nil {
		return nil, err
	}
	result := hostAllReduceShardGroup(wire)
	if isHostError(result) {
		return nil, &HostError{result}
	}
	return hostDecodeAllReduceShardGroupResponse(result)
}

// BarrierShardGroup synchronizes a shard group at a framework barrier round.
func (h *Host) BarrierShardGroup(request any) (map[string]any, error) {
	wire, err := hostWireBarrierShardGroupRequest(request)
	if err != nil {
		return nil, err
	}
	result := hostBarrierShardGroup(wire)
	if isHostError(result) {
		return nil, &HostError{result}
	}
	return hostDecodeBarrierShardGroupResponse(result)
}

// SpawnActors spawns multiple actors using the framework actor service.
func (h *Host) SpawnActors(request any) (map[string]any, error) {
	wire, err := hostWireSpawnActorsRequest(request)
	if err != nil {
		return nil, err
	}
	result := hostSpawnActors(wire)
	if isHostError(result) {
		return nil, &HostError{result}
	}
	return hostDecodeSpawnActorsResponse(result)
}

// ApplicationMetricsAdd merges a node-local application metrics delta.
func (h *Host) ApplicationMetricsAdd(applicationID string, metrics any) (map[string]any, error) {
	wire, err := hostWireApplicationMetrics(metrics)
	if err != nil {
		return nil, err
	}
	result := hostApplicationMetricsAdd(applicationID, wire)
	if isHostError(result) {
		return nil, &HostError{result}
	}
	return hostDecodeApplicationMetricsResponse(result)
}

// ApplicationGetMetrics returns node-local application metrics for a participating node.
func (h *Host) ApplicationGetMetrics(applicationID, nodeID string) (map[string]any, error) {
	result := hostApplicationGetMetrics(applicationID, nodeID)
	if isHostError(result) {
		return nil, &HostError{result}
	}
	return hostDecodeApplicationMetricsResponse(result)
}

// HTTPFetch executes an outbound HTTP request via a named service link.
//
// The link must be pre-configured in RuntimeConfig.service_links.
// The host handles retries, circuit breaking, and auth injection.
//
// linkName: Service link name (e.g. "payments-api")
// method: HTTP method ("GET", "POST", "PUT", "DELETE", "PATCH")
// pathAndQuery: Path and optional query string (e.g. "/v1/users?limit=10")
// headers: Optional extra headers (nil = no extra headers)
// body: Optional request body (nil = empty body)
//
// Returns a map with "status" (float64), "headers" (map), "body" (string).
func (h *Host) HTTPFetch(linkName, method, pathAndQuery string, headers map[string]string, body []byte) (map[string]any, error) {
	reqWire, err := encodeHttpFetchRequestWire(headers, body)
	if err != nil {
		return nil, err
	}
	result := hostHTTPFetch(linkName, method, pathAndQuery, reqWire)
	if isHostError(result) {
		return nil, &HostError{result}
	}
	// Native stubs return JSON; WASM returns HttpFetchResponse protobuf bytes as the result string.
	var jsonOut map[string]any
	if err := json.Unmarshal([]byte(result), &jsonOut); err == nil {
		return jsonOut, nil
	}
	return decodeHttpFetchResponseWire([]byte(result))
}

// ApplicationGetStatus returns application status for a participating node.
func (h *Host) ApplicationGetStatus(applicationID, nodeID string) (map[string]any, error) {
	result := hostApplicationGetStatus(applicationID, nodeID)
	if isHostError(result) {
		return nil, &HostError{result}
	}
	return hostDecodeApplicationGetStatusResponse(result)
}

// ========================================================================
// ServiceHTTPClient — ergonomic outbound HTTP client for actors
// ========================================================================

// ServiceHTTPClient is an ergonomic outbound HTTP client backed by a named service link.
//
// The link must be pre-configured in RuntimeConfig.service_links.
// The host handles retries, circuit breaking, and auth injection.
//
// Usage:
//
//	h := plexspaces.NewHost()
//	client := plexspaces.NewServiceHTTPClient(h, "payments-api")
//	resp, err := client.Get("/v1/balance?account=123", nil)
type ServiceHTTPClient struct {
	host     *Host
	linkName string
}

// NewServiceHTTPClient creates a new ServiceHTTPClient bound to a named service link.
func NewServiceHTTPClient(h *Host, linkName string) *ServiceHTTPClient {
	return &ServiceHTTPClient{host: h, linkName: linkName}
}

// Get sends a GET request. Returns response map with "status", "headers", "body".
func (c *ServiceHTTPClient) Get(pathAndQuery string, headers map[string]string) (map[string]any, error) {
	return c.host.HTTPFetch(c.linkName, "GET", pathAndQuery, headers, nil)
}

// Post sends a POST request with a JSON body. Returns response map.
func (c *ServiceHTTPClient) Post(pathAndQuery string, body []byte, headers map[string]string) (map[string]any, error) {
	return c.host.HTTPFetch(c.linkName, "POST", pathAndQuery, headers, body)
}

// Put sends a PUT request with a JSON body. Returns response map.
func (c *ServiceHTTPClient) Put(pathAndQuery string, body []byte, headers map[string]string) (map[string]any, error) {
	return c.host.HTTPFetch(c.linkName, "PUT", pathAndQuery, headers, body)
}

// Delete sends a DELETE request. Returns response map.
func (c *ServiceHTTPClient) Delete(pathAndQuery string, headers map[string]string) (map[string]any, error) {
	return c.host.HTTPFetch(c.linkName, "DELETE", pathAndQuery, headers, nil)
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

// IsHostError reports whether the host returned the error arm of result<T, actor-error>.
// Success payloads are often non-empty (for example blob-upload returns the stored blob ULID).
func IsHostError(result string) bool {
	return strings.HasPrefix(result, errorPrefix)
}

func isHostError(result string) bool {
	return IsHostError(result)
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
// marshalStringSlice serializes a string slice to a JSON array for WIT boundary crossing.
// TinyGo //go:wasmimport functions cannot accept Go slices directly.
func marshalStringSlice(s []string) string {
	if len(s) == 0 {
		return "[]"
	}
	data, err := json.Marshal(s)
	if err != nil {
		return "[]"
	}
	return string(data)
}

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
