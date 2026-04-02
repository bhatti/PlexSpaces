// SPDX-License-Identifier: LGPL-2.1-or-later
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

// WorkflowActorDefinition declares a workflow actor definition.
func WorkflowActorDefinition(factory ActorFactory, facets ...string) ActorDefinition {
	return makeDefinition(BehaviorWorkflowActor, factory, facets)
}
