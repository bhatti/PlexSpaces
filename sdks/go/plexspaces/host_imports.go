// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// WIT host function imports for TinyGo WASM compilation.
//
// These functions are linked to the actual WIT host imports at runtime.
// When compiled with TinyGo to WASM (tinygo build -target=wasi ...),
// the //go:wasmimport directives tell the linker to import these from
// the host environment.
//
// TinyGo 0.36+ does not support string return types in //go:wasmimport.
// We use the Component Model canonical ABI retptr pattern: an extra
// unsafe.Pointer parameter where the host writes (ptr: u32, len: u32),
// and a Go wrapper decodes the result into a string.
//
// This file is excluded from native Go builds (only compiled for wasm).
// See host_stubs.go for native/test stub implementations.
//
// Build with:
//
//	tinygo build -target=wasi -o actor.wasm .

//go:build wasm

package plexspaces

import "unsafe"

// retArea is a scratch buffer for Component Model canonical ABI return values.
// Functions returning a string write (ptr: u32, len: u32) = 8 bytes here.
var retArea [8]byte

// readRetString reads a (ptr, len) pair from retptr and returns a Go string.
func readRetString(retptr unsafe.Pointer) string {
	ptr := *(*uint32)(retptr)
	length := *(*uint32)(unsafe.Add(retptr, 4))
	if length == 0 {
		return ""
	}
	return unsafe.String((*byte)(unsafe.Pointer(uintptr(ptr))), int(length))
}

// ========================================================================
// Messaging
// ========================================================================

//go:wasmimport plexspaces:simple-actor/host@0.1.0 send
func rawHostSend(to, msgType, payloadJSON string, retptr unsafe.Pointer)

func hostSend(to, msgType, payloadJSON string) string {
	rawHostSend(to, msgType, payloadJSON, unsafe.Pointer(&retArea))
	return readRetString(unsafe.Pointer(&retArea))
}

//go:wasmimport plexspaces:simple-actor/host@0.1.0 ask
func rawHostAsk(to, msgType, payloadJSON string, timeoutMs uint64, retptr unsafe.Pointer)

func hostAsk(to, msgType, payloadJSON string, timeoutMs uint64) string {
	rawHostAsk(to, msgType, payloadJSON, timeoutMs, unsafe.Pointer(&retArea))
	return readRetString(unsafe.Pointer(&retArea))
}

// ========================================================================
// Actor Identity
// ========================================================================

//go:wasmimport plexspaces:simple-actor/host@0.1.0 self-id
func rawHostSelfID(retptr unsafe.Pointer)

func hostSelfID() string {
	rawHostSelfID(unsafe.Pointer(&retArea))
	return readRetString(unsafe.Pointer(&retArea))
}

// ========================================================================
// Actor Lifecycle
// ========================================================================

//go:wasmimport plexspaces:simple-actor/host@0.1.0 spawn
func rawHostSpawn(moduleRef, actorID, initConfigJSON string, retptr unsafe.Pointer)

func hostSpawn(moduleRef, actorID, initConfigJSON string) string {
	rawHostSpawn(moduleRef, actorID, initConfigJSON, unsafe.Pointer(&retArea))
	return readRetString(unsafe.Pointer(&retArea))
}

//go:wasmimport plexspaces:simple-actor/host@0.1.0 stop
func rawHostStop(actorID string, retptr unsafe.Pointer)

func hostStop(actorID string) string {
	rawHostStop(actorID, unsafe.Pointer(&retArea))
	return readRetString(unsafe.Pointer(&retArea))
}

// ========================================================================
// Actor Linking & Monitoring
// ========================================================================

//go:wasmimport plexspaces:simple-actor/host@0.1.0 link
func rawHostLink(actorID string, retptr unsafe.Pointer)

func hostLink(actorID string) string {
	rawHostLink(actorID, unsafe.Pointer(&retArea))
	return readRetString(unsafe.Pointer(&retArea))
}

//go:wasmimport plexspaces:simple-actor/host@0.1.0 unlink
func rawHostUnlink(actorID string, retptr unsafe.Pointer)

func hostUnlink(actorID string) string {
	rawHostUnlink(actorID, unsafe.Pointer(&retArea))
	return readRetString(unsafe.Pointer(&retArea))
}

//go:wasmimport plexspaces:simple-actor/host@0.1.0 monitor
func rawHostMonitor(actorID string, retptr unsafe.Pointer)

func hostMonitor(actorID string) string {
	rawHostMonitor(actorID, unsafe.Pointer(&retArea))
	return readRetString(unsafe.Pointer(&retArea))
}

//go:wasmimport plexspaces:simple-actor/host@0.1.0 demonitor
func rawHostDemonitor(monitorRef string, retptr unsafe.Pointer)

