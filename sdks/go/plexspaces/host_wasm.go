// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// PlexSpaces Go SDK - Host Functions (TinyGo WASM build)
//
// WASM variant of host.go. Does NOT import encoding/json to prevent
// TinyGo function-table corruption caused by encoding/json.init$1 running
// at WASM startup and shifting function-table indices used by Handle$invoke.
// All JSON encoding uses SafeMarshal / writeJSONString; all JSON decoding
// uses WasmJSON* helpers defined in safe_marshal_wasm.go.

//go:build wasm

package plexspaces

import (
	"encoding/base64"
	"fmt"
	"strings"
)

// errorPrefix is the convention used by WIT host functions to signal errors.
const errorPrefix = "ERROR:"

// ActorRef is a lightweight handle to a named virtual actor.
type ActorRef struct {
	actorID string
}

// GetActorRef constructs an ActorRef for a named virtual actor.
func GetActorRef(actorType, name, namespace string) *ActorRef {
	return &ActorRef{
		actorID: name + "//" + actorType + "::" + namespace + "@*",
	}
}

// ID returns the canonical actor ID string.
func (r *ActorRef) ID() string {
	return r.actorID
}

// Tell sends a fire-and-forget message to the actor.
func (r *ActorRef) Tell(msgType string, payload []byte) {
	hostSend(r.actorID, msgType, string(payload))
}

// Ask sends a request and waits for a response (up to timeoutMs milliseconds).
func (r *ActorRef) Ask(msgType string, payload []byte, timeoutMs uint64) ([]byte, error) {
	result := hostAsk(r.actorID, msgType, string(payload), timeoutMs)
	if isHostError(result) {
		return nil, &HostError{result}
	}
	return []byte(result), nil
}

// Host provides access to PlexSpaces host functions from within a WASM actor.
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

// TupleSpace provides list-in, list-out tuple space API.
type TupleSpace struct {
	host *Host
}

// TS returns the TupleSpace helper for list-in, list-out operations.
func (h *Host) TS() *TupleSpace { return h.ts }

// KeyValueStore provides a namespaced accessor for KV and durable alarm operations.
type KeyValueStore struct{}

// KV returns the KeyValueStore namespace accessor.
func (h *Host) KV() *KeyValueStore {
	return &KeyValueStore{}
}

// Alarm provides a namespaced accessor for durable alarm operations.
type Alarm struct{}

// Alarm returns the Alarm namespace accessor.
func (h *Host) Alarm() *Alarm {
	return &Alarm{}
}

// LockClient provides a namespaced accessor for distributed lock operations.
type LockClient struct{}

// Locks returns the LockClient namespace accessor.
func (h *Host) Locks() *LockClient {
	return &LockClient{}
}

// BlobStore provides a namespaced accessor for blob/object storage operations.
type BlobStore struct{}

// Blob returns the BlobStore namespace accessor.
func (h *Host) Blob() *BlobStore {
	return &BlobStore{}
}

// HTTPClient provides a namespaced accessor for outbound HTTP via service links.
type HTTPClient struct{}

// HTTP returns the HTTPClient namespace accessor.
func (h *Host) HTTP() *HTTPClient {
	return &HTTPClient{}
}

// ActorMessaging provides a namespaced accessor for actor messaging and lifecycle.
type ActorMessaging struct{}

// Actor returns the ActorMessaging namespace accessor.
func (h *Host) Actor() *ActorMessaging {
	return &ActorMessaging{}
}

// KeyValueStore methods

