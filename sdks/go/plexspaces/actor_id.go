// SPDX-License-Identifier: LGPL-2.1-or-later
// ActorID — parsed representation of a canonical PlexSpaces actor ID.
//
// Canonical format: {name}//{actor_type}::{namespace}@{node_id}
//
// The name is typically a ULID for supervisor-spawned actors, but can be a
// well-known string (e.g. the role name) when routing via virtual_actor type lookup.
//
// Usage — peer/sibling discovery from within a WASM actor:
//
//	self, err := plexspaces.ParseActorID(host.SelfID())
//	// Use PG for accurate canonical IDs (recommended for production):
//	members, _ := host.PG().Members("svc:budget_manager")
//	budgetID := members[0]
//
// The ActorID type is provided for convenience when you need to inspect or
// manipulate the components of a canonical ID string.
package plexspaces

import (
	"fmt"
	"strings"
)

// ActorID is the parsed form of a canonical PlexSpaces actor ID.
//
// Canonical string format: {Name}//{ActorType}::{Namespace}@{NodeID}
type ActorID struct {
	// Name is the unique instance identifier (usually a ULID for supervisor-spawned actors).
	Name string
	// ActorType is the behavior type registered in the application (e.g. "budget_manager").
	// For supervisor children, ActorType equals the child's config id field.
	ActorType string
	// Namespace is the application namespace (e.g. "go-resource-aware-inference").
	Namespace string
	// NodeID is the node hosting the actor (e.g. "test-node-8091").
	NodeID string
}

// ParseActorID parses a canonical actor ID string into an ActorID struct.
//
// Expected format: {name}//{actor_type}::{namespace}@{node_id}
// Returns an error if the string does not contain the expected separators.
func ParseActorID(id string) (ActorID, error) {
	// Must contain "//"
	slashIdx := strings.Index(id, "//")
	if slashIdx < 0 {
		return ActorID{}, fmt.Errorf("ParseActorID: missing '//' in %q", id)
	}
	name := id[:slashIdx]
	rest := id[slashIdx+2:] // "{actor_type}::{namespace}@{node_id}"

	// Split on "@" to separate namespace::type from node_id
	atParts := strings.SplitN(rest, "@", 2)
	nodeID := ""
	if len(atParts) == 2 {
		nodeID = atParts[1]
	}
	typeNS := atParts[0] // "{actor_type}::{namespace}"

	// Split on "::" to separate actor_type from namespace
	colonParts := strings.SplitN(typeNS, "::", 2)
	actorType := colonParts[0]
	namespace := ""
	if len(colonParts) == 2 {
		namespace = colonParts[1]
	}

	return ActorID{
		Name:      name,
		ActorType: actorType,
		Namespace: namespace,
		NodeID:    nodeID,
	}, nil
}

// String returns the canonical actor ID string: {Name}//{ActorType}::{Namespace}@{NodeID}
func (a ActorID) String() string {
	if a.NodeID != "" {
		return fmt.Sprintf("%s//%s::%s@%s", a.Name, a.ActorType, a.Namespace, a.NodeID)
	}
	return fmt.Sprintf("%s//%s::%s", a.Name, a.ActorType, a.Namespace)
}

// WithTypeAndName returns a copy with an explicit actor type and name.
// Use this to build a canonical ID for a peer actor with the given type and name,
// keeping the same namespace and node.
//
// For supervisor-spawned actors with stable role names (name == type == role):
//
//	peer := self.WithTypeAndName("budget_manager", "budget_manager")
//
// For actors where name and type differ (e.g. ULID-named workers of a shared type):
//
//	peer := self.WithTypeAndName("inference_worker", ulid)
func (a ActorID) WithTypeAndName(actorType, name string) ActorID {
	return ActorID{
		Name:      name,
		ActorType: actorType,
		Namespace: a.Namespace,
		NodeID:    a.NodeID,
	}
}

// WithName returns a copy of this ActorID with a different Name.
// Useful when the actor_type and namespace are known but the name needs to change.
func (a ActorID) WithName(name string) ActorID {
	a.Name = name
	return a
}

// WithType returns a copy of this ActorID with a different Name and ActorType.
func (a ActorID) WithType(name, actorType string) ActorID {
	a.Name = name
	a.ActorType = actorType
	return a
}
