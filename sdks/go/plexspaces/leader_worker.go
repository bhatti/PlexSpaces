// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Leader-worker client for multi-node: same API surface as Rust/Python/TypeScript SDKs.
// Virtual actors are created lazily on first message; use SpawnActorOnNode
// only for non-virtual workers.
//
//go:build !wasm

package plexspaces

import (
	"bytes"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"regexp"
	"strconv"
	"strings"
)

// LeaderWorkerClient is the client for leader-worker multi-node patterns.
// Connect to the entry (leader) node via HTTP; list worker node IDs and spawn
// non-virtual actors on specific nodes. Virtual actors are created lazily on
// first message—no explicit spawn or ensure.
type LeaderWorkerClient struct {
	entryURL      string
	nodeIDToHTTP  map[string]string
}

// NewLeaderWorkerClient creates a client for the entry/leader node.
// entryHTTPURL is the base URL of the entry node (e.g. "http://localhost:8092").
func NewLeaderWorkerClient(entryHTTPURL string) *LeaderWorkerClient {
	entryURL := strings.TrimSuffix(entryHTTPURL, "/")
	return &LeaderWorkerClient{
		entryURL:     entryURL,
		nodeIDToHTTP: make(map[string]string),
	}
}

var grpcPortRE = regexp.MustCompile(`:(\d+)\s*$`)

func grpcPortToHTTP(addr string) string {
	if addr == "" {
		return addr
	}
	addr = strings.TrimSpace(addr)
	sub := grpcPortRE.FindStringSubmatch(addr)
	if len(sub) != 2 {
		return addr
	}
	port, _ := strconv.Atoi(sub[1])
	port++
	return grpcPortRE.ReplaceAllString(addr, fmt.Sprintf(":%d", port))
}

// nodeRegistration is the JSON shape for list nodes response (camelCase or snake_case).
type nodeRegistration struct {
	NodeID      string `json:"nodeId"`
	NodeIDSnake string `json:"node_id"`
	NodeAddress string `json:"nodeAddress"`
	NodeAddressSnake string `json:"node_address"`
}

// listNodesResponse is the JSON shape for GET /api/v1/nodes.
type listNodesResponse struct {
	Nodes              []nodeRegistration `json:"nodes"`
	NodeRegistrations  []nodeRegistration `json:"nodeRegistrations"`
}

// ListWorkerNodeIds lists node IDs that can run workers (peers + self).
// It populates the internal cache so SpawnActorOnNode can resolve nodeID to HTTP URL.
// cluster is optional; pageSize defaults to 100.
func (c *LeaderWorkerClient) ListWorkerNodeIds(cluster string, pageSize int, pageToken string) ([]string, error) {
	if pageSize <= 0 {
		pageSize = 100
	}
	params := url.Values{}
	params.Set("pageSize", fmt.Sprintf("%d", pageSize))
	if cluster != "" {
		params.Set("cluster", cluster)
	}
	if pageToken != "" {
		params.Set("pageToken", pageToken)
	}
	reqURL := c.entryURL + "/api/v1/nodes?" + params.Encode()
	req, err := http.NewRequest(http.MethodGet, reqURL, nil)
	if err != nil {
		return nil, err
	}
	req.Header.Set("Accept", "application/json")
	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		body, _ := io.ReadAll(resp.Body)
		return nil, fmt.Errorf("list_worker_node_ids failed: %d %s", resp.StatusCode, string(body))
	}
	var data listNodesResponse
	if err := json.NewDecoder(resp.Body).Decode(&data); err != nil {
		return nil, err
	}
	nodes := data.Nodes
	if len(nodes) == 0 {
		nodes = data.NodeRegistrations
	}
	ids := make([]string, 0, len(nodes))
	for _, n := range nodes {
		nodeID := n.NodeID
		if nodeID == "" {
			nodeID = n.NodeIDSnake
		}
		if nodeID == "" {
			continue
		}
		ids = append(ids, nodeID)
		addr := n.NodeAddress
		if addr == "" {
			addr = n.NodeAddressSnake
		}
		if addr != "" {
			c.nodeIDToHTTP[nodeID] = grpcPortToHTTP(addr)
		}
	}
	return ids, nil
}

// SpawnActorRequest is the JSON body for POST /api/v1/actors/spawn.
type spawnActorRequest struct {
	ActorType     string            `json:"actorType"`
	ActorID       string            `json:"actorId,omitempty"`
	InitialState  string            `json:"initialState,omitempty"` // base64
	Config        map[string]any    `json:"config,omitempty"`
	Labels        map[string]string `json:"labels,omitempty"`
}

// SpawnActorResponse is the JSON response from spawn.
type spawnActorResponse struct {
	ActorRef     string `json:"actorRef"`
	ActorRefSnake string `json:"actor_ref"`
}

// SpawnActorOnNode spawns a non-virtual actor on the given node.
// The node must be known; call ListWorkerNodeIds first.
// Returns the actor ref (e.g. "worker-ulid@node_id").
func (c *LeaderWorkerClient) SpawnActorOnNode(
	nodeID string,
	actorType string,
	actorID string,
	initialState []byte,
	config map[string]any,
	labels map[string]string,
) (string, error) {
	nodeHTTP, ok := c.nodeIDToHTTP[nodeID]
	if !ok {
		return "", fmt.Errorf("unknown node_id %q; call ListWorkerNodeIds first", nodeID)
	}
	reqBody := spawnActorRequest{ActorType: actorType, ActorID: actorID}
	if len(initialState) > 0 {
		reqBody.InitialState = base64.StdEncoding.EncodeToString(initialState)
	}
	if config != nil {
		reqBody.Config = config
	}
	if labels != nil {
		reqBody.Labels = labels
	}
	body, err := json.Marshal(reqBody)
	if err != nil {
		return "", err
	}
	reqURL := strings.TrimSuffix(nodeHTTP, "/") + "/api/v1/actors/spawn"
	req, err := http.NewRequest(http.MethodPost, reqURL, bytes.NewReader(body))
	if err != nil {
		return "", err
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Accept", "application/json")
	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		return "", err
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		respBody, _ := io.ReadAll(resp.Body)
		return "", fmt.Errorf("spawn_actor_on_node failed: %d %s", resp.StatusCode, string(respBody))
	}
	var out spawnActorResponse
	if err := json.NewDecoder(resp.Body).Decode(&out); err != nil {
		return "", err
	}
	actorRef := out.ActorRef
	if actorRef == "" {
		actorRef = out.ActorRefSnake
	}
	if actorRef == "" {
		return "", fmt.Errorf("spawn_actor_on_node returned empty actorRef")
	}
	return actorRef, nil
}

// ListWorkerNodeIds is a convenience that lists worker node IDs using a one-off client.
// For multiple calls or SpawnActorOnNode, use LeaderWorkerClient.
func ListWorkerNodeIds(entryHTTPURL string, cluster string, pageSize int) ([]string, error) {
	client := NewLeaderWorkerClient(entryHTTPURL)
	return client.ListWorkerNodeIds(cluster, pageSize, "")
}
