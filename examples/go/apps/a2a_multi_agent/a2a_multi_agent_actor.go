// SPDX-License-Identifier: LGPL-2.1-or-later
// A2A Multi-Agent Collaboration (Go WASM)
//
// Demonstrates Agent-to-Agent (A2A) communication and multi-agent collaboration:
// - AgentRegistryActor: Capability registry for agent discovery (external queries only)
// - ResearchAgent:      Fact retrieval and knowledge lookup
// - AnalysisAgent:      Structured analysis and summarization
// - WriterAgent:        Document composition from analysis
// - OrchestratorAgent:  Workflow that decomposes tasks to specialist agents
//   and aggregates results via TupleSpace
// - TaskEventActor:     Fire-and-forget task lifecycle event logger (GenEvent)
// - AgentStateFSM:      Per-agent lifecycle state machine (GenFSM)
//
// Design: WASM actors communicate via canonical actor IDs only.
//   - Agents write their capability info directly to TupleSpace on init.
//   - Orchestrator discovers specialists via host.PG().Members("cap:X") which
//     returns canonical actor IDs — no type-name routing needed.
//   - AgentRegistryActor reads from shared TupleSpace for external/HTTP queries.
//
// SDK Features:
//   - plexspaces.BaseActor: JSON state serialization
//   - plexspaces.WorkflowActor: Run/Signal/Query for orchestration
//   - plexspaces.Host.Ask(): Request-reply delegation (canonical actor IDs only)
//   - plexspaces.Host.KVGet/KVPut/KVList(): Capability index persistence
//   - plexspaces.Host.TS(): TupleSpace for shared state and result coordination
//   - plexspaces.Host.PG(): Process groups for canonical-ID-based agent discovery
package main

import (
	"encoding/json"
	"fmt"
	"strings"

	"github.com/plexobject/plexspaces/sdks/go/plexspaces"
)

var host = plexspaces.NewHost()

// ========================================================================
// Helpers
// ========================================================================

func marshal(v map[string]any) string {
	data, _ := json.Marshal(v)
	return string(data)
}

func parsePayload(payloadJSON string) map[string]any {
	if payloadJSON == "" {
		return map[string]any{}
	}
	var m map[string]any
	if err := json.Unmarshal([]byte(payloadJSON), &m); err != nil {
		return map[string]any{}
	}
	return m
}

func stringVal(m map[string]any, key, fallback string) string {
	if v, ok := m[key]; ok {
		if s, ok := v.(string); ok && s != "" {
			return s
		}
	}
	return fallback
}

func intVal(m map[string]any, key string, fallback int) int {
	if v, ok := m[key]; ok {
		switch n := v.(type) {
		case float64:
			return int(n)
		case int:
			return n
		case int64:
			return int(n)
		}
	}
	return fallback
}

func stringSliceVal(m map[string]any, key string) []string {
	if v, ok := m[key]; ok {
		if items, ok := v.([]any); ok {
			out := make([]string, 0, len(items))
			for _, item := range items {
				if s, ok := item.(string); ok {
					out = append(out, s)
				}
			}
			return out
		}
	}
	return nil
}

// writeAgentInfo writes agent card and capability tuples to the shared TupleSpace.
// This is called from each agent's Init so that AgentRegistryActor can serve
// external discover/list queries without any WASM-to-WASM routing.
func writeAgentInfo(agentID, actorID, name, description string, capabilities []string) {
	ts := host.TS()
	// ["agent_card", agentID, actorID, name, description]
	if result := ts.Write([]any{"agent_card", agentID, actorID, name, description}); strings.HasPrefix(result, "ERROR:") {
		host.Warn(fmt.Sprintf("writeAgentInfo: failed to write card for %s: %s", agentID, result))
	}
	// ["agent_cap", capability, agentID, actorID]
	for _, cap := range capabilities {
		if result := ts.Write([]any{"agent_cap", cap, agentID, actorID}); strings.HasPrefix(result, "ERROR:") {
			host.Warn(fmt.Sprintf("writeAgentInfo: failed to index capability %s for %s: %s", cap, agentID, result))
		}
	}
}

// ========================================================================
// AgentRegistryActor
// ========================================================================

// AgentRegistryActor is a read-only view of the agent capability registry for
// external (HTTP/gRPC) queries. Agents write their info directly to TupleSpace
// during Init; this actor only reads from TupleSpace to serve discover/list/card
// requests. It does not need to be called from WASM actors.
type AgentRegistryActor struct {
	plexspaces.BaseActor
	RegisteredAt uint64 `json:"registered_at"`
}

