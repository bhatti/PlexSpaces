// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Stub implementations of WIT host functions for native Go builds.
// Used for unit testing and development outside of WASM.
//
// In WASM builds (tinygo.wasm), host_imports.go provides the real implementations
// via //go:wasmimport directives. This file is excluded from WASM builds.

//go:build !wasm

package plexspaces

import (
	"encoding/json"
	"fmt"
	"sync"
	"time"
)

// stubState holds test stub state for verification in tests.
var stubState = struct {
	mu                sync.Mutex
	sent              []stubMessage
	stopped           []string
	groupSent         []stubGroupMessage
	logs              []stubLog
	kvStore           map[string]string
	tupleStore        [][]any
	blobStore         map[string]string
	pgMembers         map[string][]string
	channelQueues     map[string][]stubChannelMessage
	channelTopics     map[string][]stubChannelMessage
	channelAcked      map[string]bool              // msg ID → acked
	channelNacked     map[string]bool              // msg ID → nacked
	channelSubs       map[string]string            // subscription ID → channel name
	channelSubCounter int
	registryStore     map[string]map[string]any    // object_id → registration
	selfID            string
	nowMs             uint64
	useRealTime       bool
}{
	kvStore:       make(map[string]string),
	blobStore:     make(map[string]string),
	pgMembers:     make(map[string][]string),
	registryStore: make(map[string]map[string]any),
	channelAcked:  make(map[string]bool),
	channelNacked: make(map[string]bool),
	channelSubs:   make(map[string]string),
	selfID:        "test-actor:test@test-node",
}

type stubMessage struct {
	To, MsgType, Payload string
}

type stubGroupMessage struct {
	Group, MsgType, Payload string
}

type stubLog struct {
	Level, Message string
}

type stubChannelMessage struct {
	id, msgType, payload string
}

// ResetStubs clears all stub state (call from test setup).
func ResetStubs() {
	stubState.mu.Lock()
	defer stubState.mu.Unlock()
	stubState.sent = nil
	stubState.stopped = nil
	stubState.groupSent = nil
	stubState.logs = nil
	stubState.kvStore = make(map[string]string)
	stubState.tupleStore = nil
	stubState.blobStore = make(map[string]string)
	stubState.pgMembers = make(map[string][]string)
	stubState.channelQueues = make(map[string][]stubChannelMessage)
	stubState.channelTopics = make(map[string][]stubChannelMessage)
	stubState.channelAcked = make(map[string]bool)
	stubState.channelNacked = make(map[string]bool)
	stubState.channelSubs = make(map[string]string)
	stubState.channelSubCounter = 0
	stubState.registryStore = make(map[string]map[string]any)
	stubState.selfID = "test-actor:test@test-node"
	stubState.nowMs = 0
	stubState.useRealTime = false
}

// GetStubStoppedActors returns actor IDs passed to hostStop.
func GetStubStoppedActors() []string {
	stubState.mu.Lock()
	defer stubState.mu.Unlock()
	result := make([]string, len(stubState.stopped))
	copy(result, stubState.stopped)
	return result
}

// SetStubSelfID sets the actor ID returned by hostSelfID.
func SetStubSelfID(id string) {
	stubState.mu.Lock()
	defer stubState.mu.Unlock()
	stubState.selfID = id
}

// SetStubNowMs sets a fixed timestamp for hostNowMs (0 = use real time).
func SetStubNowMs(ms uint64) {
	stubState.mu.Lock()
	defer stubState.mu.Unlock()
	stubState.nowMs = ms
	stubState.useRealTime = ms == 0
}

// GetStubSentMessages returns messages sent via hostSend.
func GetStubSentMessages() []stubMessage {
	stubState.mu.Lock()
	defer stubState.mu.Unlock()
	result := make([]stubMessage, len(stubState.sent))
	copy(result, stubState.sent)
	return result
}

// GetStubGroupMessages returns process-group broadcasts sent via hostPGBroadcast.
func GetStubGroupMessages() []stubGroupMessage {
	stubState.mu.Lock()
	defer stubState.mu.Unlock()
	result := make([]stubGroupMessage, len(stubState.groupSent))
	copy(result, stubState.groupSent)
	return result
}

