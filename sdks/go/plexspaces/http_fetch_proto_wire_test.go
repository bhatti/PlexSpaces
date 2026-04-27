// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors

package plexspaces

import (
	"testing"

	wasmv1 "github.com/plexobject/plexspaces/sdks/go/plexspaces/proto/plexspaces/v1/wasm"
	"google.golang.org/protobuf/proto"
)

func TestEncodeHttpFetchRequestMatchesProto(t *testing.T) {
	headers := map[string]string{"x-a": "1", "x-b": "2"}
	body := []byte("hello")
	manual, err := encodeHttpFetchRequestWire(headers, body)
	if err != nil {
		t.Fatal(err)
	}
	want := &wasmv1.HttpFetchRequest{
		Headers: headers,
		Body:    body,
	}
	gen, err := proto.Marshal(want)
	if err != nil {
		t.Fatal(err)
	}
	var fromManual wasmv1.HttpFetchRequest
	if err := proto.Unmarshal(manual, &fromManual); err != nil {
		t.Fatalf("unmarshal manual: %v", err)
	}
	var fromGen wasmv1.HttpFetchRequest
	if err := proto.Unmarshal(gen, &fromGen); err != nil {
		t.Fatalf("unmarshal gen: %v", err)
	}
	if !proto.Equal(&fromManual, &fromGen) {
		t.Fatalf("request mismatch:\nmanual=%v\ngen=%v", fromManual.String(), fromGen.String())
	}
}

func TestDecodeHttpFetchResponseRoundTrip(t *testing.T) {
	want := &wasmv1.HttpFetchResponse{
		Status:  418,
		Headers: map[string]string{"h": "v"},
		Body:    []byte("tea"),
	}
	wire, err := proto.Marshal(want)
	if err != nil {
		t.Fatal(err)
	}
	out, err := decodeHttpFetchResponseWire(wire)
	if err != nil {
		t.Fatal(err)
	}
	if int(out["status"].(float64)) != 418 {
		t.Fatalf("status %v", out["status"])
	}
	if out["body"] != "tea" {
		t.Fatalf("body %q", out["body"])
	}
	hm, _ := out["headers"].(map[string]any)
	if hm["h"] != "v" {
		t.Fatalf("headers %v", hm)
	}
}
