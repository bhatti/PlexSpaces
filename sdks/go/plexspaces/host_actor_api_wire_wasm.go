// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// WASM builds: shard-group and application host imports use protobuf wire bytes.
// TinyGo cannot link google.golang.org/protobuf or generated .pb.go (protoreflect
// init panics: reflect: unimplemented: AssignableTo with interface). This file
// implements the subset of messages used by Go examples via manual wire encoding
// and decoding, reusing appendVarint/appendLengthDelimited/readVarint/skipField
// from tuplespace_proto_wire.go.

//go:build wasm

package plexspaces

import (
	"fmt"
	"strconv"
	"strings"
)

// --- map / scalar helpers (same behavior as prior wasm wire) ---

func mapAsStringAny(v any) (map[string]any, bool) {
	m, ok := v.(map[string]any)
	return m, ok
}


func strVal(v any) string {
	if v == nil {
		return ""
	}
	switch t := v.(type) {
	case string:
		return t
	case float64:
		return strconv.FormatInt(int64(t), 10)
	case int:
		return strconv.Itoa(t)
	case int32:
		return strconv.FormatInt(int64(t), 10)
	case int64:
		return strconv.FormatInt(t, 10)
	case uint32:
		return strconv.FormatUint(uint64(t), 10)
	case uint64:
		return strconv.FormatUint(t, 10)
	case bool:
		if t {
			return "true"
		}
		return "false"
	default:
		// Avoid fmt.Sprint: it calls fmtsort.Sort for maps which crashes in TinyGo WASM.
		// All strVal callers pass string/numeric/nil values; unknown types return empty string.
		_ = t
		return ""
	}
}

func u32Val(v any) uint32 {
	switch t := v.(type) {
	case float64:
		if t < 0 {
			return 0
		}
		return uint32(t)
	case int:
		if t < 0 {
			return 0
		}
		return uint32(t)
	case int64:
		if t < 0 {
			return 0
		}
		return uint32(t)
	case uint32:
		return t
	case uint64:
		return uint32(t)
	case string:
		n, err := strconv.ParseUint(t, 10, 32)
		if err != nil {
			return 0
		}
		return uint32(n)
	default:
		return 0
	}
}

func u64Val(v any) uint64 {
	switch t := v.(type) {
	case float64:
		if t < 0 {
			return 0
		}
		return uint64(t)
	case int:
		if t < 0 {
			return 0
		}
		return uint64(t)
	case int64:
		if t < 0 {
			return 0
		}
		return uint64(t)
	case uint64:
		return t
	case uint32:
		return uint64(t)
	case string:
		n, err := strconv.ParseUint(t, 10, 64)
		if err != nil {
			return 0
		}
		return n
	default:
		return 0
	}
}


// --- protobuf wire (manual) ---

func wasmAppendTagVarint(buf []byte, fieldNum int, wireType int) []byte {
	return appendVarint(buf, uint64(fieldNum<<3|wireType))
}

func wasmAppendString(buf []byte, fieldNum int, s string) []byte {
	return appendLengthDelimited(buf, fieldNum, []byte(s))
}

func wasmAppendBytes(buf []byte, fieldNum int, b []byte) []byte {
	return appendLengthDelimited(buf, fieldNum, b)
}

func wasmAppendUInt32(buf []byte, fieldNum int, v uint32) []byte {
	buf = wasmAppendTagVarint(buf, fieldNum, 0)
	return appendVarint(buf, uint64(v))
}

func wasmAppendUInt64(buf []byte, fieldNum int, v uint64) []byte {
	buf = wasmAppendTagVarint(buf, fieldNum, 0)
	return appendVarint(buf, v)
}

func wasmAppendInt64(buf []byte, fieldNum int, v int64) []byte {
	buf = wasmAppendTagVarint(buf, fieldNum, 0)
	return appendVarint(buf, uint64(v))
}

func wasmAppendBool(buf []byte, fieldNum int, v bool) []byte {
	u := uint64(0)
	if v {
		u = 1
	}
	return wasmAppendUInt64(buf, fieldNum, u)
}

func wasmAppendUint64Map(buf []byte, fieldNum int, m map[string]uint64) []byte {
	for k, v := range m {
		var e []byte
		e = wasmAppendString(e, 1, k)
		e = wasmAppendUInt64(e, 2, v)
		buf = appendLengthDelimited(buf, fieldNum, e)
	}
	return buf
}

func wasmAppendStringMap(buf []byte, fieldNum int, m map[string]string) []byte {
	for k, v := range m {
		var e []byte
		e = wasmAppendString(e, 1, k)
		e = wasmAppendString(e, 2, v)
		buf = appendLengthDelimited(buf, fieldNum, e)
	}
	return buf
}

func wasmEncodeDurationFromMs(ms uint64) []byte {
	if ms == 0 {
		return nil
	}
	sec := int64(ms / 1000)
	nanos := int32((ms % 1000) * 1_000_000)
	var d []byte
	d = wasmAppendInt64(d, 1, sec)
	if nanos != 0 {
		d = wasmAppendUInt32(d, 2, uint32(nanos))
	}
	return d
}

func wasmEncodeCommonMessage(messageType string, payload []byte) []byte {
	var m []byte
	if messageType != "" {
		m = wasmAppendString(m, 5, messageType)
	}
	if len(payload) > 0 {
		m = wasmAppendBytes(m, 6, payload)
	}
	return m
}

func partitionEnum(s string) uint32 {
	switch strings.ToLower(strings.TrimSpace(s)) {
	case "hash", "partition_strategy_hash":
		return 1
	case "range":
		return 2
	case "consistent_hash", "consistent-hash":
		return 3
	case "custom":
		return 99
	default:
		return 0
	}
}

func rebalanceEnum(s string) uint32 {
	switch strings.ToLower(strings.TrimSpace(s)) {
	case "none", "manual":
		return 1
	case "on_scale", "on-scale":
		return 2
	case "load_based", "load-based":
		return 3
	default:
		return 0
	}
}

func nodePlacementEnum(s string) uint32 {
	switch strings.ToLower(strings.TrimSpace(s)) {
	case "same_node", "same-node":
		return 1
	case "from_registry", "from-registry":
		return 2
	case "node_ids", "node-ids":
		return 3
	default:
		return 0
	}
}

