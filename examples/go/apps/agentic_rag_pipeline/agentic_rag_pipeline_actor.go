// SPDX-License-Identifier: AGPL-3.0-or-later
// Agentic RAG Pipeline - Go WASM
//
// Demonstrates Agentic RAG, Trustworthy Generation, Deep Search, and
// Exception Handling patterns using PlexSpaces actors.
//
// Actors:
//   - indexer:         Splits documents into chunks and stores them in TupleSpace
//   - retriever:       Keyword-based chunk retrieval with single and deep-search modes
//   - generator:       Simulated LLM generation with circuit-breaker exception handling
//   - validator:       Answer validation (length, grounding, safety checks)
//   - rag_workflow:    Orchestrates the full index→retrieve→generate→validate pipeline (Workflow)
//   - pipeline_event:  Fire-and-forget audit logger for pipeline step events (GenEvent)
//   - rag_fsm:         State machine tracking pipeline health transitions (GenFSM)
package main

import (
	"encoding/json"
	"fmt"
	"strconv"
	"strings"

	"github.com/bhatti/plexspaces/sdks/go/plexspaces"
)

var host = plexspaces.NewHost()

// pgFirst returns the first member of a process group, or an error if empty.
// Preferred over role-based routing because the supervisor uses ULIDs as actor names.
func pgFirst(group string) (string, error) {
	members, err := host.PG().Members(group)
	if err != nil {
		return "", fmt.Errorf("pg.Members(%q): %w", group, err)
	}
	if len(members) == 0 {
		return "", fmt.Errorf("no members in pg %q", group)
	}
	return members[0], nil
}

// initConfig is the common shape of the JSON passed to every actor's Init method.
type initConfig struct {
	ActorID string            `json:"actor_id"`
	Args    map[string]string `json:"args"`
}

// ============================================================================
// IndexerActor
// ============================================================================

// IndexerActor splits documents into fixed-size chunks and stores them in KV.
type IndexerActor struct {
	plexspaces.BaseActor
	DocumentCount int    `json:"document_count"`
	ChunkCount    int    `json:"chunk_count"`
	LastIndexedAt uint64 `json:"last_indexed_at"`
}

func NewIndexerActor() plexspaces.Actor {
	a := &IndexerActor{}
	a.SetSelf(a)
	return a
}

func (idx *IndexerActor) Init(configJSON string) string {
	var config struct {
		ActorID string            `json:"actor_id"`
		Args    map[string]string `json:"args"`
	}
	if err := json.Unmarshal([]byte(configJSON), &config); err != nil {
		return "ERROR: " + err.Error()
	}
	idx.SetRuntimeMetadata(config.ActorID)
	_ = host.PG().Join("svc:indexer")
	return ""
}

func (idx *IndexerActor) Handle(fromActor, msgType, payloadJSON string) string {
	payload := parsePayload(payloadJSON)
	op := stringVal(payload, "op", msgType)
	switch op {
	case "index_document":
		return idx.indexDocument(payload)
	case "get_stats":
		return marshal(map[string]any{
			"documents": idx.DocumentCount,
			"chunks":    idx.ChunkCount,
		})
	default:
		return marshal(map[string]any{"error": "unknown_op", "op": op})
	}
}

