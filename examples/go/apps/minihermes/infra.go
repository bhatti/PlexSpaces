// SPDX-License-Identifier: AGPL-3.0-or-later
// SessionManagerActor — session lifecycle backed by KV + TupleSpace.
// HealthMonitorActor — periodic PG-membership polling via SendAfter.
// Registration — actor router wiring and WASM entry point.
package main

import (
	"encoding/json"
	"fmt"

	"github.com/bhatti/PlexSpaces/sdks/go/plexspaces"
)

// ========================================================================
// SessionManagerActor
// ========================================================================

// SessionManagerActor manages conversation session lifecycle.
// Sessions are persisted in KV; TupleSpace indexes active sessions.
type SessionManagerActor struct {
	plexspaces.BaseActor
	ActiveSessions int      `json:"active_sessions"`
	TotalCreated   int      `json:"total_created"`
	SessionIDs     []string `json:"session_ids"`
}

func NewSessionManagerActor() plexspaces.Actor {
	a := &SessionManagerActor{}
	a.SetSelf(a)
	return a
}

func newSessionManagerActor() *SessionManagerActor {
	a := &SessionManagerActor{}
	a.SetSelf(a)
	return a
}

func (s *SessionManagerActor) Init(configJSON string) string {
	var config struct {
		ActorID string `json:"actor_id"`
	}
	if err := json.Unmarshal([]byte(configJSON), &config); err != nil {
		return "ERROR: " + err.Error()
	}
	s.SetRuntimeMetadata(config.ActorID)
	if err := host.PG().Join("svc:sessions"); err != nil {
		host.Warn(fmt.Sprintf("SessionManagerActor: failed to join svc:sessions: %v", err))
	}
	host.Info(fmt.Sprintf("SessionManagerActor Init actor_id=%s", config.ActorID))
	return ""
}

func (s *SessionManagerActor) Handle(fromActor, msgType, payloadJSON string) string {
	p := parsePayload(payloadJSON)
	op := stringVal(p, "op", msgType)
	switch op {
	case "create_session":
		return s.createSession(p)
	case "get_session":
		return s.getSession(p)
	case "end_session":
		return s.endSession(p)
	case "list_sessions":
		return s.listSessions()
	case "get_stats":
		return marshal(map[string]any{
			"status":          "ok",
			"active_sessions": s.ActiveSessions,
			"total_created":   s.TotalCreated,
		})
	default:
		return marshal(map[string]any{"error": "unknown_op", "op": op})
	}
}

func (s *SessionManagerActor) createSession(p map[string]any) string {
	channel := stringVal(p, "channel", "web")
	userID := stringVal(p, "user_id", "anonymous")
	agentID := stringVal(p, "agent_id", "hermes-agent")

	sessionID := fmt.Sprintf("sess-%s-%s-%d", channel, userID, host.NowMs())
	meta := map[string]any{
		"session_id": sessionID,
		"channel":    channel,
		"user_id":    userID,
		"agent_id":   agentID,
		"created_at": host.NowMs(),
		"status":     "active",
	}
	metaJSON, _ := json.Marshal(meta)
	host.KV().Put("session:"+sessionID, string(metaJSON))
	host.KV().Put("session_map:"+channel+":"+userID, sessionID)
	_ = host.TS().Write([]any{"session", "active", channel, userID, sessionID})

	s.SessionIDs = append(s.SessionIDs, sessionID)
	s.ActiveSessions++
	s.TotalCreated++
	s.IncrCounter(host, "sessions_created")
	fireAudit("session_created", fmt.Sprintf("session_id=%s channel=%s user_id=%s", sessionID, channel, userID))
	return marshal(map[string]any{"status": "ok", "session_id": sessionID})
}

func (s *SessionManagerActor) getSession(p map[string]any) string {
	sessionID := stringVal(p, "session_id", "")
	if sessionID == "" {
		channel := stringVal(p, "channel", "")
		userID := stringVal(p, "user_id", "")
		if channel != "" && userID != "" {
			sessionID, _ = host.KV().Get("session_map:" + channel + ":" + userID)
		}
	}
	if sessionID == "" {
		return marshal(map[string]any{"error": "session not found"})
	}
	raw, _ := host.KV().Get("session:" + sessionID)
	if raw == "" {
		return marshal(map[string]any{"error": "session not found", "session_id": sessionID})
	}
	var meta map[string]any
	if err := json.Unmarshal([]byte(raw), &meta); err != nil {
		return marshal(map[string]any{"error": "corrupt session data"})
	}
	meta["status"] = "ok"
	return marshal(meta)
}

func (s *SessionManagerActor) endSession(p map[string]any) string {
	sessionID := stringVal(p, "session_id", "")
	if sessionID == "" {
		return marshal(map[string]any{"error": "session_id is required"})
	}
	host.KV().Delete("session:" + sessionID)
	host.KV().Delete("session_history:" + sessionID)
	newIDs := make([]string, 0, len(s.SessionIDs))
	for _, id := range s.SessionIDs {
		if id != sessionID {
			newIDs = append(newIDs, id)
		}
	}
	s.SessionIDs = newIDs
	if s.ActiveSessions > 0 {
		s.ActiveSessions--
	}
	fireAudit("session_ended", fmt.Sprintf("session_id=%s", sessionID))
	return marshal(map[string]any{"status": "ok", "session_id": sessionID})
}

func (s *SessionManagerActor) listSessions() string {
	sessions := make([]any, 0, len(s.SessionIDs))
	for _, id := range s.SessionIDs {
		raw, _ := host.KV().Get("session:" + id)
		if raw == "" {
			continue
		}
		var meta map[string]any
		if err := json.Unmarshal([]byte(raw), &meta); err == nil {
			sessions = append(sessions, meta)
		}
	}
	return marshal(map[string]any{"status": "ok", "sessions": sessions, "count": len(sessions)})
}

