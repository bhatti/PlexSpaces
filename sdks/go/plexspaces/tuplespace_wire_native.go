// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// TupleSpace on-wire encoding for native (!wasm) builds: JSON, matching host_stubs.

//go:build !wasm

package plexspaces

import (
	"encoding/json"
	"strings"
)

func tsWriteWire(tuple []any) ([]byte, error) {
	return json.Marshal(tuple)
}

func tsReadRequestWire(pattern []any, _take bool, _maxResults int32) ([]byte, error) {
	return json.Marshal(pattern)
}

func tsDecodeReadResponseFirstTuple(raw string) ([]any, bool) {
	if raw == "" || strings.HasPrefix(raw, errorPrefix) {
		return nil, false
	}
	var tuple []any
	if err := json.Unmarshal([]byte(raw), &tuple); err != nil {
		return nil, false
	}
	return tuple, true
}

func tsDecodeReadResponseAllTuples(raw string) [][]any {
	if raw == "" || strings.HasPrefix(raw, errorPrefix) {
		return nil
	}
	var out [][]any
	if err := json.Unmarshal([]byte(raw), &out); err != nil {
		return nil
	}
	return out
}