func (kv *KeyValueStore) Get(key string) (string, error) {
	result := hostKVGet(key)
	if isHostError(result) {
		return "", &HostError{result}
	}
	return result, nil
}
func (kv *KeyValueStore) Put(key, value string) error {
	return checkError(hostKVPut(key, value))
}
func (kv *KeyValueStore) Delete(key string) error {
	return checkError(hostKVDelete(key))
}
func (kv *KeyValueStore) List(prefix string) ([]string, error) {
	result := hostKVList(prefix)
	if isHostError(result) {
		return nil, &HostError{result}
	}
	return wasmDecodeStringArrayJSON(result)
}
func (kv *KeyValueStore) PutWithTTL(key, value string, ttlSeconds uint64) error {
	return checkError(hostKVPutWithTTL(key, value, ttlSeconds))
}
func (kv *KeyValueStore) GetTTL(key string) uint64 { return hostKVGetTTL(key) }
func (kv *KeyValueStore) CAS(key, expected, newValue string) (bool, error) {
	return hostKVCAS(key, expected, newValue), nil
}
func (kv *KeyValueStore) Increment(key string, delta int64) int64 {
	return hostKVIncrement(key, delta)
}
func (kv *KeyValueStore) MultiGet(keys []string) ([]string, error) {
	keysJSON := wasmEncodeStringSliceJSON(keys)
	raw := hostKVMultiGet(keysJSON)
	if isHostError(raw) {
		return nil, fmt.Errorf("%s", raw[len(errorPrefix):])
	}
	items, err := wasmDecodeStringArrayJSON(raw)
	if err != nil {
		return nil, fmt.Errorf("kv_multi_get: parse response: %w", err)
	}
	results := make([]string, len(items))
	for i, s := range items {
		if s == "" {
			continue
		}
		decoded, err := base64.StdEncoding.DecodeString(s)
		if err != nil {
			return nil, fmt.Errorf("kv_multi_get: decode key %d: %w", i, err)
		}
		results[i] = string(decoded)
	}
	return results, nil
}
func (kv *KeyValueStore) MultiPut(entries map[string]string) error {
	encoded := make(map[string]string, len(entries))
	for k, v := range entries {
		encoded[k] = base64.StdEncoding.EncodeToString([]byte(v))
	}
	return checkError(hostKVMultiPut(wasmEncodeStringStringMapJSON(encoded)))
}
func (kv *KeyValueStore) GetJSON(key string, dest interface{}) (bool, error) {
	// In WASM builds encoding/json is forbidden. Use kv.Get() and WasmJSON* helpers manually.
	return false, fmt.Errorf("KVGetJSON: not supported in WASM; use Get() + WasmJSON helpers")
}
func (kv *KeyValueStore) PutJSON(key string, src interface{}) error {
	return kv.Put(key, marshalPayload(src))
}

// Alarm methods

func (a *Alarm) Set(timestampMs uint64) error { return checkError(hostAlarmSet(timestampMs)) }
func (a *Alarm) SetIn(delayMs uint64) error   { return checkError(hostAlarmSet(hostNowMs() + delayMs)) }
func (a *Alarm) Get() (uint64, error)         { return hostAlarmGet(), nil }
func (a *Alarm) Delete() error                { return checkError(hostAlarmDelete()) }

// LockClient methods

func (lc *LockClient) Acquire(holderID, lockName string, leaseSecs uint32, timeoutMs uint64) ([]byte, error) {
	result := hostLockAcquire(holderID, lockName, leaseSecs, timeoutMs)
	if isHostError(result) {
		return nil, &HostError{result}
	}
	return []byte(result), nil
}
func (lc *LockClient) Release(lockID, holderID, lockVersion string) error {
	return checkError(hostLockRelease(lockID, holderID, lockVersion))
}
func (lc *LockClient) Renew(lockID, holderID, lockVersion string, leaseSecs uint32) ([]byte, error) {
	result := hostLockRenew(lockID, holderID, lockVersion, leaseSecs)
	if isHostError(result) {
		return nil, &HostError{result}
	}
	return []byte(result), nil
}

// BlobStore methods

func (bs *BlobStore) Upload(name string, data []byte, contentType string) (string, error) {
	result := hostBlobUpload(name, string(data), contentType)
	if isHostError(result) {
		return "", &HostError{result}
	}
	return result, nil
}
func (bs *BlobStore) Download(blobID string) ([]byte, error) {
	result := hostBlobDownload(blobID)
	if isHostError(result) {
		return nil, &HostError{result}
	}
	return []byte(result), nil
}
func (bs *BlobStore) Delete(blobID string) error {
	return checkError(hostBlobDelete(blobID))
}
func (bs *BlobStore) List(prefix string) ([]string, error) {
	result := hostBlobList(prefix)
	if isHostError(result) {
		return nil, &HostError{result}
	}
	return wasmDecodeStringArrayJSON(result)
}

// HTTPClient methods