func (idx *IndexerActor) indexDocument(payload map[string]any) string {
	docID := stringVal(payload, "doc_id", "")
	if docID == "" {
		return marshal(map[string]any{"error": "missing doc_id"})
	}
	content := stringVal(payload, "content", "")
	if content == "" {
		return marshal(map[string]any{"error": "missing content"})
	}
	chunkSize := intVal(payload, "chunk_size", 100)
	if chunkSize <= 0 {
		chunkSize = 100
	}

	chunks := splitIntoChunks(content, chunkSize)
	// Store chunks in TupleSpace so all actors (e.g. retriever) can access them.
	// KV store is scoped per-actor (namespace = canonical actor ID), so TupleSpace
	// is used here as a shared cross-actor data store.
	ts := host.TS()
	for i, chunk := range chunks {
		if result := ts.Write([]any{"chunk", docID, float64(i), chunk}); strings.HasPrefix(result, "ERROR:") {
			return marshal(map[string]any{"error": "ts_write chunk failed: " + result})
		}
	}

	// Write doc metadata tuple
	if result := ts.Write([]any{"doc_meta", docID, float64(len(chunks))}); strings.HasPrefix(result, "ERROR:") {
		return marshal(map[string]any{"error": "ts_write doc_meta failed: " + result})
	}

	idx.DocumentCount++
	idx.ChunkCount += len(chunks)
	idx.LastIndexedAt = host.NowMs()

	if _, err := host.ApplicationMetricsAdd(idx.ApplicationID(), map[string]any{
		"message_count": 1,
		"counter_metrics": map[string]any{
			"indexer_messages":  1,
			"documents_indexed": 1,
			"chunks_created":    len(chunks),
		},
	}); err != nil {
		host.Warn(fmt.Sprintf("indexer metrics update failed: %v", err))
	}

	return marshal(map[string]any{
		"status":      "ok",
		"doc_id":      docID,
		"chunks":      len(chunks),
		"total_docs":  idx.DocumentCount,
		"total_chunks": idx.ChunkCount,
	})
}

// ============================================================================
// RetrieverActor
// ============================================================================

// RetrieverActor retrieves relevant chunks for a query using keyword matching.
type RetrieverActor struct {
	plexspaces.BaseActor
	QueryCount         int `json:"query_count"`
	TotalChunksScanned int `json:"total_chunks_scanned"`
}

func NewRetrieverActor() plexspaces.Actor {
	a := &RetrieverActor{}
	a.SetSelf(a)
	return a
}

func (ret *RetrieverActor) Init(configJSON string) string {
	var config struct {
		ActorID string            `json:"actor_id"`
		Args    map[string]string `json:"args"`
	}
	if err := json.Unmarshal([]byte(configJSON), &config); err != nil {
		return "ERROR: " + err.Error()
	}
	ret.SetRuntimeMetadata(config.ActorID)
	_ = host.PG().Join("svc:retriever")
	return ""
}

func (ret *RetrieverActor) Handle(fromActor, msgType, payloadJSON string) string {
	payload := parsePayload(payloadJSON)
	op := stringVal(payload, "op", msgType)
	switch op {
	case "retrieve":
		return ret.retrieve(payload)
	case "get_stats":
		return marshal(map[string]any{
			"query_count":          ret.QueryCount,
			"total_chunks_scanned": ret.TotalChunksScanned,
		})
	default:
		return marshal(map[string]any{"error": "unknown_op", "op": op})
	}
}

func (ret *RetrieverActor) retrieve(payload map[string]any) string {
	query := stringVal(payload, "query", "")
	maxResults := intVal(payload, "max_results", 5)
	if maxResults <= 0 {
		maxResults = 5
	}
	mode := stringVal(payload, "mode", "single")

	// Read all chunk tuples from TupleSpace (shared across actors, unlike KV which is actor-scoped).
	// Chunks were written by IndexerActor as ["chunk", docID, chunkIndex, content].
	ts := host.TS()
	allChunkTuples := ts.ReadAll([]any{"chunk", nil, nil, nil})

	// Extract chunk contents for matching
	var chunks []string
	for _, t := range allChunkTuples {
		if len(t) >= 4 {
			if content, ok := t[3].(string); ok {
				chunks = append(chunks, content)
			}
		}
	}

	queryLower := strings.ToLower(query)
	results := ret.matchChunks(chunks, queryLower, maxResults)
	scanned := len(chunks)

	// Deep search: if mode is "deep" and we have fewer than 2 results, try individual words
	if mode == "deep" && len(results) < 2 {
		words := strings.Fields(queryLower)
		for _, word := range words {
			if len(word) < 3 {
				continue
			}
			extra := ret.matchChunks(chunks, word, maxResults-len(results))
			for _, e := range extra {
				found := false
				for _, r := range results {
					if r == e {
						found = true
						break
					}
				}
				if !found {
					results = append(results, e)
				}
				if len(results) >= maxResults {
					break
				}
			}
			if len(results) >= maxResults {
				break
			}
		}
	}

	ret.QueryCount++
	ret.TotalChunksScanned += scanned

	if _, err := host.ApplicationMetricsAdd(ret.ApplicationID(), map[string]any{
		"message_count": 1,
		"counter_metrics": map[string]any{
			"retriever_queries":  1,
			"chunks_scanned":     scanned,
			"results_returned":   len(results),
		},
	}); err != nil {
		host.Warn(fmt.Sprintf("retriever metrics update failed: %v", err))
	}

	return marshal(map[string]any{
		"query":          query,
		"results":        results,
		"count":          len(results),
		"mode":           mode,
		"chunks_scanned": scanned,
	})
}

