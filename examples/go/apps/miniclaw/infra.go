// SPDX-License-Identifier: AGPL-3.0-or-later
// TaskQueueActor — Channel-backed durable task queue (NanoClaw Pattern 4: Channel as Message Queue).
// HealthMonitorActor — periodic PG-membership polling (NanoClaw Pattern 5: Polling Over Events).
// Registration — actor router wiring and WASM entry point.
package main

import (
	"encoding/json"
	"fmt"
	"strconv"

	"github.com/bhatti/PlexSpaces/sdks/go/plexspaces"
)

// taskQueueChannel is the channel name used for task delivery.
const taskQueueChannel = "tasks:pending"

// ========================================================================
// TaskQueueActor
// ========================================================================

// TaskQueueActor is a thin actor wrapper around the host Channel primitive.
// Producers call "enqueue"; consumers call "dequeue" (returns up to limit
// tasks), then "ack" or "nack" each message ID after processing.
// The Channel host API handles durability, redelivery, and dead-lettering.
type TaskQueueActor struct {
	plexspaces.BaseActor
	Enqueued  int64 `json:"enqueued"`
	Completed int64 `json:"completed"`
	Failed    int64 `json:"failed"`
}

func NewTaskQueueActor() plexspaces.Actor {
	a := &TaskQueueActor{}
	a.SetSelf(a)
	return a
}

func newTaskQueueActor() *TaskQueueActor {
	a := &TaskQueueActor{}
	a.SetSelf(a)
	return a
}

func (q *TaskQueueActor) Init(configJSON string) string {
	var config struct {
		ActorID string `json:"actor_id"`
	}
	if err := json.Unmarshal([]byte(configJSON), &config); err != nil {
		return "ERROR: " + err.Error()
	}
	q.SetRuntimeMetadata(config.ActorID)
	if err := host.PG().Join("svc:task_queue"); err != nil {
		host.Warn(fmt.Sprintf("TaskQueueActor: failed to join svc:task_queue: %v", err))
	}
	host.Info(fmt.Sprintf("TaskQueueActor Init actor_id=%s", config.ActorID))
	return ""
}

func (q *TaskQueueActor) Handle(fromActor, msgType, payloadJSON string) string {
	p := parsePayload(payloadJSON)
	op := stringVal(p, "op", msgType)
	switch op {
	case "enqueue":
		return q.enqueue(p)
	case "dequeue":
		return q.dequeue(p)
	case "ack":
		return q.ackTask(p)
	case "nack":
		return q.nackTask(p)
	case "get_stats":
		depth, _ := host.Ch().Depth("", taskQueueChannel)
		return marshal(map[string]any{
			"status":    "ok",
			"enqueued":  q.Enqueued,
			"completed": q.Completed,
			"failed":    q.Failed,
			"depth":     depth,
		})
	default:
		return marshal(map[string]any{"error": "unknown_op", "op": op})
	}
}

func (q *TaskQueueActor) enqueue(p map[string]any) string {
	taskType := stringVal(p, "task_type", "generic")
	task := map[string]any{
		"task_type":   taskType,
		"payload":     p["payload"],
		"enqueued_at": host.NowMs(),
	}
	msgID, err := host.Ch().Send("", taskQueueChannel, taskType, task)
	if err != nil {
		host.Warn(fmt.Sprintf("TaskQueueActor: channel send failed: %v", err))
		return marshal(map[string]any{"error": "failed to enqueue task"})
	}
	q.Enqueued++
	q.IncrCounter(host, "tasks_enqueued")
	fireAudit("task_enqueued", fmt.Sprintf("msg_id=%s type=%s", msgID, taskType))
	return marshal(map[string]any{"status": "ok", "msg_id": msgID})
}

func (q *TaskQueueActor) dequeue(p map[string]any) string {
	limit := intVal(p, "limit", 1)
	timeoutMs := uint64(intVal(p, "timeout_ms", 0))
	tasks := make([]any, 0, limit)
	for range limit {
		msg, ok, err := host.Ch().Receive("", taskQueueChannel, timeoutMs)
		if err != nil {
			host.Warn(fmt.Sprintf("TaskQueueActor: channel receive failed: %v", err))
			break
		}
		if !ok {
			break
		}
		tasks = append(tasks, msg)
	}
	return marshal(map[string]any{"status": "ok", "tasks": tasks, "count": len(tasks)})
}

// ackTask acknowledges a message as successfully processed (prevents redelivery).
func (q *TaskQueueActor) ackTask(p map[string]any) string {
	msgID := stringVal(p, "msg_id", "")
	if msgID == "" {
		return marshal(map[string]any{"error": "msg_id required"})
	}
	if err := host.Ch().Ack("", taskQueueChannel, msgID); err != nil {
		return marshal(map[string]any{"error": fmt.Sprintf("ack failed: %v", err)})
	}
	q.Completed++
	q.IncrCounter(host, "tasks_completed")
	return marshal(map[string]any{"status": "ok", "msg_id": msgID})
}