// GetStubLogs returns log messages recorded via hostLog.
func GetStubLogs() []stubLog {
	stubState.mu.Lock()
	defer stubState.mu.Unlock()
	result := make([]stubLog, len(stubState.logs))
	copy(result, stubState.logs)
	return result
}

// ========================================================================
// Stub Implementations
// ========================================================================

func hostSend(to, msgType, payloadJSON string) string {
	stubState.mu.Lock()
	defer stubState.mu.Unlock()
	stubState.sent = append(stubState.sent, stubMessage{to, msgType, payloadJSON})
	return ""
}

func hostAsk(to, msgType, payloadJSON string, timeoutMs uint64) string {
	return `{"status":"ok","stub":true}`
}

func hostSelfID() string {
	stubState.mu.Lock()
	defer stubState.mu.Unlock()
	return stubState.selfID
}

func hostSpawn(moduleRef, actorName, role, argsJSON string) string {
	if actorName == "" {
		return fmt.Sprintf("auto-%s-001", moduleRef)
	}
	return actorName
}

func hostStop(actorID string) string {
	stubState.mu.Lock()
	defer stubState.mu.Unlock()
	stubState.stopped = append(stubState.stopped, actorID)
	return ""
}

func hostLink(actorID string) string   { return "" }
func hostUnlink(actorID string) string { return "" }

func hostMonitor(actorID string) string    { return "monitor-ref-001" }
func hostDemonitor(monitorRef string) string { return "" }

func hostSendAfter(delayMs uint64, msgType, payloadJSON string) string {
	return "timer-001"
}

func hostLog(level, message string) {
	stubState.mu.Lock()
	defer stubState.mu.Unlock()
	stubState.logs = append(stubState.logs, stubLog{level, message})
}

func hostNowMs() uint64 {
	stubState.mu.Lock()
	defer stubState.mu.Unlock()
	if stubState.nowMs > 0 {
		return stubState.nowMs
	}
	return uint64(time.Now().UnixMilli())
}

func hostKVGet(key string) string {
	stubState.mu.Lock()
	defer stubState.mu.Unlock()
	return stubState.kvStore[key]
}

func hostKVPut(key, value string) string {
	stubState.mu.Lock()
	defer stubState.mu.Unlock()
	stubState.kvStore[key] = value
	return ""
}

func hostKVDelete(key string) string {
	stubState.mu.Lock()
	defer stubState.mu.Unlock()
	delete(stubState.kvStore, key)
	return ""
}

func hostKVList(prefix string) string {
	stubState.mu.Lock()
	defer stubState.mu.Unlock()
	var keys []string
	for k := range stubState.kvStore {
		if len(prefix) == 0 || len(k) >= len(prefix) && k[:len(prefix)] == prefix {
			keys = append(keys, k)
		}
	}
	data, _ := json.Marshal(keys)
	return string(data)
}

func tupleMatches(tupleValue []any, pattern []any) bool {
	if len(tupleValue) != len(pattern) {
		return false
	}
	for i := range tupleValue {
		if pattern[i] == nil || pattern[i] == "*" {
			continue
		}
		if tupleValue[i] != pattern[i] {
			return false
		}
	}
	return true
}

func hostTSWrite(tupleJSON string) string {
	stubState.mu.Lock()
	defer stubState.mu.Unlock()
	var tupleValue []any
	if err := json.Unmarshal([]byte(tupleJSON), &tupleValue); err != nil {
		return "ERROR: " + err.Error()
	}
	stubState.tupleStore = append(stubState.tupleStore, tupleValue)
	return ""
}

func hostTSRead(patternJSON string) string {
	stubState.mu.Lock()
	defer stubState.mu.Unlock()
	var pattern []any
	if err := json.Unmarshal([]byte(patternJSON), &pattern); err != nil {
		return "ERROR: " + err.Error()
	}
	for _, tupleValue := range stubState.tupleStore {
		if tupleMatches(tupleValue, pattern) {
			data, _ := json.Marshal(tupleValue)
			return string(data)
		}
	}
	return ""
}

