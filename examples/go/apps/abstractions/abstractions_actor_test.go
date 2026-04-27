// SPDX-License-Identifier: AGPL-3.0-or-later

package main

import (
	"testing"

	"github.com/plexobject/plexspaces/sdks/go/plexspaces"
)

func TestAbstractionsActorStateRoundTrip(t *testing.T) {
	actor := newAbstractionsActor()
	if got := actor.Init(`{"actor_id":"cart-1//abstractions::abstractions-go@test-node","args":{"role":"abstractions"}}`); got != "" {
		t.Fatalf("Init() = %q", got)
	}
	if got := actor.Handle("caller", "increment", `{"amount":2}`); got == "" {
		t.Fatal("increment returned empty payload")
	}

	state := actor.GetState()
	restored := newAbstractionsActor()
	if got := restored.SetState(state); got != "" {
		t.Fatalf("SetState() = %q", got)
	}
	if restored.Count != 2 {
		t.Fatalf("restored.Count = %d", restored.Count)
	}
}

func TestEphemeralInitConfigResetsCount(t *testing.T) {
	actor := newAbstractionsActor()
	if got := actor.Init(`{"actor_id":"session-1//ephemeral::abstractions-go@test-node","args":{"role":"ephemeral","initial_count":"5"}}`); got != "" {
		t.Fatalf("Init() = %q", got)
	}
	actor.Count = 7

	reactivated := newAbstractionsActor()
	if got := reactivated.Init(`{"actor_id":"session-1//ephemeral::abstractions-go@test-node","args":{"role":"ephemeral","initial_count":"5"}}`); got != "" {
		t.Fatalf("Init(reactivated) = %q", got)
	}
	if reactivated.Count != 5 {
		t.Fatalf("reactivated.Count = %d", reactivated.Count)
	}
}

func TestWorkflowLifecycle(t *testing.T) {
	actor := newAbstractionsActor()
	if got := actor.Run(`{"order_id":"o-1"}`); got != `{"status":"running:o-1"}` {
		t.Fatalf("Run() = %q", got)
	}
	actor.Signal("cancel", `{"reason":"user"}`)
	if got := actor.Query("status", `{}`); got != `{"signals":["cancel:user"],"status":"cancelled"}` &&
		got != `{"status":"cancelled","signals":["cancel:user"]}` {
		t.Fatalf("Query() = %q", got)
	}
}

func TestChannelPublish(t *testing.T) {
	channel := newAbstractionsActor()
	channel.Role = "channel"
	if got := channel.Handle("publisher", "publish", `{"channel":"alerts","body":"direct"}`); got != "{}" {
		t.Fatalf("publish = %q", got)
	}
	if len(channel.Received) != 1 || channel.Received[0] != "alerts:direct" {
		t.Fatalf("Received = %#v", channel.Received)
	}
}

func TestHostContractHelpers(t *testing.T) {
	plexspaces.ResetStubs()
	if out := host.KVPut("abstractions/config", "ready"); out != "" {
		t.Fatalf("KVPut() = %q", out)
	}
	if err := host.PG().Join(defaultGroup); err != nil {
		t.Fatalf("PG().Join() error = %v", err)
	}
	if result := host.Send("alerts//channel::abstractions-go@test-node", "publish", map[string]any{"channel": "alerts", "body": "direct"}); result != "" {
		t.Fatalf("Send() = %q", result)
	}
	if err := host.PG().Broadcast(defaultGroup, "publish", map[string]any{"channel": "alerts", "body": "broadcast"}); err != nil {
		t.Fatalf("PG().Broadcast() error = %v", err)
	}
	if timerID := host.SendAfter(250, "tick", map[string]any{"kind": "timer"}); timerID == "" {
		t.Fatal("SendAfter() returned empty timer ID")
	}
}

func TestControllerStopUsesHostStop(t *testing.T) {
	plexspaces.ResetStubs()

	controller := newAbstractionsActor()
	if got := controller.Init(`{"actor_id":"01TEST//controller::abstractions-go@test-node","args":{"role":"controller"}}`); got != "" {
		t.Fatalf("Init() = %q", got)
	}

	result := controller.Handle("caller", "stop_actor", `{"actor_id":"session-1//ephemeral::abstractions-go@test-node"}`)
	if result == "" {
		t.Fatal("stop_actor returned empty payload")
	}

	stopped := plexspaces.GetStubStoppedActors()
	if len(stopped) != 1 {
		t.Fatalf("expected exactly one stopped actor, got %d", len(stopped))
	}
	if stopped[0] != "session-1//ephemeral::abstractions-go@test-node" {
		t.Fatalf("stopped actor = %q", stopped[0])
	}
}