// wasmEncodeNodePlacementTyped encodes a typed NodePlacement into proto bytes.
// Proto: strategy=1, cluster=2, node_ids=3, required_labels=4, avoid_node_ids=5, affinity_labels=7.
func wasmEncodeNodePlacementTyped(p NodePlacement) []byte {
	if p.Strategy == "" && len(p.NodeIDs) == 0 && p.Cluster == "" &&
		len(p.RequiredLabels) == 0 && len(p.AvoidNodeIDs) == 0 && len(p.AffinityLabels) == 0 {
		return nil
	}
	var pb []byte
	pb = wasmAppendUInt32(pb, 1, nodePlacementEnum(p.Strategy))
	if p.Cluster != "" {
		pb = wasmAppendString(pb, 2, p.Cluster)
	}
	for _, id := range p.NodeIDs {
		if id != "" {
			pb = wasmAppendString(pb, 3, id)
		}
	}
	if len(p.RequiredLabels) > 0 {
		pb = wasmAppendStringMap(pb, 4, p.RequiredLabels)
	}
	for _, id := range p.AvoidNodeIDs {
		if id != "" {
			pb = wasmAppendString(pb, 5, id)
		}
	}
	if len(p.AffinityLabels) > 0 {
		pb = wasmAppendStringMap(pb, 7, p.AffinityLabels)
	}
	return pb
}

// wasmEncodeDataParallelConfigTyped encodes a typed CreateShardGroupRequest config.
// Proto: group_id=1, shard_count=2, partition_strategy=4, rebalance_policy=5, placement=6.
func wasmEncodeDataParallelConfigTyped(groupID string, shardCount int, part, reb uint32, placement NodePlacement) []byte {
	var c []byte
	c = wasmAppendString(c, 1, groupID)
	c = wasmAppendUInt32(c, 2, uint32(shardCount))
	c = wasmAppendUInt32(c, 4, part)
	c = wasmAppendUInt32(c, 5, reb)
	if pb := wasmEncodeNodePlacementTyped(placement); len(pb) > 0 {
		c = appendLengthDelimited(c, 6, pb)
	}
	return c
}

func aggregationEnum(s string) uint32 {
	switch strings.ToLower(strings.TrimSpace(s)) {
	case "concat":
		return 1
	case "merge":
		return 2
	case "first":
		return 3
	case "majority":
		return 4
	default:
		return 0
	}
}

func reductionEnum(s string) uint32 {
	switch strings.ToLower(strings.TrimSpace(s)) {
	case "sum":
		return 1
	case "min":
		return 2
	case "max":
		return 3
	case "product":
		return 4
	case "concat":
		return 5
	case "bool_and", "bool-and":
		return 6
	case "bool_or", "bool-or":
		return 7
	default:
		return 0
	}
}

// encodeQueryPayload serialises q to proto payload bytes and derives the message type.
// Does NOT mutate q. If message_type is derived from "op" and not present in q,
// it is injected into the JSON payload so worker actors can route on it.
func encodeQueryPayload(q map[string]any) (msgType string, payload []byte) {
	mt, _ := q["message_type"].(string)
	if mt == "" {
		mt, _ = q["op"].(string)
	}
	base := SafeMarshal(q)
	if mt != "" {
		if _, exists := q["message_type"]; !exists {
			// Inject message_type without mutating q.
			if len(base) > 1 {
				payload = []byte(`{"message_type":"` + mt + `",` + base[1:])
			} else {
				payload = []byte(`{"message_type":"` + mt + `"}`)
			}
		} else {
			payload = []byte(base)
		}
	} else {
		payload = []byte(base)
	}
	return mt, payload
}

// encodeBroadcastPayload serialises a broadcast message map to proto payload bytes.
func encodeBroadcastPayload(msg map[string]any) (msgType string, payload []byte) {
	mt, _ := msg["message_type"].(string)
	if mt == "" {
		mt, _ = msg["op"].(string)
	}
	payload = []byte(SafeMarshal(msg))
	return mt, payload
}

// encodeMapFunctionPayload serialises a map-function map to proto payload bytes.
// Injects message_type if derived from "op" and not already present.
func encodeMapFunctionPayload(fn map[string]any) (msgType string, payload []byte) {
	mt, _ := fn["message_type"].(string)
	if mt == "" {
		mt, _ = fn["op"].(string)
	}
	base := SafeMarshal(fn)
	if mt != "" {
		if _, exists := fn["message_type"]; !exists {
			if len(base) > 1 {
				payload = []byte(`{"message_type":"` + mt + `",` + base[1:])
			} else {
				payload = []byte(`{"message_type":"` + mt + `"}`)
			}
		} else {
			payload = []byte(base)
		}
	} else {
		payload = []byte(base)
	}
	return mt, payload
}

// --- decode helpers ---

func wasmReadLengthDelimited(data []byte, pos int) (chunk []byte, newPos int, err error) {
	ln, n, err := readVarint(data, pos)
	if err != nil {
		return nil, 0, err
	}
	pos += n
	end := pos + int(ln)
	if end > len(data) {
		return nil, 0, fmt.Errorf("length-delimited underflow")
	}
	return data[pos:end], end, nil
}

func wasmParseDuration(data []byte) (ms int64) {
	pos := 0
	var sec int64
	var nanos int32
	for pos < len(data) {
		tag, n, err := readVarint(data, pos)
		if err != nil {
			return 0
		}
		pos += n
		fn := int(tag >> 3)
		wt := tag & 7
		switch wt {
		case 0:
			v, m, err := readVarint(data, pos)
			if err != nil {
				return 0
			}
			pos += m
			if fn == 1 {
				sec = int64(v)
			} else if fn == 2 {
				nanos = int32(v)
			}
		default:
			pos, err = skipField(data, pos, wt)
			if err != nil {
				return 0
			}
		}
	}
	return sec*1000 + int64(nanos)/1_000_000
}

func wasmParseCommonMessage(data []byte) (msgType string, payload []byte) {
	gParseCommonData = data // pin subslice so its GC block stays alive across string/[]byte allocations
	data = gParseCommonData
	pos := 0
	for pos < len(data) {
		tag, n, err := readVarint(data, pos)
		if err != nil {
			return "", nil
		}
		pos += n
		fn := int(tag >> 3)
		wt := tag & 7
		if fn == 5 && wt == 2 {
			sl, np, err := wasmReadLengthDelimited(data, pos)
			if err != nil {
				return "", nil
			}
			msgType = string(sl)
			pos = np
			continue
		}
		if fn == 6 && wt == 2 {
			sl, np, err := wasmReadLengthDelimited(data, pos)
			if err != nil {
				return "", nil
			}
			payload = append([]byte(nil), sl...)
			pos = np
			continue
		}
		pos, err = skipField(data, pos, wt)
		if err != nil {
			return "", nil
		}
	}
	return msgType, payload
}

func wasmPayloadToAny(payload []byte) any {
	if len(payload) == 0 {
		return nil
	}
	// Return raw JSON string. Callers that need a decoded map call json.Unmarshal
	// themselves on the string. This avoids encoding/json in the WASM binary which
	// links fmtsort and corrupts the actor interface dispatch table (TinyGo 0.40.x).
	return string(payload)
}

// gReducedResultPin pins the string created inside wasmReducedResultFromMessagePayload
// so TinyGo's conservative GC does not collect its backing array before TrimSpace reads it.
// WASM locals are not scanned; globals always are.
var gReducedResultPin string

