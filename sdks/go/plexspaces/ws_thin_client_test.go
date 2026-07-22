// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Unit tests for WsThinClient types and interface contracts.
//
// Full lifecycle tests (connect → register → ping → disconnect → unregistered)
// require a live server and are covered by the Rust integration tests in
// crates/node/tests/suite/ws_integration_tests.rs.

package plexspaces_test

import (
	"context"
	"testing"

	"github.com/bhatti/PlexSpaces/sdks/go/plexspaces"
)

// TestWsThinClientOptions verifies the options struct can be constructed with
// all fields populated — guards against accidental field removal.
func TestWsThinClientOptions_Fields(t *testing.T) {
	opts := plexspaces.WsThinClientOptions{
		WsURL:     "ws://localhost:8091/ws",
		JwtToken:  "test.jwt.token",
		NodeID:    "thin-node-go-01",
		Tenant:    "default",
		Namespace: "test-ns",
	}
	if opts.WsURL == "" {
		t.Error("WsURL must not be empty")
	}
	if opts.Tenant == "" {
		t.Error("Tenant must not be empty")
	}
}

// TestThinNodePingResult_Fields verifies all resource hint fields are present
// and correctly typed on ThinNodePingResult.
func TestThinNodePingResult_Fields(t *testing.T) {
	result := plexspaces.ThinNodePingResult{
		NodeID:            "server-node-1",
		CPUPercent:        23.5,
		MemoryAvailableMB: 4096,
		AvailableCores:    8,
	}
	if result.NodeID != "server-node-1" {
		t.Errorf("NodeID = %q, want server-node-1", result.NodeID)
	}
	if result.CPUPercent != 23.5 {
		t.Errorf("CPUPercent = %v, want 23.5", result.CPUPercent)
	}
	if result.MemoryAvailableMB != 4096 {
		t.Errorf("MemoryAvailableMB = %v, want 4096", result.MemoryAvailableMB)
	}
	if result.AvailableCores != 8 {
		t.Errorf("AvailableCores = %v, want 8", result.AvailableCores)
	}
}

// TestNewWsThinClient verifies the constructor returns a non-nil client.
func TestNewWsThinClient_NonNil(t *testing.T) {
	c := plexspaces.NewWsThinClient(plexspaces.WsThinClientOptions{
		WsURL: "ws://localhost:8091/ws",
	})
	if c == nil {
		t.Fatal("NewWsThinClient must return non-nil client")
	}
}

// TestWsThinClient_Connect_ReturnsError verifies the stub returns an error
// (not implemented) rather than panicking or hanging.
func TestWsThinClient_Connect_ReturnsError(t *testing.T) {
	c := plexspaces.NewWsThinClient(plexspaces.WsThinClientOptions{
		WsURL: "ws://127.0.0.1:0/ws",
	})
	_, err := c.Connect(context.Background())
	if err == nil {
		t.Fatal("Connect on stub must return an error")
	}
}

// TestWsThinClient_Tell_ReturnsError verifies the stub returns an error.
func TestWsThinClient_Tell_ReturnsError(t *testing.T) {
	c := plexspaces.NewWsThinClient(plexspaces.WsThinClientOptions{})
	err := c.Tell("actor//T::ns@node", "ping", nil)
	if err == nil {
		t.Fatal("Tell on stub must return an error")
	}
}

// TestWsThinClient_Ask_ReturnsError verifies the stub returns an error.
func TestWsThinClient_Ask_ReturnsError(t *testing.T) {
	c := plexspaces.NewWsThinClient(plexspaces.WsThinClientOptions{})
	_, err := c.Ask(context.Background(), "actor//T::ns@node", "get", nil)
	if err == nil {
		t.Fatal("Ask on stub must return an error")
	}
}

// TestWsThinClient_PingNode_ReturnsError verifies the stub returns an error.
func TestWsThinClient_PingNode_ReturnsError(t *testing.T) {
	c := plexspaces.NewWsThinClient(plexspaces.WsThinClientOptions{})
	_, err := c.PingNode(context.Background(), "target-node")
	if err == nil {
		t.Fatal("PingNode on stub must return an error")
	}
}

// TestWsThinClient_NodeID_Empty verifies NodeID returns empty on stub.
func TestWsThinClient_NodeID_Empty(t *testing.T) {
	c := plexspaces.NewWsThinClient(plexspaces.WsThinClientOptions{})
	if id := c.NodeID(); id != "" {
		t.Errorf("NodeID on stub must return empty, got %q", id)
	}
}