func (hc *HTTPClient) Fetch(linkName, method, pathAndQuery string, headers map[string]string, body []byte) (map[string]any, error) {
	reqWire, err := encodeHttpFetchRequestWire(headers, body)
	if err != nil {
		return nil, err
	}
	result := hostHTTPFetch(linkName, method, pathAndQuery, reqWire)
	if isHostError(result) {
		return nil, &HostError{result}
	}
	return decodeHttpFetchResponseWire([]byte(result))
}

// ActorMessaging methods

func (am *ActorMessaging) Send(to, msgType string, payload any) string {
	return hostSend(to, msgType, marshalPayload(payload))
}
func (am *ActorMessaging) Ask(to, msgType string, payload any, timeoutMs uint64) (any, error) {
	result := hostAsk(to, msgType, marshalPayload(payload), timeoutMs)
	if isHostError(result) {
		return nil, &HostError{result}
	}
	return result, nil
}
func (am *ActorMessaging) SelfID() string { return hostSelfID() }
func (am *ActorMessaging) Spawn(moduleRef, actorName, role string, args map[string]string) (string, error) {
	var argsJSON string
	if len(args) == 0 {
		argsJSON = "{}"
	} else {
		argsJSON = wasmEncodeStringStringMapJSON(args)
	}
	result := hostSpawn(moduleRef, actorName, role, argsJSON)
	if isHostError(result) {
		return "", &HostError{result}
	}
	return result, nil
}
func (am *ActorMessaging) Stop(actorID string) error   { return checkError(hostStop(actorID)) }
func (am *ActorMessaging) Link(actorID string) error   { return checkError(hostLink(actorID)) }
func (am *ActorMessaging) Unlink(actorID string) error { return checkError(hostUnlink(actorID)) }
func (am *ActorMessaging) Monitor(actorID string) (string, error) {
	result := hostMonitor(actorID)
	if isHostError(result) {
		return "", &HostError{result}
	}
	return result, nil
}
func (am *ActorMessaging) Demonitor(monitorRef string) error {
	return checkError(hostDemonitor(monitorRef))
}
func (am *ActorMessaging) SendAfter(delayMs uint64, msgType string, payload any) (string, error) {
	result := hostSendAfter(delayMs, msgType, marshalPayload(payload))
	if isHostError(result) {
		return "", &HostError{result}
	}
	return result, nil
}

// TupleSpace methods

func (ts *TupleSpace) Write(tuple []any) string {
	data, err := tsWriteWire(tuple)
	if err != nil {
		return "ERROR: " + err.Error()
	}
	return ts.host.TSWrite(string(data))
}

func (ts *TupleSpace) Take(pattern []any) ([]any, bool) {
	data, err := tsReadRequestWire(pattern, true, 1)
	if err != nil {
		return nil, false
	}
	raw := ts.host.TSTake(string(data))
	return tsDecodeReadResponseFirstTuple(raw)
}

func (ts *TupleSpace) Read(pattern []any) ([]any, bool) {
	data, err := tsReadRequestWire(pattern, false, 1)
	if err != nil {
		return nil, false
	}
	raw := ts.host.TSRead(string(data))
	return tsDecodeReadResponseFirstTuple(raw)
}

func (ts *TupleSpace) ReadAll(pattern []any) [][]any {
	data, err := tsReadRequestWire(pattern, false, 1024)
	if err != nil {
		return nil
	}
	raw := ts.host.TSReadAll(string(data))
	return tsDecodeReadResponseAllTuples(raw)
}

// Messaging

func (h *Host) Send(to, msgType string, payload any) string {
	return hostSend(to, msgType, marshalPayload(payload))
}

func (h *Host) Ask(to, msgType string, payload any, timeoutMs uint64) (any, error) {
	result := hostAsk(to, msgType, marshalPayload(payload), timeoutMs)
	if isHostError(result) {
		return nil, &HostError{result}
	}
	return result, nil
}

// Actor Identity

func (h *Host) SelfID() string {
	return hostSelfID()
}

// Actor Lifecycle

func (h *Host) Spawn(moduleRef, actorName, role string, args map[string]string) (string, error) {
	var argsJSON string
	if len(args) == 0 {
		argsJSON = "{}"
	} else {
		argsJSON = wasmEncodeStringStringMapJSON(args)
	}
	result := hostSpawn(moduleRef, actorName, role, argsJSON)
	if isHostError(result) {
		return "", &HostError{result}
	}
	return result, nil
}

func (h *Host) Stop(actorID string) error {
	return checkError(hostStop(actorID))
}