func wasmReducedResultFromMessagePayload(payload []byte) any {
	if len(payload) == 0 {
		return nil
	}
	// Pin to global before any allocation can trigger GC (TinyGo 0.40 WASM local GC issue).
	gReducedResultPin = string(payload)
	s := gReducedResultPin
	// Try to parse as a raw scalar (number, bool, null).
	if f, err := strconv.ParseFloat(strings.TrimSpace(s), 64); err == nil {
		gReducedResultPin = ""
		return f
	}
	// Try to extract a known numeric key from JSON object like {"value":42}.
	for _, key := range []string{"partial_sum", "value", "result", "total", "reduced_value"} {
		if f, ok := wasmExtractJSONNumber(s, key); ok {
			gReducedResultPin = ""
			return f
		}
	}
	// Return s (backed by gReducedResultPin); caller stores in map so the string lives on.
	return s
}

// wasmExtractJSONNumber extracts a numeric value for a JSON key without json.Unmarshal.
// s is assumed to already be pinned (backed by gReducedResultPin or another global).
func wasmExtractJSONNumber(s, key string) (float64, bool) {
	needle := `"` + key + `":`
	idx := strings.Index(s, needle)
	if idx < 0 {
		return 0, false
	}
	// Manually trim leading whitespace without strings.TrimSpace to avoid WASM local GC issue.
	start := idx + len(needle)
	for start < len(s) && (s[start] == ' ' || s[start] == '\t' || s[start] == '\n' || s[start] == '\r') {
		start++
	}
	end := start
	for end < len(s) && s[end] != ',' && s[end] != '}' && s[end] != ']' {
		end++
	}
	// Trim trailing whitespace inline without allocating a new string.
	for end > start && (s[end-1] == ' ' || s[end-1] == '\t' || s[end-1] == '\n' || s[end-1] == '\r') {
		end--
	}
	v := s[start:end]
	if f, err := strconv.ParseFloat(v, 64); err == nil {
		return f, true
	}
	return 0, false
}

func wasmParseShardQueryResponse(data []byte) map[string]any {
	// Proto: request_id=1, shard_id=2, shard_actor_id=3, response=4, latency=5, success=6, error=7
	gParseShardData = data // pin subslice to keep its GC block alive across map/string allocations
	data = gParseShardData
	gParseShardOut = map[string]any{}
	out := gParseShardOut
	pos := 0
	for pos < len(data) {
		tag, n, err := readVarint(data, pos)
		if err != nil {
			return out
		}
		pos += n
		fn := int(tag >> 3)
		wt := tag & 7
		switch {
		case fn == 2 && wt == 0:
			v, m, err := readVarint(data, pos)
			if err != nil {
				return out
			}
			pos += m
			out["shard_id"] = float64(v)
		case fn == 3 && wt == 2:
			sl, np, err := wasmReadLengthDelimited(data, pos)
			if err != nil {
				return out
			}
			out["shard_actor_id"] = string(sl)
			pos = np
		case fn == 4 && wt == 2:
			sl, np, err := wasmReadLengthDelimited(data, pos)
			if err != nil {
				return out
			}
			mt, pl := wasmParseCommonMessage(sl)
			_ = mt
			p := wasmPayloadToAny(pl)
			out["payload"] = p
			out["response"] = p
			pos = np
		case fn == 5 && wt == 2:
			sl, np, err := wasmReadLengthDelimited(data, pos)
			if err != nil {
				return out
			}
			ms := wasmParseDuration(sl)
			out["latency_ms"] = float64(ms)
			pos = np
		case fn == 6 && wt == 0:
			v, m, err := readVarint(data, pos)
			if err != nil {
				return out
			}
			pos += m
			out["success"] = v != 0
		case fn == 7 && wt == 2:
			sl, np, err := wasmReadLengthDelimited(data, pos)
			if err != nil {
				return out
			}
			out["error"] = string(sl)
			pos = np
		default:
			pos, err = skipField(data, pos, wt)
			if err != nil {
				return out
			}
		}
	}
	return out
}

func wasmParseScatterGatherStats(data []byte) map[string]any {
	out := map[string]any{
		"shards_queried": float64(0), "shards_responded": float64(0), "shards_failed": float64(0),
	}
	pos := 0
	for pos < len(data) {
		tag, n, err := readVarint(data, pos)
		if err != nil {
			return out
		}
		pos += n
		fn := int(tag >> 3)
		wt := tag & 7
		if wt != 0 && !(fn == 4 && wt == 2) {
			pos, err = skipField(data, pos, wt)
			if err != nil {
				return out
			}
			continue
		}
		if fn == 4 && wt == 2 {
			sl, np, err := wasmReadLengthDelimited(data, pos)
			if err != nil {
				return out
			}
			ms := wasmParseDuration(sl)
			out["max_latency_ms"] = float64(ms)
			pos = np
			continue
		}
		v, m, err := readVarint(data, pos)
		if err != nil {
			return out
		}
		pos += m
		switch fn {
		case 1:
			out["shards_queried"] = float64(v)
		case 2:
			out["shards_responded"] = float64(v)
		case 3:
			out["shards_failed"] = float64(v)
		}
	}
	return out
}

// wasmParseScatterGatherStatsFlat parses scatter-gather stats proto bytes and
// returns plain float64 values. Returns nothing into a map so that callers can
// write the results directly into their own local map[string]any variables.
//
// TinyGo WASM bug: writing to a map[string]any that was passed as a function
// parameter uses a broken code path (corrupted call_indirect via the interface
// value type descriptor). Callers must do the map stores themselves in the same
// scope where the map was declared as a local variable.
func wasmParseScatterGatherStatsFlat(data []byte) (queried, responded, failed, maxLatencyMs float64) {
	pos := 0
	for pos < len(data) {
		tag, n, err := readVarint(data, pos)
		if err != nil {
			return
		}
		pos += n
		fn := int(tag >> 3)
		wt := tag & 7
		if wt != 0 && !(fn == 4 && wt == 2) {
			pos, _ = skipField(data, pos, wt)
			continue
		}
		if fn == 4 && wt == 2 {
			sl, np, err := wasmReadLengthDelimited(data, pos)
			if err != nil {
				return
			}
			maxLatencyMs = float64(wasmParseDuration(sl))
			pos = np
			continue
		}
		v, m, err := readVarint(data, pos)
		if err != nil {
			return
		}
		pos += m
		switch fn {
		case 1:
			queried = float64(v)
		case 2:
			responded = float64(v)
		case 3:
			failed = float64(v)
		}
	}
	return
}

