// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Native builds: shard-group and application host payloads stay JSON for test stubs.

//go:build !wasm

package plexspaces

import "encoding/json"

func hostDecodeActorHostJSONMap(raw string) (map[string]any, error) {
	var out map[string]any
	err := json.Unmarshal([]byte(raw), &out)
	return out, err
}

func hostWireCreateShardGroupRequest(request any) (string, error) {
	return marshalPayload(request), nil
}

func hostDecodeCreateShardGroupResponse(raw string) (map[string]any, error) {
	return hostDecodeActorHostJSONMap(raw)
}

func hostWireBulkUpdateShardGroupRequest(request any) (string, error) {
	return marshalPayload(request), nil
}

func hostDecodeBulkUpdateShardGroupResponse(raw string) (map[string]any, error) {
	return hostDecodeActorHostJSONMap(raw)
}

func hostWireMapShardGroupRequest(request any) (string, error) {
	return marshalPayload(request), nil
}

func hostDecodeMapShardGroupResponse(raw string) (map[string]any, error) {
	return hostDecodeActorHostJSONMap(raw)
}

func hostWireScatterGatherRequest(request any) (string, error) {
	return marshalPayload(request), nil
}

func hostDecodeScatterGatherResponse(raw string) (map[string]any, error) {
	return hostDecodeActorHostJSONMap(raw)
}

func hostWireBroadcastShardGroupRequest(request any) (string, error) {
	return marshalPayload(request), nil
}

func hostDecodeBroadcastShardGroupResponse(raw string) (map[string]any, error) {
	return hostDecodeActorHostJSONMap(raw)
}

func hostWireReduceShardGroupRequest(request any) (string, error) {
	return marshalPayload(request), nil
}

func hostDecodeReduceShardGroupResponse(raw string) (map[string]any, error) {
	return hostDecodeActorHostJSONMap(raw)
}

func hostWireAllReduceShardGroupRequest(request any) (string, error) {
	return marshalPayload(request), nil
}

func hostDecodeAllReduceShardGroupResponse(raw string) (map[string]any, error) {
	return hostDecodeActorHostJSONMap(raw)
}

func hostWireBarrierShardGroupRequest(request any) (string, error) {
	return marshalPayload(request), nil
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

func hostDecodeApplicationMetricsResponse(raw string) (map[string]any, error) {
	return hostDecodeActorHostJSONMap(raw)
}

func hostDecodeApplicationGetStatusResponse(raw string) (map[string]any, error) {
	return hostDecodeActorHostJSONMap(raw)
}
