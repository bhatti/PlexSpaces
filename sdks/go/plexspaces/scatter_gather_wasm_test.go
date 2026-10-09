// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// WASM integration tests for scatter-gather proto wire decoding.
//
// These tests verify that the parse functions do not crash in TinyGo WASM
// and that they produce the expected output fields.
//
// Run natively with: go test -run TestWasmScatterGather (compile + logic check)
// Run in WASM with: GOOS=wasip1 GOARCH=wasm go test (crash check)

//go:build wasm

package plexspaces

import (
	"testing"
)

// buildStatsProto encodes a ScatterGatherStats proto message.
// proto fields: shards_queried=1, shards_responded=2, shards_failed=3, max_latency=4 (Duration).
func buildStatsProto(queried, responded, failed uint32) []byte {
	var b []byte
	b = testAppendUInt32Field(b, 1, queried)
	b = testAppendUInt32Field(b, 2, responded)
	b = testAppendUInt32Field(b, 3, failed)
	return b
}

// buildShardQueryResponseProto encodes a minimal ShardQueryResponse proto.
// proto: shard_id=2, shard_actor_id=3, response=4 (Message), success=6.
func buildShardQueryResponseProto(shardID uint32, actorID string, payload []byte, success bool) []byte {
	var b []byte
	b = testAppendUInt32Field(b, 2, shardID)
	b = testAppendStringField(b, 3, actorID)
	if len(payload) > 0 {
		// response is a CommonMessage; wrap payload in field 2
		var msg []byte
		msg = appendLengthDelimited(msg, 2, payload)
		b = appendLengthDelimited(b, 4, msg)
	}
	if success {
		b = testAppendUInt32Field(b, 6, 1)
	}
	return b
}

// buildScatterGatherResponseProto encodes a ScatterGatherResponse proto.
// proto: result=2, shard_responses=3, stats=4.
func buildScatterGatherResponseProto(shards [][]byte, statsBytes []byte) []byte {
	var b []byte
	for _, sh := range shards {
		b = appendLengthDelimited(b, 3, sh)
	}
	if statsBytes != nil {
		b = appendLengthDelimited(b, 4, statsBytes)
	}
	return b
}

// TestWasmParseScatterGatherStatsFlat verifies wasmParseScatterGatherStatsFlat
// correctly decodes all stat fields from proto bytes.
func TestWasmParseScatterGatherStatsFlat(t *testing.T) {
	stats := buildStatsProto(10, 9, 1)
	q, r, f, ml := wasmParseScatterGatherStatsFlat(stats)
	if q != 10 {
		t.Errorf("shards_queried: got %v, want 10", q)
	}
	if r != 9 {
		t.Errorf("shards_responded: got %v, want 9", r)
	}
	if f != 1 {
		t.Errorf("shards_failed: got %v, want 1", f)
	}
	if ml != 0 {
		t.Errorf("max_latency_ms: got %v, want 0 (no duration field)", ml)
	}
}

// TestWasmParseScatterGatherStatsFlatEmpty verifies empty input returns zeros.
func TestWasmParseScatterGatherStatsFlatEmpty(t *testing.T) {
	q, r, f, ml := wasmParseScatterGatherStatsFlat([]byte{})
	if q != 0 || r != 0 || f != 0 || ml != 0 {
		t.Errorf("empty stats: want all zeros, got q=%v r=%v f=%v ml=%v", q, r, f, ml)
	}
}

// TestWasmParseScatterGatherResponseWithStats verifies that parsing a response with
// a stats field produces the expected _stat_* keys. This is the primary regression
// test for the TinyGo WASM crash: "out[stats] = wasmParseScatterGatherStats(sl)".
func TestWasmParseScatterGatherResponseWithStats(t *testing.T) {
	shard := buildShardQueryResponseProto(0, "shard-0", []byte(`{"status":"ok"}`), true)
	stats := buildStatsProto(4, 4, 0)
	resp := buildScatterGatherResponseProto([][]byte{shard}, stats)

	out, err := wasmParseScatterGatherResponse(resp)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}

	// _stat_ fields must be present and correct
	checkFloat(t, out, "_stat_shards_queried", 4)
	checkFloat(t, out, "_stat_shards_responded", 4)
	checkFloat(t, out, "_stat_shards_failed", 0)

	// shard_responses must be a []map[string]any slice with 1 entry
	shards, ok := out["shard_responses"].([]map[string]any)
	if !ok {
		t.Fatalf("shard_responses: expected []map[string]any, got %T", out["shard_responses"])
	}
	if len(shards) != 1 {
		t.Errorf("shard_responses: expected 1 shard, got %d", len(shards))
	}
}

