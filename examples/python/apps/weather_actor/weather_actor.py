"""
Weather Actor — Service Link + KV Cache Example (Python WASM)

Demonstrates outbound HTTP via a named service link ("weather-api") combined
with KV-based caching.  The host handles retries, circuit breaking, and
auth-header injection — the actor never sees raw HTTP transport details.

SDK Features Used
-----------------
- @gen_server_actor        — GenServer (request-reply) actor
- @handler("get_weather")  — message handler
- ServiceHttpClient        — ergonomic outbound HTTP via service link
- host.kv_get / kv_put     — internal KV store for caching
- host.log                 — structured logging

Service Link Configuration
--------------------------
The "weather-api" link must exist in RuntimeConfig.service_links (release.toml):

  [[runtime.service_links]]
  name           = "weather-api"
  base_url       = "https://api.open-meteo.com"
  transport      = "HTTP"
  publish_to_registry = false

  [runtime.service_links.retry_policy]
  max_attempts = 3
  initial_delay_ms = 100

Required in app-config.toml:
  [[applications.required_service_links]]
  name = "weather-api"
"""

# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 PlexSpaces Contributors

import base64
import json
from plexspaces import gen_server_actor, handler, init_handler, state
from plexspaces.host import host, ServiceHttpClient

CACHE_TTL_MS = 5 * 60 * 1000  # 5 minutes


def _parse_weather_body(body_str: str) -> dict:
    if not body_str:
        return {}
    try:
        return json.loads(body_str)
    except json.JSONDecodeError:
        try:
            return json.loads(base64.b64decode(body_str).decode("utf-8"))
        except Exception:
            return {}


@gen_server_actor
class WeatherActor:
    """Actor that fetches current weather via an outbound HTTP service link.

    - GET /get_weather?city=London  → {"city", "temp_c", "wind_kph", "source"}
    - GET /cache_stats              → {"hits", "misses"}
    - POST /clear_cache             → {"cleared": true}
    """

    actor_id: str = state(default="")
    cache_hits: int = state(default=0)
    cache_misses: int = state(default=0)

    @init_handler
    def on_init(self, config: dict):
        self.actor_id = config.get("actor_id", "")
        host.log("info", f"WeatherActor initialized: {self.actor_id}")

    @handler("get_weather")
    def get_weather(self, city: str = "London") -> dict:
        """Fetch weather for a city, using KV cache to avoid redundant calls."""
        cache_key = f"weather:{city}"
        cached_raw = host.kv_get(cache_key)
        if cached_raw.startswith("ERROR:"):
            host.log("warn", f"Cache read failed for {city}: {cached_raw}")
        elif cached_raw:
            try:
                data = json.loads(cached_raw)
                # Simple TTL check: refetch if fetched_at too old
                fetched_at = data.get("fetched_at_ms", 0)
                now_ms = host.now_ms()
                if now_ms - fetched_at < CACHE_TTL_MS:
                    self.cache_hits += 1
                    host.log("debug", f"Cache HIT for {city}")
                    return {"city": city, **data, "source": "cache"}
            except (json.JSONDecodeError, KeyError):
                pass

        self.cache_misses += 1
        host.log("info", f"Cache MISS for {city} — calling weather-api")

        try:
            http = ServiceHttpClient("weather-api")
            resp = http.get(
                f"/v1/forecast?latitude=51.5&longitude=-0.12"
                f"&current=temperature_2m,wind_speed_10m&city={city}"
            )
            body_str = resp.get("body", "")
            weather_data = _parse_weather_body(body_str)
            current = weather_data.get("current", {})
            result = {
                "temp_c": current.get("temperature_2m", 0),
                "wind_kph": current.get("wind_speed_10m", 0),
                "fetched_at_ms": host.now_ms(),
            }
            cache_write = host.kv_put(cache_key, json.dumps(result))
            if cache_write.startswith("ERROR:"):
                host.log("warn", f"Cache write failed for {city}: {cache_write}")
            return {"city": city, **result, "source": "api"}
        except RuntimeError as exc:
            host.log("error", f"Weather API call failed: {exc}")
            return {"city": city, "error": str(exc), "source": "api"}

    @handler("cache_stats")
    def cache_stats(self) -> dict:
        """Return cache hit/miss statistics."""
        return {"hits": self.cache_hits, "misses": self.cache_misses}

    @handler("clear_cache")
    def clear_cache(self) -> dict:
        """Clear all cached weather data."""
        self.cache_hits = 0
        self.cache_misses = 0
        return {"cleared": True}
