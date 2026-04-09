// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 PlexSpaces Contributors

//go:build !wasm

package plexspaces

import (
	"testing"

	applicationv1 "github.com/plexobject/plexspaces/sdks/go/plexspaces/proto/plexspaces/v1/application"
	"google.golang.org/protobuf/proto"
)

func TestEncodeApplicationMetricsMapMatchesProtoMarshal(t *testing.T) {
	m := map[string]any{
		"actor_counts": map[string]any{
			"leader": float64(1),
			"worker": float64(8),
		},
		"supervisor_count": float64(2),
		"uptime_seconds":   float64(3600),
		"message_count":    float64(99),
		"error_count":      float64(3),
		"counter_metrics": map[string]any{
			"worker_messages": float64(10),
		},
		"latency_totals_ms": map[string]any{
			"worker": float64(500),
		},
		"latency_max_ms": map[string]any{
			"worker": float64(50),
		},
		"latency_samples": map[string]any{
			"worker": float64(5),
		},
	}

	manual, err := encodeApplicationMetricsMapToProtobuf(m)
	if err != nil {
		t.Fatalf("encodeApplicationMetricsMapToProtobuf: %v", err)
	}

	want := &applicationv1.ApplicationMetrics{
		ActorCounts: map[string]uint64{
			"leader": 1,
			"worker": 8,
		},
		SupervisorCount: 2,
		UptimeSeconds:   3600,
		MessageCount:    99,
		ErrorCount:      3,
		CounterMetrics: map[string]uint64{
			"worker_messages": 10,
		},
		LatencyTotalsMs: map[string]uint64{
			"worker": 500,
		},
		LatencyMaxMs: map[string]uint64{
			"worker": 50,
		},
		LatencySamples: map[string]uint64{
			"worker": 5,
		},
	}
	gen, err := proto.Marshal(want)
	if err != nil {
		t.Fatalf("proto.Marshal: %v", err)
	}

	var fromManual applicationv1.ApplicationMetrics
	if err := proto.Unmarshal(manual, &fromManual); err != nil {
		t.Fatalf("proto.Unmarshal(manual): %v", err)
	}
	var fromGen applicationv1.ApplicationMetrics
	if err := proto.Unmarshal(gen, &fromGen); err != nil {
		t.Fatalf("proto.Unmarshal(gen): %v", err)
	}
	if !proto.Equal(&fromManual, &fromGen) {
		t.Fatalf("decoded messages differ:\nmanual -> %s\ngen -> %s", fromManual.String(), fromGen.String())
	}
}

func TestEncodeApplicationMetricsMapRejectsBadNestedType(t *testing.T) {
	_, err := encodeApplicationMetricsMapToProtobuf(map[string]any{
		"counter_metrics": []any{1, 2},
	})
	if err == nil {
		t.Fatal("expected error for counter_metrics not map[string]any")
	}
}

func TestEncodeApplicationMetricsMapRejectsBadScalarType(t *testing.T) {
	_, err := encodeApplicationMetricsMapToProtobuf(map[string]any{
		"message_count": true,
	})
	if err == nil {
		t.Fatal("expected error for bool message_count")
	}
}

// TestEncodeApplicationMetricsPartialWorkerDelta matches the delta shape WASM actors send on
// ApplicationMetricsAdd (sparse maps; omitted proto fields encode as zero / absent).
func TestEncodeApplicationMetricsPartialWorkerDelta(t *testing.T) {
	m := map[string]any{
		"message_count": float64(1),
		"counter_metrics": map[string]any{
			"worker_messages":           float64(1),
			"chunk_operation_count":     float64(48),
			"embedding_operation_count": float64(48),
			"retrieval_operation_count": float64(5),
			"bytes_ingested":            float64(98304),
		},
		"latency_totals_ms": map[string]any{
			"worker":              float64(2),
			"worker.compute":      float64(2),
			"worker.coordination": float64(0),
		},
		"latency_max_ms": map[string]any{
			"worker":              float64(2),
			"worker.compute":      float64(2),
			"worker.coordination": float64(0),
		},
		"latency_samples": map[string]any{
			"worker":              float64(1),
			"worker.compute":      float64(1),
			"worker.coordination": float64(1),
		},
	}
	manual, err := encodeApplicationMetricsMapToProtobuf(m)
	if err != nil {
		t.Fatalf("encodeApplicationMetricsMapToProtobuf: %v", err)
	}
	want := &applicationv1.ApplicationMetrics{
		MessageCount: 1,
		CounterMetrics: map[string]uint64{
			"worker_messages":           1,
			"chunk_operation_count":     48,
			"embedding_operation_count": 48,
			"retrieval_operation_count": 5,
			"bytes_ingested":            98304,
		},
		LatencyTotalsMs: map[string]uint64{
			"worker":              2,
			"worker.compute":      2,
			"worker.coordination": 0,
		},
		LatencyMaxMs: map[string]uint64{
			"worker":              2,
			"worker.compute":      2,
			"worker.coordination": 0,
		},
		LatencySamples: map[string]uint64{
			"worker":              1,
			"worker.compute":      1,
			"worker.coordination": 1,
		},
	}
	gen, err := proto.Marshal(want)
	if err != nil {
		t.Fatalf("proto.Marshal: %v", err)
	}
	var fromManual applicationv1.ApplicationMetrics
	if err := proto.Unmarshal(manual, &fromManual); err != nil {
		t.Fatalf("proto.Unmarshal(manual): %v", err)
	}
	var fromGen applicationv1.ApplicationMetrics
	if err := proto.Unmarshal(gen, &fromGen); err != nil {
		t.Fatalf("proto.Unmarshal(gen): %v", err)
	}
	if !proto.Equal(&fromManual, &fromGen) {
		t.Fatalf("partial delta wire differs from reference marshal:\nmanual decode -> %s\nref decode -> %s",
			fromManual.String(), fromGen.String())
	}
}
