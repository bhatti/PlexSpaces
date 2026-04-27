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

# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2025 PlexSpaces Contributors

import base64
import json
from plexspaces import gen_server_actor, handler, init_handler, state
from plexspaces.host import host, ServiceHttpClient

CACHE_TTL_MS = 5 * 60 * 1000  # 5 minutes


def actor_node_id(actor_id: str) -> str:
    if "@" in actor_id:
        return actor_id.rsplit("@", 1)[1]
    return "local"


def actor_application_id(actor_id: str) -> str:
    if "//" in actor_id and "::" in actor_id:
        suffix = actor_id.split("//", 1)[1]
        qualified = suffix.split("@", 1)[0]
        return qualified.rsplit("::", 1)[1]
    if ":" in actor_id and "@" in actor_id:
        return actor_id.split(":", 1)[1].split("@", 1)[0]
    return ""


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
    application_id: str = state(default="")
    node_id: str = state(default="")
    cache_hits: int = state(default=0)
    cache_misses: int = state(default=0)

    @init_handler
    def on_init(self, config: dict):
        self.actor_id = config.get("actor_id", "")
        self.application_id = actor_application_id(self.actor_id)
        self.node_id = actor_node_id(self.actor_id)
        host.log("info", f"WeatherActor initialized: {self.actor_id}")

    def _cache_key_prefix(self) -> str:
        return f"{self.actor_id}:weather:"

    def _record_metrics(self, metric_name: str, extra_counters: dict = None) -> None:
        counters = {"weather_requests": 1, metric_name: 1}
        if isinstance(extra_counters, dict):
            counters.update(extra_counters)
        host.application_metrics_add(
            self.application_id,
            {
                "message_count": 1,
                "counter_metrics": counters,
            },
        )

    @handler("get_weather")
    def get_weather(self, city: str = "London") -> dict:
        """Fetch weather for a city, using KV cache to avoid redundant calls."""
        cache_key = f"{self._cache_key_prefix()}{city}"
        cached_raw = host.kv_get(cache_key) or ""
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
                    self._record_metrics("weather_cache_hits", {"cache_hits": 1})
                    host.log("debug", f"Cache HIT for {city}")
                    return {"city": city, **data, "source": "cache"}
            except (json.JSONDecodeError, KeyError):
                pass

        self.cache_misses += 1
        self._record_metrics("weather_cache_misses", {"cache_misses": 1})
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
            cache_write = host.kv_put(cache_key, json.dumps(result)) or ""
            if cache_write.startswith("ERROR:"):
                host.log("warn", f"Cache write failed for {city}: {cache_write}")
            self._record_metrics("weather_api_calls", {"weather_api_calls": 1})
            return {"city": city, **result, "source": "api"}
        except RuntimeError as exc:
            self._record_metrics("weather_errors", {"weather_errors": 1})
            host.log("error", f"Weather API call failed: {exc}")
            return {"city": city, "error": str(exc), "source": "api"}

    @handler("cache_stats")
    def cache_stats(self) -> dict:
        """Return cache hit/miss statistics."""
        self._record_metrics("weather_cache_stats")
        return {"hits": self.cache_hits, "misses": self.cache_misses}

    @handler("clear_cache")
    def clear_cache(self) -> dict:
        """Clear all cached weather data."""
        self.cache_hits = 0
        self.cache_misses = 0
        prefix = self._cache_key_prefix()
        keys_raw = host.kv_list(prefix) or "[]"
        try:
            keys = json.loads(keys_raw)
        except json.JSONDecodeError:
            keys = []
        if isinstance(keys, list):
            for key in keys:
                if isinstance(key, str):
                    host.kv_delete(key)
        self._record_metrics("weather_cache_clears")
        return {"cleared": True}

    @handler("app_metrics")
    def app_metrics(self) -> dict:
        """Return unified application metrics for this node via the WASM host path."""
        return host.application_get_metrics(self.application_id, self.node_id)