func wasmParseDataParallelConfig(data []byte) (groupID string, shardCount uint32, partStr, rebStr string) {
	pos := 0
	for pos < len(data) {
		tag, n, err := readVarint(data, pos)
		if err != nil {
			return
		}
		pos += n
		fn := int(tag >> 3)
		wt := tag & 7
		switch {
		case fn == 1 && wt == 2:
			sl, np, err := wasmReadLengthDelimited(data, pos)
			if err != nil {
				return
			}
			groupID = string(sl)
			pos = np
		case fn == 2 && wt == 0:
			v, m, err := readVarint(data, pos)
			if err != nil {
				return
			}
			pos += m
			shardCount = uint32(v)
		case fn == 4 && wt == 0:
			v, m, err := readVarint(data, pos)
			if err != nil {
				return
			}
			pos += m
			partStr = partitionEnumToStr(uint32(v))
		case fn == 5 && wt == 0:
			v, m, err := readVarint(data, pos)
			if err != nil {
				return
			}
			pos += m
			rebStr = rebalanceEnumToStr(uint32(v))
		default:
			pos, err = skipField(data, pos, wt)
			if err != nil {
				return
			}
		}
	}
	return
}

func partitionEnumToStr(e uint32) string {
	switch e {
	case 1:
		return "PARTITION_STRATEGY_HASH"
	case 2:
		return "PARTITION_STRATEGY_RANGE"
	case 3:
		return "PARTITION_STRATEGY_CONSISTENT_HASH"
	case 99:
		return "PARTITION_STRATEGY_CUSTOM"
	default:
		return "PARTITION_STRATEGY_UNSPECIFIED"
	}
}

func rebalanceEnumToStr(e uint32) string {
	switch e {
	case 1:
		return "REBALANCE_POLICY_NONE"
	case 2:
		return "REBALANCE_POLICY_ON_SCALE"
	case 3:
		return "REBALANCE_POLICY_LOAD_BASED"
	default:
		return "REBALANCE_POLICY_UNSPECIFIED"
	}
}

func wasmParseShardGroup(data []byte) map[string]any {
	out := map[string]any{
		"metadata": map[string]any{}, "rebalance_status": nil,
	}
	var shardIDs []string
	pos := 0
	for pos < len(data) {
		tag, n, err := readVarint(data, pos)
		if err != nil {
			return out
		}
		pos += n
		fn := int(tag >> 3)
		wt := tag & 7
		switch {
		case fn == 1 && wt == 2:
			sl, np, err := wasmReadLengthDelimited(data, pos)
			if err != nil {
				return out
			}
			gid, sc, ps, rs := wasmParseDataParallelConfig(sl)
			out["group_id"] = gid
			out["shard_count"] = float64(sc)
			out["partition_strategy"] = ps
			out["rebalance_policy"] = rs
			pos = np
		case fn == 2 && wt == 2:
			sl, np, err := wasmReadLengthDelimited(data, pos)
			if err != nil {
				return out
			}
			out["actor_type"] = string(sl)
			pos = np
		case fn == 3 && wt == 2:
			sl, np, err := wasmReadLengthDelimited(data, pos)
			if err != nil {
				return out
			}
			shardIDs = append(shardIDs, string(sl))
			pos = np
		default:
			pos, err = skipField(data, pos, wt)
			if err != nil {
				return out
			}
		}
	}
	out["shard_actor_ids"] = stringSliceToAny(shardIDs)
	return out
}

func stringSliceToAny(ids []string) []any {
	a := make([]any, len(ids))
	for i, s := range ids {
		a[i] = s
	}
	return a
}

func wasmParseCreateShardGroupResponse(data []byte) (map[string]any, error) {
	pos := 0
	for pos < len(data) {
		tag, n, err := readVarint(data, pos)
		if err != nil {
			return nil, err
		}
		pos += n
		fn := int(tag >> 3)
		wt := tag & 7
		if fn == 2 && wt == 2 {
			sl, _, err := wasmReadLengthDelimited(data, pos)
			if err != nil {
				return nil, err
			}
			return wasmParseShardGroup(sl), nil
		}
		pos, err = skipField(data, pos, wt)
		if err != nil {
			return nil, err
		}
	}
	return map[string]any{}, nil
}

func wasmParseScatterGatherResponse(data []byte) (map[string]any, error) {
	gParseOut = map[string]any{}
	out := gParseOut
	gParseShards = gParseShards[:0] // reset without re-allocating when possible
	pos := 0
	for pos < len(data) {
		tag, n, err := readVarint(data, pos)
		if err != nil {
			return nil, err
		}
		pos += n
		fn := int(tag >> 3)
		wt := tag & 7
		switch {
		case fn == 2 && wt == 2:
			sl, np, err := wasmReadLengthDelimited(data, pos)
			if err != nil {
				return nil, err
			}
			_, pl := wasmParseCommonMessage(sl)
			out["result"] = wasmPayloadToAny(pl)
			pos = np
		case fn == 3 && wt == 2:
			sl, np, err := wasmReadLengthDelimited(data, pos)
			if err != nil {
				return nil, err
			}
			gParseShards = append(gParseShards, wasmParseShardQueryResponse(sl))
			pos = np
		case fn == 4 && wt == 2:
			sl, np, err := wasmReadLengthDelimited(data, pos)
			if err != nil {
				return nil, err
			}
			q, r, f, ml := wasmParseScatterGatherStatsFlat(sl)
			out["_stat_shards_queried"] = q
			out["_stat_shards_responded"] = r
			out["_stat_shards_failed"] = f
			out["_stat_max_latency_ms"] = ml
			pos = np
		default:
			pos, err = skipField(data, pos, wt)
			if err != nil {
				return nil, err
			}
		}
	}
	out["shard_responses"] = gParseShards
	if _, ok := out["_stat_shards_queried"]; !ok {
		out["_stat_shards_queried"] = float64(0)
		out["_stat_shards_responded"] = float64(0)
		out["_stat_shards_failed"] = float64(0)
		out["_stat_max_latency_ms"] = float64(0)
	}
	gParseOut = nil
	return out, nil
}

func wasmParseBroadcastOrBarrierResponse(data []byte) (map[string]any, error) {
	gParseOut = map[string]any{}
	out := gParseOut
	gParseShards = gParseShards[:0]
	pos := 0
	for pos < len(data) {
		tag, n, err := readVarint(data, pos)
		if err != nil {
			return nil, err
		}
		pos += n
		fn := int(tag >> 3)
		wt := tag & 7
		switch {
		case fn == 2 && wt == 2:
			sl, np, err := wasmReadLengthDelimited(data, pos)
			if err != nil {
				return nil, err
			}
			gParseShards = append(gParseShards, wasmParseShardQueryResponse(sl))
			pos = np
		case fn == 3 && wt == 2:
			sl, np, err := wasmReadLengthDelimited(data, pos)
			if err != nil {
				return nil, err
			}
			q, r, f, ml := wasmParseScatterGatherStatsFlat(sl)
			out["_stat_shards_queried"] = q
			out["_stat_shards_responded"] = r
			out["_stat_shards_failed"] = f
			out["_stat_max_latency_ms"] = ml
			pos = np
		default:
			pos, err = skipField(data, pos, wt)
			if err != nil {
				return nil, err
			}
		}
	}
	out["shard_responses"] = gParseShards
	if _, ok := out["_stat_shards_queried"]; !ok {
		out["_stat_shards_queried"] = float64(0)
		out["_stat_shards_responded"] = float64(0)
		out["_stat_shards_failed"] = float64(0)
		out["_stat_max_latency_ms"] = float64(0)
	}
	gParseOut = nil
	return out, nil
}