func hostTSTake(patternJSON string) string {
	stubState.mu.Lock()
	defer stubState.mu.Unlock()
	var pattern []any
	if err := json.Unmarshal([]byte(patternJSON), &pattern); err != nil {
		return "ERROR: " + err.Error()
	}
	for index, tupleValue := range stubState.tupleStore {
		if tupleMatches(tupleValue, pattern) {
			stubState.tupleStore = append(stubState.tupleStore[:index], stubState.tupleStore[index+1:]...)
			data, _ := json.Marshal(tupleValue)
			return string(data)
		}
	}
	return ""
}

func hostTSReadAll(patternJSON string) string {
	stubState.mu.Lock()
	defer stubState.mu.Unlock()
	var pattern []any
	if err := json.Unmarshal([]byte(patternJSON), &pattern); err != nil {
		return "ERROR: " + err.Error()
	}
	matches := make([][]any, 0)
	for _, tupleValue := range stubState.tupleStore {
		if tupleMatches(tupleValue, pattern) {
			matches = append(matches, tupleValue)
		}
	}
	data, _ := json.Marshal(matches)
	return string(data)
}

func hostLockAcquire(tenantID, namespace, holderID, lockName string, leaseDurationSecs uint32, timeoutMs uint64) string {
	return `{"lock_key":"test-lock","version":"v1","holder_id":"` + holderID + `","locked":true}`
}

func hostLockRelease(lockID, tenantID, namespace, holderID, lockVersion string) string { return "" }
func hostLockRenew(lockID, tenantID, namespace, holderID, lockVersion string, leaseDurationSecs uint32) string {
	return "v2"
}

func hostBlobUpload(blobID, data, contentType string) string {
	stubState.mu.Lock()
	defer stubState.mu.Unlock()
	stubState.blobStore[blobID] = data
	return ""
}
func hostBlobDownload(blobID string) string {
	stubState.mu.Lock()
	defer stubState.mu.Unlock()
	return stubState.blobStore[blobID]
}
func hostBlobDelete(blobID string) string {
	stubState.mu.Lock()
	defer stubState.mu.Unlock()
	delete(stubState.blobStore, blobID)
	return ""
}
func hostBlobList(prefix string) string {
	stubState.mu.Lock()
	defer stubState.mu.Unlock()
	keys := make([]string, 0)
	for key := range stubState.blobStore {
		if len(prefix) == 0 || len(key) >= len(prefix) && key[:len(prefix)] == prefix {
			keys = append(keys, key)
		}
	}
	data, _ := json.Marshal(keys)
	return string(data)
}

func hostPGJoin(groupName string) string {
	stubState.mu.Lock()
	defer stubState.mu.Unlock()
	members := stubState.pgMembers[groupName]
	for _, member := range members {
		if member == stubState.selfID {
			return ""
		}
	}
	stubState.pgMembers[groupName] = append(members, stubState.selfID)
	return ""
}
func hostPGLeave(groupName string) string {
	stubState.mu.Lock()
	defer stubState.mu.Unlock()
	members := stubState.pgMembers[groupName]
	filtered := make([]string, 0, len(members))
	for _, member := range members {
		if member != stubState.selfID {
			filtered = append(filtered, member)
		}
	}
	stubState.pgMembers[groupName] = filtered
	return ""
}
func hostPGMembers(groupName string) string {
	stubState.mu.Lock()
	defer stubState.mu.Unlock()
	data, _ := json.Marshal(stubState.pgMembers[groupName])
	return string(data)
}
func hostPGBroadcast(groupName, msgType, payloadJSON string) string {
	stubState.mu.Lock()
	defer stubState.mu.Unlock()
	stubState.groupSent = append(stubState.groupSent, stubGroupMessage{groupName, msgType, payloadJSON})
	return ""
}

