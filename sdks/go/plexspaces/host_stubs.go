// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Stub implementations of WIT host functions for native Go builds.
// Used for unit testing and development outside of WASM.
//
// In WASM builds (tinygo.wasm), host_imports.go provides the real implementations
// via //go:wasmimport directives. This file is excluded from WASM builds.

//go:build !tinygo.wasm

package plexspaces

import (
	"encoding/json"
	"fmt"
	"sync"
	"time"
)

// stubState holds test stub state for verification in tests.
var stubState = struct {
	mu       sync.Mutex
	sent     []stubMessage
	logs     []stubLog
	kvStore  map[string]string
	selfID   string
	nowMs    uint64
	useRealTime bool
}{
	kvStore: make(map[string]string),
	selfID:  "test-actor:test@test-node",
}

type stubMessage struct {
	To, MsgType, Payload string
}

type stubLog struct {
	Level, Message string
}

// ResetStubs clears all stub state (call from test setup).
func ResetStubs() {
	stubState.mu.Lock()
	defer stubState.mu.Unlock()
	stubState.sent = nil
	stubState.logs = nil
	stubState.kvStore = make(map[string]string)
	stubState.selfID = "test-actor:test@test-node"
	stubState.nowMs = 0
	stubState.useRealTime = false
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

func hostSpawn(moduleRef, actorID, initConfigJSON string) string {
	if actorID == "" {
		return fmt.Sprintf("auto-%s-001", moduleRef)
	}
	return actorID
}

func hostStop(actorID string) string { return "" }

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

func hostTSWrite(tupleJSON string) string     { return "" }
func hostTSRead(patternJSON string) string     { return "" }
func hostTSTake(patternJSON string) string     { return "" }
func hostTSReadAll(patternJSON string) string  { return "[]" }

func hostLockAcquire(tenantID, namespace, holderID, lockName string, leaseDurationSecs uint32, timeoutMs uint64) string {
	return `{"lock_key":"test-lock","version":"v1","holder_id":"` + holderID + `","locked":true}`
}

func hostLockRelease(lockID, tenantID, namespace, holderID, lockVersion string) string { return "" }
func hostLockRenew(lockID, tenantID, namespace, holderID, lockVersion string, leaseDurationSecs uint32) string {
	return "v2"
}

func hostBlobUpload(blobID, data, contentType string) string { return "" }
func hostBlobDownload(blobID string) string                   { return "" }
func hostBlobDelete(blobID string) string                     { return "" }
func hostBlobList(prefix string) string                       { return "[]" }

func hostPGJoin(groupName string) string  { return "" }
func hostPGLeave(groupName string) string { return "" }
func hostPGMembers(groupName string) string {
	return `["actor-1","actor-2"]`
}
func hostPGBroadcast(groupName, msgType, payloadJSON string) string { return "" }

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

func hostApplicationMetricsAdd(applicationID, metricsJSON string) string {
	return metricsJSON
}

func hostApplicationGetStatus(applicationID, nodeID string) string {
	return `{"node_id":"` + nodeID + `","application":{"application_id":"` + applicationID + `"}}`
}
