// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// PlexSpaces Go SDK - Multi-Actor Router
//
// Dispatch rule (matches Python and TypeScript SDKs):
//   1. config.role       — exact match (wins for same-module multi-role variants).
//   2. config.actor_type — exact match (the WASM module type name).
//   3. config.role       — prefix match (e.g. "sensor" matches "sensor-dc-zone-a").
//   4. Error — no silent fallback; a missing registration is always a bug.
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
	"strings"
)

// routerStateEnvelope wraps the active actor's state with the router's dispatch
// metadata (factory key, actor ID). This enables the restore-without-init path:
// after WASM re-instantiation, SetState uses the factory_key to recreate the
// correct actor without calling Init() again.
type routerStateEnvelope struct {
	FactoryKey string          `json:"_factory_key"`
	ActorID    string          `json:"_actor_id"`
	ActorState json.RawMessage `json:"_actor_state"`
}

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
	factoryKey  string // which factory key was resolved during Init() — persisted for restore
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

	// 1. actor_type — exact match (the WASM module type name, highest priority)
	if config.ActorType != "" {
		if factory, ok := r.factories[config.ActorType]; ok {
			r.factoryKey = config.ActorType
			r.active = factory()
			return r.active.Init(configJSON)
		}
	}

	// 2. role — exact match (same-module multi-role variants)
	if config.Role != "" {
		if factory, ok := r.factories[config.Role]; ok {
			r.factoryKey = config.Role
			r.active = factory()
			return r.active.Init(configJSON)
		}
	}

	// 3. role — prefix match (e.g. "sensor" matches "sensor-dc-zone-a")
	if config.Role != "" {
		for key, factory := range r.factories {
			if strings.HasPrefix(config.Role, key) {
				r.factoryKey = key
				r.active = factory()
				return r.active.Init(configJSON)
			}
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

// GetState delegates to the active actor, wrapping in a router envelope that includes
// the resolved factory key. This allows SetState to recreate the correct actor after
// re-instantiation without calling Init().
func (r *ActorRouter) GetState() string {
	if r.active == nil {
		return "{}"
	}
	actorState := r.active.GetState()
	envelope := routerStateEnvelope{
		FactoryKey: r.factoryKey,
		ActorID:    r.actorID,
		ActorState: json.RawMessage(actorState),
	}
	data, err := json.Marshal(envelope)
	if err != nil {
		return r.active.GetState()
	}
	return string(data)
}

// SetState restores the router and active actor from the router state envelope.
// When called after re-instantiation (without init), active may be nil. In that
// case, the envelope's factory_key identifies which factory to use — the router
// recreates the actor without calling Init(), avoiding duplicate side effects
// (timer scheduling, PG joins, etc.).
func (r *ActorRouter) SetState(stateJSON string) string {
	var envelope routerStateEnvelope
	if err := json.Unmarshal([]byte(stateJSON), &envelope); err == nil && envelope.FactoryKey != "" {
		// Router envelope format: restore factory key, create actor if needed,
		// then delegate actor state restoration.
		r.factoryKey = envelope.FactoryKey
		r.actorID = envelope.ActorID
		if r.active == nil {
			if factory, ok := r.factories[envelope.FactoryKey]; ok {
				r.active = factory()
			}
		}
		if r.active == nil {
			return "ERROR: no factory registered for key '" + envelope.FactoryKey + "'"
		}
		if envelope.ActorState != nil {
			return r.active.SetState(string(envelope.ActorState))
		}
		return ""
	}
	// Legacy format (pre-router-envelope): delegate directly.
	if r.active == nil {
		return "ERROR: no active actor (set_state before init and no router envelope in state)"
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
