// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Native builds: shard-group and application host imports use JSON for test stubs.

//go:build !wasm

package plexspaces

import "encoding/json"

func hostDecodeActorHostJSONMap(raw string) (map[string]any, error) {
	var out map[string]any
	err := json.Unmarshal([]byte(raw), &out)
	return out, err
}

func hostWireCreateShardGroupRequest(req CreateShardGroupRequest) (string, error) {
	return marshalPayload(req), nil
}

func hostDecodeCreateShardGroupResponse(raw string) (map[string]any, error) {
	return hostDecodeActorHostJSONMap(raw)
}

func hostWireBulkUpdateShardGroupRequest(req BulkUpdateShardGroupRequest) (string, error) {
	return marshalPayload(req), nil
}

func hostDecodeBulkUpdateShardGroupResponse(raw string) (map[string]any, error) {
	return hostDecodeActorHostJSONMap(raw)
}

func hostWireMapShardGroupRequest(req MapShardGroupRequest) (string, error) {
	return marshalPayload(req), nil
}

func hostDecodeMapShardGroupResponse(raw string) (map[string]any, error) {
	return hostDecodeActorHostJSONMap(raw)
}

func hostWireScatterGatherRequest(req ScatterGatherRequest) (string, error) {
	return marshalPayload(req), nil
}

func hostDecodeScatterGatherResponse(raw string) (map[string]any, error) {
	return hostDecodeActorHostJSONMap(raw)
}

func hostWireBroadcastShardGroupRequest(req BroadcastShardGroupRequest) (string, error) {
	return marshalPayload(req), nil
}

func hostDecodeBroadcastShardGroupResponse(raw string) (map[string]any, error) {
	return hostDecodeActorHostJSONMap(raw)
}

func hostWireReduceShardGroupRequest(req ReduceShardGroupRequest) (string, error) {
	return marshalPayload(req), nil
}

func hostDecodeReduceShardGroupResponse(raw string) (map[string]any, error) {
	return hostDecodeActorHostJSONMap(raw)
}

func hostWireAllReduceShardGroupRequest(req AllReduceShardGroupRequest) (string, error) {
	return marshalPayload(req), nil
}

func hostDecodeAllReduceShardGroupResponse(raw string) (map[string]any, error) {
	return hostDecodeActorHostJSONMap(raw)
}

func hostWireBarrierShardGroupRequest(req BarrierShardGroupRequest) (string, error) {
	return marshalPayload(req), nil
}

func hostDecodeBarrierShardGroupResponse(raw string) (map[string]any, error) {
	return hostDecodeActorHostJSONMap(raw)
}

func hostWireSpawnActorsRequest(request any) (string, error) {
	return marshalPayload(request), nil
}

func hostDecodeSpawnActorsResponse(raw string) (map[string]any, error) {
	return hostDecodeActorHostJSONMap(raw)
}

func hostWireApplicationMetrics(metrics any) (string, error) {
	return marshalPayload(metrics), nil
}

func hostDecodeApplicationMetricsResponse(raw string) (ApplicationMetrics, error) {
	var m map[string]any
	if err := json.Unmarshal([]byte(raw), &m); err != nil {
		return ApplicationMetrics{}, err
	}
	return ApplicationMetrics{
		SupervisorCount: nativeUint64Field(m, "supervisor_count"),
		UptimeSeconds:   nativeUint64Field(m, "uptime_seconds"),
		MessageCount:    nativeUint64Field(m, "message_count"),
		ErrorCount:      nativeUint64Field(m, "error_count"),
		CounterMetrics:  nativeUint64Map(m["counter_metrics"]),
		LatencyTotalsMs: nativeUint64Map(m["latency_totals_ms"]),
		LatencyMaxMs:    nativeUint64Map(m["latency_max_ms"]),
		LatencySamples:  nativeUint64Map(m["latency_samples"]),
		ActorCounts:     nativeUint64Map(m["actor_counts"]),
	}, nil
}

func hostDecodeApplicationGetStatusResponse(raw string) (ApplicationStatus, error) {
	var m map[string]any
	if err := json.Unmarshal([]byte(raw), &m); err != nil {
		return ApplicationStatus{}, err
	}
	out := ApplicationStatus{
		NodeID:      nativeStrField(m, "node_id"),
		NodeAddress: nativeStrField(m, "node_address"),
	}
	if app, ok := m["application"].(map[string]any); ok {
		out.ApplicationID = nativeStrField(app, "application_id")
		out.ApplicationName = nativeStrField(app, "name")
		out.ApplicationVersion = nativeStrField(app, "version")
		out.StatusCode = nativeStrField(app, "status")
	}
	return out, nil
}

func nativeUint64Field(m map[string]any, key string) uint64 {
	switch v := m[key].(type) {
	case float64:
		if v > 0 {
			return uint64(v)
		}
	case uint64:
		return v
	}
	return 0
}

func nativeStrField(m map[string]any, key string) string {
	if s, ok := m[key].(string); ok {
		return s
	}
	return ""
}

func nativeUint64Map(v any) map[string]uint64 {
	m, ok := v.(map[string]any)
	if !ok {
		return map[string]uint64{}
	}
	result := make(map[string]uint64, len(m))
	for k, val := range m {
		switch tv := val.(type) {
		case float64:
			if tv > 0 {
				result[k] = uint64(tv)
			}
		case uint64:
			result[k] = tv
		}
	}
	return result
}
