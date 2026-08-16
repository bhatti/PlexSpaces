// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// CSP Structured Concurrency — PlexSpaces Actor (Go WASM)
//
// Scatter-gather pattern using supervised actors + tuplespace coordination.
// Contrast with native/main.go which shows raw Go CSP.

package main

import (
	"encoding/json"
	"fmt"
	"strings"

	"github.com/bhatti/PlexSpaces/sdks/go/plexspaces"
)

var h = plexspaces.NewHost()

type cspActor struct {
	plexspaces.BaseActor

	ActorID          string `json:"actor_id"`
	Role             string `json:"role"`
	RequestCount     int    `json:"request_count"`
	ResultsCollected int    `json:"results_collected"`
}

func newCSPActor() *cspActor {
	a := &cspActor{}
	a.SetSelf(a)
	return a
}

func (a *cspActor) Init(configJSON string) string {
	var cfg struct {
		ActorID string         `json:"actor_id"`
		Role    string         `json:"role"`
		Args    map[string]any `json:"args"`
	}
	if err := json.Unmarshal([]byte(configJSON), &cfg); err != nil {
		return "ERROR: " + err.Error()
	}
	a.ActorID = cfg.ActorID
	a.Role = cfg.Role
	if a.Role == "" {
		if r, ok := cfg.Args["role"].(string); ok {
			a.Role = r
		} else {
			a.Role = "orchestrator"
		}
	}
	h.Log("info", fmt.Sprintf("CSP actor initialized: %s (role=%s)", a.ActorID, a.Role))
	return ""
}

func (a *cspActor) Handle(from, msgType, payloadJSON string) string {
	var payload map[string]any
	if err := json.Unmarshal([]byte(payloadJSON), &payload); err != nil {
		return jsonError("invalid payload: " + err.Error())
	}

	op, _ := payload["op"].(string)
	if op == "" {
		op = msgType
	}

	switch op {
	case "scatter_gather":
		return a.handleScatterGather(payload)
	case "collect_results":
		return a.handleCollectResults(payload)
	case "process":
		return a.handleWorkerProcess(payload)
	case "status":
		return a.handleStatus()
	default:
		return jsonError("unknown op: " + op)
	}
}

func (a *cspActor) handleScatterGather(payload map[string]any) string {
	requestID, _ := payload["request_id"].(string)
	if requestID == "" {
		requestID = "req-001"
	}
	numServices := intFromPayload(payload, "num_services", 5)
	firstK := intFromPayload(payload, "first_k", 3)
	timeoutMs := intFromPayload(payload, "timeout_ms", 300)

	workerIDs := make([]string, 0, numServices)

	for i := 0; i < numServices; i++ {
		workerID := fmt.Sprintf("csp-worker-%s-%d", requestID, i)
		args := map[string]string{
			"role":      "worker",
			"worker_id": fmt.Sprintf("%d", i),
		}
		spawnedID, err := h.Spawn("csp_structured", workerID, "", args)
		if err != nil {
			return jsonError(fmt.Sprintf("spawn worker-%d: %v", i, err))
		}
		workerIDs = append(workerIDs, spawnedID)

		// Tell worker to process
		workMsg := map[string]any{
			"op":         "process",
			"request_id": requestID,
			"service_id": i,
		}
		h.Send(spawnedID, "cast", workMsg)
	}

	// Schedule collect_results after timeout
	collectMsg := map[string]any{
		"op":         "collect_results",
		"request_id": requestID,
		"first_k":    firstK,
		"worker_ids": workerIDs,
	}
	h.Actor().SendAfter(uint64(timeoutMs), "cast", collectMsg)

	a.RequestCount++
	return fmt.Sprintf(`{"status":"scattered","request_id":"%s","workers_spawned":%d,"first_k":%d,"timeout_ms":%d}`,
		requestID, numServices, firstK, timeoutMs)
}

func (a *cspActor) handleCollectResults(payload map[string]any) string {
	requestID, _ := payload["request_id"].(string)
	firstK := intFromPayload(payload, "first_k", 3)

	// Linda RD-ALL: read results from tuplespace
	pattern := fmt.Sprintf(`["result","%s",null,null]`, requestID)
	resultJSON := h.TSReadAll(pattern)

	var tuples [][]any
	if resultJSON != "" && !strings.HasPrefix(resultJSON, "ERROR") {
		_ = json.Unmarshal([]byte(resultJSON), &tuples)
	}

	// Take first K results
	results := tuples
	if len(results) > firstK {
		results = results[:firstK]
	}

	// Stop remaining workers
	if workerIDs, ok := payload["worker_ids"].([]any); ok {
		for _, wid := range workerIDs {
			if id, ok := wid.(string); ok {
				_ = h.Stop(id)
			}
		}
	}

	a.ResultsCollected += len(results)
	resultsJSON, _ := json.Marshal(results)
	return fmt.Sprintf(`{"status":"gathered","request_id":"%s","results":%s,"total_available":%d,"returned":%d}`,
		requestID, string(resultsJSON), len(tuples), len(results))
}

func (a *cspActor) handleWorkerProcess(payload map[string]any) string {
	requestID, _ := payload["request_id"].(string)
	serviceID := intFromPayload(payload, "service_id", 0)

	resultData := fmt.Sprintf("response-from-service-%d", serviceID)

	// Linda OUT: write result to tuplespace
	tuple := fmt.Sprintf(`["result","%s",%d,"%s"]`, requestID, serviceID, resultData)
	result := h.TSWrite(tuple)
	if strings.HasPrefix(result, "ERROR") {
		return jsonError("worker write: " + result)
	}

	return fmt.Sprintf(`{"status":"completed","service_id":%d,"data":"%s"}`, serviceID, resultData)
}

func (a *cspActor) handleStatus() string {
	data, _ := json.Marshal(map[string]any{
		"role":              a.Role,
		"request_count":     a.RequestCount,
		"results_collected": a.ResultsCollected,
	})
	return string(data)
}

// Helpers

func intFromPayload(payload map[string]any, key string, defaultVal int) int {
	if v, ok := payload[key].(float64); ok {
		return int(v)
	}
	return defaultVal
}

func jsonError(msg string) string {
	return fmt.Sprintf(`{"error":"%s"}`, msg)
}

func init() {
	plexspaces.Register(newCSPActor())
}

func main() {}