func (ret *RetrieverActor) matchChunks(chunks []string, queryLower string, maxResults int) []string {
	var results []string
	for _, content := range chunks {
		if strings.Contains(strings.ToLower(content), queryLower) {
			results = append(results, content)
			if len(results) >= maxResults {
				break
			}
		}
	}
	return results
}

// ============================================================================
// GeneratorActor
// ============================================================================

// GeneratorActor simulates LLM generation with retry logic and a circuit breaker.
type GeneratorActor struct {
	plexspaces.BaseActor
	GenerationCount      int  `json:"generation_count"`
	FailureCount         int  `json:"failure_count"`
	CircuitOpen          bool `json:"circuit_open"`
	ConsecutiveFailures  int  `json:"consecutive_failures"`
}

func NewGeneratorActor() plexspaces.Actor {
	a := &GeneratorActor{}
	a.SetSelf(a)
	return a
}

func (gen *GeneratorActor) Init(configJSON string) string {
	var config struct {
		ActorID string            `json:"actor_id"`
		Args    map[string]string `json:"args"`
	}
	if err := json.Unmarshal([]byte(configJSON), &config); err != nil {
		return "ERROR: " + err.Error()
	}
	gen.SetRuntimeMetadata(config.ActorID)
	_ = host.PG().Join("svc:generator")
	return ""
}

func (gen *GeneratorActor) Handle(fromActor, msgType, payloadJSON string) string {
	payload := parsePayload(payloadJSON)
	op := stringVal(payload, "op", msgType)
	switch op {
	case "generate":
		return gen.generate(payload)
	case "reset_circuit":
		gen.CircuitOpen = false
		gen.ConsecutiveFailures = 0
		return marshal(map[string]any{"status": "ok", "circuit_open": false})
	case "get_stats":
		return marshal(map[string]any{
			"generation_count":     gen.GenerationCount,
			"failure_count":        gen.FailureCount,
			"circuit_open":         gen.CircuitOpen,
			"consecutive_failures": gen.ConsecutiveFailures,
		})
	default:
		return marshal(map[string]any{"error": "unknown_op", "op": op})
	}
}

func (gen *GeneratorActor) generate(payload map[string]any) string {
	query := stringVal(payload, "query", "")
	maxRetries := intVal(payload, "max_retries", 2)
	contextRaw, _ := payload["context"].([]any)
	contextChunks := make([]string, 0, len(contextRaw))
	for _, c := range contextRaw {
		if s, ok := c.(string); ok {
			contextChunks = append(contextChunks, s)
		}
	}

	// Circuit breaker: if open, return fallback immediately
	if gen.CircuitOpen {
		return marshal(map[string]any{
			"answer":           "Service temporarily unavailable. Please try again later.",
			"sources":          []string{},
			"model":            "circuit-breaker-fallback",
			"generation_count": gen.GenerationCount,
			"circuit_open":     true,
		})
	}

	var lastErr string
	for attempt := 0; attempt <= maxRetries; attempt++ {
		answer, err := gen.tryGenerate(query, contextChunks)
		if err == "" {
			gen.GenerationCount++
			gen.ConsecutiveFailures = 0
			srcCount := len(contextChunks)
			if srcCount > 3 {
				srcCount = 3
			}
			if _, err := host.ApplicationMetricsAdd(gen.ApplicationID(), map[string]any{
				"message_count": 1,
				"counter_metrics": map[string]any{
					"generator_completions": 1,
					"generator_attempts":    attempt + 1,
				},
			}); err != nil {
				host.Warn(fmt.Sprintf("generator metrics update failed: %v", err))
			}
			return marshal(map[string]any{
				"answer":           answer,
				"sources":          contextChunks[:srcCount],
				"model":            "simulated-llm",
				"generation_count": gen.GenerationCount,
				"circuit_open":     false,
				"attempts":         attempt + 1,
			})
		}
		lastErr = err
		gen.FailureCount++
		gen.ConsecutiveFailures++
		if gen.ConsecutiveFailures >= 3 {
			gen.CircuitOpen = true
			return marshal(map[string]any{
				"error":        "circuit opened after consecutive failures",
				"circuit_open": true,
				"failures":     gen.FailureCount,
			})
		}
	}

	return marshal(map[string]any{
		"error":    "generation failed after retries: " + lastErr,
		"failures": gen.FailureCount,
	})
}

