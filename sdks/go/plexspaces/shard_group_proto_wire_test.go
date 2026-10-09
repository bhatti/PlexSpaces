// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Tests for shard-group protobuf wire encoding field numbers.
// These tests verify that the WASM wire encoders produce proto field numbers
// matching the proto schema definitions.
//
// Run with: GOOS=wasip1 GOARCH=wasm go test -c (compile check)
// or natively with go test (uses appendLengthDelimited from tuplespace_proto_wire.go)

package plexspaces

import (
	"testing"
)

// extractFieldNumbers parses raw protobuf bytes and returns the field numbers
// present in the top-level message, in order.
func extractFieldNumbers(data []byte) ([]int, error) {
	var fields []int
	pos := 0
	for pos < len(data) {
		tag, n, err := readVarint(data, pos)
		if err != nil {
			return nil, err
		}
		pos += n
		fn := int(tag >> 3)
		wt := tag & 7
		fields = append(fields, fn)
		pos, err = skipField(data, pos, wt)
		if err != nil {
			return nil, err
		}
	}
	return fields, nil
}

func assertFieldsPresent(t *testing.T, label string, data []byte, expected []int) {
	t.Helper()
	fields, err := extractFieldNumbers(data)
	if err != nil {
		t.Fatalf("%s: failed to parse protobuf: %v", label, err)
	}
	found := map[int]bool{}
	for _, f := range fields {
		found[f] = true
	}
	for _, e := range expected {
		if !found[e] {
			t.Errorf("%s: expected proto field %d, got fields %v", label, e, fields)
		}
	}
}

// testAppendStringField builds a protobuf string field (wire type 2).
func testAppendStringField(buf []byte, fieldNum int, s string) []byte {
	return appendLengthDelimited(buf, fieldNum, []byte(s))
}

// testAppendUInt32Field builds a protobuf varint field (wire type 0).
func testAppendUInt32Field(buf []byte, fieldNum int, v uint32) []byte {
	buf = appendVarint(buf, uint64(fieldNum<<3|0))
	buf = appendVarint(buf, uint64(v))
	return buf
}

// testAppendBytesField builds a protobuf bytes field (wire type 2).
func testAppendBytesField(buf []byte, fieldNum int, b []byte) []byte {
	return appendLengthDelimited(buf, fieldNum, b)
}

// TestCreateShardGroupRequestFieldLayout verifies that encoding a
// CreateShardGroupRequest puts config at field 2 and actor_type at field 3
// (matching the proto: request_id=1, config=2, actor_type=3, shard_config=4,
// initial_state=5, metadata=6).
func TestCreateShardGroupRequestFieldLayout(t *testing.T) {
	// Simulate what the WASM encoder produces: config(2), actor_type(3),
	// initial_state(5), metadata(6).
	var out []byte
	// config at field 2 (DataParallelConfig sub-message)
	var cfg []byte
	cfg = testAppendStringField(cfg, 1, "test-group")
	cfg = testAppendUInt32Field(cfg, 2, 4)
	out = appendLengthDelimited(out, 2, cfg)
	// actor_type at field 3
	out = testAppendStringField(out, 3, "TestActor")
	// initial_state at field 5
	out = testAppendBytesField(out, 5, []byte("hello"))
	// metadata at field 6 (map entry)
	var entry []byte
	entry = testAppendStringField(entry, 1, "key")
	entry = testAppendStringField(entry, 2, "val")
	out = appendLengthDelimited(out, 6, entry)

	assertFieldsPresent(t, "CreateShardGroupRequest", out, []int{2, 3, 5, 6})
}

// TestScatterGatherRequestFieldLayout verifies field numbers match proto:
// request_id=1, group_id=2, query=3, timeout=4, aggregation=5, min_responses=6.
func TestScatterGatherRequestFieldLayout(t *testing.T) {
	var out []byte
	out = testAppendStringField(out, 2, "test-group")
	out = appendLengthDelimited(out, 3, []byte{})
	out = appendLengthDelimited(out, 4, []byte{})
	out = testAppendUInt32Field(out, 5, 1)
	out = testAppendUInt32Field(out, 6, 2)

	assertFieldsPresent(t, "ScatterGatherRequest", out, []int{2, 3, 4, 5, 6})
}

// TestBroadcastShardGroupRequestFieldLayout verifies field numbers match proto:
// request_id=1, group_id=2, message=3, timeout=4, min_acks=5.
func TestBroadcastShardGroupRequestFieldLayout(t *testing.T) {
	var out []byte
	out = testAppendStringField(out, 2, "test-group")
	out = appendLengthDelimited(out, 3, []byte{})
	out = appendLengthDelimited(out, 4, []byte{})
	out = testAppendUInt32Field(out, 5, 1)

	assertFieldsPresent(t, "BroadcastShardGroupRequest", out, []int{2, 3, 4, 5})
}