func hostPoolCheckout(poolName string, timeoutMs uint64) string {
	out, _ := json.Marshal(map[string]any{
		"actor_id":    "mock-worker-0",
		"pool_name":   poolName,
		"checkout_id": "mock-checkout-1",
	})
	return string(out)
}
func hostPoolCheckin(poolName, actorID, checkoutID string, healthy bool) string { return "" }
func hostPoolGetMetrics(poolName string) string {
	out, _ := json.Marshal(map[string]any{
		"total_actors": 2, "available_actors": 1, "busy_actors": 0, "current_load": 0.0,
	})
	return string(out)
}

func hostCreateShardGroup(requestJSON string) string {
	return `{"group_id":"mock-group","actor_type":"worker","shard_actor_ids":["worker-0@test-node"]}`
}

func hostBulkUpdateShardGroup(requestJSON string) string {
	return `{"updates_sent":1,"updates_succeeded":1,"updates_failed":0,"errors":[]}`
}

func hostMapShardGroup(requestJSON string) string {
	return `{"results":[],"stats":{"succeeded":0,"failed":0,"total":0}}`
}

func hostScatterGather(requestJSON string) string {
	return `{"result":null,"shard_responses":[],"stats":{"shards_queried":0,"shards_responded":0,"shards_failed":0}}`
}

func hostBroadcastShardGroup(requestJSON string) string {
	return `{"shard_responses":[],"stats":{"shards_queried":0,"shards_responded":0,"shards_failed":0}}`
}

func hostReduceShardGroup(requestJSON string) string {
	return `{"result":null,"shard_responses":[],"stats":{"shards_queried":0,"shards_responded":0,"shards_failed":0}}`
}

func hostAllReduceShardGroup(requestJSON string) string {
	return `{"result":null,"shard_responses":[],"stats":{"shards_queried":0,"shards_responded":0,"shards_failed":0}}`
}

func hostBarrierShardGroup(requestJSON string) string {
	return `{"shard_responses":[],"stats":{"shards_queried":0,"shards_responded":0,"shards_failed":0}}`
}

func hostSpawnActors(requestJSON string) string {
	var request struct {
		Requests []map[string]any `json:"requests"`
	}
	_ = json.Unmarshal([]byte(requestJSON), &request)
	results := make([]map[string]any, 0, len(request.Requests))
	for _, item := range request.Requests {
		actorID, _ := item["actor_id"].(string)
		actorType, _ := item["actor_type"].(string)
		if actorID == "" {
			actorID = actorType
		}
		results = append(results, map[string]any{
			"success": true,
			"error":   "",
			"response": map[string]any{
				"actor_ref": actorID + "@test-node",
				"actor_id":  actorID,
			},
		})
	}
	out, _ := json.Marshal(map[string]any{"results": results})
	return string(out)
}

func hostApplicationMetricsAdd(applicationID, metricsJSON string) string {
	return metricsJSON
}

func hostApplicationGetMetrics(applicationID, nodeID string) string {
	_ = applicationID
	_ = nodeID
	return `{"message_count":0,"error_count":0,"counter_metrics":{},"latency_totals_ms":{},"latency_max_ms":{},"latency_samples":{}}`
}

func hostApplicationGetStatus(applicationID, nodeID string) string {
	return `{"node_id":"` + nodeID + `","application":{"application_id":"` + applicationID + `"}}`
}

func hostHTTPFetch(linkName, method, pathAndQuery string, requestWire []byte) string {
	_ = linkName
	_ = method
	_ = pathAndQuery
	_ = requestWire
	out, _ := json.Marshal(map[string]any{
		"status":  200,
		"headers": map[string]string{},
		"body":    "",
	})
	return string(out)
}

// ========================================================================
// Channel (queue + pub/sub)
// ========================================================================

func hostChannelSend(ctx, channelName, msgType, payloadJSON string) string {
	stubState.mu.Lock()
	defer stubState.mu.Unlock()
	if stubState.channelQueues == nil {
		stubState.channelQueues = make(map[string][]stubChannelMessage)
	}
	id := fmt.Sprintf("msg-%d", len(stubState.channelQueues[channelName])+1)
	stubState.channelQueues[channelName] = append(stubState.channelQueues[channelName], stubChannelMessage{id, msgType, payloadJSON})
	return id
}

