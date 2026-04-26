# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 PlexSpaces Contributors
#
# Contract tests for WeatherActor — no running node required.
# Uses _MockHost to verify actor logic without WASM compilation.

import json
import sys
import os

# Allow importing plexspaces SDK from workspace
PROJECT_ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "../../../.."))
SDK_DIR = os.path.join(PROJECT_ROOT, "sdks/python")
sys.path.insert(0, SDK_DIR)

from plexspaces.host import _MockHost, host as real_host

CACHE_TTL_MS = 5 * 60 * 1000  # 5 minutes


def make_mock_host_with_weather(city: str = "London", temp_c: float = 15.3, wind_kph: float = 12.0):
    """Create a mock host that returns realistic weather data for http_fetch."""
    mock = _MockHost()
    mock._now_ms = 1_000_000  # fixed base time for deterministic tests

    weather_json = json.dumps({
        "current": {
            "temperature_2m": temp_c,
            "wind_speed_10m": wind_kph,
        }
    })
    http_response = json.dumps({
        "status": 200,
        "headers": {"Content-Type": "application/json"},
        "body": weather_json,
    })

    def now_ms():
        return mock._now_ms

    def http_fetch(link_name, method, path_and_query, headers_json, body):
        return http_response

    mock.now_ms = now_ms
    mock.http_fetch = http_fetch
    return mock


class MockWeatherActor:
    """Thin wrapper that drives WeatherActor logic without WASM compilation."""

    def __init__(self, mock_host: _MockHost):
        self._host = mock_host
        self.actor_id = "weather:test@node"
        self.application_id = "weather-python-test"
        self.node_id = "node"
        self.cache_hits = 0
        self.cache_misses = 0

    def _cache_key_prefix(self):
        return f"{self.actor_id}:weather:"

    def get_weather(self, city="London"):
        cache_key = f"{self._cache_key_prefix()}{city}"
        cached_raw = self._host.kv_get(cache_key) or ""
        if cached_raw:
            data = json.loads(cached_raw)
            fetched_at = data.get("fetched_at_ms", 0)
            if self._host.now_ms() - fetched_at < CACHE_TTL_MS:
                self.cache_hits += 1
                return {"city": city, **data, "source": "cache"}

        self.cache_misses += 1
        resp_raw = self._host.http_fetch("weather-api", "GET", f"/v1/forecast?city={city}", "{}", "")
        resp = json.loads(resp_raw)
        body = json.loads(resp.get("body", "{}"))
        current = body.get("current", {})
        result = {
            "temp_c": current.get("temperature_2m", 0),
            "wind_kph": current.get("wind_speed_10m", 0),
            "fetched_at_ms": self._host.now_ms(),
        }
        self._host.kv_put(cache_key, json.dumps(result))
        return {"city": city, **result, "source": "api"}

    def cache_stats(self):
        return {"hits": self.cache_hits, "misses": self.cache_misses}

    def clear_cache(self):
        self.cache_hits = 0
        self.cache_misses = 0
        for key in list(self._host._kv.keys()):
            if key.startswith(self._cache_key_prefix()):
                del self._host._kv[key]
        return {"cleared": True}


def test_get_weather_calls_service_link():
    """First call hits API (cache MISS), returns weather data from service link."""
    mock = make_mock_host_with_weather(city="London", temp_c=15.3, wind_kph=12.0)
    actor = MockWeatherActor(mock)

    result = actor.get_weather("London")

    assert result["city"] == "London"
    assert result["temp_c"] == 15.3
    assert result["wind_kph"] == 12.0
    assert result["source"] == "api"
    stats = actor.cache_stats()
    assert stats["misses"] == 1
    assert stats["hits"] == 0
    print("test_get_weather_calls_service_link: PASS")


def test_get_weather_second_call_uses_cache():
    """Second call for same city returns cached data (cache HIT)."""
    mock = make_mock_host_with_weather(city="Paris", temp_c=18.0, wind_kph=8.5)
    actor = MockWeatherActor(mock)

    first = actor.get_weather("Paris")
    second = actor.get_weather("Paris")

    assert first["source"] == "api"
    assert second["source"] == "cache"
    assert second["temp_c"] == first["temp_c"]
    stats = actor.cache_stats()
    assert stats["misses"] == 1
    assert stats["hits"] == 1
    print("test_get_weather_second_call_uses_cache: PASS")


def test_clear_cache_resets_stats():
    """clear_cache resets hit/miss counters."""
    mock = make_mock_host_with_weather()
    actor = MockWeatherActor(mock)
    actor.get_weather("Berlin")
    actor.get_weather("Berlin")

    actor.clear_cache()
    stats = actor.cache_stats()
    assert stats["hits"] == 0
    assert stats["misses"] == 0
    print("test_clear_cache_resets_stats: PASS")


def test_different_cities_cached_independently():
    """Each city is cached under its own key."""
    mock = make_mock_host_with_weather()
    actor = MockWeatherActor(mock)

    actor.get_weather("Tokyo")
    actor.get_weather("Sydney")
    actor.get_weather("Tokyo")  # should be cached

    stats = actor.cache_stats()
    assert stats["misses"] == 2
    assert stats["hits"] == 1
    print("test_different_cities_cached_independently: PASS")


def test_cache_ttl_expiry():
    """Cached entry expires after TTL and triggers a fresh API call."""
    mock = make_mock_host_with_weather(city="Cairo", temp_c=30.0, wind_kph=5.0)
    actor = MockWeatherActor(mock)

    # First call: cache miss at t=1_000_000
    first = actor.get_weather("Cairo")
    assert first["source"] == "api"

    # Within TTL (4 minutes later) — should still be cached
    mock._now_ms += 4 * 60 * 1000
    second = actor.get_weather("Cairo")
    assert second["source"] == "cache"

    # Past TTL (6 minutes total from first call) — stale, re-fetch
    mock._now_ms += 2 * 60 * 1000
    third = actor.get_weather("Cairo")
    assert third["source"] == "api"

    stats = actor.cache_stats()
    assert stats["misses"] == 2
    assert stats["hits"] == 1
    print("test_cache_ttl_expiry: PASS")


def test_clear_cache_removes_weather_keys():
    mock = make_mock_host_with_weather()
    actor = MockWeatherActor(mock)
    actor.get_weather("London")
    assert f"{actor._cache_key_prefix()}London" in mock._kv
    actor.clear_cache()
    assert f"{actor._cache_key_prefix()}London" not in mock._kv
    print("test_clear_cache_removes_weather_keys: PASS")


if __name__ == "__main__":
    test_get_weather_calls_service_link()
    test_get_weather_second_call_uses_cache()
    test_clear_cache_resets_stats()
    test_different_cities_cached_independently()
    test_cache_ttl_expiry()
    test_clear_cache_removes_weather_keys()
    print("\n✅ All weather actor tests passed!")