func NewAgentRegistryActor() plexspaces.Actor {
	a := &AgentRegistryActor{}
	a.SetSelf(a)
	return a
}

func (r *AgentRegistryActor) Init(configJSON string) string {
	var config struct {
		ActorID string `json:"actor_id"`
	}
	_ = json.Unmarshal([]byte(configJSON), &config)
	r.SetRuntimeMetadata(config.ActorID)
	r.RegisteredAt = host.NowMs()
	host.Info(fmt.Sprintf("AgentRegistryActor Init actor_id=%s", config.ActorID))
	return ""
}

func (r *AgentRegistryActor) Handle(fromActor, msgType, payloadJSON string) string {
	p := parsePayload(payloadJSON)
	switch msgType {
	case "register":
		// Support external registration (e.g. from tests or non-WASM callers) that
		// provide an actor_id — write to TupleSpace on their behalf.
		return r.register(p)
	case "discover":
		return r.discover(p)
	case "agent_card":
		return r.agentCard(p)
	case "list_all":
		return r.listAll()
	default:
		return marshal(map[string]any{"error": "unknown_op", "op": msgType})
	}
}

func (r *AgentRegistryActor) register(p map[string]any) string {
	agentID := stringVal(p, "agent_id", "")
	if agentID == "" {
		return marshal(map[string]any{"error": "agent_id is required"})
	}
	name := stringVal(p, "name", agentID)
	description := stringVal(p, "description", "")
	capabilities := stringSliceVal(p, "capabilities")
	actorID := stringVal(p, "actor_id", agentID)

	writeAgentInfo(agentID, actorID, name, description, capabilities)
	host.Info(fmt.Sprintf("AgentRegistry: registered agent_id=%s capabilities=%v", agentID, capabilities))
	return marshal(map[string]any{"status": "ok", "agent_id": agentID})
}

func (r *AgentRegistryActor) discover(p map[string]any) string {
	required := stringSliceVal(p, "capabilities")
	if len(required) == 0 {
		return r.listAll()
	}

	ts := host.TS()
	capAgentSets := make([]map[string]string, 0, len(required))
	for _, cap := range required {
		tuples := ts.ReadAll([]any{"agent_cap", cap, nil, nil})
		set := map[string]string{} // agentID -> actorID
		for _, t := range tuples {
			if len(t) >= 4 {
				agentID, _ := t[2].(string)
				actorID, _ := t[3].(string)
				if agentID != "" {
					set[agentID] = actorID
				}
			}
		}
		capAgentSets = append(capAgentSets, set)
	}

	if len(capAgentSets) == 0 {
		return marshal(map[string]any{"agents": []any{}, "count": 0})
	}
	intersection := capAgentSets[0]
	for _, set := range capAgentSets[1:] {
		next := map[string]string{}
		for agentID, actorID := range intersection {
			if _, ok := set[agentID]; ok {
				next[agentID] = actorID
			}
		}
		intersection = next
	}

	agents := make([]any, 0, len(intersection))
	for agentID, actorID := range intersection {
		agents = append(agents, r.buildCard(ts, agentID, actorID))
	}
	return marshal(map[string]any{"agents": agents, "count": len(agents)})
}

// buildCard assembles an agent card map from TupleSpace data.
func (r *AgentRegistryActor) buildCard(ts *plexspaces.TupleSpace, agentID, actorID string) map[string]any {
	card := map[string]any{
		"agent_id": agentID,
		"actor_id": actorID,
		"status":   "ok",
	}
	if t, ok := ts.Read([]any{"agent_card", agentID, nil, nil, nil}); ok && len(t) >= 5 {
		card["name"] = t[3]
		card["description"] = t[4]
	}
	capTuples := ts.ReadAll([]any{"agent_cap", nil, agentID, nil})
	caps := make([]string, 0, len(capTuples))
	for _, ct := range capTuples {
		if len(ct) >= 2 {
			if c, ok := ct[1].(string); ok && c != "" {
				caps = append(caps, c)
			}
		}
	}
	card["capabilities"] = caps
	return card
}

