// SPDX-License-Identifier: AGPL-3.0-or-later
// MiniPi — shared helpers, host variable, and utility functions.
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

func float64Val(m map[string]any, key string, fallback float64) float64 {
	if v, ok := m[key]; ok {
		switch n := v.(type) {
		case float64:
			return n
		case int:
			return float64(n)
		case int64:
			return float64(n)
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

func mapVal(m map[string]any, key string) map[string]any {
	if v, ok := m[key]; ok {
		if mv, ok := v.(map[string]any); ok {
			return mv
		}
	}
	return map[string]any{}
}

// pgFirst returns the first member of a process group, or an error if empty.
func pgFirst(group string) (string, error) {
	return host.PG().First(group)
}

// registryFirst discovers the first actor of the given category via the object registry.
func registryFirst(category, fallbackGroup string, capabilities ...string) (string, error) {
	regs, err := host.Registry().Discover(plexspaces.DiscoverOptions{
		ObjectType:     1,
		ObjectCategory: category,
		Capabilities:   capabilities,
	})
	if err == nil && len(regs) > 0 {
		return regs[0].ObjectID, nil
	}
	return host.PG().First(fallbackGroup)
}

// marshalAny marshals any value to a JSON string.
func marshalAny(v any) string {
	data, err := json.Marshal(v)
	if err != nil {
		return `"marshal_failed"`
	}
	return string(data)
}

// parseAnySlice parses a JSON string to []any.
func parseAnySlice(s string) []any {
	if s == "" {
		return []any{}
	}
	var result []any
	if err := json.Unmarshal([]byte(s), &result); err != nil {
		return []any{}
	}
	return result
}

// parseStringSlice parses a JSON string to []string.
func parseStringSlice(s string) []string {
	if s == "" {
		return []string{}
	}
	var result []string
	if err := json.Unmarshal([]byte(s), &result); err != nil {
		return []string{}
	}
	return result
}

// parseMapSlice parses a JSON string to []map[string]any.
func parseMapSlice(s string) []map[string]any {
	if s == "" {
		return []map[string]any{}
	}
	var result []map[string]any
	if err := json.Unmarshal([]byte(s), &result); err != nil {
		return []map[string]any{}
	}
	return result
}

// askActor sends a request to another actor and returns the response map.
func askActor(actorID, op string, payload map[string]any, timeoutMs int) (map[string]any, error) {
	resp, err := host.Ask(actorID, op, payload, uint64(timeoutMs))
	if err != nil {
		return nil, err
	}
	if m, ok := resp.(map[string]any); ok {
		return m, nil
	}
	return map[string]any{}, nil
}

// containsAny checks if lower contains any of the given substrings.
func containsAny(s string, subs ...string) bool {
	for _, sub := range subs {
		if strings.Contains(s, sub) {
			return true
		}
	}
	return false
}

// joinStrings joins a slice of strings with a separator.
func joinStrings(ss []string, sep string) string {
	return strings.Join(ss, sep)
}

// extractStringSliceFromAny converts []any to []string safely.
func extractStringSliceFromAny(items []any) []string {
	result := make([]string, 0, len(items))
	for _, item := range items {
		if s, ok := item.(string); ok {
			result = append(result, s)
		}
	}
	return result
}
