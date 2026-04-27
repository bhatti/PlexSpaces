// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors

package plexspaces

// BehaviorKind identifies the actor behavior in a cross-language-consistent way.
type BehaviorKind string

const (
	BehaviorActor         BehaviorKind = "GenServer"
	BehaviorGenServer     BehaviorKind = "GenServer"
	BehaviorEventActor    BehaviorKind = "GenEvent"
	BehaviorFSMActor      BehaviorKind = "GenStateMachine"
	BehaviorWorkflowActor BehaviorKind = "Workflow"
)

// ActorDefinition captures authoring metadata alongside the actor factory.
type ActorDefinition struct {
	BehaviorType BehaviorKind
	Facets       []string
	Factory      ActorFactory
	// FSMStates lists the valid states for a GenStateMachine actor.
	// Provided for self-documentation and startup validation; ignored by other behavior types.
	FSMStates []string
	// FSMInitial is the initial state name for a GenStateMachine actor.
	// If non-empty, the runtime sets fsm_state to this value on first activation.
	FSMInitial string
}

// FSMOpts holds optional state-machine configuration for FSMActorDef.
type FSMOpts struct {
	// States lists all valid state names (for documentation and validation).
	States []string
	// Initial is the starting state name.
	Initial string
	// Facets lists optional PlexSpaces facets (e.g., "durability", "timer").
	Facets []string
}

func makeDefinition(behavior BehaviorKind, factory ActorFactory, facets []string) ActorDefinition {
	copied := append([]string(nil), facets...)
	return ActorDefinition{
		BehaviorType: behavior,
		Facets:       copied,
		Factory:      factory,
	}
}

// DefineActor declares the default GenServer-style actor definition.
func DefineActor(factory ActorFactory, facets ...string) ActorDefinition {
	return makeDefinition(BehaviorActor, factory, facets)
}

// GenServerActor declares a GenServer actor definition.
func GenServerActor(factory ActorFactory, facets ...string) ActorDefinition {
	return makeDefinition(BehaviorGenServer, factory, facets)
}

// EventActor declares an event-handler actor definition.
func EventActor(factory ActorFactory, facets ...string) ActorDefinition {
	return makeDefinition(BehaviorEventActor, factory, facets)
}

// FSMActor declares a state-machine actor definition.
func FSMActor(factory ActorFactory, facets ...string) ActorDefinition {
	return makeDefinition(BehaviorFSMActor, factory, facets)
}

// FSMActorDef declares a state-machine actor definition with explicit state configuration.
//
// Example:
//
//	router.RouteDefinition("my_fsm", plexspaces.FSMActorDef(
//	    NewMyFSM,
//	    plexspaces.FSMOpts{
//	        States:  []string{"idle", "running", "done"},
//	        Initial: "idle",
//	        Facets:  []string{"durability"},
//	    },
//	))
func FSMActorDef(factory ActorFactory, opts FSMOpts) ActorDefinition {
	def := makeDefinition(BehaviorFSMActor, factory, opts.Facets)
	if len(opts.States) > 0 {
		def.FSMStates = append([]string(nil), opts.States...)
	}
	def.FSMInitial = opts.Initial
	return def
}

// WorkflowActorDefinition declares a workflow actor definition.
func WorkflowActorDefinition(factory ActorFactory, facets ...string) ActorDefinition {
	return makeDefinition(BehaviorWorkflowActor, factory, facets)
}
