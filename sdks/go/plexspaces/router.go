// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// PlexSpaces Go SDK - Multi-Actor Router
//
// Dispatch rule (matches Python and TypeScript SDKs):
//   1. config.actor_type — exact lookup (primary; always set by the framework).
//   2. config.role       — exact lookup (same-actor_type multi-instance only,
//                          e.g. "ephemeral"/"channel" both map to the same struct).
//   3. Error — no silent fallback; a missing registration is always a bug.
//
// Example (normal case — distinct actor types):
//
//	func main() {
//	    router := plexspaces.NewActorRouter()
//	    router.Route("ChatRoom",     func() plexspaces.Actor { return &ChatRoom{} })
//	    router.Route("RateLimiter",  func() plexspaces.Actor { return &RateLimiter{} })
//	    plexspaces.Register(router)
//	}
//
// Example (same-type multi-instance — register role names too):
//
//	router.Route("AbstractionsActor", func() plexspaces.Actor { return &AbstractionsActor{} })
//	router.Route("ephemeral",         func() plexspaces.Actor { return &AbstractionsActor{} })

package plexspaces

import (
	"encoding/json"
)

// ActorFactory is a function that creates a new Actor instance.
type ActorFactory func() Actor

// initConfig is the JSON structure passed by the framework to Init().
type initConfig struct {
	ActorID   string          `json:"actor_id"`
	ActorType string          `json:"actor_type"`
	Role      string          `json:"role"`
	Args      json.RawMessage `json:"args"`
}

// ActorRouter routes messages to multiple actor types within a single WASM module.
type ActorRouter struct {
	BaseActor
	factories   map[string]ActorFactory
	definitions map[string]ActorDefinition
	active      Actor
	actorID     string
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
// Use the class name for normal dispatch (e.g. "ChatRoom").
// Use a role name for same-class multi-instance dispatch (e.g. "ephemeral").
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
func (r *ActorRouter) Init(configJSON string) string {
	var config initConfig
	if err := json.Unmarshal([]byte(configJSON), &config); err != nil {
		return "ERROR: failed to parse config: " + err.Error()
	}
	r.actorID = config.ActorID

	// 1. actor_type — exact match (primary dispatch key, always set by framework)
	if config.ActorType != "" {
		if factory, ok := r.factories[config.ActorType]; ok {
			r.active = factory()
			return r.active.Init(configJSON)
		}
	}

	// 2. role — exact match (same-actor_type multi-instance only)
	if config.Role != "" {
		if factory, ok := r.factories[config.Role]; ok {
			r.active = factory()
			return r.active.Init(configJSON)
		}
	}

	return "ERROR: no actor registered for actor_type='" + config.ActorType + "' role='" + config.Role + "'"
}

// Handle delegates to the active actor.
func (r *ActorRouter) Handle(fromActor, msgType, payloadJSON string) string {
	if r.active == nil {
		return `{"error":"no active actor (init not called)"}`
	}
	return r.active.Handle(fromActor, msgType, payloadJSON)
}

// GetState delegates to the active actor.
func (r *ActorRouter) GetState() string {
	if r.active == nil {
		return "{}"
	}
	return r.active.GetState()
}

// SetState delegates to the active actor.
func (r *ActorRouter) SetState(stateJSON string) string {
	if r.active == nil {
		return "ERROR: no active actor"
	}
	return r.active.SetState(stateJSON)
}

// Run delegates workflow execution to the active actor when it supports WorkflowActor.
func (r *ActorRouter) Run(payloadJSON string) string {
	if r.active == nil {
		return `{"error":"no active actor (init not called)"}`
	}
	if workflow, ok := r.active.(WorkflowActor); ok {
		return workflow.Run(payloadJSON)
	}
	return `{"error":"active actor does not implement workflow behavior"}`
}

// Signal delegates workflow signals to the active actor when it supports WorkflowActor.
func (r *ActorRouter) Signal(name, payloadJSON string) {
	if r.active == nil {
		return
	}
	if workflow, ok := r.active.(WorkflowActor); ok {
		workflow.Signal(name, payloadJSON)
	}
}

// Query delegates workflow queries to the active actor when it supports WorkflowActor.
func (r *ActorRouter) Query(name, payloadJSON string) string {
	if r.active == nil {
		return `{"error":"no active actor (init not called)"}`
	}
	if workflow, ok := r.active.(WorkflowActor); ok {
		return workflow.Query(name, payloadJSON)
	}
	return `{"error":"active actor does not implement workflow behavior"}`
}