func (r *AgentRegistryActor) agentCard(p map[string]any) string {
	agentID := stringVal(p, "agent_id", "")
	if agentID == "" {
		return marshal(map[string]any{"error": "agent_id is required"})
	}
	ts := host.TS()
	t, ok := ts.Read([]any{"agent_card", agentID, nil, nil, nil})
	if !ok || len(t) < 3 {
		return marshal(map[string]any{"error": "agent not found", "agent_id": agentID})
	}
	actorID, _ := t[2].(string)
	return marshal(r.buildCard(ts, agentID, actorID))
}

func (r *AgentRegistryActor) listAll() string {
	ts := host.TS()
	tuples := ts.ReadAll([]any{"agent_card", nil, nil, nil, nil})
	agents := make([]any, 0, len(tuples))
	for _, t := range tuples {
		if len(t) < 3 {
			continue
		}
		agentID, _ := t[1].(string)
		actorID, _ := t[2].(string)
		if agentID == "" {
			continue
		}
		agents = append(agents, r.buildCard(ts, agentID, actorID))
	}
	return marshal(map[string]any{"status": "ok", "agents": agents, "count": len(agents)})
}

// ========================================================================
// ResearchAgent
// ========================================================================

// ResearchAgent retrieves facts from a built-in knowledge base stored in KV.
// It supports topic-based fact retrieval for use by orchestrators.
type ResearchAgent struct {
	plexspaces.BaseActor
	TasksCompleted int    `json:"tasks_completed"`
	AgentID        string `json:"agent_id"`
}

func NewResearchAgentActor() plexspaces.Actor {
	a := &ResearchAgent{}
	a.SetSelf(a)
	return a
}

func (ra *ResearchAgent) Init(configJSON string) string {
	var config struct {
		ActorID string            `json:"actor_id"`
		Args    map[string]string `json:"args"`
	}
	_ = json.Unmarshal([]byte(configJSON), &config)
	ra.SetRuntimeMetadata(config.ActorID)
	ra.AgentID = config.ActorID
	if id := config.Args["agent_id"]; id != "" {
		ra.AgentID = id
	}

	// Seed knowledge base in KV
	facts := map[string]string{
		"actors":      "Actors are isolated units of computation with message-based communication",
		"plexspaces":  "PlexSpaces is a distributed actor framework with WASM support",
		"distributed": "Distributed systems use multiple nodes for reliability and scale",
		"wasm":        "WebAssembly enables polyglot execution in a sandboxed runtime",
		"shard":       "Shard groups partition data across multiple actors for parallel processing",
	}
	for key, value := range facts {
		host.KVPut("knowledge:"+key, value)
	}

	// Write agent info directly to TupleSpace — no cross-actor routing needed.
	writeAgentInfo(ra.AgentID, config.ActorID, "Research Agent",
		"Retrieves facts and knowledge on topics",
		[]string{"research", "fact_retrieval"})

	// Join process groups for capability-based discovery (returns canonical IDs).
	_ = host.PG().Join("agents")
	_ = host.PG().Join("cap:research")

	host.Info(fmt.Sprintf("ResearchAgent Init actor_id=%s agent_id=%s", config.ActorID, ra.AgentID))
	return ""
}

func (ra *ResearchAgent) Handle(fromActor, msgType, payloadJSON string) string {
	p := parsePayload(payloadJSON)
	switch msgType {
	case "research":
		return ra.research(p)
	case "get_capabilities":
		return marshal(map[string]any{"capabilities": []string{"research", "fact_retrieval"}})
	default:
		return marshal(map[string]any{"error": "unknown_op", "op": msgType})
	}
}

