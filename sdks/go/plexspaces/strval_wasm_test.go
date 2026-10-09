// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// WASM-only tests for strVal: verify the safe fmt.Sprint replacement
// handles all expected types and does NOT panic for unknown types.

//go:build wasm

package plexspaces

import "testing"

func TestStrValTypes(t *testing.T) {
	cases := []struct {
		in   any
		want string
	}{
		{nil, ""},
		{"hello", "hello"},
		{float64(3.7), "3"},
		{int(42), "42"},
		{int32(100), "100"},
		{int64(9999), "9999"},
		{uint32(7), "7"},
		{uint64(8), "8"},
		{true, "true"},
		{false, "false"},
	}
	for _, c := range cases {
		got := strVal(c.in)
		if got != c.want {
			t.Errorf("strVal(%v) = %q, want %q", c.in, got, c.want)
		}
	}
}

func TestStrValMapReturnsEmptyString(t *testing.T) {
	// Passing a map[string]any must NOT call fmt.Sprint (which crashes in TinyGo WASM
	// via fmtsort). The safe fallback returns "".
	result := strVal(map[string]any{"key": "value"})
	if result != "" {
		t.Errorf("strVal(map) should return empty string to avoid WASM crash, got %q", result)
	}
}