func (h *Host) Link(actorID string) error {
	return checkError(hostLink(actorID))
}

func (h *Host) Unlink(actorID string) error {
	return checkError(hostUnlink(actorID))
}

func (h *Host) Monitor(actorID string) (string, error) {
	result := hostMonitor(actorID)
	if isHostError(result) {
		return "", &HostError{result}
	}
	return result, nil
}

func (h *Host) Demonitor(monitorRef string) error {
	return checkError(hostDemonitor(monitorRef))
}

func (h *Host) Log(level, message string) { hostLog(level, message) }
func (h *Host) Debug(message string)      { h.Log("debug", message) }
func (h *Host) Info(message string)       { h.Log("info", message) }
func (h *Host) Warn(message string)       { h.Log("warn", message) }
func (h *Host) Error(message string)      { h.Log("error", message) }

func (h *Host) NowMs() uint64 { return hostNowMs() }

// TupleSpace host pass-throughs

func (h *Host) TSWrite(tupleJSON string) string    { return hostTSWrite(tupleJSON) }
func (h *Host) TSRead(patternJSON string) string   { return hostTSRead(patternJSON) }
func (h *Host) TSTake(patternJSON string) string   { return hostTSTake(patternJSON) }
func (h *Host) TSReadAll(patternJSON string) string { return hostTSReadAll(patternJSON) }

// EventLog — two-cursor watermark (embed in actor state)

type EventLog struct {
	Watermark int64
}

func (el *EventLog) Append(h *Host, prefix string, entry any) (int64, error) {
	el.Watermark++
	key := fmt.Sprintf("%sseq:%d", prefix, el.Watermark)
	data := marshalPayload(entry)
	if result := hostKVPut(key, data); isHostError(result) {
		el.Watermark--
		return 0, fmt.Errorf("KVPutJSON(%q): %s", key, result)
	}
	return el.Watermark, nil
}

func (el *EventLog) Poll(h *Host, prefix, consumerID string, limit int) ([]any, int64, error) {
	cursorKey := prefix + "cursor:" + consumerID
	var cursor int64
	if raw := hostKVGet(cursorKey); raw != "" {
		fmt.Sscanf(raw, "%d", &cursor)
	}

	var events []any
	newCursor := cursor
	for seq := cursor + 1; seq <= el.Watermark && len(events) < limit; seq++ {
		key := fmt.Sprintf("%sseq:%d", prefix, seq)
		raw := hostKVGet(key)
		if raw == "" {
			continue
		}
		events = append(events, raw)
		newCursor = seq
	}

	if newCursor != cursor {
		hostKVPut(cursorKey, fmt.Sprintf("%d", newCursor))
	}
	return events, newCursor, nil
}

// ProcessGroups

type ProcessGroups struct{}

func (h *Host) PG() *ProcessGroups { return &ProcessGroups{} }

func (pg *ProcessGroups) Join(group string) error {
	return checkError(hostPGJoin(group))
}

func (pg *ProcessGroups) Leave(group string) error {
	return checkError(hostPGLeave(group))
}

func (pg *ProcessGroups) Members(group string) ([]string, error) {
	result := hostPGMembers(group)
	if isHostError(result) {
		return nil, &HostError{result}
	}
	return wasmDecodeStringArrayJSON(result)
}

func (pg *ProcessGroups) Broadcast(group, msgType string, payload any) error {
	return checkError(hostPGBroadcast(group, msgType, marshalPayload(payload)))
}

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

// Object Registry

type ObjectRegistration struct {
	ObjectID       string
	ObjectType     string
	GRPCAddress    string
	ObjectCategory string
	TenantID       string
	Namespace      string
	Capabilities   []string
	Labels         []string
	HealthStatus   string
	CreatedAt      uint64
	UpdatedAt      uint64
	LastHeartbeat  *uint64
	Alias          *string
}

type Registry struct{}

func (r *Registry) Register(reg ObjectRegistration) error {
	reqBytes := encodeRegisterRequest(reg)
	return checkError(hostRegistryRegister(string(reqBytes)))
}

func (r *Registry) Unregister(objectID string, objectType int32, tenantID, namespace string) error {
	reqBytes := encodeUnregisterRequest(objectID, objectType, tenantID, namespace)
	return checkError(hostRegistryUnregister(string(reqBytes)))
}

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