// tryGenerate simulates LLM generation. Returns (answer, errMsg).
// errMsg is empty on success. Occasionally simulates failures for testing retry/circuit logic.
func (gen *GeneratorActor) tryGenerate(query string, contextChunks []string) (string, string) {
	// Deterministic simulated failure: hash query to occasionally fail
	h := 0
	for _, ch := range query {
		h += int(ch)
	}
	if h%7 == 0 {
		return "", "simulated_llm_timeout"
	}

	if len(contextChunks) == 0 {
		return "No relevant context found for: " + query, ""
	}

	contextSummary := contextChunks[0]
	if len(contextSummary) > 80 {
		contextSummary = contextSummary[:80] + "..."
	}
	answer := fmt.Sprintf("Based on the context: %s ... Answer to '%s': The documentation describes relevant concepts that address this topic.", contextSummary, query)
	return answer, ""
}

// ============================================================================
// ValidatorActor
// ============================================================================

// ValidatorActor runs quality and safety checks on generated answers.
type ValidatorActor struct {
	plexspaces.BaseActor
	ValidationsRun int `json:"validations_run"`
	PassCount      int `json:"pass_count"`
	FailCount      int `json:"fail_count"`
}

func NewValidatorActor() plexspaces.Actor {
	a := &ValidatorActor{}
	a.SetSelf(a)
	return a
}

func (val *ValidatorActor) Init(configJSON string) string {
	var config struct {
		ActorID string            `json:"actor_id"`
		Args    map[string]string `json:"args"`
	}
	if err := json.Unmarshal([]byte(configJSON), &config); err != nil {
		return "ERROR: " + err.Error()
	}
	val.SetRuntimeMetadata(config.ActorID)
	_ = host.PG().Join("svc:validator")
	return ""
}

func (val *ValidatorActor) Handle(fromActor, msgType, payloadJSON string) string {
	payload := parsePayload(payloadJSON)
	op := stringVal(payload, "op", msgType)
	switch op {
	case "validate":
		return val.validate(payload)
	case "get_stats":
		return marshal(map[string]any{
			"validations_run": val.ValidationsRun,
			"pass_count":      val.PassCount,
			"fail_count":      val.FailCount,
		})
	default:
		return marshal(map[string]any{"error": "unknown_op", "op": op})
	}
}