func wasmParseReduceOrAllReduceResponse(data []byte) (map[string]any, error) {
	gRawResponseBytes = data // pin subslice to keep its GC block alive across allocations
	data = gRawResponseBytes
	out := map[string]any{}
	var shards []map[string]any
	pos := 0
	for pos < len(data) {
		tag, n, err := readVarint(data, pos)
		if err != nil {
			return nil, err
		}
		pos += n
		fn := int(tag >> 3)
		wt := tag & 7
		switch {
		case fn == 2 && wt == 2:
			sl, np, err := wasmReadLengthDelimited(data, pos)
			if err != nil {
				return nil, err
			}
			_, pl := wasmParseCommonMessage(sl)
			out["result"] = wasmReducedResultFromMessagePayload(pl)
			pos = np
		case fn == 3 && wt == 2:
			sl, np, err := wasmReadLengthDelimited(data, pos)
			if err != nil {
				return nil, err
			}
			shards = append(shards, wasmParseShardQueryResponse(sl))
			pos = np
		case fn == 4 && wt == 2:
			sl, np, err := wasmReadLengthDelimited(data, pos)
			if err != nil {
				return nil, err
			}
			q, r, f, ml := wasmParseScatterGatherStatsFlat(sl)
			out["_stat_shards_queried"] = q
			out["_stat_shards_responded"] = r
			out["_stat_shards_failed"] = f
			out["_stat_max_latency_ms"] = ml
			pos = np
		default:
			pos, err = skipField(data, pos, wt)
			if err != nil {
				return nil, err
			}
		}
	}
	out["shard_responses"] = shards
	if _, ok := out["_stat_shards_queried"]; !ok {
		out["_stat_shards_queried"] = float64(0)
		out["_stat_shards_responded"] = float64(0)
		out["_stat_shards_failed"] = float64(0)
		out["_stat_max_latency_ms"] = float64(0)
	}
	return out, nil
}

func wasmParseMapEntryUInt64(entry []byte) (key string, val uint64, ok bool) {
	pos := 0
	for pos < len(entry) {
		tag, n, err := readVarint(entry, pos)
		if err != nil {
			return "", 0, false
		}
		pos += n
		fn := int(tag >> 3)
		wt := tag & 7
		if fn == 1 && wt == 2 {
			ks, np, err := wasmReadLengthDelimited(entry, pos)
			if err != nil {
				return "", 0, false
			}
			key = string(ks)
			pos = np
			continue
		}
		if fn == 2 && wt == 0 {
			v, m, err := readVarint(entry, pos)
			if err != nil {
				return "", 0, false
			}
			pos += m
			val = v
			continue
		}
		pos, err = skipField(entry, pos, wt)
		if err != nil {
			return "", 0, false
		}
	}
	return key, val, key != ""
}

// wasmParseApplicationMetricsTyped decodes an ApplicationMetrics proto message into a typed struct.
// Uses local map[string]uint64 fields — no map[string]any boxing — safe in TinyGo WASM.
func wasmParseApplicationMetricsTyped(data []byte) ApplicationMetrics {
	actorCounts := map[string]uint64{}
	counterMetrics := map[string]uint64{}
	latencyTotalsMs := map[string]uint64{}
	latencyMaxMs := map[string]uint64{}
	latencySamples := map[string]uint64{}
	var supervisorCount, uptimeSeconds, messageCount, errorCount uint64
	pos := 0
	for pos < len(data) {
		tag, n, err := readVarint(data, pos)
		if err != nil {
			break
		}
		pos += n
		fn := int(tag >> 3)
		wt := tag & 7
		switch {
		case fn == 1 && wt == 2:
			sl, np, err := wasmReadLengthDelimited(data, pos)
			if err != nil {
				return ApplicationMetrics{}
			}
			k, v, ok := wasmParseMapEntryUInt64(sl)
			if ok {
				actorCounts[k] = v
			}
			pos = np
		case fn == 2 && wt == 0:
			v, m, err := readVarint(data, pos)
			if err != nil {
				return ApplicationMetrics{}
			}
			pos += m
			supervisorCount = v
		case fn == 3 && wt == 0:
			v, m, err := readVarint(data, pos)
			if err != nil {
				return ApplicationMetrics{}
			}
			pos += m
			uptimeSeconds = v
		case fn == 4 && wt == 0:
			v, m, err := readVarint(data, pos)
			if err != nil {
				return ApplicationMetrics{}
			}
			pos += m
			messageCount = v
		case fn == 5 && wt == 0:
			v, m, err := readVarint(data, pos)
			if err != nil {
				return ApplicationMetrics{}
			}
			pos += m
			errorCount = v
		case fn == 6 && wt == 2:
			sl, np, err := wasmReadLengthDelimited(data, pos)
			if err != nil {
				return ApplicationMetrics{}
			}
			k, v, ok := wasmParseMapEntryUInt64(sl)
			if ok {
				counterMetrics[k] = v
			}
			pos = np
		case fn == 7 && wt == 2:
			sl, np, err := wasmReadLengthDelimited(data, pos)
			if err != nil {
				return ApplicationMetrics{}
			}
			k, v, ok := wasmParseMapEntryUInt64(sl)
			if ok {
				latencyTotalsMs[k] = v
			}
			pos = np
		case fn == 8 && wt == 2:
			sl, np, err := wasmReadLengthDelimited(data, pos)
			if err != nil {
				return ApplicationMetrics{}
			}
			k, v, ok := wasmParseMapEntryUInt64(sl)
			if ok {
				latencyMaxMs[k] = v
			}
			pos = np
		case fn == 9 && wt == 2:
			sl, np, err := wasmReadLengthDelimited(data, pos)
			if err != nil {
				return ApplicationMetrics{}
			}
			k, v, ok := wasmParseMapEntryUInt64(sl)
			if ok {
				latencySamples[k] = v
			}
			pos = np
		default:
			pos, err = skipField(data, pos, wt)
			if err != nil {
				return ApplicationMetrics{}
			}
		}
	}
	return ApplicationMetrics{
		ActorCounts:     actorCounts,
		SupervisorCount: supervisorCount,
		UptimeSeconds:   uptimeSeconds,
		MessageCount:    messageCount,
		ErrorCount:      errorCount,
		CounterMetrics:  counterMetrics,
		LatencyTotalsMs: latencyTotalsMs,
		LatencyMaxMs:    latencyMaxMs,
		LatencySamples:  latencySamples,
	}
}