func hostChannelSendWithOptions(ctx, channelName, msgType, payloadJSON string, delayMs, ttlMs uint64, headersJSON string) string {
	return hostChannelSend(ctx, channelName, msgType, payloadJSON)
}

func hostChannelReceive(ctx, channelName string, timeoutMs uint64) string {
	stubState.mu.Lock()
	defer stubState.mu.Unlock()
	if stubState.channelQueues == nil {
		return ""
	}
	msgs := stubState.channelQueues[channelName]
	if len(msgs) == 0 {
		return ""
	}
	msg := msgs[0]
	stubState.channelQueues[channelName] = msgs[1:]
	out, _ := json.Marshal(map[string]any{
		"id":             msg.id,
		"msg_type":       msg.msgType,
		"payload":        msg.payload,
		"timestamp":      0,
		"delivery_count": 1,
		"headers":        []any{},
	})
	return string(out)
}

func hostChannelPublish(ctx, channelName, msgType, payloadJSON string) string {
	stubState.mu.Lock()
	defer stubState.mu.Unlock()
	if stubState.channelTopics == nil {
		stubState.channelTopics = make(map[string][]stubChannelMessage)
	}
	id := fmt.Sprintf("pub-%d", len(stubState.channelTopics[channelName])+1)
	stubState.channelTopics[channelName] = append(stubState.channelTopics[channelName], stubChannelMessage{id, msgType, payloadJSON})
	return id
}

func hostChannelSubscribe(ctx, channelName string, filter string) string {
	stubState.mu.Lock()
	defer stubState.mu.Unlock()
	stubState.channelSubCounter++
	subID := fmt.Sprintf("sub-%d", stubState.channelSubCounter)
	if stubState.channelSubs == nil {
		stubState.channelSubs = make(map[string]string)
	}
	stubState.channelSubs[subID] = channelName
	return subID
}

func hostChannelUnsubscribe(subscriptionID string) string {
	stubState.mu.Lock()
	defer stubState.mu.Unlock()
	if stubState.channelSubs != nil {
		delete(stubState.channelSubs, subscriptionID)
	}
	return ""
}

func hostChannelAck(ctx, channelName, messageID string) string {
	stubState.mu.Lock()
	defer stubState.mu.Unlock()
	if stubState.channelAcked == nil {
		stubState.channelAcked = make(map[string]bool)
	}
	stubState.channelAcked[messageID] = true
	return ""
}

func hostChannelNack(ctx, channelName, messageID string, requeue bool) string {
	stubState.mu.Lock()
	defer stubState.mu.Unlock()
	if stubState.channelNacked == nil {
		stubState.channelNacked = make(map[string]bool)
	}
	stubState.channelNacked[messageID] = true
	return ""
}

// GetStubChannelAcked returns the set of message IDs that have been acked.
func GetStubChannelAcked() map[string]bool {
	stubState.mu.Lock()
	defer stubState.mu.Unlock()
	result := make(map[string]bool, len(stubState.channelAcked))
	for k, v := range stubState.channelAcked {
		result[k] = v
	}
	return result
}

// GetStubChannelNacked returns the set of message IDs that have been nacked.
func GetStubChannelNacked() map[string]bool {
	stubState.mu.Lock()
	defer stubState.mu.Unlock()
	result := make(map[string]bool, len(stubState.channelNacked))
	for k, v := range stubState.channelNacked {
		result[k] = v
	}
	return result
}

// GetStubChannelSubscriptions returns active subscription ID → channel name mappings.
func GetStubChannelSubscriptions() map[string]string {
	stubState.mu.Lock()
	defer stubState.mu.Unlock()
	result := make(map[string]string, len(stubState.channelSubs))
	for k, v := range stubState.channelSubs {
		result[k] = v
	}
	return result
}

func hostChannelCreate(ctx, channelName string, maxSize uint32, messageTTLMs uint64) string {
	stubState.mu.Lock()
	defer stubState.mu.Unlock()
	if stubState.channelQueues == nil {
		stubState.channelQueues = make(map[string][]stubChannelMessage)
	}
	if _, ok := stubState.channelQueues[channelName]; !ok {
		stubState.channelQueues[channelName] = nil
	}
	return ""
}