// ========================================================================
// HealthMonitorActor
// ========================================================================

// HealthMonitorActor polls process group membership on a fixed interval.
// Uses SendAfter (not event subscriptions) for simplicity and correctness.
type HealthMonitorActor struct {
	plexspaces.BaseActor
	PollCount    int            `json:"poll_count"`
	LastPollMs   uint64         `json:"last_poll_ms"`
	GroupHealth  map[string]int `json:"group_health"`
	PollInterval uint64         `json:"poll_interval_ms"`
}

func NewHealthMonitorActor() plexspaces.Actor {
	a := &HealthMonitorActor{GroupHealth: map[string]int{}, PollInterval: 10000}
	a.SetSelf(a)
	return a
}

func newHealthMonitorActor() *HealthMonitorActor {
	a := &HealthMonitorActor{GroupHealth: map[string]int{}, PollInterval: 10000}
	a.SetSelf(a)
	return a
}

func (h *HealthMonitorActor) Init(configJSON string) string {
	var config struct {
		ActorID string            `json:"actor_id"`
		Args    map[string]string `json:"args"`
	}
	if err := json.Unmarshal([]byte(configJSON), &config); err != nil {
		return "ERROR: " + err.Error()
	}
	h.SetRuntimeMetadata(config.ActorID)
	if iv := config.Args["poll_interval_ms"]; iv != "" {
		var n uint64
		if _, err := fmt.Sscan(iv, &n); err == nil && n >= 1000 {
			const maxPoll = 300_000
			if n > maxPoll {
				n = maxPoll
			}
			h.PollInterval = n
		}
	}
	if err := host.PG().Join("svc:health_monitor"); err != nil {
		host.Warn(fmt.Sprintf("HealthMonitorActor: failed to join svc:health_monitor: %v", err))
	}
	_, _ = host.Actor().SendAfter(h.PollInterval, "poll_tick", map[string]any{"op": "poll_tick"})
	host.Info(fmt.Sprintf("HealthMonitorActor Init actor_id=%s interval_ms=%d", config.ActorID, h.PollInterval))
	return ""
}

var hermesServiceGroups = []string{
	"svc:llm_gateway",
	"svc:tools",
	"svc:agent",
	"svc:skills",
	"svc:memory",
	"svc:compressor",
	"svc:cron",
	"svc:sessions",
	"svc:guardrails",
	"svc:audit",
}

func (h *HealthMonitorActor) Handle(fromActor, msgType, payloadJSON string) string {
	p := parsePayload(payloadJSON)
	op := stringVal(p, "op", msgType)
	switch op {
	case "poll_tick":
		return h.doPoll()
	case "get_health":
		return h.getHealth()
	case "get_stats":
		return marshal(map[string]any{
			"status":       "ok",
			"poll_count":   h.PollCount,
			"last_poll_ms": h.LastPollMs,
			"group_health": h.GroupHealth,
		})
	default:
		return marshal(map[string]any{"error": "unknown_op", "op": op})
	}
}

func (h *HealthMonitorActor) doPoll() string {
	now := host.NowMs()
	if h.LastPollMs > 0 && now-h.LastPollMs < h.PollInterval/2 {
		_, _ = host.Actor().SendAfter(h.PollInterval, "poll_tick", map[string]any{"op": "poll_tick"})
		return marshal(map[string]any{"status": "ok", "skipped": true})
	}

	if h.GroupHealth == nil {
		h.GroupHealth = map[string]int{}
	}
	for _, grp := range hermesServiceGroups {
		members, err := host.PG().Members(grp)
		if err != nil {
			h.GroupHealth[grp] = 0
		} else {
			h.GroupHealth[grp] = len(members)
		}
	}
	h.PollCount++
	h.LastPollMs = now

	snapJSON, _ := json.Marshal(h.GroupHealth)
	_ = host.TS().Write([]any{"health_snapshot", h.LastPollMs, string(snapJSON)})
	_, _ = host.Actor().SendAfter(h.PollInterval, "poll_tick", map[string]any{"op": "poll_tick"})

	return marshal(map[string]any{"status": "ok", "poll_count": h.PollCount})
}

func (h *HealthMonitorActor) getHealth() string {
	if h.GroupHealth == nil {
		h.GroupHealth = map[string]int{}
	}
	healthy := 0
	degraded := []string{}
	for grp, count := range h.GroupHealth {
		if count > 0 {
			healthy++
		} else {
			degraded = append(degraded, grp)
		}
	}
	return marshal(map[string]any{
		"status":       "ok",
		"group_health": h.GroupHealth,
		"healthy":      healthy,
		"degraded":     degraded,
		"last_poll_ms": h.LastPollMs,
	})
}

// ========================================================================
// Registration
// ========================================================================

// init registers all actors with the ActorRouter.
// MUST be in init(), not main() — the router must be populated before the framework
// starts dispatching messages.
func init() {
	router := plexspaces.NewActorRouter()
	router.Route("llm_gateway", NewLLMGatewayActor)
	router.Route("tool_executor", NewToolExecutorActor)
	router.Route("agent", NewAgentActor)
	router.Route("skill_store", NewSkillStoreActor)
	router.Route("memory", NewMemoryActor)
	router.Route("context_compressor", NewContextCompressorActor)
	router.Route("cron_scheduler", NewCronSchedulerActor)
	router.Route("guardrails", NewGuardrailsGateActor)
	router.Route("session_manager", NewSessionManagerActor)
	router.Route("audit_event", NewAuditEventActor)
	router.Route("health_monitor", NewHealthMonitorActor)
	plexspaces.Register(router)
}

func main() {}