// nackTask negative-acknowledges a message. requeue=true retries; false dead-letters.
func (q *TaskQueueActor) nackTask(p map[string]any) string {
	msgID := stringVal(p, "msg_id", "")
	if msgID == "" {
		return marshal(map[string]any{"error": "msg_id required"})
	}
	requeue := boolVal(p, "requeue")
	if err := host.Ch().Nack("", taskQueueChannel, msgID, requeue); err != nil {
		return marshal(map[string]any{"error": fmt.Sprintf("nack failed: %v", err)})
	}
	q.Failed++
	q.IncrCounter(host, "tasks_failed")
	return marshal(map[string]any{"status": "ok", "msg_id": msgID, "requeue": requeue})
}

// ========================================================================
// HealthMonitorActor
// ========================================================================

// HealthMonitorActor polls process group membership on a fixed interval using
// SendAfter instead of subscribing to events. Simple polling eliminates races
// and is correct when sub-second latency is not required.
type HealthMonitorActor struct {
	plexspaces.BaseActor
	PollCount    int            `json:"poll_count"`
	LastPollMs   uint64         `json:"last_poll_ms"`
	GroupHealth  map[string]int `json:"group_health"`
	PollInterval uint64         `json:"poll_interval_ms"`
}

func NewHealthMonitorActor() plexspaces.Actor {
	a := &HealthMonitorActor{GroupHealth: map[string]int{}, PollInterval: 5000}
	a.SetSelf(a)
	return a
}

func newHealthMonitorActor() *HealthMonitorActor {
	a := &HealthMonitorActor{GroupHealth: map[string]int{}, PollInterval: 5000}
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
		if n, err := strconv.ParseUint(iv, 10, 64); err == nil && n > 0 {
			const maxPollInterval = 300_000 // 5 minutes
			if n > maxPollInterval {
				host.Warn(fmt.Sprintf("HealthMonitorActor: poll_interval_ms=%d exceeds max %d, capping", n, maxPollInterval))
				n = maxPollInterval
			}
			h.PollInterval = n
		}
	}
	if err := host.PG().Join("svc:health_monitor"); err != nil {
		host.Warn(fmt.Sprintf("HealthMonitorActor: failed to join svc:health_monitor: %v", err))
	}
	_ = host.SendAfter(h.PollInterval, "poll_tick", map[string]any{"op": "poll_tick"})
	host.Info(fmt.Sprintf("HealthMonitorActor Init actor_id=%s interval_ms=%d", config.ActorID, h.PollInterval))
	return ""
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

// serviceGroups lists every PG group this monitor watches.
var serviceGroups = []string{
	"svc:llm_router",
	"svc:tool_registry",
	"svc:agent",
	"svc:session_manager",
	"svc:memory",
	"svc:audit",
	"svc:agent_fsm",
	"svc:task_queue",
}

func (h *HealthMonitorActor) doPoll() string {
	if h.GroupHealth == nil {
		h.GroupHealth = map[string]int{}
	}
	for _, grp := range serviceGroups {
		members, err := host.PG().Members(grp)
		if err != nil {
			h.GroupHealth[grp] = 0
		} else {
			h.GroupHealth[grp] = len(members)
		}
	}
	h.PollCount++
	h.LastPollMs = host.NowMs()

	snapJSON, err := json.Marshal(h.GroupHealth)
	if err != nil {
		host.Warn(fmt.Sprintf("HealthMonitorActor: failed to marshal health snapshot: %v", err))
	} else {
		_ = host.TS().Write([]any{"health_snapshot", h.LastPollMs, string(snapJSON)})
	}
	_ = host.SendAfter(h.PollInterval, "poll_tick", map[string]any{"op": "poll_tick"})
	return marshal(map[string]any{"status": "ok", "poll_count": h.PollCount})
}

func (h *HealthMonitorActor) getHealth() string {
	if h.GroupHealth == nil {
		h.GroupHealth = map[string]int{}
	}
	healthy := 0
	degraded := make([]string, 0)
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
// Registration MUST happen in init(), not main().
// The WASM runtime calls init() before main(); the router must be populated
// before the framework starts dispatching messages.
func init() {
	router := plexspaces.NewActorRouter()
	router.Route("llm_router", NewLLMRouterActor)
	router.Route("tool_registry", NewToolRegistryActor)
	router.Route("agent", NewAgentActor)
	router.Route("session_manager", NewSessionManagerActor)
	router.Route("orchestrator", NewOrchestratorActor)
	router.Route("memory", NewMemoryActor)
	router.Route("audit_event", NewAuditEventActor)
	router.Route("agent_fsm", NewAgentStateFSM)
	router.Route("task_queue", NewTaskQueueActor)
	router.Route("health_monitor", NewHealthMonitorActor)
	plexspaces.Register(router)
}

func main() {}