func hostChannelDelete(ctx, channelName string) string {
	stubState.mu.Lock()
	defer stubState.mu.Unlock()
	if stubState.channelQueues != nil {
		delete(stubState.channelQueues, channelName)
	}
	return ""
}

func hostChannelDepth(ctx, channelName string) string {
	stubState.mu.Lock()
	defer stubState.mu.Unlock()
	if stubState.channelQueues == nil {
		return "0"
	}
	return fmt.Sprintf("%d", len(stubState.channelQueues[channelName]))
}

// ========================================================================
// Object Registry stubs
// ========================================================================

func hostRegistryRegister(request string) string {
	// request is proto-encoded RegisterRequest: field 1 = ObjectRegistration (length-delimited)
	b := []byte(request)
	var reg ObjectRegistration
	pos := 0
	for pos < len(b) {
		tag, n, err := readVarint(b, pos)
		if err != nil {
			break
		}
		pos += n
		fn := tag >> 3
		wt := tag & 7
		if fn == 1 && wt == 2 {
			ln, m, err := readVarint(b, pos)
			if err != nil {
				break
			}
			pos += m
			end := pos + int(ln)
			if end > len(b) {
				break
			}
			reg = decodeObjectRegistration(b[pos:end])
			pos = end
		} else {
			var err error
			pos, err = skipField(b, pos, wt)
			if err != nil {
				break
			}
		}
	}
	stubState.mu.Lock()
	defer stubState.mu.Unlock()
	if stubState.registryStore == nil {
		stubState.registryStore = make(map[string]map[string]any)
	}
	stubState.registryStore[reg.ObjectID] = map[string]any{
		"object_id":       reg.ObjectID,
		"object_type":     reg.ObjectType,
		"grpc_address":    reg.GRPCAddress,
		"object_category": reg.ObjectCategory,
		"tenant_id":       reg.TenantID,
		"namespace":       reg.Namespace,
		"capabilities":    reg.Capabilities,
		"labels":          reg.Labels,
		"health_status":   "healthy",
		"alias":           aliasFromPtr(reg.Alias),
	}
	return ""
}

func aliasFromPtr(a *string) string {
	if a == nil {
		return ""
	}
	return *a
}


func hostRegistryUnregister(request string) string {
	b := []byte(request)
	objectID := ""
	pos := 0
	for pos < len(b) {
		tag, n, err := readVarint(b, pos)
		if err != nil {
			break
		}
		pos += n
		fn := tag >> 3
		wt := tag & 7
		if fn == 1 && wt == 2 {
			ln, m, err := readVarint(b, pos)
			if err != nil {
				break
			}
			pos += m
			objectID = string(b[pos : pos+int(ln)])
			pos += int(ln)
		} else {
			var err error
			pos, err = skipField(b, pos, wt)
			if err != nil {
				break
			}
		}
	}
	stubState.mu.Lock()
	defer stubState.mu.Unlock()
	if stubState.registryStore != nil && objectID != "" {
		delete(stubState.registryStore, objectID)
	}
	return ""
}

func hostRegistryLookup(request string) string {
	b := []byte(request)
	objectID := ""
	pos := 0
	for pos < len(b) {
		tag, n, err := readVarint(b, pos)
		if err != nil {
			break
		}
		pos += n
		fn := tag >> 3
		wt := tag & 7
		if fn == 1 && wt == 2 {
			ln, m, err := readVarint(b, pos)
			if err != nil {
				break
			}
			pos += m
			objectID = string(b[pos : pos+int(ln)])
			pos += int(ln)
		} else {
			var err error
			pos, err = skipField(b, pos, wt)
			if err != nil {
				break
			}
		}
	}
	stubState.mu.Lock()
	defer stubState.mu.Unlock()
	if stubState.registryStore == nil || objectID == "" {
		return string(encodeLookupResponseNotFound())
	}
	raw, ok := stubState.registryStore[objectID]
	if !ok {
		return string(encodeLookupResponseNotFound())
	}
	reg := mapToObjectRegistration(raw)
	return string(encodeLookupResponseFound(reg))
}