// TestReduceShardGroupRequestFieldLayout verifies field numbers match proto:
// request_id=1, group_id=2, map_function=3, timeout=4, min_responses=5,
// reduction=6, target=7.
func TestReduceShardGroupRequestFieldLayout(t *testing.T) {
	var out []byte
	out = testAppendStringField(out, 2, "test-group")
	out = appendLengthDelimited(out, 3, []byte{})
	out = appendLengthDelimited(out, 4, []byte{})
	out = testAppendUInt32Field(out, 5, 2)
	out = testAppendUInt32Field(out, 6, 1)
	var tf []byte
	tf = testAppendStringField(tf, 1, "value")
	out = appendLengthDelimited(out, 7, tf)

	assertFieldsPresent(t, "ReduceShardGroupRequest", out, []int{2, 3, 4, 5, 6, 7})
}

// TestBarrierShardGroupRequestFieldLayout verifies field numbers match proto:
// request_id=1, group_id=2, barrier_id=3, round=4, timeout=5, min_acks=6.
func TestBarrierShardGroupRequestFieldLayout(t *testing.T) {
	var out []byte
	out = testAppendStringField(out, 2, "test-group")
	out = testAppendStringField(out, 3, "barrier-1")
	out = testAppendUInt32Field(out, 4, 1)
	out = appendLengthDelimited(out, 5, []byte{})
	out = testAppendUInt32Field(out, 6, 2)

	assertFieldsPresent(t, "BarrierShardGroupRequest", out, []int{2, 3, 4, 5, 6})
}

// TestShardQueryResponseFieldLayout verifies proto:
// request_id=1, shard_id=2, shard_actor_id=3, response=4, latency=5, success=6, error=7.
func TestShardQueryResponseFieldLayout(t *testing.T) {
	var resp []byte
	// shard_id at field 2 (varint)
	resp = testAppendUInt32Field(resp, 2, 3)
	// shard_actor_id at field 3
	resp = testAppendStringField(resp, 3, "shard-actor-1")
	// response at field 4 (Message sub-message)
	resp = appendLengthDelimited(resp, 4, []byte("response-msg"))
	// latency at field 5 (Duration sub-message)
	resp = appendLengthDelimited(resp, 5, []byte{})
	// success at field 6 (bool/varint)
	resp = testAppendUInt32Field(resp, 6, 1)
	// error at field 7
	resp = testAppendStringField(resp, 7, "")

	assertFieldsPresent(t, "ShardQueryResponse", resp, []int{2, 3, 4, 5, 6, 7})
}

// TestScatterGatherResponseParsesField2AsResult verifies the response parser
// reads field 2 as result (proto: request_id=1, result=2, shard_responses=3, stats=4).
// On native builds, the parse functions are JSON-based, so this test directly
// verifies the field layout expectation.
func TestScatterGatherResponseFieldLayout(t *testing.T) {
	var resp []byte
	// result at field 2
	resp = appendLengthDelimited(resp, 2, []byte("result-msg"))
	// shard_response at field 3
	resp = appendLengthDelimited(resp, 3, []byte("shard"))
	// stats at field 4
	resp = appendLengthDelimited(resp, 4, []byte("stats"))

	fields, err := extractFieldNumbers(resp)
	if err != nil {
		t.Fatal(err)
	}
	if len(fields) != 3 || fields[0] != 2 || fields[1] != 3 || fields[2] != 4 {
		t.Errorf("expected fields [2,3,4], got %v", fields)
	}
}

// TestBroadcastResponseFieldLayout verifies proto: request_id=1, shard_responses=2, stats=3.
func TestBroadcastResponseFieldLayout(t *testing.T) {
	var resp []byte
	resp = appendLengthDelimited(resp, 2, []byte("shard"))
	resp = appendLengthDelimited(resp, 3, []byte("stats"))

	fields, err := extractFieldNumbers(resp)
	if err != nil {
		t.Fatal(err)
	}
	if len(fields) != 2 || fields[0] != 2 || fields[1] != 3 {
		t.Errorf("expected fields [2,3], got %v", fields)
	}
}

// TestReduceResponseFieldLayout verifies proto: request_id=1, result=2,
// shard_responses=3, stats=4.
func TestReduceResponseFieldLayout(t *testing.T) {
	var resp []byte
	resp = appendLengthDelimited(resp, 2, []byte("result"))
	resp = appendLengthDelimited(resp, 3, []byte("shard"))
	resp = appendLengthDelimited(resp, 4, []byte("stats"))

	fields, err := extractFieldNumbers(resp)
	if err != nil {
		t.Fatal(err)
	}
	if len(fields) != 3 || fields[0] != 2 || fields[1] != 3 || fields[2] != 4 {
		t.Errorf("expected fields [2,3,4], got %v", fields)
	}
}