func (ra *ResearchAgent) research(p map[string]any) string {
	topic := stringVal(p, "topic", "")
	if topic == "" {
		return marshal(map[string]any{"error": "topic is required"})
	}

	topicWords := strings.Fields(strings.ToLower(topic))
	findings := make([]string, 0)

	knownKeys := []string{"actors", "plexspaces", "distributed", "wasm", "shard"}
	for _, key := range knownKeys {
		for _, word := range topicWords {
			if strings.Contains(key, word) || strings.Contains(word, key) {
				fact := host.KVGet("knowledge:" + key)
				if fact != "" {
					findings = append(findings, fact)
				}
				break
			}
		}
	}

	keysJSON := host.KVList("knowledge:")
	var keys []string
	_ = json.Unmarshal([]byte(keysJSON), &keys)
	for _, key := range keys {
		shortKey := strings.TrimPrefix(key, "knowledge:")
		alreadyFound := false
		for _, known := range knownKeys {
			if shortKey == known {
				alreadyFound = true
				break
			}
		}
		if alreadyFound {
			continue
		}
		for _, word := range topicWords {
			if strings.Contains(shortKey, word) || strings.Contains(word, shortKey) {
				fact := host.KVGet(key)
				if fact != "" {
					findings = append(findings, fact)
				}
				break
			}
		}
	}

	if len(findings) == 0 {
		findings = append(findings, fmt.Sprintf("No specific facts found for topic: %s", topic))
	}

	ra.TasksCompleted++

	if _, err := host.ApplicationMetricsAdd(ra.ApplicationID(), map[string]any{
		"message_count": 1,
		"counter_metrics": map[string]any{
			"research_tasks": 1,
		},
	}); err != nil {
		host.Warn(fmt.Sprintf("ResearchAgent: metrics update failed: %v", err))
	}

	return marshal(map[string]any{
		"status":     "ok",
		"topic":      topic,
		"findings":   findings,
		"confidence": 0.8,
		"agent":      ra.AgentID,
	})
}

// ========================================================================
// AnalysisAgent
// ========================================================================

// AnalysisAgent performs structured analysis and summarization of research findings.
type AnalysisAgent struct {
	plexspaces.BaseActor
	AnalysesRun int    `json:"analyses_run"`
	AgentID     string `json:"agent_id"`
}

func NewAnalysisAgentActor() plexspaces.Actor {
	a := &AnalysisAgent{}
	a.SetSelf(a)
	return a
}

func (aa *AnalysisAgent) Init(configJSON string) string {
	var config struct {
		ActorID string            `json:"actor_id"`
		Args    map[string]string `json:"args"`
	}
	_ = json.Unmarshal([]byte(configJSON), &config)
	aa.SetRuntimeMetadata(config.ActorID)
	aa.AgentID = config.ActorID
	if id := config.Args["agent_id"]; id != "" {
		aa.AgentID = id
	}

	// Write agent info directly to TupleSpace — no cross-actor routing needed.
	writeAgentInfo(aa.AgentID, config.ActorID, "Analysis Agent",
		"Analyzes data and produces structured summaries",
		[]string{"analysis", "summarization"})

	_ = host.PG().Join("agents")
	_ = host.PG().Join("cap:analysis")

	host.Info(fmt.Sprintf("AnalysisAgent Init actor_id=%s agent_id=%s", config.ActorID, aa.AgentID))
	return ""
}

func (aa *AnalysisAgent) Handle(fromActor, msgType, payloadJSON string) string {
	p := parsePayload(payloadJSON)
	switch msgType {
	case "analyze":
		return aa.analyze(p)
	case "get_capabilities":
		return marshal(map[string]any{"capabilities": []string{"analysis", "summarization"}})
	default:
		return marshal(map[string]any{"error": "unknown_op", "op": msgType})
	}
}

func (aa *AnalysisAgent) analyze(p map[string]any) string {
	question := stringVal(p, "question", "")
	dataRaw, _ := p["data"]
	data := []string{}
	if items, ok := dataRaw.([]any); ok {
		for _, item := range items {
			if s, ok := item.(string); ok {
				data = append(data, s)
			}
		}
	}

	if len(data) == 0 {
		return marshal(map[string]any{"error": "data is required"})
	}

	questionWords := strings.Fields(strings.ToLower(question))
	keyPoints := make([]string, 0, len(data))
	relevanceScores := make([]int, 0, len(data))

	for _, fact := range data {
		score := 0
		lowerFact := strings.ToLower(fact)
		for _, word := range questionWords {
			if len(word) > 3 && strings.Contains(lowerFact, word) {
				score++
			}
		}
		relevanceScores = append(relevanceScores, score)
		if score > 0 || len(keyPoints) < 3 {
			keyPoints = append(keyPoints, fact)
		}
	}

	summary := ""
	if len(keyPoints) > 0 {
		summary = fmt.Sprintf("Analysis of %d sources for question '%s': ", len(data), question)
		if len(keyPoints) >= 2 {
			summary += fmt.Sprintf("%s Furthermore, %s", keyPoints[0], strings.ToLower(keyPoints[1]))
		} else {
			summary += keyPoints[0]
		}
	}

	aa.AnalysesRun++

	if _, err := host.ApplicationMetricsAdd(aa.ApplicationID(), map[string]any{
		"message_count": 1,
		"counter_metrics": map[string]any{
			"analysis_tasks": 1,
		},
	}); err != nil {
		host.Warn(fmt.Sprintf("AnalysisAgent: metrics update failed: %v", err))
	}

	return marshal(map[string]any{
		"status":     "ok",
		"question":   question,
		"key_points": keyPoints,
		"summary":    summary,
		"confidence": 0.75,
		"agent":      aa.AgentID,
	})
}

