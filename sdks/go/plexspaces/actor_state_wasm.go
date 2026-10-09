// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Actor state serialization — TinyGo WASM build.
//
// Does NOT import encoding/json. In TinyGo 0.40.x, importing encoding/json (even for
// Unmarshal into typed structs) links reflectlite.Value.UnsafePointer and
// decodeState.convertNumber into the WASM function table at slots that collide with
// actor interface dispatch, corrupting Handle$invoke. All JSON parsing uses the
// wasm-safe helpers in safe_marshal_wasm.go instead.

//go:build wasm

package plexspaces

import (
	"strings"
)

// GetState serializes actor state for checkpointing (WASM build).
// If the actor implements WASMStateful, MarshalStateWASM() is called (no json.Marshal).
// Otherwise returns a metadata-only envelope with empty state.
func (b *BaseActor) GetState() string {
	if b.self == nil {
		return "{}"
	}
	var state string
	if ws, ok := b.self.(WASMStateful); ok {
		state = ws.MarshalStateWASM()
	} else {
		state = "{}"
	}
	var sb strings.Builder
	sb.WriteString(`{"_meta":{"actor_id":`)
	writeJSONString(&sb, b.actorID)
	sb.WriteString(`,"application_id":`)
	writeJSONString(&sb, b.applicationID)
	sb.WriteString(`},"_state":`)
	sb.WriteString(state)
	sb.WriteByte('}')
	return sb.String()
}

// SetState restores actor state from the envelope (WASM build).
// Parses the envelope {"_meta":{...},"_state":{...}} without encoding/json.
func (b *BaseActor) SetState(stateJSON string) string {
	if b.self == nil {
		return ""
	}
	metaStr := WasmJSONFindObject(stateJSON, "_meta")
	if metaStr != "" {
		actorID := WasmJSONExtractStr(metaStr, "actor_id")
		appID := WasmJSONExtractStr(metaStr, "application_id")
		if actorID != "" {
			b.actorID = actorID
		}
		if appID != "" {
			b.applicationID = appID
		}
		stateStr := WasmJSONFindObject(stateJSON, "_state")
		if stateStr != "" {
			if ws, ok := b.self.(WASMStateful); ok {
				ws.UnmarshalStateWASM(stateStr)
			}
		}
		return ""
	}
	// Legacy: bare JSON — try WASMStateful directly
	if ws, ok := b.self.(WASMStateful); ok {
		ws.UnmarshalStateWASM(stateJSON)
	}
	return ""
}
