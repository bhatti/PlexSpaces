// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Weather Actor — Service Link + KV Cache Example (Go WASM)
//
// Demonstrates outbound HTTP via a named service link ("weather-api") combined
// with KV-based caching.  The host handles retries, circuit breaking, and
// auth-header injection transparently.
//
// Service Link Configuration
// ---------------------------
// The "weather-api" link must exist in RuntimeConfig.service_links (release.toml):
//
//	[[runtime.service_links]]
//	name     = "weather-api"
//	base_url = "https://api.open-meteo.com"
//	transport = "HTTP"
//
// Build with:
//
//	tinygo build -target=wasi -o weather_actor.wasm .

package main

import (
	"encoding/base64"
	"encoding/json"
	"fmt"
	"strings"

	"github.com/bhatti/PlexSpaces/sdks/go/plexspaces"
)

const (
	linkName   = "weather-api"
	cacheTTLMs = 5 * 60 * 1000 // 5 minutes
)

var h = plexspaces.NewHost()

// weatherActor holds actor state (serialized via GetState/SetState).
type weatherActor struct {
	plexspaces.BaseActor

	ActorID     string `json:"actor_id"`
	CacheHits   int    `json:"cache_hits"`
	CacheMisses int    `json:"cache_misses"`
}

func newWeatherActor() *weatherActor {
	a := &weatherActor{}
	a.SetSelf(a)
	return a
}

// Init is called once when the actor is first spawned or reactivated.
func (a *weatherActor) Init(configJSON string) string {
	var cfg struct {
		ActorID string         `json:"actor_id"`
		Args    map[string]any `json:"args"`
	}
	if err := json.Unmarshal([]byte(configJSON), &cfg); err != nil {
		return "ERROR: " + err.Error()
	}
	a.ActorID = cfg.ActorID
	h.Log("info", fmt.Sprintf("WeatherActor initialized: %s", a.ActorID))
	return ""
}

// Handle dispatches messages to handler methods.
func (a *weatherActor) Handle(from, msgType, payloadJSON string) string {
	switch msgType {
	case "get_weather":
		return a.handleGetWeather(payloadJSON)
	case "cache_stats":
		return a.handleCacheStats()
	case "clear_cache":
		return a.handleClearCache()
	default:
		return fmt.Sprintf(`{"error":"unknown message type: %s"}`, msgType)
	}
}

func (a *weatherActor) handleGetWeather(payloadJSON string) string {
	var req struct {
		City string `json:"city"`
	}
	if err := json.Unmarshal([]byte(payloadJSON), &req); err != nil || req.City == "" {
		req.City = "London"
	}
	city := req.City
	cacheKey := "weather:" + city

	// Try cache first
	cached := h.KVGet(cacheKey)
	if strings.HasPrefix(cached, "ERROR:") {
		h.Log("warn", fmt.Sprintf("Cache read failed for %s: %s", city, cached))
	} else if cached != "" {
		var data map[string]any
		if json.Unmarshal([]byte(cached), &data) == nil {
			fetchedAt, _ := data["fetched_at_ms"].(float64)
			nowMs := float64(h.NowMs())
			if nowMs-fetchedAt < cacheTTLMs {
				a.CacheHits++
				h.Log("debug", fmt.Sprintf("Cache HIT for %s", city))
				data["city"] = city
				data["source"] = "cache"
				out, _ := json.Marshal(data)
				return string(out)
			}
		}
	}

	// Cache miss — call service link
	a.CacheMisses++
	h.Log("info", fmt.Sprintf("Cache MISS for %s — calling %s", city, linkName))

	client := plexspaces.NewServiceHTTPClient(h, linkName)
	path := fmt.Sprintf("/v1/forecast?latitude=51.5&longitude=-0.12&current=temperature_2m,wind_speed_10m&city=%s", city)
	resp, err := client.Get(path, nil)
	if err != nil {
		h.Log("error", fmt.Sprintf("Weather API call failed: %v", err))
		out, _ := json.Marshal(map[string]any{
			"city":   city,
			"error":  err.Error(),
			"source": "api",
		})
		return string(out)
	}

	bodyStr, _ := resp["body"].(string)
	weatherData := parseWeatherBody(bodyStr)
	current, _ := weatherData["current"].(map[string]any)
	tempC, _ := current["temperature_2m"].(float64)
	windKph, _ := current["wind_speed_10m"].(float64)

	cachedData := map[string]any{
		"temp_c":        tempC,
		"wind_kph":      windKph,
		"fetched_at_ms": float64(h.NowMs()),
	}
	if cacheJSON, err := json.Marshal(cachedData); err == nil {
		if cacheWrite := h.KVPut(cacheKey, string(cacheJSON)); strings.HasPrefix(cacheWrite, "ERROR:") {
			h.Log("warn", fmt.Sprintf("Cache write failed for %s: %s", city, cacheWrite))
		}
	}

	cachedData["city"] = city
	cachedData["source"] = "api"
	out, _ := json.Marshal(cachedData)
	return string(out)
}

func parseWeatherBody(bodyStr string) map[string]any {
	if bodyStr == "" {
		return map[string]any{}
	}

	var weatherData map[string]any
	if json.Unmarshal([]byte(bodyStr), &weatherData) == nil {
		return weatherData
	}

	decoded, err := base64.StdEncoding.DecodeString(bodyStr)
	if err != nil {
		return map[string]any{}
	}
	if json.Unmarshal(decoded, &weatherData) != nil {
		return map[string]any{}
	}
	return weatherData
}

func (a *weatherActor) handleCacheStats() string {
	out, _ := json.Marshal(map[string]int{
		"hits":   a.CacheHits,
		"misses": a.CacheMisses,
	})
	return string(out)
}

func (a *weatherActor) handleClearCache() string {
	a.CacheHits = 0
	a.CacheMisses = 0
	out, _ := json.Marshal(map[string]bool{"cleared": true})
	return string(out)
}

func init() {
	router := plexspaces.NewActorRouter()
	router.Route("weather", func() plexspaces.Actor { return newWeatherActor() })
	plexspaces.Register(router)
}

func main() {}
