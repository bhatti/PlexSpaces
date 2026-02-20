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
// This file is excluded from native Go builds (only compiled for wasm).
// See host_stubs.go for native/test stub implementations.
//
// Build with:
//
//	tinygo build -target=wasi -o actor.wasm .

//go:build tinygo.wasm

package plexspaces

// ========================================================================
// Messaging
// ========================================================================

//go:wasmimport plexspaces:simple-actor/host@0.1.0 send
func hostSend(to, msgType, payloadJSON string) string

//go:wasmimport plexspaces:simple-actor/host@0.1.0 ask
func hostAsk(to, msgType, payloadJSON string, timeoutMs uint64) string

// ========================================================================
// Actor Identity
// ========================================================================

//go:wasmimport plexspaces:simple-actor/host@0.1.0 self-id
func hostSelfID() string

// ========================================================================
// Actor Lifecycle
// ========================================================================

//go:wasmimport plexspaces:simple-actor/host@0.1.0 spawn
func hostSpawn(moduleRef, actorID, initConfigJSON string) string

//go:wasmimport plexspaces:simple-actor/host@0.1.0 stop
func hostStop(actorID string) string

// ========================================================================
// Actor Linking & Monitoring
// ========================================================================

//go:wasmimport plexspaces:simple-actor/host@0.1.0 link
func hostLink(actorID string) string

//go:wasmimport plexspaces:simple-actor/host@0.1.0 unlink
func hostUnlink(actorID string) string

//go:wasmimport plexspaces:simple-actor/host@0.1.0 monitor
func hostMonitor(actorID string) string

//go:wasmimport plexspaces:simple-actor/host@0.1.0 demonitor
func hostDemonitor(monitorRef string) string

// ========================================================================
// Timers
// ========================================================================

//go:wasmimport plexspaces:simple-actor/host@0.1.0 send-after
func hostSendAfter(delayMs uint64, msgType, payloadJSON string) string

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
func hostKVGet(key string) string

//go:wasmimport plexspaces:simple-actor/host@0.1.0 kv-put
func hostKVPut(key, value string) string

//go:wasmimport plexspaces:simple-actor/host@0.1.0 kv-delete
func hostKVDelete(key string) string

//go:wasmimport plexspaces:simple-actor/host@0.1.0 kv-list
func hostKVList(prefix string) string

// ========================================================================
// TupleSpace
// ========================================================================

//go:wasmimport plexspaces:simple-actor/host@0.1.0 ts-write
func hostTSWrite(tupleJSON string) string

//go:wasmimport plexspaces:simple-actor/host@0.1.0 ts-read
func hostTSRead(patternJSON string) string

//go:wasmimport plexspaces:simple-actor/host@0.1.0 ts-take
func hostTSTake(patternJSON string) string

//go:wasmimport plexspaces:simple-actor/host@0.1.0 ts-read-all
func hostTSReadAll(patternJSON string) string

// ========================================================================
// Distributed Locks
// ========================================================================

//go:wasmimport plexspaces:simple-actor/host@0.1.0 lock-acquire
func hostLockAcquire(tenantID, namespace, holderID, lockName string, leaseDurationSecs uint32, timeoutMs uint64) string

//go:wasmimport plexspaces:simple-actor/host@0.1.0 lock-release
func hostLockRelease(lockID, tenantID, namespace, holderID, lockVersion string) string

//go:wasmimport plexspaces:simple-actor/host@0.1.0 lock-renew
func hostLockRenew(lockID, tenantID, namespace, holderID, lockVersion string, leaseDurationSecs uint32) string

// ========================================================================
// Blob Storage
// ========================================================================

//go:wasmimport plexspaces:simple-actor/host@0.1.0 blob-upload
func hostBlobUpload(blobID, data, contentType string) string

//go:wasmimport plexspaces:simple-actor/host@0.1.0 blob-download
func hostBlobDownload(blobID string) string

//go:wasmimport plexspaces:simple-actor/host@0.1.0 blob-delete
func hostBlobDelete(blobID string) string

//go:wasmimport plexspaces:simple-actor/host@0.1.0 blob-list
func hostBlobList(prefix string) string

// ========================================================================
// Process Groups
// ========================================================================

//go:wasmimport plexspaces:simple-actor/host@0.1.0 pg-join
func hostPGJoin(groupName string) string

//go:wasmimport plexspaces:simple-actor/host@0.1.0 pg-leave
func hostPGLeave(groupName string) string

//go:wasmimport plexspaces:simple-actor/host@0.1.0 pg-members
func hostPGMembers(groupName string) string

//go:wasmimport plexspaces:simple-actor/host@0.1.0 pg-broadcast
func hostPGBroadcast(groupName, msgType, payloadJSON string) string