// wasmParseApplicationInfoInto fills ApplicationStatus fields from ApplicationInfo proto bytes.
// Writes to a struct pointer — safe in TinyGo WASM (no map parameter writes).
func wasmParseApplicationInfoInto(out *ApplicationStatus, data []byte) {
	pos := 0
	for pos < len(data) {
		tag, n, err := readVarint(data, pos)
		if err != nil {
			return
		}
		pos += n
		fn := int(tag >> 3)
		wt := tag & 7
		switch {
		case fn == 1 && wt == 2:
			sl, np, err := wasmReadLengthDelimited(data, pos)
			if err != nil {
				return
			}
			out.ApplicationID = string(sl)
			pos = np
		case fn == 2 && wt == 2:
			sl, np, err := wasmReadLengthDelimited(data, pos)
			if err != nil {
				return
			}
			out.ApplicationName = string(sl)
			pos = np
		case fn == 3 && wt == 2:
			sl, np, err := wasmReadLengthDelimited(data, pos)
			if err != nil {
				return
			}
			out.ApplicationVersion = string(sl)
			pos = np
		case fn == 4 && wt == 0:
			v, m, err := readVarint(data, pos)
			if err != nil {
				return
			}
			pos += m
			out.StatusCode = applicationStatusEnumString(v)
		case fn == 5 && wt == 2:
			_, np, err := wasmReadLengthDelimited(data, pos) // deployed_at timestamp — skip
			if err != nil {
				return
			}
			pos = np
		case fn == 6 && wt == 2:
			_, np, err := wasmReadLengthDelimited(data, pos) // metrics inside ApplicationInfo — skip
			if err != nil {
				return
			}
			pos = np
		default:
			pos, err = skipField(data, pos, wt)
			if err != nil {
				return
			}
		}
	}
}

// wasmParseApplicationStatusTyped decodes a GetApplicationStatusResponse proto into a typed struct.
// Safe in TinyGo WASM: all writes target struct fields or local maps, never map parameters.
func wasmParseApplicationStatusTyped(data []byte) (ApplicationStatus, error) {
	var out ApplicationStatus
	pos := 0
	for pos < len(data) {
		tag, n, err := readVarint(data, pos)
		if err != nil {
			return out, nil
		}
		pos += n
		fn := int(tag >> 3)
		wt := tag & 7
		switch {
		case fn == 1 && wt == 2: // application ApplicationInfo sub-message
			sl, np, err := wasmReadLengthDelimited(data, pos)
			if err != nil {
				return out, nil
			}
			wasmParseApplicationInfoInto(&out, sl)
			pos = np
		case fn == 3 && wt == 2: // error string
			_, np, err := wasmReadLengthDelimited(data, pos)
			if err != nil {
				return out, nil
			}
			pos = np
		case fn == 4 && wt == 2: // node_id
			sl, np, err := wasmReadLengthDelimited(data, pos)
			if err != nil {
				return out, nil
			}
			out.NodeID = string(sl)
			pos = np
		case fn == 5 && wt == 2: // node_address
			sl, np, err := wasmReadLengthDelimited(data, pos)
			if err != nil {
				return out, nil
			}
			out.NodeAddress = string(sl)
			pos = np
		default:
			pos, err = skipField(data, pos, wt)
			if err != nil {
				return out, nil
			}
		}
	}
	return out, nil
}

func applicationStatusEnumString(v uint64) string {
	switch v {
	case 0:
		return "APPLICATION_STATUS_UNSPECIFIED"
	case 1:
		return "APPLICATION_STATUS_LOADING"
	case 2:
		return "APPLICATION_STATUS_STARTING"
	case 3:
		return "APPLICATION_STATUS_RUNNING"
	case 4:
		return "APPLICATION_STATUS_STOPPING"
	case 5:
		return "APPLICATION_STATUS_STOPPED"
	case 6:
		return "APPLICATION_STATUS_FAILED"
	default:
		return fmt.Sprintf("APPLICATION_STATUS_%d", v)
	}
}


func wasmParseTimestamp(data []byte) int64 {
	var sec int64
	pos := 0
	for pos < len(data) {
		tag, n, err := readVarint(data, pos)
		if err != nil {
			return sec
		}
		pos += n
		fn := int(tag >> 3)
		wt := tag & 7
		if wt != 0 {
			pos, err = skipField(data, pos, wt)
			if err != nil {
				return sec
			}
			continue
		}
		v, m, err := readVarint(data, pos)
		if err != nil {
			return sec
		}
		pos += m
		if fn == 1 {
			sec = int64(v)
		}
	}
	return sec
}


// --- hostWire* / hostDecode* entry points ---

func hostWireCreateShardGroupRequest(req CreateShardGroupRequest) (string, error) {
	part := partitionEnum(req.PartitionStrategy)
	reb := rebalanceEnum(req.RebalancePolicy)
	cfg := wasmEncodeDataParallelConfigTyped(req.GroupID, req.ShardCount, part, reb, req.Placement)
	var out []byte
	out = appendLengthDelimited(out, 2, cfg)
	out = wasmAppendString(out, 3, req.ActorType)
	if len(req.InitialState) > 0 {
		out = wasmAppendBytes(out, 5, []byte(SafeMarshal(req.InitialState)))
	}
	if len(req.Metadata) > 0 {
		out = wasmAppendStringMap(out, 6, req.Metadata)
	}
	return string(out), nil
}

func hostDecodeCreateShardGroupResponse(raw string) (map[string]any, error) {
	return wasmPinDecodeRaw(raw, wasmParseCreateShardGroupResponse)
}

func hostWireScatterGatherRequest(req ScatterGatherRequest) (string, error) {
	mt, payload := encodeQueryPayload(req.Query)
	if mt == "" && (len(payload) == 0 || string(payload) == "{}") {
		return "", fmt.Errorf("scatter_gather: missing query/message_type (set op or message_type in Query)")
	}
	qm := wasmEncodeCommonMessage(mt, payload)
	var out []byte
	out = wasmAppendString(out, 2, req.GroupID)
	out = appendLengthDelimited(out, 3, qm)
	if req.TimeoutMs > 0 {
		if d := wasmEncodeDurationFromMs(uint64(req.TimeoutMs)); len(d) > 0 {
			out = appendLengthDelimited(out, 4, d)
		}
	}
	out = wasmAppendUInt32(out, 5, aggregationEnum(req.Aggregation))
	if req.MinResponses > 0 {
		out = wasmAppendUInt32(out, 6, uint32(req.MinResponses))
	}
	return string(out), nil
}

// gScatterGatherRaw and gScatterGatherData pin the ScatterGather response bytes in
// GC-visible linear memory (globals are always scanned). After asyncify resumes,
// the raw string returned by the host call lives only in WASM locals; any allocation
// inside wasmParseScatterGatherResponse can trigger GC that would free the backing
// array. By assigning to globals FIRST we ensure GC sees and preserves the data.
// Single-threaded TinyGo WASM: globals are safe as temporary pins.
var gScatterGatherRaw string
var gScatterGatherData []byte