func hostDemonitor(monitorRef string) string {
	rawHostDemonitor(monitorRef, unsafe.Pointer(&retArea))
	return readRetString(unsafe.Pointer(&retArea))
}

// ========================================================================
// Timers
// ========================================================================

//go:wasmimport plexspaces:simple-actor/host@0.1.0 send-after
func rawHostSendAfter(delayMs uint64, msgType, payloadJSON string, retptr unsafe.Pointer)

func hostSendAfter(delayMs uint64, msgType, payloadJSON string) string {
	rawHostSendAfter(delayMs, msgType, payloadJSON, unsafe.Pointer(&retArea))
	return readRetString(unsafe.Pointer(&retArea))
}

// ========================================================================
// Logging & Time
// ========================================================================

//go:wasmimport plexspaces:simple-actor/host@0.1.0 log
func hostLog(level, message string)

//go:wasmimport plexspaces:simple-actor/host@0.1.0 now-ms
func hostNowMs() uint64

// ========================================================================
// Key-Value Store
// ========================================================================

//go:wasmimport plexspaces:simple-actor/host@0.1.0 kv-get
func rawHostKVGet(key string, retptr unsafe.Pointer)

func hostKVGet(key string) string {
	rawHostKVGet(key, unsafe.Pointer(&retArea))
	return readRetString(unsafe.Pointer(&retArea))
}

//go:wasmimport plexspaces:simple-actor/host@0.1.0 kv-put
func rawHostKVPut(key, value string, retptr unsafe.Pointer)

func hostKVPut(key, value string) string {
	rawHostKVPut(key, value, unsafe.Pointer(&retArea))
	return readRetString(unsafe.Pointer(&retArea))
}

//go:wasmimport plexspaces:simple-actor/host@0.1.0 kv-delete
func rawHostKVDelete(key string, retptr unsafe.Pointer)

func hostKVDelete(key string) string {
	rawHostKVDelete(key, unsafe.Pointer(&retArea))
	return readRetString(unsafe.Pointer(&retArea))
}

//go:wasmimport plexspaces:simple-actor/host@0.1.0 kv-list
func rawHostKVList(prefix string, retptr unsafe.Pointer)

func hostKVList(prefix string) string {
	rawHostKVList(prefix, unsafe.Pointer(&retArea))
	return readRetString(unsafe.Pointer(&retArea))
}

// ========================================================================
// TupleSpace
// ========================================================================

//go:wasmimport plexspaces:simple-actor/host@0.1.0 ts-write
func rawHostTSWrite(tupleJSON string, retptr unsafe.Pointer)

func hostTSWrite(tupleJSON string) string {
	rawHostTSWrite(tupleJSON, unsafe.Pointer(&retArea))
	return readRetString(unsafe.Pointer(&retArea))
}

//go:wasmimport plexspaces:simple-actor/host@0.1.0 ts-read
func rawHostTSRead(patternJSON string, retptr unsafe.Pointer)

func hostTSRead(patternJSON string) string {
	rawHostTSRead(patternJSON, unsafe.Pointer(&retArea))
	return readRetString(unsafe.Pointer(&retArea))
}

//go:wasmimport plexspaces:simple-actor/host@0.1.0 ts-take
func rawHostTSTake(patternJSON string, retptr unsafe.Pointer)

func hostTSTake(patternJSON string) string {
	rawHostTSTake(patternJSON, unsafe.Pointer(&retArea))
	return readRetString(unsafe.Pointer(&retArea))
}

//go:wasmimport plexspaces:simple-actor/host@0.1.0 ts-read-all
func rawHostTSReadAll(patternJSON string, retptr unsafe.Pointer)

func hostTSReadAll(patternJSON string) string {
	rawHostTSReadAll(patternJSON, unsafe.Pointer(&retArea))
	return readRetString(unsafe.Pointer(&retArea))
}

// ========================================================================
// Distributed Locks
// ========================================================================

//go:wasmimport plexspaces:simple-actor/host@0.1.0 lock-acquire
func rawHostLockAcquire(tenantID, namespace, holderID, lockName string, leaseDurationSecs uint32, timeoutMs uint64, retptr unsafe.Pointer)

func hostLockAcquire(tenantID, namespace, holderID, lockName string, leaseDurationSecs uint32, timeoutMs uint64) string {
	rawHostLockAcquire(tenantID, namespace, holderID, lockName, leaseDurationSecs, timeoutMs, unsafe.Pointer(&retArea))
	return readRetString(unsafe.Pointer(&retArea))
}

//go:wasmimport plexspaces:simple-actor/host@0.1.0 lock-release
func rawHostLockRelease(lockID, tenantID, namespace, holderID, lockVersion string, retptr unsafe.Pointer)

