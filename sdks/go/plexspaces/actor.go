// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// PlexSpaces Go SDK - Actor Interface
//
// Provides the Actor interface and base implementation for building
// PlexSpaces actors in Go. Actors implement Init, Handle, GetState,
// and SetState to be compatible with the simple-actor WIT interface.
//
// Example:
//
//	type Calculator struct {
//	    plexspaces.BaseActor
//	    LastOp     string    `json:"last_operation"`
//	    LastResult float64   `json:"last_result"`
//	    History    []OpEntry `json:"history"`
//	}
//
//	func (c *Calculator) Handle(from, msgType, payloadJSON string) string {
//	    switch msgType {
//	    case "add":
//	        var p struct{ Operands []float64 `json:"operands"` }
//	        json.Unmarshal([]byte(payloadJSON), &p)
//	        result := 0.0
//	        for _, v := range p.Operands { result += v }
//	        c.LastOp = "add"
//	        c.LastResult = result
//	        return marshal(map[string]any{"result": result, "operation": "add"})
//	    }
//	    return `{"error":"unknown operation"}`
//	}

package plexspaces

import "encoding/json"

// Actor is the interface that all PlexSpaces actors must implement.
// It maps directly to the simple-actor WIT interface.
type Actor interface {
	// Init initializes the actor with a JSON config string.
	// Returns empty string on success, or error message.
	Init(configJSON string) string

	// Handle processes an incoming message.
	// Returns JSON-serialized result string.
	Handle(fromActor, msgType, payloadJSON string) string

	// GetState returns the actor's current state as JSON.
	GetState() string

	// SetState restores the actor's state from JSON.
	// Returns empty string on success, or error message.
	SetState(stateJSON string) string
}

// BaseActor provides default implementations for Init, GetState, and SetState.
// Embed this in your actor struct and override Handle.
//
// GetState and SetState use JSON serialization of the embedding struct.
// Override them if you need custom serialization.
type BaseActor struct {
	// self is set by Register() to point to the embedding struct
	self Actor
}

// Init is a no-op default. Override in your actor if needed.
func (b *BaseActor) Init(configJSON string) string {
	return ""
}

// GetState serializes the actor to JSON. Override for custom serialization.
func (b *BaseActor) GetState() string {
	if b.self != nil {
		data, err := json.Marshal(b.self)
		if err != nil {
			return `{"error":"` + err.Error() + `"}`
		}
		return string(data)
	}
	return "{}"
}

// SetState deserializes JSON into the actor. Override for custom deserialization.
func (b *BaseActor) SetState(stateJSON string) string {
	if b.self != nil {
		if err := json.Unmarshal([]byte(stateJSON), b.self); err != nil {
			return "ERROR: " + err.Error()
		}
	}
	return ""
}

// SetSelf sets the self reference for BaseActor's default GetState/SetState.
// Call this in your actor's constructor or before registering.
func (b *BaseActor) SetSelf(self Actor) {
	b.self = self
}

// ========================================================================
// Actor Registration (for WASM export)
// ========================================================================

// registeredActor holds the singleton actor instance
var registeredActor Actor

// Register registers the actor implementation for WASM export.
// Call this from main() or init().
//
//	func main() {
//	    calc := &Calculator{}
//	    calc.SetSelf(calc)
//	    plexspaces.Register(calc)
//	}
func Register(actor Actor) {
	registeredActor = actor
}

// GetRegisteredActor returns the registered actor (used by WASM exports).
func GetRegisteredActor() Actor {
	return registeredActor
}
