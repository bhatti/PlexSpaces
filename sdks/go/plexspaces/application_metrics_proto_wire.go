// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Manual protobuf wire encoding for plexspaces.application.v1.ApplicationMetrics.
// Must match prost decoding in crates/wasm-runtime (simple_component_host) and
// google.golang.org/protobuf/proto.Marshal on the generated Go type.
//
// TinyGo WASM cannot use generated .pb.go + protoreflect; host imports use these bytes.

package plexspaces

import (
	"encoding/json"
	"fmt"
	"math"
	"strconv"
)

// encodeApplicationMetricsMapToProtobuf encodes a map shaped like JSON deltas from actors
// into canonical protobuf wire for ApplicationMetrics. Returns an error if any field has
// a type that cannot be represented as the corresponding proto scalar or map value.
func encodeApplicationMetricsMapToProtobuf(m map[string]any) ([]byte, error) {
	if m == nil {
		return nil, fmt.Errorf("application_metrics: nil map")
	}
	actorCounts, err := u64StringMapFromAnyStrict(m, "actor_counts")
	if err != nil {
		return nil, err
	}
	supervisorCount, err := u32FromAnyStrict(m, "supervisor_count")
	if err != nil {
		return nil, err
	}
	uptimeSeconds, err := u64FromAnyStrict(m, "uptime_seconds")
	if err != nil {
		return nil, err
	}
	messageCount, err := u64FromAnyStrict(m, "message_count")
	if err != nil {
		return nil, err
	}
	errorCount, err := u64FromAnyStrict(m, "error_count")
	if err != nil {
		return nil, err
	}
	counterMetrics, err := u64StringMapFromAnyStrict(m, "counter_metrics")
	if err != nil {
		return nil, err
	}
	latencyTotals, err := u64StringMapFromAnyStrict(m, "latency_totals_ms")
	if err != nil {
		return nil, err
	}
	latencyMax, err := u64StringMapFromAnyStrict(m, "latency_max_ms")
	if err != nil {
		return nil, err
	}
	latencySamples, err := u64StringMapFromAnyStrict(m, "latency_samples")
	if err != nil {
		return nil, err
	}

	var buf []byte
	buf = appendAppMetricsUint64Map(buf, 1, actorCounts)
	buf = appendAppMetricsUInt32(buf, 2, supervisorCount)
	buf = appendAppMetricsUInt64(buf, 3, uptimeSeconds)
	buf = appendAppMetricsUInt64(buf, 4, messageCount)
	buf = appendAppMetricsUInt64(buf, 5, errorCount)
	buf = appendAppMetricsUint64Map(buf, 6, counterMetrics)
	buf = appendAppMetricsUint64Map(buf, 7, latencyTotals)
	buf = appendAppMetricsUint64Map(buf, 8, latencyMax)
	buf = appendAppMetricsUint64Map(buf, 9, latencySamples)
	return buf, nil
}

func appendAppMetricsTagVarint(buf []byte, fieldNum int, wireType int) []byte {
	return appendVarint(buf, uint64(fieldNum<<3|wireType))
}

func appendAppMetricsUInt32(buf []byte, fieldNum int, v uint32) []byte {
	buf = appendAppMetricsTagVarint(buf, fieldNum, 0)
	return appendVarint(buf, uint64(v))
}

func appendAppMetricsUInt64(buf []byte, fieldNum int, v uint64) []byte {
	buf = appendAppMetricsTagVarint(buf, fieldNum, 0)
	return appendVarint(buf, v)
}

func appendAppMetricsUint64Map(buf []byte, fieldNum int, m map[string]uint64) []byte {
	for k, v := range m {
		var e []byte
		e = appendLengthDelimited(e, 1, []byte(k))
		e = appendAppMetricsUInt64(e, 2, v)
		buf = appendLengthDelimited(buf, fieldNum, e)
	}
	return buf
}

func u64StringMapFromAnyStrict(m map[string]any, field string) (map[string]uint64, error) {
	raw, ok := m[field]
	if !ok || raw == nil {
		return nil, nil
	}
	inner, ok := raw.(map[string]any)
	if !ok {
		return nil, fmt.Errorf("application_metrics.%s: expected map[string]any, got %T", field, raw)
	}
	out := make(map[string]uint64, len(inner))
	for k, v := range inner {
		u, err := uint64FromMetricsValue(field+"["+k+"]", v)
		if err != nil {
			return nil, err
		}
		out[k] = u
	}
	return out, nil
}

func u64FromAnyStrict(m map[string]any, field string) (uint64, error) {
	raw, ok := m[field]
	if !ok || raw == nil {
		return 0, nil
	}
	return uint64FromMetricsValue(field, raw)
}

func u32FromAnyStrict(m map[string]any, field string) (uint32, error) {
	u, err := u64FromAnyStrict(m, field)
	if err != nil {
		return 0, err
	}
	if u > math.MaxUint32 {
		return 0, fmt.Errorf("application_metrics.%s: value %d overflows uint32", field, u)
	}
	return uint32(u), nil
}

func uint64FromMetricsValue(path string, v any) (uint64, error) {
	switch t := v.(type) {
	case uint64:
		return t, nil
	case uint32:
		return uint64(t), nil
	case uint:
		return uint64(t), nil
	case int:
		if t < 0 {
			return 0, fmt.Errorf("%s: negative int", path)
		}
		return uint64(t), nil
	case int64:
		if t < 0 {
			return 0, fmt.Errorf("%s: negative int64", path)
		}
		return uint64(t), nil
	case int32:
		if t < 0 {
			return 0, fmt.Errorf("%s: negative int32", path)
		}
		return uint64(t), nil
	case float64:
		if t < 0 || t != math.Trunc(t) {
			return 0, fmt.Errorf("%s: float64 must be non-negative integer, got %v", path, t)
		}
		u := uint64(t)
		if float64(u) != t {
			return 0, fmt.Errorf("%s: float64 loses precision as uint64 (use int or string)", path)
		}
		return u, nil
	case string:
		n, err := strconv.ParseUint(t, 10, 64)
		if err != nil {
			return 0, fmt.Errorf("%s: parse uint from string: %w", path, err)
		}
		return n, nil
	case json.Number:
		n, err := strconv.ParseUint(string(t), 10, 64)
		if err != nil {
			return 0, fmt.Errorf("%s: json.Number to uint64: %w", path, err)
		}
		return n, nil
	default:
		return 0, fmt.Errorf("%s: unsupported type %T (use int, uint64, float64 whole number, string, or json.Number)", path, v)
	}
}