type DiscoverOptions struct {
	ObjectType     int32
	ObjectCategory string
	TenantID       string
	Namespace      string
	Capabilities   []string
	Labels         []string
	PageSize       int32
}

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
	return decodeDiscoverResponse([]byte(result)), nil
}

func (r *Registry) Heartbeat(objectID string, objectType int32, tenantID, namespace string) error {
	reqBytes := encodeHeartbeatRequest(objectID, objectType, tenantID, namespace)
	return checkError(hostRegistryHeartbeat(string(reqBytes)))
}

// Channel

type Channel struct {
	host *Host
}

func (h *Host) Ch() *Channel { return h.ch }

func (ch *Channel) Send(ctx, channelName, msgType string, payload any) (string, error) {
	result := hostChannelSend(marshalPayload(ctx), channelName, msgType, marshalPayload(payload))
	if isHostError(result) {
		return "", &HostError{result}
	}
	return result, nil
}

func (ch *Channel) SendWithOptions(ctx, channelName, msgType string, payload any, delayMs, ttlMs uint64, headers map[string]string) (string, error) {
	result := hostChannelSendWithOptions(marshalPayload(ctx), channelName, msgType, marshalPayload(payload), delayMs, ttlMs, marshalPayload(headers))
	if isHostError(result) {
		return "", &HostError{result}
	}
	return result, nil
}

func (ch *Channel) Receive(ctx, channelName string, timeoutMs uint64) (map[string]any, bool, error) {
	result := hostChannelReceive(marshalPayload(ctx), channelName, timeoutMs)
	if isHostError(result) {
		return nil, false, &HostError{result}
	}
	if result == "" {
		return nil, false, nil
	}
	msg := map[string]any{
		"id":             WasmJSONExtractStr(result, "id"),
		"msg_type":       WasmJSONExtractStr(result, "msg_type"),
		"payload":        WasmJSONFindObject(result, "payload"),
		"timestamp":      WasmJSONExtractInt64(result, "timestamp"),
		"delivery_count": WasmJSONExtractInt(result, "delivery_count"),
	}
	return msg, true, nil
}

func (ch *Channel) Publish(ctx, channelName, msgType string, payload any) (string, error) {
	result := hostChannelPublish(marshalPayload(ctx), channelName, msgType, marshalPayload(payload))
	if isHostError(result) {
		return "", &HostError{result}
	}
	return result, nil
}

func (ch *Channel) Subscribe(ctx, channelName, filter string) (string, error) {
	result := hostChannelSubscribe(marshalPayload(ctx), channelName, filter)
	if isHostError(result) {
		return "", &HostError{result}
	}
	return result, nil
}

func (ch *Channel) Unsubscribe(subscriptionID string) error {
	return checkError(hostChannelUnsubscribe(subscriptionID))
}

func (ch *Channel) Ack(ctx, channelName, messageID string) error {
	return checkError(hostChannelAck(marshalPayload(ctx), channelName, messageID))
}

func (ch *Channel) Nack(ctx, channelName, messageID string, requeue bool) error {
	return checkError(hostChannelNack(marshalPayload(ctx), channelName, messageID, requeue))
}

func (ch *Channel) Create(ctx, channelName string, maxSize uint32, messageTTLMs uint64) error {
	return checkError(hostChannelCreate(marshalPayload(ctx), channelName, maxSize, messageTTLMs))
}

func (ch *Channel) Delete(ctx, channelName string) error {
	return checkError(hostChannelDelete(marshalPayload(ctx), channelName))
}

func (ch *Channel) Depth(ctx, channelName string) (uint64, error) {
	result := hostChannelDepth(marshalPayload(ctx), channelName)
	if isHostError(result) {
		return 0, &HostError{result}
	}
	var depth uint64
	if _, err := fmt.Sscanf(result, "%d", &depth); err != nil {
		return 0, fmt.Errorf("channel depth: malformed response %q: %w", result, err)
	}
	return depth, nil
}

// Elastic pool

func (h *Host) PoolCheckout(poolName string, timeoutMs uint64) map[string]any {
	result := hostPoolCheckout(poolName, timeoutMs)
	if result == "" || isHostError(result) {
		return nil
	}
	return map[string]any{
		"actor_id":    WasmJSONExtractStr(result, "actor_id"),
		"pool_name":   WasmJSONExtractStr(result, "pool_name"),
		"checkout_id": WasmJSONExtractStr(result, "checkout_id"),
	}
}

