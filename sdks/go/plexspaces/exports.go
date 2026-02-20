// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// WIT actor export functions for TinyGo WASM compilation.
//
// These functions are exported to the WASM host via //export directives.
// They delegate to the registered Actor implementation (set via Register()).
//
// The host calls these functions to drive the actor lifecycle:
//   init(config-json) -> error string (empty = success)
//   handle(from, msg-type, payload-json) -> result JSON string
//   get-state() -> state JSON string
//   set-state(state-json) -> error string (empty = success)
//
// Build with:
//
//	tinygo build -target=wasi -o actor.wasm .

package plexspaces

//export init
func wasmInit(configJSON string) string {
	actor := GetRegisteredActor()
	if actor == nil {
		return "ERROR: no actor registered"
	}
	return actor.Init(configJSON)
}

//export handle
func wasmHandle(fromActor, msgType, payloadJSON string) string {
	actor := GetRegisteredActor()
	if actor == nil {
		return `{"error":"no actor registered"}`
	}
	return actor.Handle(fromActor, msgType, payloadJSON)
}

//export get-state
func wasmGetState() string {
	actor := GetRegisteredActor()
	if actor == nil {
		return "{}"
	}
	return actor.GetState()
}

//export set-state
func wasmSetState(stateJSON string) string {
	actor := GetRegisteredActor()
	if actor == nil {
		return "ERROR: no actor registered"
	}
	return actor.SetState(stateJSON)
}