// TestWasmParseScatterGatherResponseNoStats verifies the fallback zeros for
// responses that have no stats field.
func TestWasmParseScatterGatherResponseNoStats(t *testing.T) {
	resp := buildScatterGatherResponseProto(nil, nil)
	out, err := wasmParseScatterGatherResponse(resp)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	checkFloat(t, out, "_stat_shards_queried", 0)
	checkFloat(t, out, "_stat_shards_responded", 0)
	checkFloat(t, out, "_stat_shards_failed", 0)
	checkFloat(t, out, "_stat_max_latency_ms", 0)
}

// TestWasmParseScatterGatherResponseMultipleShards verifies multi-shard responses.
func TestWasmParseScatterGatherResponseMultipleShards(t *testing.T) {
	shards := [][]byte{
		buildShardQueryResponseProto(0, "shard-0", []byte(`{"v":1}`), true),
		buildShardQueryResponseProto(1, "shard-1", []byte(`{"v":2}`), true),
		buildShardQueryResponseProto(2, "shard-2", []byte(`{"v":3}`), false),
	}
	stats := buildStatsProto(3, 2, 1)
	resp := buildScatterGatherResponseProto(shards, stats)

	out, err := wasmParseScatterGatherResponse(resp)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	checkFloat(t, out, "_stat_shards_queried", 3)
	checkFloat(t, out, "_stat_shards_responded", 2)
	checkFloat(t, out, "_stat_shards_failed", 1)

	sl, ok := out["shard_responses"].([]map[string]any)
	if !ok || len(sl) != 3 {
		t.Errorf("shard_responses: expected 3, got %v (type %T)", out["shard_responses"], out["shard_responses"])
	}
}

// TestWasmParseShardQueryResponseBasic verifies shard query response parsing.
func TestWasmParseShardQueryResponseBasic(t *testing.T) {
	shard := buildShardQueryResponseProto(2, "shard-actor-2", []byte(`{"count":42}`), true)
	out := wasmParseShardQueryResponse(shard)

	if id, ok := out["shard_id"]; !ok || id != float64(2) {
		t.Errorf("shard_id: got %v, want 2", out["shard_id"])
	}
	if aid, ok := out["shard_actor_id"]; !ok || aid != "shard-actor-2" {
		t.Errorf("shard_actor_id: got %v, want shard-actor-2", out["shard_actor_id"])
	}
	if ok, _ := out["success"].(bool); !ok {
		t.Errorf("success: got %v, want true", out["success"])
	}
}

// TestWasmScatterGatherRequestEncoding verifies proto field layout of a typed request.
func TestWasmScatterGatherRequestEncoding(t *testing.T) {
	req := ScatterGatherRequest{
		GroupID:      "test-group",
		Query:        map[string]any{"op": "compute", "value": float64(42)},
		Aggregation:  "concat",
		TimeoutMs:    5000,
		MinResponses: 2,
	}
	wire, err := hostWireScatterGatherRequest(req)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	fieldSet := protoFieldSet(t, []byte(wire))
	for _, f := range []int{2, 3, 4, 5, 6} {
		if !fieldSet[f] {
			t.Errorf("expected proto field %d, got set %v", f, fieldSet)
		}
	}
}

// TestWasmScatterGatherRequestMissingQuery expects an error when Query has no op/message_type.
func TestWasmScatterGatherRequestMissingQuery(t *testing.T) {
	req := ScatterGatherRequest{GroupID: "g", Query: map[string]any{}}
	_, err := hostWireScatterGatherRequest(req)
	if err == nil {
		t.Error("expected error for missing query message_type, got nil")
	}
}

// TestWasmBroadcastRequestEncoding verifies proto field layout.
func TestWasmBroadcastRequestEncoding(t *testing.T) {
	req := BroadcastShardGroupRequest{
		GroupID:   "grp",
		Message:   map[string]any{"op": "ping"},
		TimeoutMs: 2000,
		MinAcks:   1,
	}
	wire, err := hostWireBroadcastShardGroupRequest(req)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	fieldSet := protoFieldSet(t, []byte(wire))
	for _, f := range []int{2, 3, 4, 5} {
		if !fieldSet[f] {
			t.Errorf("expected proto field %d, got set %v", f, fieldSet)
		}
	}
}

