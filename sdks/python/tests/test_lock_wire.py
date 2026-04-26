import json
import importlib

host_module = importlib.import_module("plexspaces.host")
from plexspaces.proto_wire import _encode_length_delimited, _encode_varint, decode_lock_response


def _encode_timestamp(seconds: int, nanos: int = 0) -> bytes:
    payload = _encode_varint((1 << 3) | 0) + _encode_varint(seconds)
    if nanos:
        payload += _encode_varint((2 << 3) | 0) + _encode_varint(nanos)
    return payload


def _encode_map_entry(key: str, value: str) -> bytes:
    payload = _encode_length_delimited(1, key.encode("utf-8"))
    payload += _encode_length_delimited(2, value.encode("utf-8"))
    return payload


def _encode_lock_bytes() -> bytes:
    payload = _encode_length_delimited(1, b"kueue-allocator")
    payload += _encode_length_delimited(2, b"sched-1")
    payload += _encode_length_delimited(3, b"v123")
    payload += _encode_length_delimited(4, _encode_timestamp(1700000000, 123000000))
    payload += _encode_varint((5 << 3) | 0) + _encode_varint(10)
    payload += _encode_length_delimited(6, _encode_timestamp(1700000005))
    payload += _encode_length_delimited(7, _encode_map_entry("scope", "scheduler"))
    payload += _encode_varint((8 << 3) | 0) + _encode_varint(1)
    return payload


def test_decode_lock_response_returns_expected_fields():
    decoded = decode_lock_response(_encode_lock_bytes())

    assert decoded == {
        "lock_key": "kueue-allocator",
        "holder_id": "sched-1",
        "version": "v123",
        "expires_at_ms": 1700000000123,
        "lease_duration_secs": 10,
        "last_heartbeat_ms": 1700000005000,
        "metadata": {"scope": "scheduler"},
        "locked": True,
    }


class _FakeWitLockHost:
    def lock_acquire(self, *args):
        return _encode_lock_bytes()

    def lock_renew(self, *args):
        return _encode_lock_bytes()


def test_host_lock_helpers_return_json_from_wit_lock_payload():
    previous_impl = host_module._host_impl
    previous_attempted = host_module._host_init_attempted
    try:
        host_module._host_impl = _FakeWitLockHost()
        host_module._host_init_attempted = True

        acquired = host_module.host.lock_acquire(
            "default", "default", "sched-1", "kueue-allocator", 10, 5000
        )
        renewed = host_module.host.lock_renew(
            "kueue-allocator", "default", "default", "sched-1", "v123", 10
        )

        assert json.loads(acquired)["version"] == "v123"
        assert json.loads(acquired)["lock_key"] == "kueue-allocator"
        assert json.loads(renewed)["holder_id"] == "sched-1"
    finally:
        host_module._host_impl = previous_impl
        host_module._host_init_attempted = previous_attempted
