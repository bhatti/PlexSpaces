// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// PlexSpaces Go SDK - Multi-Actor Router (TinyGo WASM build)
//
// WASM variant of router.go. Does NOT import encoding/json to prevent
// TinyGo function-table corruption. All JSON parsing uses WasmJSON helpers.

//go:build wasm

package plexspaces

import "strings"

// routerStateEnvelope wraps the active actor's state with router dispatch metadata.
// WASM version uses []byte instead of json.RawMessage (same wire format, no json import).
type routerStateEnvelope struct {
	FactoryKey string
	ActorID    string
	ActorState []byte
}

// ActorFactory is a function that creates a new Actor instance.
type ActorFactory func() Actor

// initConfig is the JSON structure passed by the framework to Init().
// WASM version uses []byte for Args instead of json.RawMessage.
type initConfig struct {
	ActorID   string
	ActorType string
	Role      string
	Args      map[string]string
}

// ActorRouter routes messages to multiple actor types within a single WASM module.
type ActorRouter struct {
	BaseActor
	factories   map[string]ActorFactory
	definitions map[string]ActorDefinition
	active      Actor
	actorID     string
	factoryKey  string
}

// NewActorRouter creates a new multi-actor router.
func NewActorRouter() *ActorRouter {
	r := &ActorRouter{
		factories:   make(map[string]ActorFactory),
		definitions: make(map[string]ActorDefinition),
	}
	r.SetSelf(r)
	return r
}

// Route registers an actor factory under an exact key.
func (r *ActorRouter) Route(key string, factory ActorFactory) {
	r.RouteDefinition(key, DefineActor(factory))
}

// RouteDefinition registers an actor definition with explicit behavior/facet metadata.
func (r *ActorRouter) RouteDefinition(key string, definition ActorDefinition) {
	r.factories[key] = definition.Factory
	r.definitions[key] = definition
}

// Definition returns the registered actor definition for a key.
func (r *ActorRouter) Definition(key string) (ActorDefinition, bool) {
	definition, ok := r.definitions[key]
	return definition, ok
}

// Init selects and initializes the correct actor from the framework-supplied config.
// Uses WasmJSON helpers instead of encoding/json to avoid TinyGo WASM function-table corruption.
func (r *ActorRouter) Init(configJSON string) string {
	actorType := WasmJSONExtractStr(configJSON, "actor_type")
	role := WasmJSONExtractStr(configJSON, "role")
	r.actorID = WasmJSONExtractStr(configJSON, "actor_id")

	// 1. actor_type — exact match
	if actorType != "" {
		if factory, ok := r.factories[actorType]; ok {
			r.factoryKey = actorType
			r.active = factory()
			return r.active.Init(configJSON)
		}
	}

	// 2. role — exact match
	if role != "" {
		if factory, ok := r.factories[role]; ok {
			r.factoryKey = role
			r.active = factory()
			return r.active.Init(configJSON)
		}
	}

	// 3. role — prefix match
	if role != "" {
		for key, factory := range r.factories {
			if strings.HasPrefix(role, key) {
				r.factoryKey = key
				r.active = factory()
				return r.active.Init(configJSON)
			}
		}
	}

	return "ERROR: no actor registered for actor_type='" + actorType + "' role='" + role + "'"
}

// Handle delegates to the active actor.
func (r *ActorRouter) Handle(fromActor, msgType, payloadJSON string) string {
	if r.active == nil {
		return `{"error":"no active actor (init not called)"}`
	}
	return r.active.Handle(fromActor, msgType, payloadJSON)
}

// GetState serializes router + actor state without encoding/json.
func (r *ActorRouter) GetState() string {
	if r.active == nil {
		return "{}"
	}
	actorState := r.active.GetState()
	var sb strings.Builder
	sb.WriteString(`{"_factory_key":`)
	writeJSONString(&sb, r.factoryKey)
	sb.WriteString(`,"_actor_id":`)
	writeJSONString(&sb, r.actorID)
	sb.WriteString(`,"_actor_state":`)
	sb.WriteString(actorState)
	sb.WriteByte('}')
	return sb.String()
}

// SetState restores router + actor state without encoding/json.
func (r *ActorRouter) SetState(stateJSON string) string {
	factoryKey := WasmJSONExtractStr(stateJSON, "_factory_key")
	if factoryKey != "" {
		r.factoryKey = factoryKey
		r.actorID = WasmJSONExtractStr(stateJSON, "_actor_id")
		if r.active == nil {
			if factory, ok := r.factories[factoryKey]; ok {
				r.active = factory()
			}
		}
		if r.active == nil {
			return "ERROR: no factory registered for key '" + factoryKey + "'"
		}
		actorStateStr := WasmJSONFindObject(stateJSON, "_actor_state")
		if actorStateStr == "" {
			actorStateStr = stateJSON
		}
		return r.active.SetState(actorStateStr)
	}
	// Legacy format
	if r.active == nil {
		return "ERROR: no active actor (set_state before init and no router envelope in state)"
	}
	return r.active.SetState(stateJSON)
}

// Run delegates workflow execution to the active actor.
func (r *ActorRouter) Run(payloadJSON string) string {
	if r.active == nil {
		return `{"error":"no active actor (init not called)"}`
	}
	if workflow, ok := r.active.(WorkflowActor); ok {
		return workflow.Run(payloadJSON)
	}
	return `{"error":"active actor does not implement workflow behavior"}`
}

// Signal delegates workflow signals to the active actor.
func (r *ActorRouter) Signal(name, payloadJSON string) {
	if r.active == nil {
		return
	}
	if workflow, ok := r.active.(WorkflowActor); ok {
		workflow.Signal(name, payloadJSON)
	}
}

// Query delegates workflow queries to the active actor.
func (r *ActorRouter) Query(name, payloadJSON string) string {
	if r.active == nil {
		return `{"error":"no active actor (init not called)"}`
	}
	if workflow, ok := r.active.(WorkflowActor); ok {
		return workflow.Query(name, payloadJSON)
	}
	return `{"error":"active actor does not implement workflow behavior"}`
}
