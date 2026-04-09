# SPDX-License-Identifier: LGPL-2.1-or-later
"""Round-trip tests for wasm host HttpFetchRequest/HttpFetchResponse proto wire."""

from plexspaces.proto_wire import (
    decode_http_fetch_response,
    encode_http_fetch_request,
    encode_http_fetch_response,
)


def test_http_fetch_request_encodes_map_and_body():
    wire = encode_http_fetch_request({"X-Test": "1"}, b"payload")
    assert b"payload" in wire
    assert wire  # non-empty


def test_http_fetch_response_roundtrip():
    raw = encode_http_fetch_response(
        200,
        {"Content-Type": "application/json"},
        b'{"current":{"temperature_2m":12}}',
    )
    status, headers, body = decode_http_fetch_response(raw)
    assert status == 200
    assert headers["Content-Type"] == "application/json"
    assert body == b'{"current":{"temperature_2m":12}}'


def test_http_fetch_request_empty_body():
    wire = encode_http_fetch_request({}, b"")
    assert isinstance(wire, bytes)