func (val *ValidatorActor) validate(payload map[string]any) string {
	answer := stringVal(payload, "answer", "")
	query := stringVal(payload, "query", "")
	sourcesRaw, _ := payload["sources"].([]any)
	sources := make([]string, 0, len(sourcesRaw))
	for _, s := range sourcesRaw {
		if str, ok := s.(string); ok {
			sources = append(sources, str)
		}
	}

	// Check 1: Length — answer must be longer than 10 chars
	lengthOK := len(answer) > 10

	// Check 2: Source grounding — answer should share words with at least one source
	groundedOK := false
	if len(sources) > 0 {
		answerWords := wordSet(strings.ToLower(answer))
		for _, src := range sources {
			srcWords := wordSet(strings.ToLower(src))
			for w := range answerWords {
				if len(w) > 3 && srcWords[w] {
					groundedOK = true
					break
				}
			}
			if groundedOK {
				break
			}
		}
	}
	// If there are no sources, grounding check is not applicable — treat as pass
	if len(sources) == 0 {
		groundedOK = true
	}

	// Check 3: Safety — answer must not contain prohibited phrases
	forbidden := []string{"ignore", "bypass", "jailbreak", "forget"}
	safeOK := true
	answerLower := strings.ToLower(answer)
	for _, f := range forbidden {
		if strings.Contains(answerLower, f) {
			safeOK = false
			break
		}
	}

	// Also check query relevance: if query is non-empty, the answer should not be completely unrelated
	_ = query // used for context; basic checks above are sufficient for simulation

	allPassed := lengthOK && groundedOK && safeOK
	passedCount := 0
	for _, b := range []bool{lengthOK, groundedOK, safeOK} {
		if b {
			passedCount++
		}
	}
	confidence := float64(passedCount) / 3.0

	val.ValidationsRun++
	passLabel := "validation_fail"
	if allPassed {
		val.PassCount++
		passLabel = "validation_pass"
	} else {
		val.FailCount++
	}

	if _, err := host.ApplicationMetricsAdd(val.ApplicationID(), map[string]any{
		"message_count": 1,
		"counter_metrics": map[string]any{
			"validations_run": 1,
			passLabel:         1,
		},
	}); err != nil {
		host.Warn(fmt.Sprintf("validator metrics update failed: %v", err))
	}

	return marshal(map[string]any{
		"valid":      allPassed,
		"score":      confidence,
		"checks": map[string]any{
			"length":   lengthOK,
			"grounded": groundedOK,
			"safe":     safeOK,
		},
		"validations_run": val.ValidationsRun,
	})
}

// ============================================================================
// RAGWorkflow
// ============================================================================

// RAGWorkflow orchestrates the full retrieve→generate→validate pipeline.
// It implements WorkflowActor so it receives workflow_run, workflow_signal, and workflow_query messages.
type RAGWorkflow struct {
	plexspaces.BaseActor
	SelfActorID string `json:"self_actor_id"`
	Status      string `json:"status"`
	LastQuery   string `json:"last_query"`
	CurrentStep string `json:"current_step"`
	RetryCount  int    `json:"retry_count"`
	MaxRetries  int    `json:"max_retries"`
	LastAnswer  string `json:"last_answer"`
	ErrorMsg    string `json:"error_msg"`
}

func NewRAGWorkflow() plexspaces.Actor {
	a := &RAGWorkflow{
		Status:     "idle",
		MaxRetries: 2,
	}
	a.SetSelf(a)
	return a
}

func (wf *RAGWorkflow) Init(configJSON string) string {
	var config struct {
		ActorID string            `json:"actor_id"`
		Args    map[string]string `json:"args"`
	}
	if err := json.Unmarshal([]byte(configJSON), &config); err != nil {
		return "ERROR: " + err.Error()
	}
	wf.SetRuntimeMetadata(config.ActorID)
	wf.SelfActorID = config.ActorID
	return ""
}

func (wf *RAGWorkflow) Handle(fromActor, msgType, payloadJSON string) string {
	return marshal(map[string]any{"error": "use workflow_run / workflow_signal / workflow_query"})
}

