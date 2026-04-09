// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// WASM build: tuplespace host imports use protobuf wire bytes from tuplespace_proto_wire.go.

//go:build wasm

package plexspaces

func tsWriteWire(tuple []any) ([]byte, error) {
	return encodeWriteRequest(tuple)
}

func tsReadRequestWire(pattern []any, take bool, maxResults int32) ([]byte, error) {
	return encodeReadRequest(pattern, take, maxResults)
}

func tsDecodeReadResponseFirstTuple(raw string) ([]any, bool) {
	return decodeReadResponseFirstTuple(raw)
}

func tsDecodeReadResponseAllTuples(raw string) [][]any {
	return decodeReadResponseAllTuples(raw)
}