// gParseOut and gParseShards pin the map and slice being built during
// wasmParseScatterGatherResponse / wasmParseShardQueryResponse. In TinyGo 0.40, the
// conservative GC does not scan WASM locals. A local map[string]any{} created in one of
// these functions can be freed by the NEXT allocation if it is only referenced by a WASM
// local. Writing to a freed map corrupts the heap, causing crashes inside the GC scan on
// subsequent iterations. Assigning to globals FIRST makes the objects GC-visible.
// Safe because TinyGo WASM is single-threaded: parse calls are never nested.
var gParseOut map[string]any
var gParseShards []map[string]any
var gParseShardOut map[string]any

// gParseCommonData and gParseShardData pin the []byte slice passed to
// wasmParseCommonMessage / wasmParseShardQueryResponse. These are subslices of
// gScatterGatherData, so they share the same backing array. However, for large
// proto responses the allocation spans multiple GC blocks: the global
// gScatterGatherData only keeps the FIRST block alive. The interior blocks (where the
// subslice points) can be collected unless we also store the subslice's base pointer
// in a global. TinyGo 0.40 conservative GC marks any block that contains a pointer
// found during scanning; storing the subslice in a global ensures the exact block
// containing the subslice is scanned and marked live.
var gParseCommonData []byte
var gParseShardData []byte

// gRawResponsePin and gRawResponseBytes are shared GC-pinning temporaries for all other
// host-response string→[]byte conversions. Same reasoning as ScatterGather above.
// Safe because TinyGo WASM is single-threaded: only one host response is decoded at a time.
var gRawResponsePin string
var gRawResponseBytes []byte

// wasmPinDecodeRaw pins raw in GC-visible globals, converts to []byte, calls parse, then
// releases the pins. Use for any host response decode that calls []byte(raw) before parsing.
func wasmPinDecodeRaw(raw string, parse func([]byte) (map[string]any, error)) (map[string]any, error) {
	gRawResponsePin = raw
	gRawResponseBytes = []byte(raw)
	gRawResponsePin = ""
	result, err := parse(gRawResponseBytes)
	gRawResponseBytes = nil
	return result, err
}

func hostDecodeScatterGatherResponse(raw string) (map[string]any, error) {
	gScatterGatherRaw = raw          // pin raw's backing array before []byte(raw) may trigger GC
	gScatterGatherData = []byte(raw) // allocate a copy; GC safe because raw is pinned above
	gScatterGatherRaw = ""           // raw backing array now duplicated in gScatterGatherData; release
	result, err := wasmParseScatterGatherResponse(gScatterGatherData)
	gScatterGatherData = nil // release pin after parsing is complete
	return result, err
}

func hostWireBroadcastShardGroupRequest(req BroadcastShardGroupRequest) (string, error) {
	if len(req.Message) == 0 {
		return "", fmt.Errorf("broadcast_shard_group: Message must not be empty")
	}
	mt, payload := encodeBroadcastPayload(req.Message)
	msg := wasmEncodeCommonMessage(mt, payload)
	var out []byte
	out = wasmAppendString(out, 2, req.GroupID)
	out = appendLengthDelimited(out, 3, msg)
	if req.TimeoutMs > 0 {
		if d := wasmEncodeDurationFromMs(uint64(req.TimeoutMs)); len(d) > 0 {
			out = appendLengthDelimited(out, 4, d)
		}
	}
	if req.MinAcks > 0 {
		out = wasmAppendUInt32(out, 5, uint32(req.MinAcks))
	}
	return string(out), nil
}

func hostDecodeBroadcastShardGroupResponse(raw string) (map[string]any, error) {
	return wasmPinDecodeRaw(raw, wasmParseBroadcastOrBarrierResponse)
}

func hostWireReduceShardGroupRequest(req ReduceShardGroupRequest) (string, error) {
	return wasmEncodeReduceLike(req)
}

func hostWireAllReduceShardGroupRequest(req AllReduceShardGroupRequest) (string, error) {
	return wasmEncodeReduceLike(req)
}

func wasmEncodeReduceLike(req ReduceShardGroupRequest) (string, error) {
	mt, payload := encodeMapFunctionPayload(req.MapFunction)
	if mt == "" && len(payload) == 0 {
		return "", fmt.Errorf("reduce_shard_group: MapFunction must contain op or message_type")
	}
	mf := wasmEncodeCommonMessage(mt, payload)
	var out []byte
	out = wasmAppendString(out, 2, req.GroupID)
	out = appendLengthDelimited(out, 3, mf)
	if req.TimeoutMs > 0 {
		if d := wasmEncodeDurationFromMs(uint64(req.TimeoutMs)); len(d) > 0 {
			out = appendLengthDelimited(out, 4, d)
		}
	}
	if req.MinResponses > 0 {
		out = wasmAppendUInt32(out, 5, uint32(req.MinResponses))
	}
	out = wasmAppendUInt32(out, 6, reductionEnum(req.Reduction))
	if req.Target != "" {
		var tf []byte
		tf = wasmAppendString(tf, 1, req.Target)
		out = appendLengthDelimited(out, 7, tf)
	}
	return string(out), nil
}

func hostDecodeReduceShardGroupResponse(raw string) (map[string]any, error) {
	return wasmPinDecodeRaw(raw, wasmParseReduceOrAllReduceResponse)
}

func hostDecodeAllReduceShardGroupResponse(raw string) (map[string]any, error) {
	return wasmPinDecodeRaw(raw, wasmParseReduceOrAllReduceResponse)
}

func hostWireBarrierShardGroupRequest(req BarrierShardGroupRequest) (string, error) {
	var out []byte
	out = wasmAppendString(out, 2, req.GroupID)
	out = wasmAppendString(out, 3, req.BarrierID)
	out = wasmAppendUInt64(out, 4, req.Round)
	if req.TimeoutMs > 0 {
		if d := wasmEncodeDurationFromMs(uint64(req.TimeoutMs)); len(d) > 0 {
			out = appendLengthDelimited(out, 5, d)
		}
	}
	if req.MinAcks > 0 {
		out = wasmAppendUInt32(out, 6, uint32(req.MinAcks))
	}
	return string(out), nil
}

func hostDecodeBarrierShardGroupResponse(raw string) (map[string]any, error) {
	return wasmPinDecodeRaw(raw, wasmParseBroadcastOrBarrierResponse)
}

