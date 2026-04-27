// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Contract tests for WeatherActor (Go) — no running node required.

package main

import (
	"encoding/json"
	"strings"
	"testing"

	"github.com/plexobject/plexspaces/sdks/go/plexspaces"
)

func TestGetWeatherCacheMiss(t *testing.T) {
	plexspaces.ResetStubs()

	// Seed stub KV as empty (no cache)
	actor := newWeatherActor()
	actor.Init(`{"actor_id":"weather:test@node","args":{}}`)

	result := actor.Handle("caller", "get_weather", `{"city":"London"}`)

	var resp map[string]any
	if err := json.Unmarshal([]byte(result), &resp); err != nil {
		t.Fatalf("invalid JSON response: %v — raw: %s", err, result)
	}
	// Stub returns status:200 body:"" → weather data will be empty/zero
	if city, _ := resp["city"].(string); city != "London" {
		t.Errorf("expected city=London got %q", city)
	}
	if actor.CacheMisses != 1 {
		t.Errorf("expected CacheMisses=1 got %d", actor.CacheMisses)
	}
	if actor.CacheHits != 0 {
		t.Errorf("expected CacheHits=0 got %d", actor.CacheHits)
	}
}

func TestGetWeatherCacheHit(t *testing.T) {
	plexspaces.ResetStubs()

	actor := newWeatherActor()
	actor.Init(`{"actor_id":"weather:test@node","args":{}}`)

	// First call: cache miss, stores in KV
	actor.Handle("caller", "get_weather", `{"city":"Paris"}`)

	// Second call: should hit KV cache
	result := actor.Handle("caller", "get_weather", `{"city":"Paris"}`)

	var resp map[string]any
	if err := json.Unmarshal([]byte(result), &resp); err != nil {
		t.Fatalf("invalid JSON: %v", err)
	}
	if actor.CacheMisses != 1 {
		t.Errorf("expected CacheMisses=1 got %d", actor.CacheMisses)
	}
	if actor.CacheHits != 1 {
		t.Errorf("expected CacheHits=1 got %d", actor.CacheHits)
	}
}

func TestCacheStats(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newWeatherActor()
	actor.Init(`{"actor_id":"weather:test@node","args":{}}`)

	actor.Handle("caller", "get_weather", `{"city":"Berlin"}`)
	actor.Handle("caller", "get_weather", `{"city":"Berlin"}`)

	result := actor.Handle("caller", "cache_stats", `{}`)
	var stats map[string]int
	if err := json.Unmarshal([]byte(result), &stats); err != nil {
		t.Fatalf("invalid JSON: %v", err)
	}
	if stats["misses"] != 1 {
		t.Errorf("expected misses=1 got %d", stats["misses"])
	}
	if stats["hits"] != 1 {
		t.Errorf("expected hits=1 got %d", stats["hits"])
	}
}

func TestClearCache(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newWeatherActor()
	actor.Init(`{"actor_id":"weather:test@node","args":{}}`)

	actor.Handle("caller", "get_weather", `{"city":"Tokyo"}`)
	actor.Handle("caller", "get_weather", `{"city":"Tokyo"}`)

	result := actor.Handle("caller", "clear_cache", `{}`)
	var resp map[string]bool
	if err := json.Unmarshal([]byte(result), &resp); err != nil {
		t.Fatalf("invalid JSON: %v", err)
	}
	if !resp["cleared"] {
		t.Error("expected cleared=true")
	}
	if actor.CacheHits != 0 || actor.CacheMisses != 0 {
		t.Errorf("expected counters reset, got hits=%d misses=%d", actor.CacheHits, actor.CacheMisses)
	}
}

func TestDifferentCitiesCachedIndependently(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newWeatherActor()
	actor.Init(`{"actor_id":"weather:test@node","args":{}}`)

	actor.Handle("caller", "get_weather", `{"city":"Sydney"}`)
	actor.Handle("caller", "get_weather", `{"city":"Cairo"}`)
	actor.Handle("caller", "get_weather", `{"city":"Sydney"}`) // cached

	if actor.CacheMisses != 2 {
		t.Errorf("expected CacheMisses=2 got %d", actor.CacheMisses)
	}
	if actor.CacheHits != 1 {
		t.Errorf("expected CacheHits=1 got %d", actor.CacheHits)
	}
}

func TestUnknownMessage(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newWeatherActor()
	actor.Init(`{"actor_id":"weather:test@node","args":{}}`)
	result := actor.Handle("caller", "unknown_op", `{}`)
	if !strings.Contains(result, "unknown message type") {
		t.Errorf("expected error, got %s", result)
	}
}

func TestCacheTTLExpiry(t *testing.T) {
	plexspaces.ResetStubs()
	// Set a fixed base time so TTL checks are deterministic
	const baseMs = uint64(1_000_000)
	plexspaces.SetStubNowMs(baseMs)

	actor := newWeatherActor()
	actor.Init(`{"actor_id":"weather:test@node","args":{}}`)

	// First call: cache miss at t=baseMs
	first := actor.Handle("caller", "get_weather", `{"city":"Cairo"}`)
	var r1 map[string]any
	if err := json.Unmarshal([]byte(first), &r1); err != nil {
		t.Fatalf("invalid JSON: %v", err)
	}
	if src, _ := r1["source"].(string); src != "api" {
		t.Errorf("expected source=api, got %q", src)
	}
	if actor.CacheMisses != 1 {
		t.Errorf("expected CacheMisses=1, got %d", actor.CacheMisses)
	}

	// Within TTL (4 minutes): should hit cache
	plexspaces.SetStubNowMs(baseMs + 4*60*1000)
	second := actor.Handle("caller", "get_weather", `{"city":"Cairo"}`)
	var r2 map[string]any
	if err := json.Unmarshal([]byte(second), &r2); err != nil {
		t.Fatalf("invalid JSON: %v", err)
	}
	if src, _ := r2["source"].(string); src != "cache" {
		t.Errorf("expected source=cache after 4 min, got %q", src)
	}

	// Past TTL (6 minutes total): stale, must re-fetch from API
	plexspaces.SetStubNowMs(baseMs + 6*60*1000)
	third := actor.Handle("caller", "get_weather", `{"city":"Cairo"}`)
	var r3 map[string]any
	if err := json.Unmarshal([]byte(third), &r3); err != nil {
		t.Fatalf("invalid JSON: %v", err)
	}
	if src, _ := r3["source"].(string); src != "api" {
		t.Errorf("expected source=api after TTL expiry, got %q", src)
	}
	if actor.CacheMisses != 2 {
		t.Errorf("expected CacheMisses=2 after TTL expiry, got %d", actor.CacheMisses)
	}
	if actor.CacheHits != 1 {
		t.Errorf("expected CacheHits=1, got %d", actor.CacheHits)
	}
}

func TestStateRoundTrip(t *testing.T) {
	plexspaces.ResetStubs()
	actor := newWeatherActor()
	actor.Init(`{"actor_id":"weather:test@node","args":{}}`)
	actor.Handle("caller", "get_weather", `{"city":"Rome"}`)

	state := actor.GetState()
	restored := newWeatherActor()
	restored.SetState(state)

	if restored.CacheMisses != 1 {
		t.Errorf("state not restored: CacheMisses=%d", restored.CacheMisses)
	}
}