// ========================================================================
// WriterAgent
// ========================================================================

// WriterAgent composes formatted documents from analysis results.
type WriterAgent struct {
	plexspaces.BaseActor
	DocumentsWritten int    `json:"documents_written"`
	AgentID          string `json:"agent_id"`
}

func NewWriterAgentActor() plexspaces.Actor {
	a := &WriterAgent{}
	a.SetSelf(a)
	return a
}

func (wa *WriterAgent) Init(configJSON string) string {
	var config struct {
		ActorID string            `json:"actor_id"`
		Args    map[string]string `json:"args"`
	}
	_ = json.Unmarshal([]byte(configJSON), &config)
	wa.SetRuntimeMetadata(config.ActorID)
	wa.AgentID = config.ActorID
	if id := config.Args["agent_id"]; id != "" {
		wa.AgentID = id
	}

	// Write agent info directly to TupleSpace — no cross-actor routing needed.
	writeAgentInfo(wa.AgentID, config.ActorID, "Writer Agent",
		"Composes formatted documents from analysis",
		[]string{"writing", "formatting"})

	_ = host.PG().Join("agents")
	_ = host.PG().Join("cap:writing")

	host.Info(fmt.Sprintf("WriterAgent Init actor_id=%s agent_id=%s", config.ActorID, wa.AgentID))
	return ""
}

func (wa *WriterAgent) Handle(fromActor, msgType, payloadJSON string) string {
	p := parsePayload(payloadJSON)
	switch msgType {
	case "write":
		return wa.write(p)
	case "get_capabilities":
		return marshal(map[string]any{"capabilities": []string{"writing", "formatting"}})
	default:
		return marshal(map[string]any{"error": "unknown_op", "op": msgType})
	}
}

func (wa *WriterAgent) write(p map[string]any) string {
	style := stringVal(p, "style", "professional")

	analysisRaw, _ := p["analysis"]
	analysis, ok := analysisRaw.(map[string]any)
	if !ok {
		if s, ok := analysisRaw.(string); ok {
			var parsed map[string]any
			if err := json.Unmarshal([]byte(s), &parsed); err == nil {
				analysis = parsed
			}
		}
	}
	if analysis == nil {
		return marshal(map[string]any{"error": "analysis is required"})
	}

	summary := stringVal(analysis, "summary", "")
	question := stringVal(analysis, "question", "")

	keyPointsRaw, _ := analysis["key_points"]
	keyPoints := []string{}
	if items, ok := keyPointsRaw.([]any); ok {
		for _, item := range items {
			if s, ok := item.(string); ok {
				keyPoints = append(keyPoints, s)
			}
		}
	}

	var doc strings.Builder
	switch style {
	case "professional":
		doc.WriteString(fmt.Sprintf("Executive Summary: %s\n\n", question))
		if summary != "" {
			doc.WriteString(summary)
			doc.WriteString("\n\n")
		}
		if len(keyPoints) > 0 {
			doc.WriteString("Key Points:\n")
			for i, kp := range keyPoints {
				doc.WriteString(fmt.Sprintf("  %d. %s\n", i+1, kp))
			}
		}
	case "casual":
		doc.WriteString(fmt.Sprintf("Here's what we found about: %s\n\n", question))
		if summary != "" {
			doc.WriteString(summary)
			doc.WriteString("\n\n")
		}
		for _, kp := range keyPoints {
			doc.WriteString("- " + kp + "\n")
		}
	default:
		doc.WriteString(summary)
		for _, kp := range keyPoints {
			doc.WriteString(" " + kp)
		}
	}

	document := doc.String()
	wordCount := len(strings.Fields(document))

	wa.DocumentsWritten++

	if _, err := host.ApplicationMetricsAdd(wa.ApplicationID(), map[string]any{
		"message_count": 1,
		"counter_metrics": map[string]any{
			"documents_written": 1,
		},
	}); err != nil {
		host.Warn(fmt.Sprintf("WriterAgent: metrics update failed: %v", err))
	}

	return marshal(map[string]any{
		"status":     "ok",
		"document":   document,
		"word_count": wordCount,
		"style":      style,
		"agent":      wa.AgentID,
	})
}

