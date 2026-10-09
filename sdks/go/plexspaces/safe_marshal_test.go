// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors

package plexspaces

import (
	"encoding/json"
	"math"
	"strings"
	"testing"
)

func TestSafeMarshalPrimitives(t *testing.T) {
	cases := []struct {
		name string
		in   any
		want string
	}{
		{"nil", nil, "null"},
		{"true", true, "true"},
		{"false", false, "false"},
		{"int", 42, "42"},
		{"int64", int64(1234567890), "1234567890"},
		{"uint64", uint64(9876543210), "9876543210"},
		{"string", "hello", `"hello"`},
		{"string with quotes", `he"llo`, `"he\"llo"`},
		{"string with newline", "a\nb", `"a\nb"`},
		{"string with tab", "a\tb", `"a\tb"`},
		{"string control char", "a\x01b", `"a\u0001b"`},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			got := SafeMarshal(tc.in)
			if got != tc.want {
				t.Errorf("SafeMarshal(%v) = %q, want %q", tc.in, got, tc.want)
			}
		})
	}
}

func TestSafeMarshalFloat64(t *testing.T) {
	cases := []struct {
		name string
		in   float64
		want string
	}{
		{"zero", 0.0, "0"},
		{"positive integer float", 3.0, "3"},
		{"one decimal", 2.8, "2.8"},
		{"two decimals", 1.25, "1.25"},
		{"three decimals", 0.001, "0.001"},
		{"negative", -1.5, "-1.5"},
		{"large int float", 100000.0, "100000"},
		{"NaN", math.NaN(), "0"},
		{"Inf", math.Inf(1), "0"},
		{"NegInf", math.Inf(-1), "0"},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			got := SafeMarshal(tc.in)
			if got != tc.want {
				t.Errorf("SafeMarshal(%v) = %q, want %q", tc.in, got, tc.want)
			}
		})
	}
}

func TestSafeMarshalFloat64InMap(t *testing.T) {
	m := map[string]any{
		"ratio":   2.8,
		"pct":     99.5,
		"speedup": 1.75,
		"count":   42,
	}
	out := SafeMarshal(m)

	// Verify valid JSON
	var parsed map[string]any
	if err := json.Unmarshal([]byte(out), &parsed); err != nil {
		t.Fatalf("SafeMarshal produced invalid JSON: %v\noutput: %s", err, out)
	}
	if parsed["ratio"].(float64) != 2.8 {
		t.Errorf("ratio: got %v, want 2.8", parsed["ratio"])
	}
	if parsed["pct"].(float64) != 99.5 {
		t.Errorf("pct: got %v, want 99.5", parsed["pct"])
	}
	if parsed["count"].(float64) != 42 {
		t.Errorf("count: got %v, want 42", parsed["count"])
	}
}

func TestSafeMarshalNestedMap(t *testing.T) {
	// Nested map[string]any — the fmtsort crash trigger in TinyGo WASM
	m := map[string]any{
		"outer_key": "outer_val",
		"nested": map[string]any{
			"inner_float": 3.14,
			"inner_int":   100,
		},
	}
	out := SafeMarshal(m)

	var parsed map[string]any
	if err := json.Unmarshal([]byte(out), &parsed); err != nil {
		t.Fatalf("SafeMarshal nested map invalid JSON: %v\noutput: %s", err, out)
	}
	nested, ok := parsed["nested"].(map[string]any)
	if !ok {
		t.Fatalf("nested key is not a map: %T", parsed["nested"])
	}
	if nested["inner_int"].(float64) != 100 {
		t.Errorf("inner_int: got %v, want 100", nested["inner_int"])
	}
}

func TestSafeMarshalSliceOfMaps(t *testing.T) {
	// []map[string]any — common for benchmark result lists
	results := []map[string]any{
		{"shards": 4, "speedup": 1.0, "eff_pct": 100.0},
		{"shards": 8, "speedup": 1.88, "eff_pct": 94.0},
		{"shards": 16, "speedup": 3.12, "eff_pct": 78.0},
	}
	out := SafeMarshal(map[string]any{"results": results})

	var parsed map[string]any
	if err := json.Unmarshal([]byte(out), &parsed); err != nil {
		t.Fatalf("invalid JSON: %v\noutput: %s", err, out)
	}
	rows, ok := parsed["results"].([]any)
	if !ok {
		t.Fatalf("results not a slice: %T", parsed["results"])
	}
	if len(rows) != 3 {
		t.Fatalf("expected 3 rows, got %d", len(rows))
	}
	row1 := rows[1].(map[string]any)
	if row1["speedup"].(float64) != 1.88 {
		t.Errorf("row1 speedup: got %v, want 1.88", row1["speedup"])
	}
}

func TestSafeMarshalStringSlice(t *testing.T) {
	out := SafeMarshal([]string{"a", "b", "c"})
	if out != `["a","b","c"]` {
		t.Errorf("got %q, want [\"a\",\"b\",\"c\"]", out)
	}
}

func TestSafeMarshalIntSlice(t *testing.T) {
	out := SafeMarshal([]int{1, 2, 3})
	if out != "[1,2,3]" {
		t.Errorf("got %q, want [1,2,3]", out)
	}
}

func TestSafeMarshalMapStringString(t *testing.T) {
	m := map[string]string{"host": "web-01", "env": "prod"}
	out := SafeMarshal(m)
	var parsed map[string]any
	if err := json.Unmarshal([]byte(out), &parsed); err != nil {
		t.Fatalf("invalid JSON: %v", err)
	}
	if parsed["host"].(string) != "web-01" {
		t.Errorf("host: got %v, want web-01", parsed["host"])
	}
}

func TestSafeMarshalAnySliceWithFloats(t *testing.T) {
	s := []any{1.5, "text", 42, true, nil}
	out := SafeMarshal(s)
	var parsed []any
	if err := json.Unmarshal([]byte(out), &parsed); err != nil {
		t.Fatalf("invalid JSON: %v\noutput: %s", err, out)
	}
	if parsed[0].(float64) != 1.5 {
		t.Errorf("first element: got %v, want 1.5", parsed[0])
	}
	if parsed[2].(float64) != 42 {
		t.Errorf("third element: got %v, want 42", parsed[2])
	}
	if parsed[3].(bool) != true {
		t.Errorf("fourth element: got %v, want true", parsed[3])
	}
	if parsed[4] != nil {
		t.Errorf("fifth element: got %v, want nil", parsed[4])
	}
}

func TestSafeMarshalEmptyMap(t *testing.T) {
	out := SafeMarshal(map[string]any{})
	if out != "{}" {
		t.Errorf("got %q, want {}", out)
	}
}

func TestSafeMarshalSpecialStringChars(t *testing.T) {
	out := SafeMarshal(map[string]any{"k": "line1\nline2\ttab"})
	if !strings.Contains(out, `\n`) || !strings.Contains(out, `\t`) {
		t.Errorf("escape sequences missing in %q", out)
	}
	var parsed map[string]any
	if err := json.Unmarshal([]byte(out), &parsed); err != nil {
		t.Fatalf("invalid JSON: %v", err)
	}
	if parsed["k"].(string) != "line1\nline2\ttab" {
		t.Errorf("string round-trip failed: got %q", parsed["k"])
	}
}

func TestSafeMarshalFloat32(t *testing.T) {
	out := SafeMarshal(float32(1.5))
	if out != "1.5" {
		t.Errorf("got %q, want 1.5", out)
	}
}

func TestSafeMarshalUint32(t *testing.T) {
	out := SafeMarshal(uint32(4294967295))
	if out != "4294967295" {
		t.Errorf("got %q, want 4294967295", out)
	}
}