// TestWasmBroadcastRequestEmptyMessage expects error.
func TestWasmBroadcastRequestEmptyMessage(t *testing.T) {
	req := BroadcastShardGroupRequest{GroupID: "g"}
	_, err := hostWireBroadcastShardGroupRequest(req)
	if err == nil {
		t.Error("expected error for empty Message, got nil")
	}
}

// TestWasmCreateShardGroupRequestEncoding verifies proto field layout.
func TestWasmCreateShardGroupRequestEncoding(t *testing.T) {
	req := CreateShardGroupRequest{
		GroupID:    "my-group",
		ActorType:  "Worker",
		ShardCount: 4,
		Placement:  NodePlacement{Strategy: "from_registry"},
	}
	wire, err := hostWireCreateShardGroupRequest(req)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	fieldSet := protoFieldSet(t, []byte(wire))
	// config=2, actor_type=3
	for _, f := range []int{2, 3} {
		if !fieldSet[f] {
			t.Errorf("expected proto field %d, got set %v", f, fieldSet)
		}
	}
}

// TestWasmReduceRequestEncoding verifies proto field layout.
func TestWasmReduceRequestEncoding(t *testing.T) {
	req := ReduceShardGroupRequest{
		GroupID:      "g",
		MapFunction:  map[string]any{"op": "sum_values"},
		Reduction:    "sum",
		TimeoutMs:    3000,
		MinResponses: 1,
		Target:       "result",
	}
	wire, err := hostWireReduceShardGroupRequest(req)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	fieldSet := protoFieldSet(t, []byte(wire))
	// group_id=2, map_function=3, timeout=4, min_responses=5, reduction=6, target=7
	for _, f := range []int{2, 3, 4, 5, 6, 7} {
		if !fieldSet[f] {
			t.Errorf("expected proto field %d, got set %v", f, fieldSet)
		}
	}
}

// TestWasmBarrierRequestEncoding verifies proto field layout.
func TestWasmBarrierRequestEncoding(t *testing.T) {
	req := BarrierShardGroupRequest{
		GroupID:   "g",
		BarrierID: "b1",
		Round:     2,
		TimeoutMs: 1000,
		MinAcks:   3,
	}
	wire, err := hostWireBarrierShardGroupRequest(req)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	fieldSet := protoFieldSet(t, []byte(wire))
	for _, f := range []int{2, 3, 4, 5, 6} {
		if !fieldSet[f] {
			t.Errorf("expected proto field %d, got set %v", f, fieldSet)
		}
	}
}

// TestWasmEncodeQueryPayloadInjectsMessageType verifies message_type is in payload when derived from op.
func TestWasmEncodeQueryPayloadInjectsMessageType(t *testing.T) {
	q := map[string]any{"op": "train", "value": float64(1)}
	mt, payload := encodeQueryPayload(q)
	if mt != "train" {
		t.Errorf("msgType: got %q, want %q", mt, "train")
	}
	// payload JSON must contain "message_type"
	ps := string(payload)
	if ps == "" || (len(ps) > 0 && !containsStr(ps, `"message_type":"train"`)) {
		t.Errorf("payload %q should contain message_type:train", ps)
	}
	// Must not mutate original map
	if _, ok := q["message_type"]; ok {
		t.Error("encodeQueryPayload must not mutate the input map")
	}
}

func containsStr(s, sub string) bool {
	return len(s) >= len(sub) && (s == sub || len(s) > 0 && indexOf(s, sub) >= 0)
}

func indexOf(s, sub string) int {
	for i := 0; i <= len(s)-len(sub); i++ {
		if s[i:i+len(sub)] == sub {
			return i
		}
	}
	return -1
}

// protoFieldSet parses top-level field numbers from proto bytes.
func protoFieldSet(t *testing.T, data []byte) map[int]bool {
	t.Helper()
	fields, err := extractFieldNumbers(data)
	if err != nil {
		t.Fatalf("failed to parse proto fields: %v", err)
	}
	set := map[int]bool{}
	for _, f := range fields {
		set[f] = true
	}
	return set
}

func checkFloat(t *testing.T, m map[string]any, key string, want float64) {
	t.Helper()
	v, ok := m[key]
	if !ok {
		t.Errorf("key %q missing from map", key)
		return
	}
	f, ok := v.(float64)
	if !ok {
		t.Errorf("key %q: expected float64, got %T (%v)", key, v, v)
		return
	}
	if f != want {
		t.Errorf("key %q: got %v, want %v", key, f, want)
	}
}