// ========================================================================
// OrchestratorAgent (WorkflowActor)
// ========================================================================

// OrchestratorAgent decomposes a high-level task into specialist sub-tasks,
// delegates to ResearchAgent, AnalysisAgent, and WriterAgent in sequence,
// and aggregates intermediate results via TupleSpace coordination.
//
// Discovery uses Process Groups (host.PG().Members) which returns canonical
// actor IDs — no type-name routing or registry lookup from WASM.
type OrchestratorAgent struct {
	plexspaces.BaseActor
	Status         string `json:"status"`
	TaskID         string `json:"task_id"`
	Progress       int    `json:"progress"`
	SubtaskResults string `json:"subtask_results"` // JSON-encoded map
}

func NewOrchestratorAgent() plexspaces.Actor {
	a := &OrchestratorAgent{Status: "idle"}
	a.SetSelf(a)
	return a
}

func (o *OrchestratorAgent) Init(configJSON string) string {
	var config struct {
		ActorID string `json:"actor_id"`
	}
	_ = json.Unmarshal([]byte(configJSON), &config)
	o.SetRuntimeMetadata(config.ActorID)
	o.Status = "idle"
	host.Info(fmt.Sprintf("OrchestratorAgent Init actor_id=%s", config.ActorID))
	return ""
}

func (o *OrchestratorAgent) Handle(from, msgType, payload string) string {
	p := parsePayload(payload)
	switch msgType {
	case "workflow_run":
		return o.Run(payload)
	case "workflow_query":
		name := stringVal(p, "name", "status")
		return o.Query(name, payload)
	case "workflow_signal":
		name := stringVal(p, "name", "")
		o.Signal(name, payload)
		return marshal(map[string]any{"ok": true})
	}
	return marshal(map[string]any{"error": "use workflow_run / workflow_signal / workflow_query for orchestrator"})
}

