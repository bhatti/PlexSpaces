// SPDX-License-Identifier: AGPL-3.0-or-later
// MiniHermes — shared helpers, host variable, and utility functions.
package main

import (
	"encoding/json"
	"fmt"
	"strings"

	"github.com/bhatti/PlexSpaces/sdks/go/plexspaces"
)

var host = plexspaces.NewHost()

func marshal(v map[string]any) string {
	data, err := json.Marshal(v)
	if err != nil {
		host.Warn(fmt.Sprintf("marshal: json.Marshal failed: %v", err))
		return `{"error":"marshal_failed"}`
	}
	return string(data)
}

func parsePayload(payloadJSON string) map[string]any {
	if payloadJSON == "" {
		return map[string]any{}
	}
	var m map[string]any
	if err := json.Unmarshal([]byte(payloadJSON), &m); err != nil {
		host.Warn(fmt.Sprintf("parsePayload: json.Unmarshal failed: %v", err))
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

func boolVal(m map[string]any, key string) bool {
	if v, ok := m[key]; ok {
		if b, ok := v.(bool); ok {
			return b
		}
	}
	return false
}

func sliceVal(m map[string]any, key string) []any {
	if v, ok := m[key]; ok {
		if s, ok := v.([]any); ok {
			return s
		}
	}
	return []any{}
}

// pgFirst returns the first member of a process group, or an error if empty.
func pgFirst(group string) (string, error) {
	return host.PG().First(group)
}

// registryFirst discovers the first actor of the given category via the object
// registry. Falls back to pgFirst(fallbackGroup) if no registry results.
// Using the registry instead of process groups enables richer metadata queries
// (capability filters, label selectors) and is recommended for production.
func registryFirst(category, fallbackGroup string, capabilities ...string) (string, error) {
	regs, err := host.Registry().Discover(plexspaces.DiscoverOptions{
		ObjectType:     1, // actor
		ObjectCategory: category,
		Capabilities:   capabilities,
	})
	if err == nil && len(regs) > 0 {
		return regs[0].ObjectID, nil
	}
	// Fall back to PG if registry is empty (e.g. first start)
	return host.PG().First(fallbackGroup)
}

// registryAll discovers all actors of the given category. Returns IDs for
// fan-out/scatter-gather operations.
func registryAll(category string, capabilities ...string) []string {
	regs, err := host.Registry().Discover(plexspaces.DiscoverOptions{
		ObjectType:     1, // actor
		ObjectCategory: category,
		Capabilities:   capabilities,
	})
	if err != nil || len(regs) == 0 {
		return nil
	}
	ids := make([]string, len(regs))
	for i, r := range regs {
		ids[i] = r.ObjectID
	}
	return ids
}

// fireAudit sends a fire-and-forget audit event. Never blocks the caller.
func fireAudit(eventType, detail string) {
	auditID, err := pgFirst("svc:audit")
	if err != nil {
		host.Debug(fmt.Sprintf("fireAudit: pgFirst failed: %v", err))
		return
	}
	if result := host.Send(auditID, "log_event", map[string]any{
		"op":         "log_event",
		"event_type": eventType,
		"detail":     detail,
		"timestamp":  host.NowMs(),
	}); result != "" {
		host.Warn(fmt.Sprintf("fireAudit: Send failed: %s", result))
	}
}

// writeActorInfo writes actor capability tuples to the shared TupleSpace.
func writeActorInfo(actorID, name, description string, capabilities []string) {
	ts := host.TS()
	if result := ts.Write([]any{"agent_card", actorID, name, description}); strings.HasPrefix(result, "ERROR:") {
		host.Warn(fmt.Sprintf("writeActorInfo: failed to write card for %s: %s", actorID, result))
	}
	for _, cap := range capabilities {
		if result := ts.Write([]any{"agent_cap", cap, actorID}); strings.HasPrefix(result, "ERROR:") {
			host.Warn(fmt.Sprintf("writeActorInfo: failed to index capability %s for %s: %s", cap, actorID, result))
		}
	}
}
