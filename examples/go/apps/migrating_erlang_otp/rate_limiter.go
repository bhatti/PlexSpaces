// Rate Limiter - Sliding Window Rate Limiting Service (Go WASM)
//
// Demonstrates Erlang/OTP GenServer pattern for API rate limiting:
// - SlidingWindowLimiter actor tracks request counts per client
// - Sliding window algorithm with configurable window size and max requests
// - Multi-client support with per-client state isolation
//
// Real-world use case: API gateway rate limiting (like NGINX, Kong, Envoy).
//
// ## SDK Features Used
//
// - plexspaces.BaseActor: Go actor base with JSON state serialization
// - plexspaces.Host: Host function wrappers (Ask, NowMs, Info, etc.)
// - Init(): Actor initialization from framework config
// - Handle(): Message routing by msg-type
// - GetState()/SetState(): Checkpoint-based state persistence
//
// ## Comparison to Erlang/OTP
//
// | Erlang/OTP                     | PlexSpaces Go                     |
// |--------------------------------|-----------------------------------|
// | gen_server:start_link/3        | Supervisor ApplicationSpec        |
// | handle_call/3                  | Handle(from, msgType, payload)    |
// | #state{} record               | Go struct with JSON tags          |
// | gen_server:call(Pid, Msg)      | host.Ask(actorID, msgType, data)  |
// | application:start/2            | app-config.toml                   |

package main

import (
	"encoding/json"
	"fmt"
	"github.com/bhatti/plexspaces/sdks/go/plexspaces"
	"math"
)

// ========================================================================
// Sliding Window Rate Limiter Actor
// ========================================================================

// SlidingWindowLimiter implements a sliding window rate limiter.
// Each client gets an independent window with configurable limits.
type SlidingWindowLimiter struct {
	plexspaces.BaseActor

	// Configuration
	WindowSizeMs uint64 `json:"window_size_ms"`
	MaxRequests  int    `json:"max_requests"`

	// Per-client state: client_id -> window entries
	// Each entry is a timestamp (ms) of a request within the window
	Clients map[string]*ClientWindow `json:"clients"`

	// Benchmark tracking
	TotalChecks    int     `json:"total_checks"`
	TotalAllowed   int     `json:"total_allowed"`
	TotalDenied    int     `json:"total_denied"`
	TotalComputeMs float64 `json:"total_compute_ms"`
	TotalCoordMs   float64 `json:"total_coord_ms"`
}

// ClientWindow tracks request timestamps for a single client.
type ClientWindow struct {
	Timestamps []uint64 `json:"timestamps"`
	Allowed    int      `json:"allowed"`
	Denied     int      `json:"denied"`
}

var host = plexspaces.NewHost()

func NewSlidingWindowLimiter() *SlidingWindowLimiter {
	a := &SlidingWindowLimiter{
		WindowSizeMs: 60000, // 1 minute default
		MaxRequests:  100,   // 100 req/min default
		Clients:      make(map[string]*ClientWindow),
	}
	a.SetSelf(a)
	return a
}

func (s *SlidingWindowLimiter) Init(configJSON string) string {
	var config struct {
		ActorID string         `json:"actor_id"`
		Args    map[string]any `json:"args"`
	}
	if err := json.Unmarshal([]byte(configJSON), &config); err != nil {
		return "ERROR: " + err.Error()
	}
	s.SetRuntimeMetadata(config.ActorID)

	if args := config.Args; args != nil {
		if v, ok := args["window_size_ms"]; ok {
			s.WindowSizeMs = toUint64(v)
		}
		if v, ok := args["max_requests"]; ok {
			s.MaxRequests = toInt(v)
		}
	}
	if s.WindowSizeMs == 0 {
		s.WindowSizeMs = 60000
	}
	if s.MaxRequests == 0 {
		s.MaxRequests = 100
	}

	host.Info(fmt.Sprintf("RateLimiter %s: window=%dms, max=%d req/window",
		s.ActorID(), s.WindowSizeMs, s.MaxRequests))
	return ""
}

func (s *SlidingWindowLimiter) Handle(fromActor, msgType, payloadJSON string) string {
	switch msgType {
	case "check_rate":
		return s.checkRate(payloadJSON)
	case "check_rate_batch":
		return s.checkRateBatch(payloadJSON)
	case "get_client_status":
		return s.getClientStatus(payloadJSON)
	case "stats":
		return s.getStats()
	case "reset_client":
		return s.resetClient(payloadJSON)
	default:
		return marshal(map[string]any{"error": "unknown operation: " + msgType})
	}
}

