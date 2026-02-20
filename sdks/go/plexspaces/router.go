// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// PlexSpaces Go SDK - Multi-Actor Router
//
// Provides ActorRouter for routing messages to multiple actor types
// within a single WASM module. This is the Go equivalent of the Python
// SDK's ACTOR_ROLES mapping.
//
// Example:
//
//	func main() {
//	    router := plexspaces.NewActorRouter()
//	    router.Register("rate-limiter", func() plexspaces.Actor {
//	        a := &RateLimiter{}
//	        a.SetSelf(a)
//	        return a
//	    })
//	    router.Register("counter", func() plexspaces.Actor {
//	        a := &Counter{}
//	        a.SetSelf(a)
//	        return a
//	    })
//	    plexspaces.Register(router)
//	}

package plexspaces

import (
	"encoding/json"
	"strings"
)

// ActorFactory is a function that creates a new Actor instance.
type ActorFactory func() Actor

// ActorRouter routes messages to multiple actor types within a single
// WASM module. It implements the Actor interface and delegates to the
// appropriate actor based on the actor_id prefix in the init config.
//
// This is the Go equivalent of Python SDK's ACTOR_ROLES dict.
type ActorRouter struct {
	BaseActor
	factories map[string]ActorFactory
	active    Actor  // currently active actor for this instance
	actorID   string // actor ID from init config
}

// NewActorRouter creates a new multi-actor router.
func NewActorRouter() *ActorRouter {
	r := &ActorRouter{
		factories: make(map[string]ActorFactory),
	}
	r.SetSelf(r)
	return r
}

// Route registers an actor factory for a given prefix.
// When the actor_id starts with this prefix, messages are routed to
// an instance created by the factory.
//
// Example:
//
//	router.Route("rate-limiter", func() plexspaces.Actor { ... })
//	// Matches: "rate-limiter", "rate-limiter-0", "rate-limiter-1", etc.
func (r *ActorRouter) Route(prefix string, factory ActorFactory) {
	r.factories[prefix] = factory
}

// Init initializes the router by selecting the appropriate actor type
// based on the actor_id field in the config JSON.
func (r *ActorRouter) Init(configJSON string) string {
	// Parse config to extract actor_id
	var config struct {
		ActorID string                 `json:"actor_id"`
		Args    map[string]interface{} `json:"args"`
	}
	if err := json.Unmarshal([]byte(configJSON), &config); err != nil {
		return "ERROR: failed to parse config: " + err.Error()
	}
	r.actorID = config.ActorID

	// Extract the name part from full actor ID (name:namespace@node)
	name := config.ActorID
	if idx := strings.Index(name, ":"); idx >= 0 {
		name = name[:idx]
	}

	// Find matching factory by prefix (longest match wins)
	var bestPrefix string
	var bestFactory ActorFactory
	for prefix, factory := range r.factories {
		if name == prefix || strings.HasPrefix(name, prefix) {
			if len(prefix) > len(bestPrefix) {
				bestPrefix = prefix
				bestFactory = factory
			}
		}
	}

	if bestFactory == nil {
		return "ERROR: no actor registered for prefix: " + name
	}

	// Create and initialize the actor
	r.active = bestFactory()
	return r.active.Init(configJSON)
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