func (wf *RAGWorkflow) Run(payloadJSON string) string {
	payload := parsePayload(payloadJSON)
	query := stringVal(payload, "query", "")
	if query == "" {
		return marshal(map[string]any{"error": "missing query"})
	}
	mode := stringVal(payload, "mode", "single")
	maxRetries := intVal(payload, "max_retries", 2)

	wf.LastQuery = query
	wf.Status = "running"
	wf.MaxRetries = maxRetries
	wf.ErrorMsg = ""

	// Derive sibling actor IDs from own actor ID
	retrieverID := wf.siblingActorID("retriever")
	generatorID := wf.siblingActorID("generator")
	validatorID := wf.siblingActorID("validator")
	eventActorID := wf.siblingActorID("pipeline_event")

	// Helper: fire-and-forget pipeline step events (GenEvent actor)
	fireEvent := func(step, status string) {
		_ = host.Send(eventActorID, "pipeline_step_completed", map[string]any{
			"op":     "pipeline_step_completed",
			"step":   step,
			"status": status,
			"query":  query,
		})
	}

	for attempt := 0; attempt <= maxRetries; attempt++ {
		wf.RetryCount = attempt
		effectiveMode := mode
		if attempt > 0 {
			effectiveMode = "deep"
		}

		// Step 1: Retrieve
		wf.CurrentStep = "retrieve"
		retrieveResp, err := host.Ask(retrieverID, "retrieve", map[string]any{
			"op":          "retrieve",
			"query":       query,
			"mode":        effectiveMode,
			"max_results": 5,
		}, 15000)
		if err != nil {
			fireEvent("retrieve", "failed")
			wf.Status = "failed"
			wf.ErrorMsg = "retrieve failed: " + err.Error()
			return marshal(map[string]any{"error": wf.ErrorMsg, "step": "retrieve"})
		}
		fireEvent("retrieve", "completed")
		chunks := extractStringSlice(retrieveResp, "results")

		// Step 2: Generate
		wf.CurrentStep = "generate"
		generateResp, err := host.Ask(generatorID, "generate", map[string]any{
			"op":          "generate",
			"query":       query,
			"context":     chunks,
			"max_retries": 1,
		}, 15000)
		if err != nil {
			fireEvent("generate", "failed")
			wf.Status = "failed"
			wf.ErrorMsg = "generate failed: " + err.Error()
			return marshal(map[string]any{"error": wf.ErrorMsg, "step": "generate"})
		}
		fireEvent("generate", "completed")
		answer := extractString(generateResp, "answer")
		sources := extractStringSlice(generateResp, "sources")

		// Step 3: Validate
		wf.CurrentStep = "validate"
		validateResp, err := host.Ask(validatorID, "validate", map[string]any{
			"op":      "validate",
			"answer":  answer,
			"query":   query,
			"sources": sources,
		}, 10000)
		if err != nil {
			fireEvent("validate", "failed")
			wf.Status = "failed"
			wf.ErrorMsg = "validate failed: " + err.Error()
			return marshal(map[string]any{"error": wf.ErrorMsg, "step": "validate"})
		}

		valid := extractBool(validateResp, "valid")
		score := extractFloat(validateResp, "score")

		if valid || attempt >= maxRetries {
			if valid {
				fireEvent("validate", "passed")
			} else {
				fireEvent("validate", "exhausted_retries")
			}
			wf.Status = "completed"
			wf.LastAnswer = answer
			wf.CurrentStep = "done"
			completionLabel := "rag_completed_valid"
			if !valid {
				completionLabel = "rag_completed_invalid"
			}
			_, _ = host.ApplicationMetricsAdd(wf.ApplicationID(), map[string]any{
				"message_count": 1,
				"counter_metrics": map[string]any{
					"rag_workflows_completed": 1,
					completionLabel:           1,
					"rag_retry_count":         attempt,
				},
			})
			return marshal(map[string]any{
				"status":      "completed",
				"query":       query,
				"answer":      answer,
				"sources":     sources,
				"valid":       valid,
				"score":       score,
				"retry_count": attempt,
				"mode":        effectiveMode,
			})
		}
		// Validation failed — retry with deep mode
		fireEvent("validate", "retry")
	}

	wf.Status = "failed"
	wf.ErrorMsg = "max retries exceeded"
	return marshal(map[string]any{
		"error":       wf.ErrorMsg,
		"retry_count": wf.RetryCount,
	})
}

func (wf *RAGWorkflow) Signal(name, payloadJSON string) {
	if name == "reset" {
		wf.RetryCount = 0
		wf.Status = "idle"
		wf.ErrorMsg = ""
		wf.CurrentStep = ""
	}
}

func (wf *RAGWorkflow) Query(name, payloadJSON string) string {
	if name == "status" {
		return marshal(map[string]any{
			"status":       wf.Status,
			"query":        wf.LastQuery,
			"current_step": wf.CurrentStep,
			"retry_count":  wf.RetryCount,
			"max_retries":  wf.MaxRetries,
			"last_answer":  wf.LastAnswer,
			"error_msg":    wf.ErrorMsg,
		})
	}
	return marshal(map[string]any{"error": "unknown_query", "name": name})
}

