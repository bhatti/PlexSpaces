// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// WIT actor export functions for TinyGo WASM compilation.
//
// These functions implement the Component Model canonical ABI for the
// plexspaces:actor/actor interface. They use raw uint32 types
// for string parameters (ptr, len pairs) and return values to match
// the canonical ABI signatures expected by wasm-tools component new.
//
// Canonical ABI mapping:
//   - string / list<u8> params → (ptr: i32, len: i32)
//   - result<...> return → i32 pointer to in-memory variant (discriminant + ptr/len)
//   - cabi_realloc → memory allocation for host-to-guest passing
//   - cabi_post_* → cleanup after host reads return values
//
// The host calls these functions to drive the actor lifecycle. WIT types are
// result<payload, actor-error> (and result<_, actor-error> for init/set-state):
// opaque payload bytes on success, host error string on failure. SDK actors may
// still use JSON inside those bytes; the boundary is raw list<u8>, not a JSON string.
//
// This file is excluded from native Go builds (only compiled for wasm).

//go:build wasm

package plexspaces

import (
	"encoding/binary"
	"encoding/json"
	"strings"
	"unsafe"
)

// cabiResultArea stores result<payload, actor-error> in the Component Model
// canonical in-memory layout (variant with two cases: ok, error):
//   offset 0: u8 discriminant (0 = ok, 1 = error)
//   offsets 1-3: padding to 4-byte alignment
//   offsets 4-7: ptr (list<u8> or string body)
//   offsets 8-11: byte length
//
// Exports return the address of this area (single i32) when the flattened result
// does not fit in one register.
var cabiResultArea [12]byte

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

func clearCabiResultArea() {
	for i := range cabiResultArea {
		cabiResultArea[i] = 0
	}
}

func cabiResultAreaAddress() uint32 {
	return uint32(uintptr(unsafe.Pointer(&cabiResultArea[0])))
}

// copyBytesToGuestAlloc copies data into a cabi_realloc-allocated buffer; returns ptr and length.
func copyBytesToGuestAlloc(data []byte) (ptr uint32, length uint32) {
	length = uint32(len(data))
	if length == 0 {
		return 0, 0
	}
	ptr = cabiRealloc(0, 0, 1, length)
	dst := unsafe.Slice((*byte)(unsafe.Pointer(uintptr(ptr))), len(data))
	copy(dst, data)
	return ptr, length
}

// resultOkPayload encodes result::ok(list<u8>) for handle/get-state responses.
func resultOkPayload(data []byte) uint32 {
	clearCabiResultArea()
	cabiResultArea[0] = 0 // ok
	ptr, length := copyBytesToGuestAlloc(data)
	binary.LittleEndian.PutUint32(cabiResultArea[4:], ptr)
	binary.LittleEndian.PutUint32(cabiResultArea[8:], length)
	return cabiResultAreaAddress()
}

// resultOkUnit encodes result::ok for init/set-state success (empty ok payload).
func resultOkUnit() uint32 {
	clearCabiResultArea()
	cabiResultArea[0] = 0
	binary.LittleEndian.PutUint32(cabiResultArea[4:], 0)
	binary.LittleEndian.PutUint32(cabiResultArea[8:], 0)
	return cabiResultAreaAddress()
}

// resultErr encodes result::error(string) for init/set-state failures.
func resultErr(msg string) uint32 {
	clearCabiResultArea()
	cabiResultArea[0] = 1 // error
	ptr, length := copyBytesToGuestAlloc([]byte(msg))
	binary.LittleEndian.PutUint32(cabiResultArea[4:], ptr)
	binary.LittleEndian.PutUint32(cabiResultArea[8:], length)
	return cabiResultAreaAddress()
}

func resultOkJSON(s string) uint32 {
	return resultOkPayload([]byte(s))
}

// ========================================================================
// Actor Interface Exports (canonical ABI signatures)
// ========================================================================

// init(config-json: string) -> string
// Canonical ABI: (i32, i32) -> (i32)
//
//export plexspaces:actor/actor@0.1.0#init
func wasmInit(configPtr, configLen uint32) uint32 {
	configJSON := ptrToString(configPtr, configLen)
	actor := GetRegisteredActor()
	if actor == nil {
		return resultErr("ERROR: no actor registered")
	}
	if errMsg := actor.Init(configJSON); errMsg != "" {
		return resultErr(errMsg)
	}
	return resultOkUnit()
}

// handle(from: string, msg-type: string, payload-json: string) -> string
// Canonical ABI: (i32, i32, i32, i32, i32, i32) -> (i32)
//
//export plexspaces:actor/actor@0.1.0#handle
func wasmHandle(fromPtr, fromLen, msgTypePtr, msgTypeLen, payloadPtr, payloadLen uint32) uint32 {
	fromActor := ptrToString(fromPtr, fromLen)
	msgType := ptrToString(msgTypePtr, msgTypeLen)
	payloadJSON := ptrToString(payloadPtr, payloadLen)
	actor := GetRegisteredActor()
	if actor == nil {
		return resultOkJSON(`{"error":"no actor registered"}`)
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
			return resultOkJSON(wa.Run(payloadJSON))
		case strings.HasPrefix(msgType, "workflow_signal:"):
			name := strings.TrimSpace(strings.TrimPrefix(msgType, "workflow_signal:"))
			wa.Signal(name, payloadJSON)
			return resultOkJSON("{}")
		case strings.HasPrefix(msgType, "workflow_query:"):
			name := strings.TrimSpace(strings.TrimPrefix(msgType, "workflow_query:"))
			return resultOkJSON(wa.Query(name, payloadJSON))
		}
	}

	return resultOkJSON(actor.Handle(fromActor, msgType, payloadJSON))
}

// get-state() -> string
// Canonical ABI: () -> (i32)
//
//export plexspaces:actor/actor@0.1.0#get-state
func wasmGetState() uint32 {
	actor := GetRegisteredActor()
	if actor == nil {
		return resultOkJSON("{}")
	}
	return resultOkJSON(actor.GetState())
}

// set-state(state-json: string) -> string
// Canonical ABI: (i32, i32) -> (i32)
//
//export plexspaces:actor/actor@0.1.0#set-state
func wasmSetState(statePtr, stateLen uint32) uint32 {
	stateJSON := ptrToString(statePtr, stateLen)
	actor := GetRegisteredActor()
	if actor == nil {
		return resultErr("ERROR: no actor registered")
	}
	if errMsg := actor.SetState(stateJSON); errMsg != "" {
		return resultErr(errMsg)
	}
	return resultOkUnit()
}

// ========================================================================
// Post-return cleanup functions (canonical ABI)
// Called by the host after reading the return value.
// ========================================================================

//export cabi_post_plexspaces:actor/actor@0.1.0#init
func cabiPostInit(_ uint32) {}

//export cabi_post_plexspaces:actor/actor@0.1.0#handle
func cabiPostHandle(_ uint32) {}

//export cabi_post_plexspaces:actor/actor@0.1.0#get-state
func cabiPostGetState(_ uint32) {}

//export cabi_post_plexspaces:actor/actor@0.1.0#set-state
func cabiPostSetState(_ uint32) {}