func hostLockRelease(lockID, tenantID, namespace, holderID, lockVersion string) string {
	rawHostLockRelease(lockID, tenantID, namespace, holderID, lockVersion, unsafe.Pointer(&retArea))
	return readRetString(unsafe.Pointer(&retArea))
}

//go:wasmimport plexspaces:simple-actor/host@0.1.0 lock-renew
func rawHostLockRenew(lockID, tenantID, namespace, holderID, lockVersion string, leaseDurationSecs uint32, retptr unsafe.Pointer)

func hostLockRenew(lockID, tenantID, namespace, holderID, lockVersion string, leaseDurationSecs uint32) string {
	rawHostLockRenew(lockID, tenantID, namespace, holderID, lockVersion, leaseDurationSecs, unsafe.Pointer(&retArea))
	return readRetString(unsafe.Pointer(&retArea))
}

// ========================================================================
// Blob Storage
// ========================================================================

//go:wasmimport plexspaces:simple-actor/host@0.1.0 blob-upload
func rawHostBlobUpload(blobID, data, contentType string, retptr unsafe.Pointer)

func hostBlobUpload(blobID, data, contentType string) string {
	rawHostBlobUpload(blobID, data, contentType, unsafe.Pointer(&retArea))
	return readRetString(unsafe.Pointer(&retArea))
}

//go:wasmimport plexspaces:simple-actor/host@0.1.0 blob-download
func rawHostBlobDownload(blobID string, retptr unsafe.Pointer)

func hostBlobDownload(blobID string) string {
	rawHostBlobDownload(blobID, unsafe.Pointer(&retArea))
	return readRetString(unsafe.Pointer(&retArea))
}

//go:wasmimport plexspaces:simple-actor/host@0.1.0 blob-delete
func rawHostBlobDelete(blobID string, retptr unsafe.Pointer)

func hostBlobDelete(blobID string) string {
	rawHostBlobDelete(blobID, unsafe.Pointer(&retArea))
	return readRetString(unsafe.Pointer(&retArea))
}

//go:wasmimport plexspaces:simple-actor/host@0.1.0 blob-list
func rawHostBlobList(prefix string, retptr unsafe.Pointer)

func hostBlobList(prefix string) string {
	rawHostBlobList(prefix, unsafe.Pointer(&retArea))
	return readRetString(unsafe.Pointer(&retArea))
}

// ========================================================================
// Process Groups
// ========================================================================

//go:wasmimport plexspaces:simple-actor/host@0.1.0 pg-join
func rawHostPGJoin(groupName string, retptr unsafe.Pointer)

func hostPGJoin(groupName string) string {
	rawHostPGJoin(groupName, unsafe.Pointer(&retArea))
	return readRetString(unsafe.Pointer(&retArea))
}

//go:wasmimport plexspaces:simple-actor/host@0.1.0 pg-leave
func rawHostPGLeave(groupName string, retptr unsafe.Pointer)

func hostPGLeave(groupName string) string {
	rawHostPGLeave(groupName, unsafe.Pointer(&retArea))
	return readRetString(unsafe.Pointer(&retArea))
}

//go:wasmimport plexspaces:simple-actor/host@0.1.0 pg-members
func rawHostPGMembers(groupName string, retptr unsafe.Pointer)

func hostPGMembers(groupName string) string {
	rawHostPGMembers(groupName, unsafe.Pointer(&retArea))
	return readRetString(unsafe.Pointer(&retArea))
}

//go:wasmimport plexspaces:simple-actor/host@0.1.0 pg-broadcast
func rawHostPGBroadcast(groupName, msgType, payloadJSON string, retptr unsafe.Pointer)

func hostPGBroadcast(groupName, msgType, payloadJSON string) string {
	rawHostPGBroadcast(groupName, msgType, payloadJSON, unsafe.Pointer(&retArea))
	return readRetString(unsafe.Pointer(&retArea))
}

// ========================================================================
// Elastic pool (checkout/checkin)
// ========================================================================

//go:wasmimport plexspaces:simple-actor/host@0.1.0 pool-checkout
func rawHostPoolCheckout(poolName string, timeoutMs uint64, retptr unsafe.Pointer)

func hostPoolCheckout(poolName string, timeoutMs uint64) string {
	rawHostPoolCheckout(poolName, timeoutMs, unsafe.Pointer(&retArea))
	return readRetString(unsafe.Pointer(&retArea))
}

//go:wasmimport plexspaces:simple-actor/host@0.1.0 pool-checkin
func rawHostPoolCheckin(poolName, actorID, checkoutID string, healthy bool, retptr unsafe.Pointer)

func hostPoolCheckin(poolName, actorID, checkoutID string, healthy bool) string {
	rawHostPoolCheckin(poolName, actorID, checkoutID, healthy, unsafe.Pointer(&retArea))
	return readRetString(unsafe.Pointer(&retArea))
}