// checkRate checks if a single request is allowed under rate limits.
func (s *SlidingWindowLimiter) checkRate(payloadJSON string) string {
	var req struct {
		ClientID string `json:"client_id"`
	}
	if err := json.Unmarshal([]byte(payloadJSON), &req); err != nil {
		return marshal(map[string]any{"error": "invalid payload"})
	}
	if req.ClientID == "" {
		return marshal(map[string]any{"error": "client_id required"})
	}

	computeStart := host.NowMs()

	// Get or create client window
	window, exists := s.Clients[req.ClientID]
	if !exists {
		window = &ClientWindow{Timestamps: make([]uint64, 0)}
		s.Clients[req.ClientID] = window
	}

	now := host.NowMs()
	cutoff := now - s.WindowSizeMs

	// Slide window: remove expired timestamps
	var active []uint64
	for _, ts := range window.Timestamps {
		if ts > cutoff {
			active = append(active, ts)
		}
	}
	window.Timestamps = active

	// Check limit
	allowed := len(window.Timestamps) < s.MaxRequests
	if allowed {
		window.Timestamps = append(window.Timestamps, now)
		window.Allowed++
		s.TotalAllowed++
	} else {
		window.Denied++
		s.TotalDenied++
	}
	s.TotalChecks++

	computeEnd := host.NowMs()
	s.TotalComputeMs += float64(computeEnd - computeStart)

	remaining := s.MaxRequests - len(window.Timestamps)
	if remaining < 0 {
		remaining = 0
	}

	// Calculate retry-after (ms until oldest entry expires)
	retryAfterMs := uint64(0)
	if !allowed && len(window.Timestamps) > 0 {
		oldestInWindow := window.Timestamps[0]
		retryAfterMs = oldestInWindow + s.WindowSizeMs - now
	}

	return marshal(map[string]any{
		"status":         "ok",
		"allowed":        allowed,
		"client_id":      req.ClientID,
		"remaining":      remaining,
		"limit":          s.MaxRequests,
		"window_ms":      s.WindowSizeMs,
		"current_count":  len(window.Timestamps),
		"retry_after_ms": retryAfterMs,
	})
}

// checkRateBatch checks multiple requests at once for benchmarking.
func (s *SlidingWindowLimiter) checkRateBatch(payloadJSON string) string {
	var req struct {
		ClientID string `json:"client_id"`
		Count    int    `json:"count"`
	}
	if err := json.Unmarshal([]byte(payloadJSON), &req); err != nil {
		return marshal(map[string]any{"error": "invalid payload"})
	}
	if req.ClientID == "" {
		req.ClientID = "batch-client"
	}
	if req.Count <= 0 {
		req.Count = 1000
	}

	batchStart := host.NowMs()
	allowed := 0
	denied := 0

	// Simulate multiple clients for realistic load
	numClients := 50
	requestsPerClient := req.Count / numClients
	if requestsPerClient < 1 {
		requestsPerClient = 1
	}

	for c := 0; c < numClients; c++ {
		clientID := fmt.Sprintf("%s-%d", req.ClientID, c)

		for i := 0; i < requestsPerClient; i++ {
			// Inline rate check (avoid JSON overhead for batch)
			window, exists := s.Clients[clientID]
			if !exists {
				window = &ClientWindow{Timestamps: make([]uint64, 0)}
				s.Clients[clientID] = window
			}

			now := host.NowMs()
			cutoff := now - s.WindowSizeMs

			// Slide window
			var active []uint64
			for _, ts := range window.Timestamps {
				if ts > cutoff {
					active = append(active, ts)
				}
			}
			window.Timestamps = active

			ok := len(window.Timestamps) < s.MaxRequests
			if ok {
				window.Timestamps = append(window.Timestamps, now)
				window.Allowed++
				s.TotalAllowed++
				allowed++
			} else {
				window.Denied++
				s.TotalDenied++
				denied++
			}
			s.TotalChecks++
		}
	}

	batchEnd := host.NowMs()
	durationMs := float64(batchEnd - batchStart)
	s.TotalComputeMs += durationMs

	total := allowed + denied
	opsPerSec := 0.0
	if durationMs > 0 {
		opsPerSec = float64(total) / (durationMs / 1000.0)
	}

	return marshal(map[string]any{
		"status":          "ok",
		"total_requests":  total,
		"allowed":         allowed,
		"denied":          denied,
		"duration_ms":     durationMs,
		"ops_per_sec":     opsPerSec,
		"unique_clients":  numClients,
		"reqs_per_client": requestsPerClient,
	})
}