func (o *OrchestratorAgent) Run(payloadJSON string) string {
	p := parsePayload(payloadJSON)
	task := stringVal(p, "task", "explain distributed systems")
	taskID := stringVal(p, "task_id", fmt.Sprintf("task-%d", host.NowMs()))

	o.Status = "running"
	o.TaskID = taskID
	o.Progress = 0

	host.Info(fmt.Sprintf("OrchestratorAgent Run taskID=%s task=%s", taskID, task))

	// Step 1: Discover research agents via Process Group.
	// host.PG().Members returns canonical actor IDs — no type-name routing needed.
	o.Progress = 10
	researchMembers, err := host.PG().Members("cap:research")
	if err != nil || len(researchMembers) == 0 {
		o.Status = "failed"
		msg := "no research agents in process group cap:research"
		if err != nil {
			msg = err.Error()
		}
		return marshal(map[string]any{"error": "discover research agents failed: " + msg, "task_id": taskID})
	}
	researchAgentID := researchMembers[0] // canonical actor ID

	// Step 2: Delegate research using canonical actor ID
	o.Progress = 30
	researchResp, err := host.Ask(researchAgentID, "research", map[string]any{
		"topic": task,
		"depth": 1,
	}, 10000)
	if err != nil {
		o.Status = "failed"
		return marshal(map[string]any{"error": "research failed: " + err.Error(), "task_id": taskID})
	}

	// Store research result in TupleSpace for coordination
	researchJSON, _ := json.Marshal(researchResp)
	_ = host.TS().Write([]any{"task", taskID, "step", "research", string(researchJSON)})

	// Step 3: Discover analysis agents via Process Group
	o.Progress = 50
	analysisMembers, err := host.PG().Members("cap:analysis")
	if err != nil || len(analysisMembers) == 0 {
		o.Status = "failed"
		msg := "no analysis agents in process group cap:analysis"
		if err != nil {
			msg = err.Error()
		}
		return marshal(map[string]any{"error": "discover analysis agents failed: " + msg, "task_id": taskID})
	}
	analysisAgentID := analysisMembers[0] // canonical actor ID

	// Extract findings from research for analysis
	findings := []string{}
	if respMap, ok := researchResp.(map[string]any); ok {
		if items, ok := respMap["findings"].([]any); ok {
			for _, item := range items {
				if s, ok := item.(string); ok {
					findings = append(findings, s)
				}
			}
		}
	}

	// Step 4: Delegate analysis using canonical actor ID
	o.Progress = 60
	analysisResp, err := host.Ask(analysisAgentID, "analyze", map[string]any{
		"data":     findings,
		"question": task,
	}, 10000)
	if err != nil {
		o.Status = "failed"
		return marshal(map[string]any{"error": "analysis failed: " + err.Error(), "task_id": taskID})
	}

	// Store analysis result in TupleSpace
	analysisJSON, _ := json.Marshal(analysisResp)
	_ = host.TS().Write([]any{"task", taskID, "step", "analysis", string(analysisJSON)})

	// Step 5: Discover writer agents via Process Group
	o.Progress = 70
	writerMembers, err := host.PG().Members("cap:writing")
	if err != nil || len(writerMembers) == 0 {
		o.Status = "failed"
		msg := "no writer agents in process group cap:writing"
		if err != nil {
			msg = err.Error()
		}
		return marshal(map[string]any{"error": "discover writer agents failed: " + msg, "task_id": taskID})
	}
	writerAgentID := writerMembers[0] // canonical actor ID

	// Step 6: Delegate writing using canonical actor ID
	o.Progress = 80
	writeResp, err := host.Ask(writerAgentID, "write", map[string]any{
		"analysis": analysisResp,
		"style":    "professional",
	}, 10000)
	if err != nil {
		o.Status = "failed"
		return marshal(map[string]any{"error": "writing failed: " + err.Error(), "task_id": taskID})
	}

	// Store writing result in TupleSpace
	writeJSON, _ := json.Marshal(writeResp)
	_ = host.TS().Write([]any{"task", taskID, "step", "writing", string(writeJSON)})

	// Step 7: Aggregate all results from TupleSpace
	o.Progress = 90
	allResults := host.TS().ReadAll([]any{"task", taskID, "step", nil, nil})

	stepResults := map[string]any{}
	for _, tuple := range allResults {
		if len(tuple) >= 5 {
			stepName, _ := tuple[3].(string)
			resultJSON, _ := tuple[4].(string)
			if stepName != "" && resultJSON != "" {
				var result any
				if err := json.Unmarshal([]byte(resultJSON), &result); err == nil {
					stepResults[stepName] = result
				} else {
					stepResults[stepName] = resultJSON
				}
			}
		}
	}

	subtaskJSON, _ := json.Marshal(stepResults)
	o.SubtaskResults = string(subtaskJSON)

	o.Progress = 100
	o.Status = "completed"

	if _, err := host.ApplicationMetricsAdd(o.ApplicationID(), map[string]any{
		"message_count": 1,
		"counter_metrics": map[string]any{
			"orchestrator_runs":      1,
			"orchestrator_completed": 1,
		},
	}); err != nil {
		host.Warn(fmt.Sprintf("OrchestratorAgent: metrics update failed: %v", err))
	}

	return marshal(map[string]any{
		"status":   "completed",
		"task_id":  taskID,
		"task":     task,
		"research": researchResp,
		"analysis": analysisResp,
		"document": writeResp,
		"steps":    stepResults,
	})
}

func (o *OrchestratorAgent) Signal(name, payloadJSON string) {
	switch name {
	case "cancel":
		o.Status = "cancelled"
		host.Info(fmt.Sprintf("OrchestratorAgent cancelled task_id=%s", o.TaskID))
	case "update_progress":
		p := parsePayload(payloadJSON)
		o.Progress = intVal(p, "progress", o.Progress)
	}
}

func (o *OrchestratorAgent) Query(name, _ string) string {
	if name == "status" {
		return marshal(map[string]any{
			"task_id":  o.TaskID,
			"status":   o.Status,
			"progress": o.Progress,
		})
	}
	return marshal(map[string]any{"error": "unknown_query", "name": name})
}

// ========================================================================
// TaskEventActor (GenEvent — fire-and-forget task lifecycle events)
// ========================================================================

// TaskEventActor receives fire-and-forget task lifecycle events (started,
// completed, failed). Declared with behavior_kind = "GenEvent" in app-config.toml.
type TaskEventActor struct {
	plexspaces.BaseActor
	EventsPublished int    `json:"events_published"`
	LastTaskID      string `json:"last_task_id"`
}