// siblingActorID returns the actual canonical ID for a sibling actor by role,
// using PG discovery to find the supervisor-spawned instance.
// Falls back to role-based routing if PG is not yet populated.
func (wf *RAGWorkflow) siblingActorID(role string) string {
	if id, err := pgFirst("svc:" + role); err == nil {
		return id
	}
	// Fallback: role-based routing (may miss actor's KV if name != type)
	if wf.SelfActorID == "" {
		return role
	}
	a, err := plexspaces.ParseActorID(wf.SelfActorID)
	if err != nil {
		return role
	}
	return a.WithTypeAndName(role, role).String()
}

// ─── Pipeline Event Actor (GenEvent — fire-and-forget audit) ────────────────

// PipelineEventActor receives fire-and-forget audit events for each pipeline step.
// It is declared with behavior_kind = "GenEvent" in app-config.toml.
type PipelineEventActor struct {
	plexspaces.BaseActor
	EventsReceived int    `json:"events_received"`
	LastEventType  string `json:"last_event_type"`
}

func NewPipelineEventActor() plexspaces.Actor {
	a := &PipelineEventActor{}
	a.SetSelf(a)
	return a
}

func (e *PipelineEventActor) Init(configJSON string) string {
	var cfg initConfig
	if err := json.Unmarshal([]byte(configJSON), &cfg); err != nil {
		return "ERROR: " + err.Error()
	}
	e.SetRuntimeMetadata(cfg.ActorID)
	// Join process group for pipeline events
	if err := host.PG().Join("pipeline-events"); err != nil {
		host.Warn("failed to join pipeline-events group: " + err.Error())
	}
	return ""
}

func (e *PipelineEventActor) Handle(fromActor, msgType, payloadJSON string) string {
	payload := parsePayload(payloadJSON)
	switch stringVal(payload, "op", "") {
	case "pipeline_step_completed":
		e.EventsReceived++
		e.LastEventType = stringVal(payload, "step", "unknown")
		// Emit metrics for observability
		_, _ = host.ApplicationMetricsAdd(e.ApplicationID(), map[string]any{
			"message_count": 1,
			"counter_metrics": map[string]any{
				"pipeline_events":                        1,
				"step_" + e.LastEventType + "_completed": 1,
			},
		})
		return marshal(map[string]any{"ok": true})
	case "get_stats":
		return marshal(map[string]any{
			"events_received": e.EventsReceived,
			"last_event_type": e.LastEventType,
		})
	}
	return marshal(map[string]any{"ok": true})
}

// ─── RAG Pipeline FSM (GenFSM — state machine for pipeline health) ──────────

// RAGPipelineFSM tracks pipeline health as a state machine.
// Valid states: idle | indexing | retrieving | generating | validating | error
// Declared with behavior_kind = "GenFSM" in app-config.toml.
type RAGPipelineFSM struct {
	plexspaces.BaseActor
	FSMState        string `json:"fsm_state"`
	FailureCount    int    `json:"failure_count"`
	LastError       string `json:"last_error"`
	TransitionCount int    `json:"transition_count"`
}

func NewRAGPipelineFSM() plexspaces.Actor {
	a := &RAGPipelineFSM{FSMState: "idle"}
	a.SetSelf(a)
	return a
}

func (f *RAGPipelineFSM) Init(configJSON string) string {
	var cfg initConfig
	if err := json.Unmarshal([]byte(configJSON), &cfg); err != nil {
		return "ERROR: " + err.Error()
	}
	f.SetRuntimeMetadata(cfg.ActorID)
	return ""
}