func (h *Host) PoolCheckin(poolName, actorID, checkoutID string, healthy bool) error {
	return checkError(hostPoolCheckin(poolName, actorID, checkoutID, healthy))
}

func (h *Host) PoolGetMetrics(poolName string) map[string]any {
	result := hostPoolGetMetrics(poolName)
	if result == "" || isHostError(result) {
		return nil
	}
	return map[string]any{
		"total_actors":     WasmJSONExtractInt(result, "total_actors"),
		"available_actors": WasmJSONExtractInt(result, "available_actors"),
		"busy_actors":      WasmJSONExtractInt(result, "busy_actors"),
		"current_load":     WasmJSONExtractInt(result, "current_load"),
	}
}

func strOrEmpty(v any) string {
	if s, ok := v.(string); ok {
		return s
	}
	return ""
}

func floatOrZero(v any) float64 {
	if f, ok := v.(float64); ok {
		return f
	}
	return 0
}

func anySliceToStrings(v any) []string {
	s, ok := v.([]any)
	if !ok {
		return nil
	}
	out := make([]string, 0, len(s))
	for _, item := range s {
		if str, ok := item.(string); ok {
			out = append(out, str)
		}
	}
	return out
}

func anySliceToMaps(v any) []map[string]any {
	if s, ok := v.([]map[string]any); ok {
		return s
	}
	s, ok := v.([]any)
	if !ok {
		return nil
	}
	out := make([]map[string]any, 0, len(s))
	for _, item := range s {
		if m, ok := item.(map[string]any); ok {
			out = append(out, m)
		}
	}
	return out
}

// Shard group operations (proto-wire based, no encoding/json needed)

func (h *Host) CreateShardGroup(req CreateShardGroupRequest) (CreateShardGroupResponse, error) {
	wire, err := hostWireCreateShardGroupRequest(req)
	if err != nil {
		return CreateShardGroupResponse{}, err
	}
	result := hostCreateShardGroup(wire)
	if isHostError(result) {
		return CreateShardGroupResponse{}, &HostError{result}
	}
	resMap, err := hostDecodeCreateShardGroupResponse(result)
	if err != nil {
		return CreateShardGroupResponse{}, err
	}
	return CreateShardGroupResponse{
		GroupID:       strOrEmpty(resMap["group_id"]),
		ActorType:     strOrEmpty(resMap["actor_type"]),
		ShardActorIDs: anySliceToStrings(resMap["shard_actor_ids"]),
		ShardCount:    int(floatOrZero(resMap["shard_count"])),
	}, nil
}

func (h *Host) BulkUpdateShardGroup(req BulkUpdateShardGroupRequest) (map[string]any, error) {
	wire, err := hostWireBulkUpdateShardGroupRequest(req)
	if err != nil {
		return nil, err
	}
	result := hostBulkUpdateShardGroup(wire)
	if isHostError(result) {
		return nil, &HostError{result}
	}
	return hostDecodeBulkUpdateShardGroupResponse(result)
}

func (h *Host) MapShardGroup(req MapShardGroupRequest) (map[string]any, error) {
	wire, err := hostWireMapShardGroupRequest(req)
	if err != nil {
		return nil, err
	}
	result := hostMapShardGroup(wire)
	if isHostError(result) {
		return nil, &HostError{result}
	}
	return hostDecodeMapShardGroupResponse(result)
}

func (h *Host) ScatterGather(req ScatterGatherRequest) (ScatterGatherResponse, error) {
	wire, err := hostWireScatterGatherRequest(req)
	if err != nil {
		return ScatterGatherResponse{}, err
	}
	result := hostScatterGather(wire)
	if isHostError(result) {
		return ScatterGatherResponse{}, &HostError{result}
	}
	resMap, err := hostDecodeScatterGatherResponse(result)
	if err != nil {
		return ScatterGatherResponse{}, err
	}
	shards := anySliceToMaps(resMap["shard_responses"])
	stats, _ := resMap["stats"].(map[string]any)
	if stats == nil {
		stats = map[string]any{}
		if v, ok := resMap["_stat_shards_queried"]; ok {
			stats["shards_queried"] = v
		}
		if v, ok := resMap["_stat_shards_responded"]; ok {
			stats["shards_responded"] = v
		}
		if v, ok := resMap["_stat_shards_failed"]; ok {
			stats["shards_failed"] = v
		}
		if v, ok := resMap["_stat_max_latency_ms"]; ok {
			stats["max_latency_ms"] = v
		}
	}
	return ScatterGatherResponse{ShardResponses: shards, Stats: stats}, nil
}

