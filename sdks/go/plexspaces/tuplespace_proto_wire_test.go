// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 PlexSpaces Contributors

package plexspaces

import (
	"bytes"
	"fmt"
	"testing"
)

func firstLengthDelimitedPayload(buf []byte) ([]byte, error) {
	if len(buf) == 0 {
		return nil, fmt.Errorf("empty buffer")
	}
	tag, tn, err := readVarint(buf, 0)
	if err != nil {
		return nil, err
	}
	if tag&7 != 2 {
		return nil, fmt.Errorf("expected length-delimited field, wire type %d", tag&7)
	}
	ln, lnN, err := readVarint(buf, tn)
	if err != nil {
		return nil, err
	}
	start := tn + lnN
	end := start + int(ln)
	if end > len(buf) {
		return nil, fmt.Errorf("length-delimited out of range")
	}
	return buf[start:end], nil
}

func TestReadRequestTemplateEqualsTupleWire(t *testing.T) {
	pat := []any{"abstractions", "task", "*"}
	wantTupleMsg, err := encodeTupleFields(pat, true)
	if err != nil {
		t.Fatal(err)
	}
	req, err := encodeReadRequest(pat, false, 1)
	if err != nil {
		t.Fatal(err)
	}
	gotTemplate, err := firstLengthDelimitedPayload(req)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(gotTemplate, wantTupleMsg) {
		t.Fatalf("ReadRequest.template bytes mismatch\n got %x\nwant %x", gotTemplate, wantTupleMsg)
	}
}

func TestWriteRequestInnerTupleMatchesTupleFields(t *testing.T) {
	tuple := []any{"abstractions", "task", "t-1"}
	want, err := encodeTupleFields(tuple, false)
	if err != nil {
		t.Fatal(err)
	}
	wr, err := encodeWriteRequest(tuple)
	if err != nil {
		t.Fatal(err)
	}
	inner, err := firstLengthDelimitedPayload(wr)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(inner, want) {
		t.Fatalf("WriteRequest tuple mismatch\n got %x\nwant %x", inner, want)
	}
}

func TestDecodeReadResponseRoundTrip(t *testing.T) {
	tuple := []any{"abstractions", "task", "t-1"}
	writeBytes, err := encodeWriteRequest(tuple)
	if err != nil {
		t.Fatal(err)
	}
	innerTuple, err := firstLengthDelimitedPayload(writeBytes)
	if err != nil {
		t.Fatal(err)
	}
	readResp := appendLengthDelimited(nil, 1, innerTuple)
	parsed, err := parseReadResponseTuples(readResp)
	if err != nil {
		t.Fatal(err)
	}
	if len(parsed) != 1 || len(parsed[0]) != 3 {
		t.Fatalf("parsed %v", parsed)
	}
	if parsed[0][0] != "abstractions" || parsed[0][1] != "task" || parsed[0][2] != "t-1" {
		t.Fatalf("fields %v", parsed[0])
	}
}

func TestDecodeReadResponseFirstTupleHelper(t *testing.T) {
	tuple := []any{"a", "b", "c"}
	writeBytes, _ := encodeWriteRequest(tuple)
	inner, _ := firstLengthDelimitedPayload(writeBytes)
	readResp := appendLengthDelimited(nil, 1, inner)
	got, ok := decodeReadResponseFirstTuple(string(readResp))
	if !ok || len(got) != 3 {
		t.Fatalf("ok=%v got=%v", ok, got)
	}
}
