// SPDX-License-Identifier: AGPL-3.0-or-later
// Abstractions (Go WASM)
//
// A deployable multi-actor example that exercises the same high-level actor
// abstractions as the Rust example: durable and non-durable virtual actors,
// workflow handlers, process groups, timers/reminders, and shared services.

package main

import (
	"encoding/json"
	"strconv"
	"strings"

	"github.com/bhatti/PlexSpaces/sdks/go/plexspaces"
)

const defaultGroup = "abstractions-group"

var host = plexspaces.NewHost()

type appConfig struct {
	ActorID string         `json:"actor_id"`
	Args    map[string]any `json:"args"`
}

type abstractionsActor struct {
	plexspaces.BaseActor

	ApplicationID string   `json:"application_id"`
	ActorID       string   `json:"actor_id"`
	Role          string   `json:"role"`
	Count         int      `json:"count"`
	WorkflowStatus string  `json:"workflow_status"`
	WorkflowSignals []string `json:"workflow_signals"`
	Received      []string `json:"received"`
	TimerTicks    int      `json:"timer_ticks"`
	ReminderTicks int      `json:"reminder_ticks"`
	JoinedGroup   string   `json:"joined_group"`
	LastSpawnedID string   `json:"last_spawned_id"`
}

func newAbstractionsActor() *abstractionsActor {
	a := &abstractionsActor{
		Role:            "abstractions",
		WorkflowSignals: []string{},
		Received:        []string{},
	}
	a.SetSelf(a)
	return a
}

func (a *abstractionsActor) Init(configJSON string) string {
	var cfg appConfig
	if err := json.Unmarshal([]byte(configJSON), &cfg); err != nil {
		return "ERROR: " + err.Error()
	}

	a.ActorID = cfg.ActorID
	a.ApplicationID = appApplicationIDFromActorID(cfg.ActorID)
	a.SetRuntimeMetadata(cfg.ActorID)
	a.WorkflowSignals = []string{}
	a.Received = []string{}
	a.WorkflowStatus = ""
	a.TimerTicks = 0
	a.ReminderTicks = 0
	a.JoinedGroup = ""
	a.LastSpawnedID = ""

	if role := configString(cfg.Args, "role"); role != "" {
		a.Role = role
	} else {
		a.Role = "abstractions"
	}
	a.Count = configInt(cfg.Args, "initial_count", 0)

	if a.Role == "channel" {
		group := configString(cfg.Args, "group")
		if group == "" {
			group = defaultGroup
		}
		if err := host.PG().Join(group); err != nil {
			return "ERROR: pg_join: " + err.Error()
		}
		a.JoinedGroup = group
	}
	return ""
}

func (a *abstractionsActor) Handle(_ string, msgType, payloadJSON string) string {
	payload := parsePayload(payloadJSON)
	switch msgType {
	case "increment":
		a.Count += payloadInt(payload, "amount", 1)
		return marshal(map[string]any{"actor_id": a.ActorID, "count": a.Count})
	case "status":
		return a.status()
	case "schedule_timer":
		timerID := host.SendAfter(uint64(payloadInt(payload, "delay_ms", 100)), "tick", map[string]any{"kind": "timer"})
		return marshal(map[string]any{"timer_id": timerID})
	case "schedule_reminder":
		timerID := host.SendAfter(uint64(payloadInt(payload, "delay_ms", 150)), "reminder", map[string]any{"kind": "reminder"})
		return marshal(map[string]any{"reminder_id": timerID})
	case "tick":
		a.TimerTicks++
		return "{}"
	case "reminder":
		a.ReminderTicks++
		return "{}"
	case "kv_put":
		key := payloadString(payload, "key")
		value := payloadString(payload, "value")
		if result := host.KVPut(key, value); result != "" {
			return hostError("kv_put", result)
		}
		return marshal(map[string]any{"ok": true, "key": key, "value": value})
	case "kv_get":
		return marshal(map[string]any{"key": payloadString(payload, "key"), "value": host.KVGet(payloadString(payload, "key"))})
	case "ts_write":
		tuple := payloadArray(payload, "tuple")
		if result := host.TS().Write(tuple); result != "" {
			return hostError("ts_write", result)
		}
		return marshal(map[string]any{"ok": true, "tuple": tuple})
	case "ts_read":
		pattern := payloadArray(payload, "pattern")
		if tuple, ok := host.TS().Read(pattern); ok {
			return marshal(map[string]any{"tuple": tuple})
		}
		return marshal(map[string]any{"tuple": nil})
	case "blob_upload":
		storedID := host.BlobUpload(
			payloadString(payload, "blob_id"),
			payloadString(payload, "data"),
			payloadString(payload, "content_type"),
		)
		if plexspaces.IsHostError(storedID) {
			return hostError("blob_upload", storedID)
		}
		// Match Rust guest: blob_id in JSON is the internal id returned by the host (ULID on WASM).
		return marshal(map[string]any{"ok": true, "blob_id": storedID})
	case "blob_download":
		return marshal(map[string]any{"blob_id": payloadString(payload, "blob_id"), "data": host.BlobDownload(payloadString(payload, "blob_id"))})
	case "group_members":
		members, err := host.PG().Members(payloadString(payload, "group"))
		if err != nil {
			return marshal(map[string]any{"error": "pg_members: ERROR: " + err.Error()})
		}
		return marshal(map[string]any{"members": members})
	case "send_event":
		target := canonicalActorTarget(payloadString(payload, "target"))
		if result := host.Send(target, "publish", map[string]any{
			"channel": payloadString(payload, "channel"),
			"body":    payloadString(payload, "body"),
		}); strings.HasPrefix(result, "ERROR:") {
			return marshal(map[string]any{"error": "send: " + result})
		}
		return marshal(map[string]any{"ok": true, "target": target})
	case "broadcast_event":
		if err := host.PG().Broadcast(payloadString(payload, "group"), "publish", map[string]any{
			"channel": payloadString(payload, "channel"),
			"body":    payloadString(payload, "body"),
		}); err != nil {
			return marshal(map[string]any{"error": "broadcast: ERROR: " + err.Error()})
		}
		return marshal(map[string]any{"ok": true, "group": payloadString(payload, "group")})
	case "publish":
		channel := payloadString(payload, "channel")
		body := payloadString(payload, "body")
		a.Received = append(a.Received, channel+":"+body)
		return "{}"
	case "stop_actor":
		actorID := payloadString(payload, "actor_id")
		if err := host.Stop(actorID); err != nil {
			return marshal(map[string]any{"error": "stop: ERROR: " + err.Error()})
		}
		return marshal(map[string]any{"ok": true, "actor_id": actorID})
	default:
		return marshal(map[string]any{"error": "unknown operation: " + msgType})
	}
}