func (h *Host) BroadcastShardGroup(req BroadcastShardGroupRequest) (map[string]any, error) {
	wire, err := hostWireBroadcastShardGroupRequest(req)
	if err != nil {
		return nil, err
	}
	result := hostBroadcastShardGroup(wire)
	if isHostError(result) {
		return nil, &HostError{result}
	}
	return hostDecodeBroadcastShardGroupResponse(result)
}

func (h *Host) ReduceShardGroup(req ReduceShardGroupRequest) (map[string]any, error) {
	wire, err := hostWireReduceShardGroupRequest(req)
	if err != nil {
		return nil, err
	}
	result := hostReduceShardGroup(wire)
	if isHostError(result) {
		return nil, &HostError{result}
	}
	return hostDecodeReduceShardGroupResponse(result)
}

func (h *Host) AllReduceShardGroup(req AllReduceShardGroupRequest) (map[string]any, error) {
	wire, err := hostWireAllReduceShardGroupRequest(req)
	if err != nil {
		return nil, err
	}
	result := hostAllReduceShardGroup(wire)
	if isHostError(result) {
		return nil, &HostError{result}
	}
	return hostDecodeAllReduceShardGroupResponse(result)
}

func (h *Host) BarrierShardGroup(req BarrierShardGroupRequest) (map[string]any, error) {
	wire, err := hostWireBarrierShardGroupRequest(req)
	if err != nil {
		return nil, err
	}
	result := hostBarrierShardGroup(wire)
	if isHostError(result) {
		return nil, &HostError{result}
	}
	return hostDecodeBarrierShardGroupResponse(result)
}

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

// ApplicationMetrics and ApplicationStatus

type ApplicationMetrics struct {
	ActorCounts     map[string]uint64
	SupervisorCount uint64
	UptimeSeconds   uint64
	MessageCount    uint64
	ErrorCount      uint64
	CounterMetrics  map[string]uint64
	LatencyTotalsMs map[string]uint64
	LatencyMaxMs    map[string]uint64
	LatencySamples  map[string]uint64
}

type ApplicationStatus struct {
	ApplicationID      string
	ApplicationName    string
	ApplicationVersion string
	StatusCode         string
	NodeID             string
	NodeAddress        string
}

func (h *Host) ApplicationMetricsAdd(applicationID string, metrics any) (ApplicationMetrics, error) {
	wire, err := hostWireApplicationMetrics(metrics)
	if err != nil {
		return ApplicationMetrics{}, err
	}
	result := hostApplicationMetricsAdd(applicationID, wire)
	if isHostError(result) {
		return ApplicationMetrics{}, &HostError{result}
	}
	return hostDecodeApplicationMetricsResponse(result)
}

func (h *Host) ApplicationGetMetrics(applicationID, nodeID string) (ApplicationMetrics, error) {
	result := hostApplicationGetMetrics(applicationID, nodeID)
	if isHostError(result) {
		return ApplicationMetrics{}, &HostError{result}
	}
	return hostDecodeApplicationMetricsResponse(result)
}

func (h *Host) HTTPFetch(linkName, method, pathAndQuery string, headers map[string]string, body []byte) (map[string]any, error) {
	reqWire, err := encodeHttpFetchRequestWire(headers, body)
	if err != nil {
		return nil, err
	}
	result := hostHTTPFetch(linkName, method, pathAndQuery, reqWire)
	if isHostError(result) {
		return nil, &HostError{result}
	}
	return decodeHttpFetchResponseWire([]byte(result))
}

func (h *Host) ApplicationGetStatus(applicationID, nodeID string) (ApplicationStatus, error) {
	result := hostApplicationGetStatus(applicationID, nodeID)
	if isHostError(result) {
		return ApplicationStatus{}, &HostError{result}
	}
	return hostDecodeApplicationGetStatusResponse(result)
}

// ServiceHTTPClient

