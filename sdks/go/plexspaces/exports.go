// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// WIT actor export functions for TinyGo WASM compilation.
//
// These functions implement the Component Model canonical ABI for the
// plexspaces:simple-actor/actor interface. They use raw uint32 types
// for string parameters (ptr, len pairs) and return values to match
// the canonical ABI signatures expected by wasm-tools component new.
//
// Canonical ABI mapping:
//   - string param → (ptr: i32, len: i32)
//   - string return → i32 (pointer to (ptr, len) pair in return area)
//   - cabi_realloc → memory allocation for host-to-guest string passing
//   - cabi_post_* → cleanup after host reads return values
//
// The host calls these functions to drive the actor lifecycle:
//   - init(config-json) -> error string (empty = success)
//   - handle(from, msg-type, payload-json) -> result JSON string
//   - get-state() -> state JSON string
//   - set-state(state-json) -> error string (empty = success)
//
// This file is excluded from native Go builds (only compiled for wasm).

//go:build tinygo.wasm

package plexspaces

import (
	"encoding/json"
	"strings"
	"unsafe"
)

// cabiReturnArea is a fixed buffer where canonical ABI return values are written.
// The host reads (ptr: u32, len: u32) from this area after function returns.
var cabiReturnArea [8]byte

// cabi_realloc is required by the Component Model canonical ABI.
// The host calls this to allocate memory in the WASM module for string parameters.
//
//export cabi_realloc
func cabiRealloc(oldPtr, oldSize, align, newSize uint32) uint32 {
	if newSize == 0 {
		return 0 // align as uintptr to suppress unused warning
	}
	buf := make([]byte, newSize)
	if oldPtr != 0 && oldSize > 0 {
		copyLen := oldSize
		if newSize < copyLen {
			copyLen = newSize
		}
		src := unsafe.Pointer(uintptr(oldPtr))
		for i := uint32(0); i < copyLen; i++ {
			buf[i] = *(*byte)(unsafe.Add(src, i))
		}
	}
	return uint32(uintptr(unsafe.Pointer(unsafe.SliceData(buf))))
}

// ptrToString reads a Go string from WASM linear memory given (ptr, len).
func ptrToString(ptr, length uint32) string {
	if length == 0 {
		return ""
	}
	return unsafe.String((*byte)(unsafe.Pointer(uintptr(ptr))), int(length))
}

// stringToRetArea writes a string result to the canonical ABI return area.
// Returns the address of the return area for the host to read (ptr, len).
func stringToRetArea(s string) uint32 {
	sLen := uint32(len(s))
	var sPtr uint32
	if sLen > 0 {
		// Allocate a copy so the string data stays alive after return
		buf := make([]byte, sLen)
		copy(buf, s)
		sPtr = uint32(uintptr(unsafe.Pointer(unsafe.SliceData(buf))))
	}
	// Write (ptr, len) to return area
	retPtr := unsafe.Pointer(&cabiReturnArea[0])
	*(*uint32)(retPtr) = sPtr
	*(*uint32)(unsafe.Add(retPtr, 4)) = sLen
	return uint32(uintptr(retPtr))
}

// ========================================================================
// Actor Interface Exports (canonical ABI signatures)
// ========================================================================

// init(config-json: string) -> string
// Canonical ABI: (i32, i32) -> (i32)
//
//export plexspaces:simple-actor/actor@0.1.0#init
func wasmInit(configPtr, configLen uint32) uint32 {
	configJSON := ptrToString(configPtr, configLen)
	actor := GetRegisteredActor()
	if actor == nil {
		return stringToRetArea("ERROR: no actor registered")
	}
	return stringToRetArea(actor.Init(configJSON))
}

// handle(from: string, msg-type: string, payload-json: string) -> string
// Canonical ABI: (i32, i32, i32, i32, i32, i32) -> (i32)
//
//export plexspaces:simple-actor/actor@0.1.0#handle
func wasmHandle(fromPtr, fromLen, msgTypePtr, msgTypeLen, payloadPtr, payloadLen uint32) uint32 {
	fromActor := ptrToString(fromPtr, fromLen)
	msgType := ptrToString(msgTypePtr, msgTypeLen)
	payloadJSON := ptrToString(payloadPtr, payloadLen)
	actor := GetRegisteredActor()
	if actor == nil {
		return stringToRetArea(`{"error":"no actor registered"}`)
	}
	// Resolve operation from payload when envelope is "call" or "cast". Payload key order
	// (aligned with Rust/Python/TS): message_type (canonical) -> op -> msg_type.
	if msgType == "call" || msgType == "cast" {
		var envelope struct {
			MessageType string `json:"message_type"`
			Op          string `json:"op"`
			MsgType     string `json:"msg_type"`
		}
		if json.Unmarshal([]byte(payloadJSON), &envelope) == nil {
			for _, v := range []string{envelope.MessageType, envelope.Op, envelope.MsgType} {
				if v != "" && v != "call" && v != "cast" {
					msgType = v
					break
				}
			}
		}
	}

	// Workflow behavior (aligned with Rust/Python/TS): route workflow_run / workflow_signal:name / workflow_query:name
	if wa, ok := actor.(WorkflowActor); ok {
		switch {
		case msgType == "workflow_run":
			return stringToRetArea(wa.Run(payloadJSON))
		case strings.HasPrefix(msgType, "workflow_signal:"):
			name := strings.TrimSpace(strings.TrimPrefix(msgType, "workflow_signal:"))
			wa.Signal(name, payloadJSON)
			return stringToRetArea("{}")
		case strings.HasPrefix(msgType, "workflow_query:"):
			name := strings.TrimSpace(strings.TrimPrefix(msgType, "workflow_query:"))
			return stringToRetArea(wa.Query(name, payloadJSON))
		}
	}

	return stringToRetArea(actor.Handle(fromActor, msgType, payloadJSON))
}

// get-state() -> string
// Canonical ABI: () -> (i32)
//
//export plexspaces:simple-actor/actor@0.1.0#get-state
func wasmGetState() uint32 {
	actor := GetRegisteredActor()
	if actor == nil {
		return stringToRetArea("{}")
	}
	return stringToRetArea(actor.GetState())
}

// set-state(state-json: string) -> string
// Canonical ABI: (i32, i32) -> (i32)
//
//export plexspaces:simple-actor/actor@0.1.0#set-state
func wasmSetState(statePtr, stateLen uint32) uint32 {
	stateJSON := ptrToString(statePtr, stateLen)
	actor := GetRegisteredActor()
	if actor == nil {
		return stringToRetArea("ERROR: no actor registered")
	}
	return stringToRetArea(actor.SetState(stateJSON))
}

// ========================================================================
// Post-return cleanup functions (canonical ABI)
// Called by the host after reading the return value.
// ========================================================================

//export cabi_post_plexspaces:simple-actor/actor@0.1.0#init
func cabiPostInit(_ uint32) {}

//export cabi_post_plexspaces:simple-actor/actor@0.1.0#handle
func cabiPostHandle(_ uint32) {}

//export cabi_post_plexspaces:simple-actor/actor@0.1.0#get-state
func cabiPostGetState(_ uint32) {}

//export cabi_post_plexspaces:simple-actor/actor@0.1.0#set-state
func cabiPostSetState(_ uint32) {}