func (f *RAGPipelineFSM) Handle(fromActor, msgType, payloadJSON string) string {
	payload := parsePayload(payloadJSON)
	op := stringVal(payload, "op", "")
	switch op {
	case "transition":
		newState := stringVal(payload, "state", "")
		valid := f.isValidTransition(f.FSMState, newState)
		if valid {
			f.FSMState = newState
			f.TransitionCount++
			if newState == "error" {
				f.FailureCount++
				f.LastError = stringVal(payload, "error", "unknown")
			}
		}
		return marshal(map[string]any{"state": f.FSMState, "valid": valid, "transitions": f.TransitionCount})
	case "reset":
		f.FSMState = "idle"
		f.LastError = ""
		return marshal(map[string]any{"state": f.FSMState})
	case "get_state":
		return marshal(map[string]any{
			"state": f.FSMState, "failures": f.FailureCount,
			"last_error": f.LastError, "transitions": f.TransitionCount,
		})
	}
	return marshal(map[string]any{"state": f.FSMState})
}

func (f *RAGPipelineFSM) isValidTransition(from, to string) bool {
	transitions := map[string][]string{
		"idle":       {"indexing", "retrieving"},
		"indexing":   {"idle", "error"},
		"retrieving": {"generating", "error"},
		"generating": {"validating", "error"},
		"validating": {"idle", "error"},
		"error":      {"idle"},
	}
	for _, allowed := range transitions[from] {
		if allowed == to {
			return true
		}
	}
	return false
}

// ============================================================================
// Helper functions
// ============================================================================

func splitIntoChunks(text string, chunkSize int) []string {
	if chunkSize <= 0 {
		chunkSize = 100
	}
	var chunks []string
	runes := []rune(text)
	for i := 0; i < len(runes); i += chunkSize {
		end := i + chunkSize
		if end > len(runes) {
			end = len(runes)
		}
		chunk := strings.TrimSpace(string(runes[i:end]))
		if chunk != "" {
			chunks = append(chunks, chunk)
		}
	}
	if len(chunks) == 0 && text != "" {
		chunks = append(chunks, text)
	}
	return chunks
}

func wordSet(text string) map[string]bool {
	words := strings.Fields(text)
	set := make(map[string]bool, len(words))
	for _, w := range words {
		// Strip punctuation from start/end
		w = strings.Trim(w, ".,!?;:'\"()")
		if w != "" {
			set[w] = true
		}
	}
	return set
}

func extractString(v any, key string) string {
	m, ok := v.(map[string]any)
	if !ok {
		return ""
	}
	if s, ok := m[key].(string); ok {
		return s
	}
	return ""
}

func extractBool(v any, key string) bool {
	m, ok := v.(map[string]any)
	if !ok {
		return false
	}
	switch b := m[key].(type) {
	case bool:
		return b
	case float64:
		return b != 0
	}
	return false
}

func extractFloat(v any, key string) float64 {
	m, ok := v.(map[string]any)
	if !ok {
		return 0
	}
	if f, ok := m[key].(float64); ok {
		return f
	}
	return 0
}

func extractStringSlice(v any, key string) []string {
	m, ok := v.(map[string]any)
	if !ok {
		return nil
	}
	raw, _ := m[key].([]any)
	result := make([]string, 0, len(raw))
	for _, item := range raw {
		if s, ok := item.(string); ok {
			result = append(result, s)
		}
	}
	return result
}

func marshal(v map[string]any) string {
	b, _ := json.Marshal(v)
	return string(b)
}

func parsePayload(payloadJSON string) map[string]any {
	if payloadJSON == "" {
		return map[string]any{}
	}
	var m map[string]any
	_ = json.Unmarshal([]byte(payloadJSON), &m)
	return m
}

func stringVal(m map[string]any, key, def string) string {
	if v, ok := m[key].(string); ok {
		return v
	}
	return def
}

func intVal(m map[string]any, key string, def int) int {
	switch v := m[key].(type) {
	case float64:
		return int(v)
	case int:
		return v
	case string:
		if parsed, err := strconv.Atoi(v); err == nil {
			return parsed
		}
	}
	return def
}

func init() {
	router := plexspaces.NewActorRouter()
	router.Route("indexer", NewIndexerActor)
	router.Route("retriever", NewRetrieverActor)
	router.Route("generator", NewGeneratorActor)
	router.Route("validator", NewValidatorActor)
	router.Route("rag_workflow", NewRAGWorkflow)
	router.Route("pipeline_event", NewPipelineEventActor)
	router.Route("rag_fsm", NewRAGPipelineFSM)
	plexspaces.Register(router)
}

func main() {}