func (a *abstractionsActor) Run(payloadJSON string) string {
	payload := parsePayload(payloadJSON)
	orderID := payloadString(payload, "order_id")
	if orderID == "" {
		orderID = "unknown"
	}
	a.WorkflowStatus = "running:" + orderID
	return marshal(map[string]any{"status": a.WorkflowStatus})
}

func (a *abstractionsActor) Signal(name, payloadJSON string) {
	payload := parsePayload(payloadJSON)
	reason := payloadString(payload, "reason")
	if reason == "" {
		reason = "unknown"
	}
	a.WorkflowSignals = append(a.WorkflowSignals, name+":"+reason)
	if name == "cancel" {
		a.WorkflowStatus = "cancelled"
	}
}

func (a *abstractionsActor) Query(name, _ string) string {
	if name != "status" {
		return marshal(map[string]any{"error": "unknown query: " + name})
	}
	return marshal(map[string]any{
		"status":  a.WorkflowStatus,
		"signals": a.WorkflowSignals,
	})
}

func (a *abstractionsActor) status() string {
	return marshal(map[string]any{
		"actor_id":        a.ActorID,
		"application_id":  a.ApplicationID,
		"count":           a.Count,
		"joined_group":    a.JoinedGroup,
		"last_spawned_id": a.LastSpawnedID,
		"received":        a.Received,
		"reminder_ticks":  a.ReminderTicks,
		"role":            a.Role,
		"self_id":         host.SelfID(),
		"timer_ticks":     a.TimerTicks,
		"workflow_signals": a.WorkflowSignals,
		"workflow_status": a.WorkflowStatus,
	})
}

func canonicalActorTarget(target string) string {
	return target
}

func appApplicationIDFromActorID(actorID string) string {
	if strings.Contains(actorID, "//") && strings.Contains(actorID, "::") {
		suffix := strings.SplitN(actorID, "//", 2)[1]
		qualified := strings.SplitN(suffix, "@", 2)[0]
		parts := strings.SplitN(qualified, "::", 2)
		if len(parts) == 2 {
			return parts[1]
		}
	}
	if strings.Contains(actorID, ":") && strings.Contains(actorID, "@") {
		return strings.SplitN(strings.SplitN(actorID, ":", 2)[1], "@", 2)[0]
	}
	return ""
}

func parsePayload(payloadJSON string) map[string]any {
	if payloadJSON == "" {
		return map[string]any{}
	}
	var payload map[string]any
	if err := json.Unmarshal([]byte(payloadJSON), &payload); err != nil {
		return map[string]any{}
	}
	return payload
}

func configString(args map[string]any, key string) string {
	if args == nil {
		return ""
	}
	if value, ok := args[key]; ok {
		switch typed := value.(type) {
		case string:
			return typed
		}
	}
	return ""
}

func configInt(args map[string]any, key string, fallback int) int {
	if args == nil {
		return fallback
	}
	if value, ok := args[key]; ok {
		switch typed := value.(type) {
		case float64:
			return int(typed)
		case int:
			return typed
		case string:
			if parsed, err := strconv.Atoi(typed); err == nil {
				return parsed
			}
		}
	}
	return fallback
}

func payloadString(payload map[string]any, key string) string {
	if value, ok := payload[key]; ok {
		if text, ok := value.(string); ok {
			return text
		}
	}
	return ""
}

func payloadInt(payload map[string]any, key string, fallback int) int {
	if value, ok := payload[key]; ok {
		switch typed := value.(type) {
		case float64:
			return int(typed)
		case int:
			return typed
		case string:
			if parsed, err := strconv.Atoi(typed); err == nil {
				return parsed
			}
		}
	}
	return fallback
}

func payloadArray(payload map[string]any, key string) []any {
	if value, ok := payload[key]; ok {
		if items, ok := value.([]any); ok {
			return items
		}
	}
	return []any{}
}

func hostError(op string, result string) string {
	return marshal(map[string]any{"error": op + ": " + result})
}

func marshal(value map[string]any) string {
	data, _ := json.Marshal(value)
	return string(data)
}

func init() {
	router := plexspaces.NewActorRouter()
	router.Route("abstractions", func() plexspaces.Actor { return newAbstractionsActor() })
	router.Route("ephemeral", func() plexspaces.Actor { return newAbstractionsActor() })
	router.Route("workflow", func() plexspaces.Actor { return newAbstractionsActor() })
	router.Route("channel", func() plexspaces.Actor { return newAbstractionsActor() })
	router.Route("controller", func() plexspaces.Actor { return newAbstractionsActor() })
	plexspaces.Register(router)
}

func main() {}