func NewTaskEventActor() plexspaces.Actor {
	a := &TaskEventActor{}
	a.SetSelf(a)
	return a
}

func (t *TaskEventActor) Init(configJSON string) string {
	var cfg struct {
		ActorID string            `json:"actor_id"`
		Args    map[string]string `json:"args"`
	}
	if err := json.Unmarshal([]byte(configJSON), &cfg); err != nil {
		return "ERROR: " + err.Error()
	}
	t.SetRuntimeMetadata(cfg.ActorID)
	_ = host.PG().Join("task-events")
	return ""
}

func (t *TaskEventActor) Handle(fromActor, msgType, payloadJSON string) string {
	payload := parsePayload(payloadJSON)
	switch stringVal(payload, "op", "") {
	case "task_started", "task_completed", "task_failed":
		t.EventsPublished++
		t.LastTaskID = stringVal(payload, "task_id", "")
		_, _ = host.ApplicationMetricsAdd(t.ApplicationID(), map[string]any{
			"message_count": 1,
			"counter_metrics": map[string]any{
				"task_events":                                 1,
				stringVal(payload, "op", "task_event") + "s": 1,
			},
		})
		return marshal(map[string]any{"ok": true})
	case "get_stats":
		return marshal(map[string]any{
			"events_published": t.EventsPublished,
			"last_task_id":     t.LastTaskID,
		})
	}
	return marshal(map[string]any{"ok": true})
}

// ========================================================================
// AgentStateFSM (GenFSM — agent lifecycle state machine)
// ========================================================================

// AgentStateFSM tracks a single agent's lifecycle as a state machine.
// Valid states: idle | assigned | working | reporting | error
// Declared with behavior_kind = "GenFSM" in app-config.toml.
type AgentStateFSM struct {
	plexspaces.BaseActor
	FSMState string `json:"fsm_state"`
	AgentID  string `json:"agent_id"`
	TasksRun int    `json:"tasks_run"`
}

func NewAgentStateFSM() plexspaces.Actor {
	a := &AgentStateFSM{FSMState: "idle"}
	a.SetSelf(a)
	return a
}

func (f *AgentStateFSM) Init(configJSON string) string {
	var cfg struct {
		ActorID string            `json:"actor_id"`
		Args    map[string]string `json:"args"`
	}
	if err := json.Unmarshal([]byte(configJSON), &cfg); err != nil {
		return "ERROR: " + err.Error()
	}
	f.SetRuntimeMetadata(cfg.ActorID)
	f.AgentID = cfg.Args["agent_id"]
	return ""
}

func (f *AgentStateFSM) Handle(fromActor, msgType, payloadJSON string) string {
	payload := parsePayload(payloadJSON)
	switch stringVal(payload, "op", "") {
	case "assign":
		if f.FSMState == "idle" {
			f.FSMState = "assigned"
		}
		return marshal(map[string]any{"state": f.FSMState})
	case "start_work":
		if f.FSMState == "assigned" {
			f.FSMState = "working"
		}
		return marshal(map[string]any{"state": f.FSMState})
	case "report":
		if f.FSMState == "working" {
			f.FSMState = "reporting"
			f.TasksRun++
		}
		return marshal(map[string]any{"state": f.FSMState, "tasks_run": f.TasksRun})
	case "complete":
		f.FSMState = "idle"
		return marshal(map[string]any{"state": f.FSMState})
	case "error":
		f.FSMState = "error"
		return marshal(map[string]any{"state": f.FSMState})
	case "recover":
		f.FSMState = "idle"
		return marshal(map[string]any{"state": f.FSMState})
	case "get_state":
		return marshal(map[string]any{
			"state": f.FSMState, "agent_id": f.AgentID, "tasks_run": f.TasksRun,
		})
	}
	return marshal(map[string]any{"state": f.FSMState})
}

// ========================================================================
// Registration
// ========================================================================

func init() {
	router := plexspaces.NewActorRouter()
	router.Route("agent_registry", NewAgentRegistryActor)
	router.Route("research_agent", NewResearchAgentActor)
	router.Route("analysis_agent", NewAnalysisAgentActor)
	router.Route("writer_agent", NewWriterAgentActor)
	router.Route("orchestrator", NewOrchestratorAgent)
	router.Route("task_event", NewTaskEventActor)
	router.Route("agent_fsm", NewAgentStateFSM)
	plexspaces.Register(router)
}

func main() {}