//go:wasmimport plexspaces:simple-actor/host@0.1.0 pool-get-metrics
func rawHostPoolGetMetrics(poolName string, retptr unsafe.Pointer)

func hostPoolGetMetrics(poolName string) string {
	rawHostPoolGetMetrics(poolName, unsafe.Pointer(&retArea))
	return readRetString(unsafe.Pointer(&retArea))
}

// ============================================================================
// ShardGroup / Application Metrics
// ============================================================================

//go:wasmimport plexspaces:simple-actor/host@0.1.0 create-shard-group
func rawHostCreateShardGroup(requestJSON string, retptr unsafe.Pointer)

func hostCreateShardGroup(requestJSON string) string {
	rawHostCreateShardGroup(requestJSON, unsafe.Pointer(&retArea))
	return readRetString(unsafe.Pointer(&retArea))
}

//go:wasmimport plexspaces:simple-actor/host@0.1.0 bulk-update-shard-group
func rawHostBulkUpdateShardGroup(requestJSON string, retptr unsafe.Pointer)

func hostBulkUpdateShardGroup(requestJSON string) string {
	rawHostBulkUpdateShardGroup(requestJSON, unsafe.Pointer(&retArea))
	return readRetString(unsafe.Pointer(&retArea))
}

//go:wasmimport plexspaces:simple-actor/host@0.1.0 map-shard-group
func rawHostMapShardGroup(requestJSON string, retptr unsafe.Pointer)

func hostMapShardGroup(requestJSON string) string {
	rawHostMapShardGroup(requestJSON, unsafe.Pointer(&retArea))
	return readRetString(unsafe.Pointer(&retArea))
}

//go:wasmimport plexspaces:simple-actor/host@0.1.0 scatter-gather
func rawHostScatterGather(requestJSON string, retptr unsafe.Pointer)

func hostScatterGather(requestJSON string) string {
	rawHostScatterGather(requestJSON, unsafe.Pointer(&retArea))
	return readRetString(unsafe.Pointer(&retArea))
}

//go:wasmimport plexspaces:simple-actor/host@0.1.0 broadcast-shard-group
func rawHostBroadcastShardGroup(requestJSON string, retptr unsafe.Pointer)

func hostBroadcastShardGroup(requestJSON string) string {
	rawHostBroadcastShardGroup(requestJSON, unsafe.Pointer(&retArea))
	return readRetString(unsafe.Pointer(&retArea))
}

//go:wasmimport plexspaces:simple-actor/host@0.1.0 reduce-shard-group
func rawHostReduceShardGroup(requestJSON string, retptr unsafe.Pointer)

func hostReduceShardGroup(requestJSON string) string {
	rawHostReduceShardGroup(requestJSON, unsafe.Pointer(&retArea))
	return readRetString(unsafe.Pointer(&retArea))
}

//go:wasmimport plexspaces:simple-actor/host@0.1.0 all-reduce-shard-group
func rawHostAllReduceShardGroup(requestJSON string, retptr unsafe.Pointer)

func hostAllReduceShardGroup(requestJSON string) string {
	rawHostAllReduceShardGroup(requestJSON, unsafe.Pointer(&retArea))
	return readRetString(unsafe.Pointer(&retArea))
}

//go:wasmimport plexspaces:simple-actor/host@0.1.0 barrier-shard-group
func rawHostBarrierShardGroup(requestJSON string, retptr unsafe.Pointer)

func hostBarrierShardGroup(requestJSON string) string {
	rawHostBarrierShardGroup(requestJSON, unsafe.Pointer(&retArea))
	return readRetString(unsafe.Pointer(&retArea))
}

//go:wasmimport plexspaces:simple-actor/host@0.1.0 spawn-actors
func rawHostSpawnActors(requestJSON string, retptr unsafe.Pointer)

func hostSpawnActors(requestJSON string) string {
	rawHostSpawnActors(requestJSON, unsafe.Pointer(&retArea))
	return readRetString(unsafe.Pointer(&retArea))
}

//go:wasmimport plexspaces:simple-actor/host@0.1.0 application-metrics-add
func rawHostApplicationMetricsAdd(applicationID, metricsJSON string, retptr unsafe.Pointer)

func hostApplicationMetricsAdd(applicationID, metricsJSON string) string {
	rawHostApplicationMetricsAdd(applicationID, metricsJSON, unsafe.Pointer(&retArea))
	return readRetString(unsafe.Pointer(&retArea))
}

//go:wasmimport plexspaces:simple-actor/host@0.1.0 application-get-status
func rawHostApplicationGetStatus(applicationID, nodeID string, retptr unsafe.Pointer)

func hostApplicationGetStatus(applicationID, nodeID string) string {
	rawHostApplicationGetStatus(applicationID, nodeID, unsafe.Pointer(&retArea))
	return readRetString(unsafe.Pointer(&retArea))
}