type ServiceHTTPClient struct {
	host     *Host
	linkName string
}

func NewServiceHTTPClient(h *Host, linkName string) *ServiceHTTPClient {
	return &ServiceHTTPClient{host: h, linkName: linkName}
}

func (c *ServiceHTTPClient) Get(pathAndQuery string, headers map[string]string) (map[string]any, error) {
	return c.host.HTTPFetch(c.linkName, "GET", pathAndQuery, headers, nil)
}

func (c *ServiceHTTPClient) Post(pathAndQuery string, body []byte, headers map[string]string) (map[string]any, error) {
	return c.host.HTTPFetch(c.linkName, "POST", pathAndQuery, headers, body)
}

func (c *ServiceHTTPClient) Put(pathAndQuery string, body []byte, headers map[string]string) (map[string]any, error) {
	return c.host.HTTPFetch(c.linkName, "PUT", pathAndQuery, headers, body)
}

func (c *ServiceHTTPClient) Delete(pathAndQuery string, headers map[string]string) (map[string]any, error) {
	return c.host.HTTPFetch(c.linkName, "DELETE", pathAndQuery, headers, nil)
}

// Helpers

type HostError struct {
	Message string
}

func (e *HostError) Error() string { return e.Message }

type ErrorDetail struct {
	Code    string
	Message string
}

func (e *HostError) ParseErrorDetail() *ErrorDetail {
	s := strings.TrimPrefix(e.Message, errorPrefix)
	s = strings.TrimSpace(s)
	code := WasmJSONExtractStr(s, "code")
	msg := WasmJSONExtractStr(s, "message")
	if code == "" && msg == "" {
		return nil
	}
	return &ErrorDetail{Code: code, Message: msg}
}

func IsHostError(result string) bool {
	return strings.HasPrefix(result, errorPrefix)
}

func isHostError(result string) bool {
	return IsHostError(result)
}

func checkError(result string) error {
	if isHostError(result) {
		return &HostError{result}
	}
	return nil
}

// marshalPayload serializes a payload for WIT communication without encoding/json.
func marshalPayload(payload any) string {
	if payload == nil {
		return "{}"
	}
	if s, ok := payload.(string); ok {
		return s
	}
	if m, ok := payload.(map[string]any); ok {
		return SafeMarshal(m)
	}
	if m, ok := payload.(map[string]string); ok {
		return wasmEncodeStringStringMapJSON(m)
	}
	if ss, ok := payload.([]string); ok {
		return wasmEncodeStringSliceJSON(ss)
	}
	if b, ok := payload.([]byte); ok {
		return string(b)
	}
	return fmt.Sprintf("%v", payload)
}

// wasmEncodeStringStringMapJSON encodes map[string]string as a JSON object
// without importing encoding/json.
func wasmEncodeStringStringMapJSON(m map[string]string) string {
	var sb strings.Builder
	sb.WriteByte('{')
	first := true
	for k, v := range m {
		if !first {
			sb.WriteByte(',')
		}
		writeJSONString(&sb, k)
		sb.WriteByte(':')
		writeJSONString(&sb, v)
		first = false
	}
	sb.WriteByte('}')
	return sb.String()
}

// wasmEncodeStringSliceJSON encodes []string as a JSON array
// without importing encoding/json.
func wasmEncodeStringSliceJSON(ss []string) string {
	var sb strings.Builder
	sb.WriteByte('[')
	for i, s := range ss {
		if i > 0 {
			sb.WriteByte(',')
		}
		writeJSONString(&sb, s)
	}
	sb.WriteByte(']')
	return sb.String()
}

// wasmDecodeStringArrayJSON decodes a JSON array of strings (e.g. ["a","b","c"])
// without importing encoding/json.
func wasmDecodeStringArrayJSON(s string) ([]string, error) {
	inner := WasmJSONBracketedContent(s, '[', ']')
	if inner == "" {
		return nil, nil
	}
	items := WasmJSONSplitArray(inner)
	result := make([]string, 0, len(items))
	for _, item := range items {
		item = strings.TrimSpace(item)
		if len(item) == 0 {
			continue
		}
		if item[0] == '"' {
			str, _ := WasmJSONConsumeParsedString(item)
			result = append(result, str)
		} else if item != "null" {
			result = append(result, item)
		}
	}
	return result, nil
}