func hostWireBulkUpdateShardGroupRequest(req BulkUpdateShardGroupRequest) (string, error) {
	var out []byte
	out = wasmAppendString(out, 2, req.GroupID)
	for partitionKey, payloadDict := range req.Updates {
		msgType, _ := payloadDict["op"].(string)
		if msgType == "" {
			msgType, _ = payloadDict["message_type"].(string)
		}
		payloadBytes := []byte(SafeMarshal(payloadDict))
		msgBytes := wasmEncodeCommonMessage(msgType, payloadBytes)
		var mapEntry []byte
		mapEntry = wasmAppendString(mapEntry, 1, partitionKey)
		mapEntry = appendLengthDelimited(mapEntry, 2, msgBytes)
		out = appendLengthDelimited(out, 3, mapEntry)
	}
	if req.TimeoutMs > 0 {
		if d := wasmEncodeDurationFromMs(uint64(req.TimeoutMs)); len(d) > 0 {
			out = appendLengthDelimited(out, 5, d)
		}
	}
	if req.WaitForResponses {
		out = wasmAppendBool(out, 6, true)
	}
	return string(out), nil
}

func hostDecodeBulkUpdateShardGroupResponse(raw string) (map[string]any, error) {
	gRawResponsePin = raw
	gRawResponseBytes = []byte(raw)
	gRawResponsePin = ""
	data := gRawResponseBytes
	defer func() { gRawResponseBytes = nil }()
	out := map[string]any{
		"updates_sent":      uint32(0),
		"updates_succeeded": uint32(0),
		"updates_failed":    uint32(0),
		"errors":            []any{},
	}
	var shardStats []any
	var errors []any
	pos := 0
	for pos < len(data) {
		tag, n, err := readVarint(data, pos)
		if err != nil {
			return out, nil
		}
		pos += n
		fn := int(tag >> 3)
		wt := tag & 7
		switch {
		case fn == 1 && wt == 2:
			sl, np, err := wasmReadLengthDelimited(data, pos)
			if err != nil {
				return out, nil
			}
			out["request_id"] = string(sl)
			pos = np
		case fn == 2 && wt == 0:
			v, n2, err := readVarint(data, pos)
			if err != nil {
				return out, nil
			}
			out["updates_sent"] = uint32(v)
			pos += n2
		case fn == 3 && wt == 0:
			v, n2, err := readVarint(data, pos)
			if err != nil {
				return out, nil
			}
			out["updates_succeeded"] = uint32(v)
			pos += n2
		case fn == 4 && wt == 0:
			v, n2, err := readVarint(data, pos)
			if err != nil {
				return out, nil
			}
			out["updates_failed"] = uint32(v)
			pos += n2
		case fn == 5 && wt == 2:
			sl, np, err := wasmReadLengthDelimited(data, pos)
			if err != nil {
				return out, nil
			}
			shardStats = append(shardStats, wasmParseShardUpdateStats(sl))
			pos = np
		case fn == 6 && wt == 2:
			sl, np, err := wasmReadLengthDelimited(data, pos)
			if err != nil {
				return out, nil
			}
			errors = append(errors, string(sl))
			pos = np
		default:
			pos, err = skipField(data, pos, wt)
			if err != nil {
				return out, nil
			}
		}
	}
	if shardStats != nil {
		out["shard_stats"] = shardStats
	}
	if errors != nil {
		out["errors"] = errors
	}
	return out, nil
}

func wasmParseShardUpdateStats(data []byte) map[string]any {
	out := map[string]any{}
	pos := 0
	for pos < len(data) {
		tag, n, err := readVarint(data, pos)
		if err != nil {
			return out
		}
		pos += n
		fn := int(tag >> 3)
		wt := tag & 7
		switch {
		case fn == 1 && wt == 0:
			v, n2, err := readVarint(data, pos)
			if err != nil {
				return out
			}
			out["shard_id"] = uint32(v)
			pos += n2
		case fn == 2 && wt == 2:
			sl, np, err := wasmReadLengthDelimited(data, pos)
			if err != nil {
				return out
			}
			out["shard_actor_id"] = string(sl)
			pos = np
		case fn == 3 && wt == 0:
			v, n2, err := readVarint(data, pos)
			if err != nil {
				return out
			}
			out["updates_sent"] = uint32(v)
			pos += n2
		case fn == 4 && wt == 0:
			v, n2, err := readVarint(data, pos)
			if err != nil {
				return out
			}
			out["updates_succeeded"] = uint32(v)
			pos += n2
		case fn == 5 && wt == 0:
			v, n2, err := readVarint(data, pos)
			if err != nil {
				return out
			}
			out["updates_failed"] = uint32(v)
			pos += n2
		default:
			var e error
			pos, e = skipField(data, pos, wt)
			if e != nil {
				return out
			}
		}
	}
	return out
}

func hostWireMapShardGroupRequest(req MapShardGroupRequest) (string, error) {
	mt, payload := encodeMapFunctionPayload(req.MapFunction)
	mf := wasmEncodeCommonMessage(mt, payload)
	var out []byte
	out = wasmAppendString(out, 2, req.GroupID)
	out = appendLengthDelimited(out, 3, mf)
	if req.TimeoutMs > 0 {
		if d := wasmEncodeDurationFromMs(uint64(req.TimeoutMs)); len(d) > 0 {
			out = appendLengthDelimited(out, 4, d)
		}
	}
	if req.MinResponses > 0 {
		out = wasmAppendUInt32(out, 5, uint32(req.MinResponses))
	}
	return string(out), nil
}

func hostDecodeMapShardGroupResponse(raw string) (map[string]any, error) {
	return nil, fmt.Errorf("map_shard_group: protobuf decode not available in TinyGo WASM")
}

func hostWireSpawnActorsRequest(request any) (string, error) {
	return "", fmt.Errorf("spawn_actors: not supported in TinyGo WASM (manual wire not implemented)")
}

func hostDecodeSpawnActorsResponse(raw string) (map[string]any, error) {
	return nil, fmt.Errorf("spawn_actors: protobuf decode not available in TinyGo WASM")
}

func hostWireApplicationMetrics(metrics any) (string, error) {
	m, ok := mapAsStringAny(metrics)
	if !ok {
		return "", fmt.Errorf("application_metrics: expected map[string]any")
	}
	wire, err := encodeApplicationMetricsMapToProtobuf(m)
	if err != nil {
		return "", fmt.Errorf("application_metrics: %w", err)
	}
	return string(wire), nil
}

func hostDecodeApplicationMetricsResponse(raw string) (ApplicationMetrics, error) {
	gRawResponsePin = raw
	gRawResponseBytes = []byte(raw)
	gRawResponsePin = ""
	result := wasmParseApplicationMetricsTyped(gRawResponseBytes)
	gRawResponseBytes = nil
	return result, nil
}

func hostDecodeApplicationGetStatusResponse(raw string) (ApplicationStatus, error) {
	gRawResponsePin = raw
	gRawResponseBytes = []byte(raw)
	gRawResponsePin = ""
	result, err := wasmParseApplicationStatusTyped(gRawResponseBytes)
	gRawResponseBytes = nil
	return result, err
}
