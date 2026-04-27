// SPDX-License-Identifier: AGPL-3.0-or-later
package plexspaces_test

import (
	"testing"

	"github.com/plexobject/plexspaces/sdks/go/plexspaces"
)

func TestParseActorID_Full(t *testing.T) {
	id := "01KP8WMBRKP6KGQTARATQQ1H5M//agent_registry::go-a2a-multi-agent@test-node-8091"
	a, err := plexspaces.ParseActorID(id)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if a.Name != "01KP8WMBRKP6KGQTARATQQ1H5M" {
		t.Errorf("Name = %q, want ULID", a.Name)
	}
	if a.ActorType != "agent_registry" {
		t.Errorf("ActorType = %q, want agent_registry", a.ActorType)
	}
	if a.Namespace != "go-a2a-multi-agent" {
		t.Errorf("Namespace = %q, want go-a2a-multi-agent", a.Namespace)
	}
	if a.NodeID != "test-node-8091" {
		t.Errorf("NodeID = %q, want test-node-8091", a.NodeID)
	}
	if a.String() != id {
		t.Errorf("String() = %q, want %q", a.String(), id)
	}
}

func TestParseActorID_NoNode(t *testing.T) {
	id := "myname//mytype::mynamespace"
	a, err := plexspaces.ParseActorID(id)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if a.Name != "myname" {
		t.Errorf("Name = %q", a.Name)
	}
	if a.ActorType != "mytype" {
		t.Errorf("ActorType = %q", a.ActorType)
	}
	if a.Namespace != "mynamespace" {
		t.Errorf("Namespace = %q", a.Namespace)
	}
	if a.NodeID != "" {
		t.Errorf("NodeID = %q, want empty", a.NodeID)
	}
	if a.String() != id {
		t.Errorf("String() = %q, want %q", a.String(), id)
	}
}

func TestParseActorID_MissingSlashes(t *testing.T) {
	_, err := plexspaces.ParseActorID("noslashes")
	if err == nil {
		t.Error("expected error for missing //")
	}
}

func TestActorID_WithTypeAndName(t *testing.T) {
	self, _ := plexspaces.ParseActorID("01KP//routing_workflow::go-resource-aware-inference@test-node-8091")
	peer := self.WithTypeAndName("budget_manager", "budget_manager")
	want := "budget_manager//budget_manager::go-resource-aware-inference@test-node-8091"
	if peer.String() != want {
		t.Errorf("WithTypeAndName() = %q, want %q", peer.String(), want)
	}
}

func TestActorID_WithTypeAndName_DifferentNameAndType(t *testing.T) {
	self, _ := plexspaces.ParseActorID("01KP//routing_workflow::go-resource-aware-inference@test-node-8091")
	peer := self.WithTypeAndName("inference_worker", "01KP8WORKER1")
	want := "01KP8WORKER1//inference_worker::go-resource-aware-inference@test-node-8091"
	if peer.String() != want {
		t.Errorf("WithTypeAndName() = %q, want %q", peer.String(), want)
	}
}

func TestActorID_WithName(t *testing.T) {
	self, _ := plexspaces.ParseActorID("01KP//worker::ns@node")
	got := self.WithName("newname").String()
	want := "newname//worker::ns@node"
	if got != want {
		t.Errorf("WithName() = %q, want %q", got, want)
	}
}

func TestActorID_WithType(t *testing.T) {
	self, _ := plexspaces.ParseActorID("01KP//worker::ns@node")
	got := self.WithType("other", "other_type").String()
	want := "other//other_type::ns@node"
	if got != want {
		t.Errorf("WithType() = %q, want %q", got, want)
	}
}