func encodeLookupResponseNotFound() []byte {
	// LookupResponse: found=2 (bool false) — just empty bytes is fine since found=false is default
	return []byte{}
}

func encodeLookupResponseFound(reg ObjectRegistration) []byte {
	// LookupResponse: registration=1 (message), found=2 (bool true)
	inner := encodeObjectRegistration(reg)
	var b []byte
	b = appendLengthDelimited(b, 1, inner)
	b = appendVarintField(b, 2, 1) // found = true
	return b
}

func mapToObjectRegistration(m map[string]any) ObjectRegistration {
	reg := ObjectRegistration{}
	if v, ok := m["object_id"].(string); ok {
		reg.ObjectID = v
	}
	if v, ok := m["object_type"].(string); ok {
		reg.ObjectType = v
	}
	if v, ok := m["grpc_address"].(string); ok {
		reg.GRPCAddress = v
	}
	if v, ok := m["object_category"].(string); ok {
		reg.ObjectCategory = v
	}
	if v, ok := m["tenant_id"].(string); ok {
		reg.TenantID = v
	}
	if v, ok := m["namespace"].(string); ok {
		reg.Namespace = v
	}
	if v, ok := m["capabilities"].([]string); ok {
		reg.Capabilities = v
	}
	if v, ok := m["labels"].([]string); ok {
		reg.Labels = v
	}
	if v, ok := m["alias"].(string); ok && v != "" {
		reg.Alias = &v
	}
	return reg
}

func hostRegistryLookupByAlias(alias string) string {
	stubState.mu.Lock()
	defer stubState.mu.Unlock()
	if stubState.registryStore == nil {
		return string(encodeLookupResponseNotFound())
	}
	for _, raw := range stubState.registryStore {
		if a, ok := raw["alias"].(string); ok && a == alias {
			reg := mapToObjectRegistration(raw)
			return string(encodeLookupResponseFound(reg))
		}
	}
	return string(encodeLookupResponseNotFound())
}

func hostRegistryDiscover(request string) string {
	b := []byte(request)
	var filterObjectType int32
	var filterObjectCategory string
	pos := 0
	for pos < len(b) {
		tag, n, err := readVarint(b, pos)
		if err != nil {
			break
		}
		pos += n
		fn := tag >> 3
		wt := tag & 7
		switch {
		case fn == 1 && wt == 0: // object_type varint
			v, m, err := readVarint(b, pos)
			if err != nil {
				break
			}
			pos += m
			filterObjectType = int32(v)
		case fn == 2 && wt == 2: // object_category string
			ln, m, err := readVarint(b, pos)
			if err != nil {
				break
			}
			pos += m
			filterObjectCategory = string(b[pos : pos+int(ln)])
			pos += int(ln)
		default:
			var err error
			pos, err = skipField(b, pos, wt)
			if err != nil {
				break
			}
		}
	}
	stubState.mu.Lock()
	defer stubState.mu.Unlock()
	if stubState.registryStore == nil {
		return string(encodeDiscoverResponseEmpty())
	}
	var regs []ObjectRegistration
	for _, raw := range stubState.registryStore {
		if filterObjectType != 0 {
			if ot, ok := raw["object_type"].(string); !ok || ot != objectTypeToString(filterObjectType) {
				continue
			}
		}
		if filterObjectCategory != "" {
			if oc, ok := raw["object_category"].(string); !ok || oc != filterObjectCategory {
				continue
			}
		}
		regs = append(regs, mapToObjectRegistration(raw))
	}
	return string(encodeDiscoverResponseRegs(regs))
}

func encodeDiscoverResponseEmpty() []byte {
	return []byte{}
}

func encodeDiscoverResponseRegs(regs []ObjectRegistration) []byte {
	var b []byte
	for _, reg := range regs {
		inner := encodeObjectRegistration(reg)
		b = appendLengthDelimited(b, 1, inner)
	}
	return b
}

func hostRegistryHeartbeat(request string) string {
	return ""
}
