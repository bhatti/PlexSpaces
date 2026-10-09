// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Actor state serialization — native (non-WASM) build.
// Uses encoding/json for full struct serialization.

//go:build !wasm

package plexspaces

import "encoding/json"

// stateEnvelope wraps actor state with runtime metadata so that metadata survives
// WASM re-instantiation (wasmtime#8943 workaround) without re-calling Init().
// The "_meta" key is reserved by the framework; actor state lives under "_state".
type stateEnvelope struct {
	Meta  *stateMeta      `json:"_meta,omitempty"`
	State json.RawMessage `json:"_state"`
}

// stateMeta holds framework-owned identity that must persist across re-instantiation.
type stateMeta struct {
	ActorID       string `json:"actor_id"`
	ApplicationID string `json:"application_id"`
}

// GetState serializes the actor to JSON wrapped in a state envelope that includes
// runtime metadata (actor_id, application_id). The envelope ensures metadata survives
// WASM re-instantiation without requiring init() to be re-called — matching the actor
// lifecycle contract where Init() runs exactly once at birth (like Erlang's init/1).
func (b *BaseActor) GetState() string {
	if b.self == nil {
		return "{}"
	}
	actorState, err := json.Marshal(b.self)
	if err != nil {
		return `{"error":"` + err.Error() + `"}`
	}
	envelope := stateEnvelope{
		Meta: &stateMeta{
			ActorID:       b.actorID,
			ApplicationID: b.applicationID,
		},
		State: actorState,
	}
	data, err := json.Marshal(envelope)
	if err != nil {
		return `{"error":"` + err.Error() + `"}`
	}
	return string(data)
}

// SetState restores actor state from the state envelope produced by GetState.
// Handles both envelope format (with _meta) and legacy bare-JSON for backward
// compatibility with actors that override GetState/SetState.
func (b *BaseActor) SetState(stateJSON string) string {
	if b.self == nil {
		return ""
	}
	var envelope stateEnvelope
	if err := json.Unmarshal([]byte(stateJSON), &envelope); err == nil && envelope.Meta != nil {
		b.actorID = envelope.Meta.ActorID
		b.applicationID = envelope.Meta.ApplicationID
		if envelope.State != nil {
			if err := json.Unmarshal(envelope.State, b.self); err != nil {
				return "ERROR: " + err.Error()
			}
		}
		return ""
	}
	if err := json.Unmarshal([]byte(stateJSON), b.self); err != nil {
		return "ERROR: " + err.Error()
	}
	return ""
}