// getClientStatus returns rate limit status for a specific client.
func (s *SlidingWindowLimiter) getClientStatus(payloadJSON string) string {
	var req struct {
		ClientID string `json:"client_id"`
	}
	if err := json.Unmarshal([]byte(payloadJSON), &req); err != nil {
		return marshal(map[string]any{"error": "invalid payload"})
	}

	window, exists := s.Clients[req.ClientID]
	if !exists {
		return marshal(map[string]any{
			"status":        "ok",
			"client_id":     req.ClientID,
			"current_count": 0,
			"remaining":     s.MaxRequests,
			"limit":         s.MaxRequests,
			"window_ms":     s.WindowSizeMs,
		})
	}

	// Clean expired entries
	now := host.NowMs()
	cutoff := now - s.WindowSizeMs
	var active []uint64
	for _, ts := range window.Timestamps {
		if ts > cutoff {
			active = append(active, ts)
		}
	}
	window.Timestamps = active

	remaining := s.MaxRequests - len(active)
	if remaining < 0 {
		remaining = 0
	}

	return marshal(map[string]any{
		"status":        "ok",
		"client_id":     req.ClientID,
		"current_count": len(active),
		"remaining":     remaining,
		"limit":         s.MaxRequests,
		"window_ms":     s.WindowSizeMs,
		"total_allowed": window.Allowed,
		"total_denied":  window.Denied,
	})
}

// getStats returns comprehensive benchmarks.
func (s *SlidingWindowLimiter) getStats() string {
	totalTime := s.TotalComputeMs + s.TotalCoordMs
	computePct := 0.0
	coordPct := 0.0
	opsPerSec := 0.0
	granularity := 0.0

	if totalTime > 0 {
		computePct = s.TotalComputeMs / totalTime * 100
		coordPct = s.TotalCoordMs / totalTime * 100
		opsPerSec = float64(s.TotalChecks) / (totalTime / 1000.0)
	}
	if s.TotalCoordMs > 0 {
		granularity = s.TotalComputeMs / s.TotalCoordMs
	}

	denyRate := 0.0
	if s.TotalChecks > 0 {
		denyRate = float64(s.TotalDenied) / float64(s.TotalChecks) * 100
	}

	// Memory estimate: ~8 bytes per timestamp entry
	totalEntries := 0
	for _, w := range s.Clients {
		totalEntries += len(w.Timestamps)
	}
	memoryKB := float64(totalEntries*8+len(s.Clients)*64) / 1024.0

	return marshal(map[string]any{
		"status": "ok",
		"config": map[string]any{
			"window_ms":    s.WindowSizeMs,
			"max_requests": s.MaxRequests,
		},
		"counters": map[string]any{
			"total_checks":   s.TotalChecks,
			"total_allowed":  s.TotalAllowed,
			"total_denied":   s.TotalDenied,
			"deny_rate_pct":  math.Round(denyRate*100) / 100,
			"active_clients": len(s.Clients),
		},
		"benchmarks": map[string]any{
			"total_ms":      math.Round(totalTime*100) / 100,
			"compute_ms":    math.Round(s.TotalComputeMs*100) / 100,
			"coord_ms":      math.Round(s.TotalCoordMs*100) / 100,
			"compute_pct":   math.Round(computePct*100) / 100,
			"coord_pct":     math.Round(coordPct*100) / 100,
			"granularity":   math.Round(granularity*100) / 100,
			"ops_per_sec":   math.Round(opsPerSec*100) / 100,
			"memory_kb":     math.Round(memoryKB*100) / 100,
			"entries_total": totalEntries,
		},
	})
}

// resetClient clears rate limit state for a specific client.
func (s *SlidingWindowLimiter) resetClient(payloadJSON string) string {
	var req struct {
		ClientID string `json:"client_id"`
	}
	if err := json.Unmarshal([]byte(payloadJSON), &req); err != nil {
		return marshal(map[string]any{"error": "invalid payload"})
	}
	delete(s.Clients, req.ClientID)
	return marshal(map[string]any{"status": "ok", "reset": req.ClientID})
}

// ========================================================================
// Helpers
// ========================================================================

func marshal(v any) string {
	data, err := json.Marshal(v)
	if err != nil {
		return `{"error":"marshal failed"}`
	}
	return string(data)
}

func toUint64(v any) uint64 {
	switch n := v.(type) {
	case float64:
		return uint64(n)
	case string:
		var result uint64
		fmt.Sscanf(n, "%d", &result)
		return result
	default:
		return 0
	}
}

func toInt(v any) int {
	switch n := v.(type) {
	case float64:
		return int(n)
	case string:
		var result int
		fmt.Sscanf(n, "%d", &result)
		return result
	default:
		return 0
	}
}

// ========================================================================
// Registration - Register actor for WASM export
// ========================================================================

// init() runs during _initialize (before main), ensuring the actor is
// registered before the host calls any exported functions like init/handle.
func init() {
	plexspaces.Register(NewSlidingWindowLimiter())
}

func main() {}
