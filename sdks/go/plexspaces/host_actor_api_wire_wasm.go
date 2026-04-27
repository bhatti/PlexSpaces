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
	"encoding/json"
	"fmt"
	"strconv"
	"strings"
)

// --- map / scalar helpers (same behavior as prior wasm wire) ---

func mapAsStringAny(v any) (map[string]any, bool) {
	m, ok := v.(map[string]any)
	return m, ok
}

func cfgGet(root map[string]any, key string) any {
	if v, ok := root[key]; ok {
		return v
	}
	if cfg, ok := root["config"].(map[string]any); ok {
		return cfg[key]
	}
	return nil
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
	case int64:
		return strconv.FormatInt(t, 10)
	case uint64:
		return strconv.FormatUint(t, 10)
	case json.Number:
		return string(t)
	default:
		return fmt.Sprint(t)
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

func stringMapFromAnyMap(m map[string]any) map[string]string {
	out := make(map[string]string, len(m))
	for k, v := range m {
		out[k] = strVal(v)
	}
	return out
}

func u64StringMapFromNested(v any) map[string]uint64 {
	m, ok := mapAsStringAny(v)
	if !ok {
		return nil
	}
	out := make(map[string]uint64, len(m))
	for k, val := range m {
		out[k] = u64Val(val)
	}
	return out
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

func wasmEncodeNodePlacement(v any) []byte {
	m, ok := mapAsStringAny(v)
	if !ok || len(m) == 0 {
		return nil
	}
	var p []byte
	p = wasmAppendUInt32(p, 1, nodePlacementEnum(strVal(m["strategy"])))
	if c := strVal(m["cluster"]); c != "" {
		p = wasmAppendString(p, 2, c)
	}
	if raw, ok := m["node_ids"].([]any); ok {
		for _, id := range raw {
			s := strVal(id)
			if s != "" {
				p = wasmAppendString(p, 3, s)
			}
		}
	}
	if rl, ok := mapAsStringAny(m["required_labels"]); ok {
		p = wasmAppendStringMap(p, 4, stringMapFromAnyMap(rl))
	}
	if raw, ok := m["avoid_node_ids"].([]any); ok {
		for _, id := range raw {
			s := strVal(id)
			if s != "" {
				p = wasmAppendString(p, 5, s)
			}
		}
	}
	if al, ok := mapAsStringAny(m["affinity_labels"]); ok {
		p = wasmAppendStringMap(p, 7, stringMapFromAnyMap(al))
	}
	return p
}

func wasmEncodeDataParallelConfig(groupID string, shardCount uint32, part, reb uint32, placement any) []byte {
	var c []byte
	c = wasmAppendString(c, 1, groupID)
	c = wasmAppendUInt32(c, 2, shardCount)
	c = wasmAppendUInt32(c, 4, part)
	c = wasmAppendUInt32(c, 5, reb)
	if pb := wasmEncodeNodePlacement(placement); len(pb) > 0 {
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

func buildQueryPayload(m map[string]any) (msgType string, payload []byte) {
	mt := strVal(m["message_type"])
	q, hasQuery := mapAsStringAny(m["query"])
	if !hasQuery {
		q = map[string]any{}
	}
	if mt == "" {
		if op, ok := q["op"].(string); ok {
			mt = op
		}
	}
	if mt != "" {
		if _, exists := q["message_type"]; !exists {
			q["message_type"] = mt
		}
	}
	var err error
	payload, err = json.Marshal(q)
	if err != nil {
		payload = []byte("{}")
	}
	return mt, payload
}

func buildBroadcastPayload(m map[string]any) (msgType string, payload []byte, ok bool) {
	body, isMap := mapAsStringAny(m["message"])
	if !isMap {
		return "", nil, false
	}
	mt := strVal(m["message_type"])
	if mt == "" {
		mt = strVal(body["op"])
	}
	var err error
	payload, err = json.Marshal(body)
	if err != nil {
		payload = []byte("{}")
	}
	return mt, payload, true
}

func buildMapFunctionPayload(m map[string]any) (msgType string, payload []byte, ok bool) {
	mt := strVal(m["message_type"])
	body, isMap := mapAsStringAny(m["map_function"])
	if !isMap {
		body = map[string]any{}
	}
	if mt == "" {
		mt = strVal(body["op"])
	}
	if mt != "" {
		if _, exists := body["message_type"]; !exists {
			body["message_type"] = mt
		}
	}
	var err error
	payload, err = json.Marshal(body)
	if err != nil {
		payload = []byte("{}")
	}
	return mt, payload, true
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
	var parsed any
	if err := json.Unmarshal(payload, &parsed); err != nil {
		return string(payload)
	}
	return parsed
}

func wasmReducedResultFromMessagePayload(payload []byte) any {
	raw := wasmPayloadToAny(payload)
	switch v := raw.(type) {
	case float64, int, int64, uint64, bool, string:
		return v
	case map[string]any:
		for _, k := range []string{"partial_sum", "value", "result", "total", "reduced_value"} {
			if x, ok := v[k]; ok {
				return x
			}
		}
		return v
	default:
		return raw
	}
}

func wasmParseShardQueryResponse(data []byte) map[string]any {
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
			v, m, err := readVarint(data, pos)
			if err != nil {
				return out
			}
			pos += m
			out["shard_id"] = float64(v)
		case fn == 2 && wt == 2:
			sl, np, err := wasmReadLengthDelimited(data, pos)
			if err != nil {
				return out
			}
			out["shard_actor_id"] = string(sl)
			pos = np
		case fn == 3 && wt == 2:
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
		case fn == 4 && wt == 2:
			sl, np, err := wasmReadLengthDelimited(data, pos)
			if err != nil {
				return out
			}
			ms := wasmParseDuration(sl)
			out["latency_ms"] = float64(ms)
			out["latency"] = map[string]any{"ms": float64(ms)}
			pos = np
		case fn == 5 && wt == 0:
			v, m, err := readVarint(data, pos)
			if err != nil {
				return out
			}
			pos += m
			out["success"] = v != 0
		case fn == 6 && wt == 2:
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
			out["max_latency"] = map[string]any{"ms": float64(ms)}
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
		if fn == 1 && wt == 2 {
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
	out := map[string]any{}
	var shards []any
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
		case fn == 1 && wt == 2:
			sl, np, err := wasmReadLengthDelimited(data, pos)
			if err != nil {
				return nil, err
			}
			_, pl := wasmParseCommonMessage(sl)
			out["result"] = wasmPayloadToAny(pl)
			pos = np
		case fn == 2 && wt == 2:
			sl, np, err := wasmReadLengthDelimited(data, pos)
			if err != nil {
				return nil, err
			}
			shards = append(shards, wasmParseShardQueryResponse(sl))
			pos = np
		case fn == 3 && wt == 2:
			sl, np, err := wasmReadLengthDelimited(data, pos)
			if err != nil {
				return nil, err
			}
			out["stats"] = wasmParseScatterGatherStats(sl)
			pos = np
		default:
			pos, err = skipField(data, pos, wt)
			if err != nil {
				return nil, err
			}
		}
	}
	out["shard_responses"] = shards
	if out["stats"] == nil {
		out["stats"] = wasmParseScatterGatherStats(nil)
	}
	return out, nil
}

func wasmParseBroadcastOrBarrierResponse(data []byte) (map[string]any, error) {
	out := map[string]any{}
	var shards []any
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
		case fn == 1 && wt == 2:
			sl, np, err := wasmReadLengthDelimited(data, pos)
			if err != nil {
				return nil, err
			}
			shards = append(shards, wasmParseShardQueryResponse(sl))
			pos = np
		case fn == 2 && wt == 2:
			sl, np, err := wasmReadLengthDelimited(data, pos)
			if err != nil {
				return nil, err
			}
			out["stats"] = wasmParseScatterGatherStats(sl)
			pos = np
		default:
			pos, err = skipField(data, pos, wt)
			if err != nil {
				return nil, err
			}
		}
	}
	out["shard_responses"] = shards
	if out["stats"] == nil {
		out["stats"] = wasmParseScatterGatherStats(nil)
	}
	return out, nil
}

func wasmParseReduceOrAllReduceResponse(data []byte) (map[string]any, error) {
	out := map[string]any{}
	var shards []any
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
		case fn == 1 && wt == 2:
			sl, np, err := wasmReadLengthDelimited(data, pos)
			if err != nil {
				return nil, err
			}
			_, pl := wasmParseCommonMessage(sl)
			out["result"] = wasmReducedResultFromMessagePayload(pl)
			pos = np
		case fn == 2 && wt == 2:
			sl, np, err := wasmReadLengthDelimited(data, pos)
			if err != nil {
				return nil, err
			}
			shards = append(shards, wasmParseShardQueryResponse(sl))
			pos = np
		case fn == 3 && wt == 2:
			sl, np, err := wasmReadLengthDelimited(data, pos)
			if err != nil {
				return nil, err
			}
			out["stats"] = wasmParseScatterGatherStats(sl)
			pos = np
		default:
			pos, err = skipField(data, pos, wt)
			if err != nil {
				return nil, err
			}
		}
	}
	out["shard_responses"] = shards
	if out["stats"] == nil {
		out["stats"] = wasmParseScatterGatherStats(nil)
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

func wasmMergeU64MapField(out map[string]any, field string, entry []byte) {
	k, v, ok := wasmParseMapEntryUInt64(entry)
	if !ok {
		return
	}
	raw, exists := out[field]
	acc, isMap := raw.(map[string]any)
	if !exists || !isMap {
		acc = map[string]any{}
		out[field] = acc
	}
	acc[k] = float64(v)
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

func wasmParseApplicationMetrics(data []byte) map[string]any {
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
		case fn == 1 && wt == 2:
			sl, np, err := wasmReadLengthDelimited(data, pos)
			if err != nil {
				return out
			}
			wasmMergeU64MapField(out, "actor_counts", sl)
			pos = np
		case fn == 2 && wt == 0:
			v, m, err := readVarint(data, pos)
			if err != nil {
				return out
			}
			pos += m
			out["supervisor_count"] = float64(v)
		case fn == 3 && wt == 0:
			v, m, err := readVarint(data, pos)
			if err != nil {
				return out
			}
			pos += m
			out["uptime_seconds"] = float64(v)
		case fn == 4 && wt == 0:
			v, m, err := readVarint(data, pos)
			if err != nil {
				return out
			}
			pos += m
			out["message_count"] = float64(v)
		case fn == 5 && wt == 0:
			v, m, err := readVarint(data, pos)
			if err != nil {
				return out
			}
			pos += m
			out["error_count"] = float64(v)
		case fn == 6 && wt == 2:
			sl, np, err := wasmReadLengthDelimited(data, pos)
			if err != nil {
				return out
			}
			wasmMergeU64MapField(out, "counter_metrics", sl)
			pos = np
		case fn == 7 && wt == 2:
			sl, np, err := wasmReadLengthDelimited(data, pos)
			if err != nil {
				return out
			}
			wasmMergeU64MapField(out, "latency_totals_ms", sl)
			pos = np
		case fn == 8 && wt == 2:
			sl, np, err := wasmReadLengthDelimited(data, pos)
			if err != nil {
				return out
			}
			wasmMergeU64MapField(out, "latency_max_ms", sl)
			pos = np
		case fn == 9 && wt == 2:
			sl, np, err := wasmReadLengthDelimited(data, pos)
			if err != nil {
				return out
			}
			wasmMergeU64MapField(out, "latency_samples", sl)
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

func wasmParseApplicationInfo(data []byte) map[string]any {
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
		case fn == 1 && wt == 2:
			sl, np, err := wasmReadLengthDelimited(data, pos)
			if err != nil {
				return out
			}
			out["application_id"] = string(sl)
			pos = np
		case fn == 2 && wt == 2:
			sl, np, err := wasmReadLengthDelimited(data, pos)
			if err != nil {
				return out
			}
			out["name"] = string(sl)
			pos = np
		case fn == 3 && wt == 2:
			sl, np, err := wasmReadLengthDelimited(data, pos)
			if err != nil {
				return out
			}
			out["version"] = string(sl)
			pos = np
		case fn == 4 && wt == 0:
			v, m, err := readVarint(data, pos)
			if err != nil {
				return out
			}
			pos += m
			out["status"] = applicationStatusEnumString(v)
		case fn == 5 && wt == 2:
			sl, np, err := wasmReadLengthDelimited(data, pos)
			if err != nil {
				return out
			}
			out["deployed_at"] = float64(wasmParseTimestamp(sl))
			pos = np
		case fn == 6 && wt == 2:
			sl, np, err := wasmReadLengthDelimited(data, pos)
			if err != nil {
				return out
			}
			out["metrics"] = wasmParseApplicationMetrics(sl)
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

func wasmParseGetApplicationStatusResponse(data []byte) (map[string]any, error) {
	out := map[string]any{}
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
		case fn == 1 && wt == 2:
			sl, np, err := wasmReadLengthDelimited(data, pos)
			if err != nil {
				return nil, err
			}
			out["application"] = wasmParseApplicationInfo(sl)
			pos = np
		case fn == 3 && wt == 2:
			sl, np, err := wasmReadLengthDelimited(data, pos)
			if err != nil {
				return nil, err
			}
			out["error"] = string(sl)
			pos = np
		case fn == 4 && wt == 2:
			sl, np, err := wasmReadLengthDelimited(data, pos)
			if err != nil {
				return nil, err
			}
			out["node_id"] = string(sl)
			pos = np
		case fn == 5 && wt == 2:
			sl, np, err := wasmReadLengthDelimited(data, pos)
			if err != nil {
				return nil, err
			}
			out["node_address"] = string(sl)
			pos = np
		default:
			pos, err = skipField(data, pos, wt)
			if err != nil {
				return nil, err
			}
		}
	}
	return out, nil
}

// --- hostWire* / hostDecode* entry points ---

func hostWireCreateShardGroupRequest(request any) (string, error) {
	m, ok := mapAsStringAny(request)
	if !ok {
		return "", fmt.Errorf("create_shard_group: expected map[string]any")
	}
	if _, has := mapAsStringAny(m["shard_config"]); has {
		return "", fmt.Errorf("create_shard_group: shard_config is not supported in TinyGo WASM (omit or use native Go)")
	}
	groupID := strVal(cfgGet(m, "group_id"))
	shardCount := u32Val(cfgGet(m, "shard_count"))
	part := partitionEnum(strVal(cfgGet(m, "partition_strategy")))
	reb := rebalanceEnum(strVal(cfgGet(m, "rebalance_policy")))
	cfg := wasmEncodeDataParallelConfig(groupID, shardCount, part, reb, cfgGet(m, "placement"))
	var out []byte
	out = appendLengthDelimited(out, 1, cfg)
	out = wasmAppendString(out, 2, strVal(cfgGet(m, "actor_type")))
	switch st := cfgGet(m, "initial_state").(type) {
	case nil:
	case string:
		out = wasmAppendBytes(out, 4, []byte(st))
	case map[string]any:
		b, err := json.Marshal(st)
		if err != nil {
			return "", fmt.Errorf("create_shard_group: initial_state: %w", err)
		}
		out = wasmAppendBytes(out, 4, b)
	default:
		b, err := json.Marshal(st)
		if err != nil {
			return "", fmt.Errorf("create_shard_group: initial_state: %w", err)
		}
		out = wasmAppendBytes(out, 4, b)
	}
	if meta, ok := mapAsStringAny(cfgGet(m, "metadata")); ok && len(meta) > 0 {
		out = wasmAppendStringMap(out, 5, stringMapFromAnyMap(meta))
	}
	return string(out), nil
}

func hostDecodeCreateShardGroupResponse(raw string) (map[string]any, error) {
	return wasmParseCreateShardGroupResponse([]byte(raw))
}

func hostWireScatterGatherRequest(request any) (string, error) {
	m, ok := mapAsStringAny(request)
	if !ok {
		return "", fmt.Errorf("scatter_gather: expected map[string]any")
	}
	mt, payload := buildQueryPayload(m)
	if mt == "" && (len(payload) == 0 || string(payload) == "{}") {
		return "", fmt.Errorf("scatter_gather: missing query/message_type")
	}
	qm := wasmEncodeCommonMessage(mt, payload)
	var req []byte
	req = wasmAppendString(req, 1, strVal(m["group_id"]))
	req = appendLengthDelimited(req, 2, qm)
	if d := wasmEncodeDurationFromMs(u64Val(m["timeout_ms"])); len(d) > 0 {
		req = appendLengthDelimited(req, 3, d)
	}
	req = wasmAppendUInt32(req, 4, aggregationEnum(strVal(m["aggregation"])))
	req = wasmAppendUInt32(req, 5, u32Val(m["min_responses"]))
	return string(req), nil
}

func hostDecodeScatterGatherResponse(raw string) (map[string]any, error) {
	return wasmParseScatterGatherResponse([]byte(raw))
}

func hostWireBroadcastShardGroupRequest(request any) (string, error) {
	m, ok := mapAsStringAny(request)
	if !ok {
		return "", fmt.Errorf("broadcast_shard_group: expected map[string]any")
	}
	mt, payload, ok := buildBroadcastPayload(m)
	if !ok {
		return "", fmt.Errorf("broadcast_shard_group: missing message")
	}
	msg := wasmEncodeCommonMessage(mt, payload)
	var req []byte
	req = wasmAppendString(req, 1, strVal(m["group_id"]))
	req = appendLengthDelimited(req, 2, msg)
	if d := wasmEncodeDurationFromMs(u64Val(m["timeout_ms"])); len(d) > 0 {
		req = appendLengthDelimited(req, 3, d)
	}
	req = wasmAppendUInt32(req, 4, u32Val(m["min_acks"]))
	return string(req), nil
}

func hostDecodeBroadcastShardGroupResponse(raw string) (map[string]any, error) {
	return wasmParseBroadcastOrBarrierResponse([]byte(raw))
}

func hostWireReduceShardGroupRequest(request any) (string, error) {
	return wasmWireReduceLike(request, false)
}

func hostWireAllReduceShardGroupRequest(request any) (string, error) {
	return wasmWireReduceLike(request, true)
}

func wasmWireReduceLike(request any, allReduce bool) (string, error) {
	m, ok := mapAsStringAny(request)
	if !ok {
		return "", fmt.Errorf("reduce_shard_group: expected map[string]any")
	}
	mt, payload, _ := buildMapFunctionPayload(m)
	if mt == "" && len(payload) == 0 {
		return "", fmt.Errorf("reduce_shard_group: missing map_function/message_type")
	}
	mf := wasmEncodeCommonMessage(mt, payload)
	var req []byte
	req = wasmAppendString(req, 1, strVal(m["group_id"]))
	req = appendLengthDelimited(req, 2, mf)
	if d := wasmEncodeDurationFromMs(u64Val(m["timeout_ms"])); len(d) > 0 {
		req = appendLengthDelimited(req, 3, d)
	}
	req = wasmAppendUInt32(req, 4, u32Val(m["min_responses"]))
	req = wasmAppendUInt32(req, 5, reductionEnum(strVal(m["reduction"])))
	if target := strVal(m["target"]); target != "" {
		var tf []byte
		tf = wasmAppendString(tf, 1, target)
		req = appendLengthDelimited(req, 6, tf)
	}
	_ = allReduce
	return string(req), nil
}

func hostDecodeReduceShardGroupResponse(raw string) (map[string]any, error) {
	return wasmParseReduceOrAllReduceResponse([]byte(raw))
}

func hostDecodeAllReduceShardGroupResponse(raw string) (map[string]any, error) {
	return wasmParseReduceOrAllReduceResponse([]byte(raw))
}

func hostWireBarrierShardGroupRequest(request any) (string, error) {
	m, ok := mapAsStringAny(request)
	if !ok {
		return "", fmt.Errorf("barrier_shard_group: expected map[string]any")
	}
	var req []byte
	req = wasmAppendString(req, 1, strVal(m["group_id"]))
	req = wasmAppendString(req, 2, strVal(m["barrier_id"]))
	req = wasmAppendUInt64(req, 3, u64Val(m["round"]))
	if d := wasmEncodeDurationFromMs(u64Val(m["timeout_ms"])); len(d) > 0 {
		req = appendLengthDelimited(req, 4, d)
	}
	req = wasmAppendUInt32(req, 5, u32Val(m["min_acks"]))
	return string(req), nil
}

func hostDecodeBarrierShardGroupResponse(raw string) (map[string]any, error) {
	return wasmParseBroadcastOrBarrierResponse([]byte(raw))
}

func hostWireBulkUpdateShardGroupRequest(request any) (string, error) {
	return "", fmt.Errorf("bulk_update_shard_group: not supported in TinyGo WASM (manual wire not implemented)")
}

func hostDecodeBulkUpdateShardGroupResponse(raw string) (map[string]any, error) {
	return nil, fmt.Errorf("bulk_update_shard_group: protobuf decode not available in TinyGo WASM")
}

func hostWireMapShardGroupRequest(request any) (string, error) {
	return "", fmt.Errorf("map_shard_group: not supported in TinyGo WASM (manual wire not implemented)")
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

func hostDecodeApplicationMetricsResponse(raw string) (map[string]any, error) {
	return wasmParseApplicationMetrics([]byte(raw)), nil
}

func hostDecodeApplicationGetStatusResponse(raw string) (map[string]any, error) {
	return wasmParseGetApplicationStatusResponse([]byte(raw))
}
