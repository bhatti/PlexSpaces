// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// WsThinClient — Go thin-node WebSocket client skeleton.
//
// TODO: Implement WsFrame protobuf encoding over gorilla/websocket or
//       the standard library net/http WebSocket (Go 1.24+).
//
// Wire protocol: proto/plexspaces/v1/transport/websocket.proto
// Reference implementation: sdks/typescript/src/ws_thin_client.ts

package plexspaces

import (
	"context"
	"errors"
)

// WsThinClientOptions configures a thin-node WebSocket connection.
type WsThinClientOptions struct {
	// WebSocket URL, e.g. "ws://localhost:8091/ws"
	WsURL string
	// JWT Bearer token. Appended as ?token=<jwt> if non-empty.
	JwtToken string
	// ULID preferred. Server assigns one if empty or collision detected.
	NodeID    string
	Tenant    string
	Namespace string
}

// ThinNodePingResult carries resource hints from a PingResponse.
// Corresponds to proto fields 9–11 of PingResponse in node.proto.
type ThinNodePingResult struct {
	NodeID            string
	CPUPercent        float32
	MemoryAvailableMB uint64
	AvailableCores    uint32
}

// WsThinClient is a thin-node WebSocket client.
// All methods return errors until the TODO implementation is complete.
type WsThinClient struct{}

// NewWsThinClient returns a WsThinClient configured with opts.
func NewWsThinClient(_ WsThinClientOptions) *WsThinClient {
	return &WsThinClient{}
}

// Connect opens the WebSocket and completes the NodeRegistration handshake.
// Returns the server-assigned node_id on success.
func (c *WsThinClient) Connect(_ context.Context) (string, error) {
	return "", errors.New("TODO: implement WsFrame protobuf encoding over WebSocket")
}

// Tell sends a fire-and-forget message to a canonical actor ID.
func (c *WsThinClient) Tell(_, _ string, _ any) error {
	return errors.New("TODO")
}

// Ask sends a request-reply message and returns the response payload.
func (c *WsThinClient) Ask(_ context.Context, _, _ string, _ any) (any, error) {
	return nil, errors.New("TODO")
}

// OnMessage registers a handler for incoming tell frames addressed to this thin node.
func (c *WsThinClient) OnMessage(_ func(actorID, msgType string, payload any)) {}

// NodeID returns the server-assigned node_id (available after Connect).
func (c *WsThinClient) NodeID() string { return "" }

// LocalActorID builds a canonical actor ID on this thin node.
// Format: {name}//{type}::{namespace}@{nodeId}
func (c *WsThinClient) LocalActorID(name, actorType, namespace string) string {
	return name + "//" + actorType + "::" + namespace + "@" + c.NodeID()
}

// PingNode sends a SWIM-compatible ping and returns resource hints from the response.
func (c *WsThinClient) PingNode(_ context.Context, _ string) (ThinNodePingResult, error) {
	return ThinNodePingResult{}, errors.New("TODO")
}

// Heartbeat sends a heartbeat frame to keep the WS session alive.
func (c *WsThinClient) Heartbeat() error { return errors.New("TODO") }

// Disconnect closes the WebSocket connection.
func (c *WsThinClient) Disconnect() error { return errors.New("TODO") }
