# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2025 PlexSpaces Contributors
#
# Lightweight protobuf wire encoding/decoding for WASM host payloads.
# Matches:
#   - sdks/go/plexspaces/tuplespace_proto_wire.go
#   - sdks/go/plexspaces/host_actor_api_wire_wasm.go
#   - sdks/go/plexspaces/application_metrics_proto_wire.go
#   - sdks/typescript/src/wire/tuplespace-proto-wire.ts
#   - crates/wasm-runtime simple_component_host (prost)
#
# No external dependencies — works inside componentize-py WASM guests.

"""
Minimal protobuf wire encoding for PlexSpaces WASM host functions.

The WASM host functions (ts-write, ts-read, etc.) expect protobuf-encoded
bytes matching the proto definitions in proto/plexspaces/v1/tuplespace/.
This module encodes/decodes those messages using raw wire format so that
the Python SDK can call host functions without depending on betterproto
or protobuf libraries at runtime.
"""

import json
import struct
from typing import Any, Dict, List, Optional, Tuple


# ---------------------------------------------------------------------------
# Protobuf wire primitives
# ---------------------------------------------------------------------------

def _encode_varint(value: int) -> bytes:
    """Encode a non-negative integer as a protobuf varint."""
    if value < 0:
        # Signed varint (two's complement, 10 bytes)
        value = value & 0xFFFFFFFFFFFFFFFF
    parts = []
    while value > 0x7F:
        parts.append((value & 0x7F) | 0x80)
        value >>= 7
    parts.append(value & 0x7F)
    return bytes(parts)


def _decode_varint(data: bytes, pos: int) -> Tuple[int, int]:
    """Decode a varint from data at pos. Returns (value, bytes_consumed)."""
    result = 0
    shift = 0
    start = pos
    for _ in range(10):
        if pos >= len(data):
            raise ValueError("varint buffer underflow")
        b = data[pos]
        pos += 1
        result |= (b & 0x7F) << shift
        if b < 0x80:
            return result, pos - start
        shift += 7
    raise ValueError("varint too long")


def _encode_length_delimited(field_num: int, inner: bytes) -> bytes:
    """Encode a length-delimited protobuf field (wire type 2)."""
    tag = _encode_varint((field_num << 3) | 2)
    length = _encode_varint(len(inner))
    return tag + length + inner


def _skip_field(data: bytes, pos: int, wire_type: int) -> int:
    """Skip an unknown field in the wire data."""
    if wire_type == 0:  # varint
        _, n = _decode_varint(data, pos)
        return pos + n
    elif wire_type == 1:  # fixed64
        return pos + 8
    elif wire_type == 2:  # length-delimited
        length, n = _decode_varint(data, pos)
        return pos + n + length
    elif wire_type == 5:  # fixed32
        return pos + 4
    else:
        raise ValueError(f"unknown wire type {wire_type}")


# ---------------------------------------------------------------------------
# TupleField encoding (matches proto TupleField oneof)
# ---------------------------------------------------------------------------
# TupleField proto layout:
#   integer = 1 (int64, varint)
#   float   = 2 (double, fixed64)
#   string  = 3 (string, length-delimited)
#   boolean = 4 (bool, varint)
#   binary  = 5 (bytes, length-delimited)
#   null    = 6 (bool, varint)
#   wildcard= 7 (bool, varint)

def _encode_tuple_field(value: Any, allow_wildcard: bool = False) -> bytes:
    """Encode a single value as a TupleField protobuf message body."""
    if value is None:
        # null field (field 6, bool=true)
        return b'\x30' + _encode_varint(1)  # tag=6<<3|0=48=0x30

    if allow_wildcard and value == "*":
        # wildcard field (field 7, bool=true)
        return b'\x38' + _encode_varint(1)  # tag=7<<3|0=56=0x38

    if isinstance(value, bool):
        # boolean field (field 4)
        return b'\x20' + _encode_varint(1 if value else 0)  # tag=4<<3|0=32=0x20

    if isinstance(value, int) and not isinstance(value, bool):
        # integer field (field 1, int64 varint)
        return b'\x08' + _encode_varint(value)  # tag=1<<3|0=8=0x08

    if isinstance(value, float):
        if value == int(value) and -2**63 <= int(value) <= 2**63 - 1:
            # Integer-valued float → encode as int64
            return b'\x08' + _encode_varint(int(value))
        # double field (field 2, fixed64 LE)
        return b'\x11' + struct.pack('<d', value)  # tag=2<<3|1=17=0x11

    if isinstance(value, str):
        # string field (field 3, length-delimited)
        encoded = value.encode('utf-8')
        return b'\x1a' + _encode_varint(len(encoded)) + encoded  # tag=3<<3|2=26=0x1a

    if isinstance(value, (bytes, bytearray)):
        # binary field (field 5, length-delimited)
        return b'\x2a' + _encode_varint(len(value)) + bytes(value)  # tag=5<<3|2=42=0x2a

    raise ValueError(f"unsupported tuple field type: {type(value).__name__}")


def _encode_tuple_fields(values: list, allow_wildcard: bool = False) -> bytes:
    """Encode a list of values as repeated TupleField (Tuple.fields = field 2)."""
    out = b''
    for v in values:
        tf = _encode_tuple_field(v, allow_wildcard)
        out += _encode_length_delimited(2, tf)  # Tuple.fields = field 2
    return out


# ---------------------------------------------------------------------------
# WriteRequest / ReadRequest / ReadResponse encoding
# ---------------------------------------------------------------------------

def encode_write_request(tuple_values: list) -> bytes:
    """
    Encode a list of values as a tuplespace WriteRequest protobuf.

    WriteRequest { repeated Tuple tuples = 1; }
    Tuple { repeated TupleField fields = 2; }
    """
    tuple_body = _encode_tuple_fields(tuple_values, allow_wildcard=False)
    return _encode_length_delimited(1, tuple_body)  # WriteRequest.tuples = field 1


def encode_read_request(
    pattern: list,
    take: bool = False,
    max_results: int = 100,
) -> bytes:
    """
    Encode a pattern as a tuplespace ReadRequest protobuf.

    ReadRequest {
        Tuple template = 1;
        bool take = 4;
        int32 max_results = 5;
    }

    Pattern values: use None or "*" for wildcards.
    """
    template_body = _encode_tuple_fields(pattern, allow_wildcard=True)
    out = _encode_length_delimited(1, template_body)  # ReadRequest.template = field 1
    if take:
        out += b'\x20\x01'  # field 4 (take) = true: tag=4<<3|0=32=0x20, value=1
    out += b'\x28' + _encode_varint(max_results)  # field 5 (max_results): tag=5<<3|0=40=0x28
    return out


# ---------------------------------------------------------------------------
# ReadResponse decoding
# ---------------------------------------------------------------------------

def _decode_tuple_field_msg(data: bytes) -> Any:
    """Decode a TupleField message body into a Python value."""
    pos = 0
    last = None
    while pos < len(data):
        tag_val, n = _decode_varint(data, pos)
        pos += n
        field_num = tag_val >> 3
        wire_type = tag_val & 7

        if wire_type == 0:  # varint
            val, n = _decode_varint(data, pos)
            pos += n
            if field_num == 1:  # integer
                # Decode as signed int64
                if val > 0x7FFFFFFFFFFFFFFF:
                    val -= 0x10000000000000000
                last = val
            elif field_num == 4:  # boolean
                last = val != 0
            elif field_num in (6, 7):  # null or wildcard
                last = None
        elif wire_type == 1:  # fixed64 (double)
            if pos + 8 > len(data):
                raise ValueError("double underflow")
            last = struct.unpack('<d', data[pos:pos + 8])[0]
            pos += 8
        elif wire_type == 2:  # length-delimited
            length, n = _decode_varint(data, pos)
            pos += n
            chunk = data[pos:pos + length]
            pos += length
            if field_num in (3, 5):  # string or binary
                last = chunk.decode('utf-8', errors='replace')
        else:
            pos = _skip_field(data, pos, wire_type)
    return last


def _decode_tuple_msg(data: bytes) -> list:
    """Decode a Tuple message body into a list of field values."""
    fields = []
    pos = 0
    while pos < len(data):
        tag_val, n = _decode_varint(data, pos)
        pos += n
        field_num = tag_val >> 3
        wire_type = tag_val & 7
        if field_num == 2 and wire_type == 2:  # Tuple.fields
            length, n = _decode_varint(data, pos)
            pos += n
            sub = data[pos:pos + length]
            pos += length
            fields.append(_decode_tuple_field_msg(sub))
        else:
            pos = _skip_field(data, pos, wire_type)
    return fields


def decode_read_response(data: bytes) -> List[list]:
    """
    Decode a ReadResponse protobuf into a list of tuples.

    ReadResponse { repeated Tuple tuples = 1; }
    """
    if not data:
        return []
    tuples = []
    pos = 0
    while pos < len(data):
        tag_val, n = _decode_varint(data, pos)
        pos += n
        field_num = tag_val >> 3
        wire_type = tag_val & 7
        if field_num == 1 and wire_type == 2:  # ReadResponse.tuples
            length, n = _decode_varint(data, pos)
            pos += n
            sub = data[pos:pos + length]
            pos += length
            tuples.append(_decode_tuple_msg(sub))
        else:
            pos = _skip_field(data, pos, wire_type)
    return tuples


def decode_read_response_first(data: bytes) -> Optional[list]:
    """Decode ReadResponse and return the first tuple, or None."""
    tuples = decode_read_response(data)
    return tuples[0] if tuples else None


def decode_read_response_all(data: bytes) -> List[list]:
    """Decode ReadResponse and return all tuples."""
    return decode_read_response(data)


# ---------------------------------------------------------------------------
# Shared wire encoding helpers for shard group / application messages
# (Mirrors sdks/go/plexspaces/host_actor_api_wire_wasm.go)
# ---------------------------------------------------------------------------

def _append_string_field(buf: bytes, field_num: int, s: str) -> bytes:
    if not s:
        return buf
    return buf + _encode_length_delimited(field_num, s.encode('utf-8'))


def _append_bytes_field(buf: bytes, field_num: int, b: bytes) -> bytes:
    if not b:
        return buf
    return buf + _encode_length_delimited(field_num, b)


def _append_varint_field(buf: bytes, field_num: int, v: int) -> bytes:
    """Encode a uint32/uint64/enum varint field. Skips if v == 0."""
    if v == 0:
        return buf
    return buf + _encode_varint((field_num << 3) | 0) + _encode_varint(v)


def _append_uint64_map(buf: bytes, field_num: int, m: Dict[str, int]) -> bytes:
    """Encode a map<string, uint64> proto field."""
    for k, v in m.items():
        entry = _encode_length_delimited(1, k.encode('utf-8'))
        entry += _encode_varint((2 << 3) | 0) + _encode_varint(int(v))
        buf += _encode_length_delimited(field_num, entry)
    return buf


def _append_string_map(buf: bytes, field_num: int, m: Dict[str, str]) -> bytes:
    """Encode a map<string, string> proto field."""
    for k, v in m.items():
        entry = _encode_length_delimited(1, k.encode('utf-8'))
        entry += _encode_length_delimited(2, str(v).encode('utf-8'))
        buf += _encode_length_delimited(field_num, entry)
    return buf


def encode_http_fetch_request(headers: Dict[str, str], body: bytes) -> bytes:
    """
    Encode plexspaces.wasm.v1.HttpFetchRequest for actor-world host.http-fetch.

    Link name, method, and path are WIT parameters; only headers and body are protobuf.
    """
    buf = _append_string_map(b"", 1, dict(headers))
    return buf + _encode_length_delimited(2, body)


def _decode_string_map_entry(entry: bytes) -> Tuple[str, str]:
    """Decode one protobuf map<string,string> entry message."""
    key = ""
    value = ""
    pos = 0
    while pos < len(entry):
        tag_val, n = _decode_varint(entry, pos)
        pos += n
        fn = tag_val >> 3
        wt = tag_val & 7
        if fn == 1 and wt == 2:
            chunk, pos = _read_length_delimited(entry, pos)
            key = chunk.decode("utf-8", errors="replace")
        elif fn == 2 and wt == 2:
            chunk, pos = _read_length_delimited(entry, pos)
            value = chunk.decode("utf-8", errors="replace")
        else:
            pos = _skip_field(entry, pos, wt)
    return key, value


def decode_http_fetch_response(data: bytes) -> Tuple[int, Dict[str, str], bytes]:
    """
    Decode plexspaces.wasm.v1.HttpFetchResponse from host.http-fetch result bytes.

    Returns:
        (status, headers, body_bytes)
    """
    status = 0
    headers: Dict[str, str] = {}
    body = b""
    pos = 0
    while pos < len(data):
        tag_val, n = _decode_varint(data, pos)
        pos += n
        fn = tag_val >> 3
        wt = tag_val & 7
        if fn == 1 and wt == 0:
            val, m = _decode_varint(data, pos)
            pos += m
            status = val
        elif fn == 2 and wt == 2:
            chunk, pos = _read_length_delimited(data, pos)
            ek, ev = _decode_string_map_entry(chunk)
            headers[ek] = ev
        elif fn == 3 and wt == 2:
            chunk, pos = _read_length_delimited(data, pos)
            body = chunk
        else:
            pos = _skip_field(data, pos, wt)
    return status, headers, body


def encode_http_fetch_response(status: int, headers: Dict[str, str], body: bytes) -> bytes:
    """Encode HttpFetchResponse (for tests and tooling)."""
    out = b""
    if status:
        out += _encode_varint((1 << 3) | 0) + _encode_varint(int(status))
    out = _append_string_map(out, 2, dict(headers))
    return out + _encode_length_delimited(3, body)


def _encode_duration_from_ms(ms: int) -> bytes:
    """Encode google.protobuf.Duration from milliseconds."""
    if not ms:
        return b''
    sec = ms // 1000
    nanos = (ms % 1000) * 1_000_000
    d = b''
    if sec:
        d += _encode_varint((1 << 3) | 0) + _encode_varint(sec)
    if nanos:
        d += _encode_varint((2 << 3) | 0) + _encode_varint(nanos)
    return d


def _encode_common_message(message_type: str, payload: bytes) -> bytes:
    """Encode a plexspaces.common.v1.Message (message_type=field5, payload=field6)."""
    m = b''
    if message_type:
        m += _encode_length_delimited(5, message_type.encode('utf-8'))
    if payload:
        m += _encode_length_delimited(6, payload)
    return m


def _partition_enum(s: str) -> int:
    s = s.lower().strip()
    if s in ('hash', 'partition_strategy_hash'):
        return 1
    if s == 'range':
        return 2
    if s in ('consistent_hash', 'consistent-hash'):
        return 3
    if s == 'custom':
        return 99
    return 0


def _rebalance_enum(s: str) -> int:
    s = s.lower().strip()
    if s in ('none', 'manual'):
        return 1
    if s in ('on_scale', 'on-scale'):
        return 2
    if s in ('load_based', 'load-based'):
        return 3
    return 0


def _node_placement_enum(s: str) -> int:
    s = s.lower().strip()
    if s in ('same_node', 'same-node'):
        return 1
    if s in ('from_registry', 'from-registry'):
        return 2
    if s in ('node_ids', 'node-ids'):
        return 3
    return 0


def _aggregation_enum(s: str) -> int:
    s = s.lower().strip()
    if s == 'concat':
        return 1
    if s == 'merge':
        return 2
    if s == 'first':
        return 3
    if s == 'majority':
        return 4
    return 0


def _reduction_enum(s: str) -> int:
    s = s.lower().strip()
    if s == 'sum':
        return 1
    if s == 'min':
        return 2
    if s == 'max':
        return 3
    if s == 'product':
        return 4
    if s == 'concat':
        return 5
    if s in ('bool_and', 'bool-and'):
        return 6
    if s in ('bool_or', 'bool-or'):
        return 7
    return 0


def _encode_node_placement(v: Any) -> bytes:
    """Encode NodePlacement proto message."""
    if not isinstance(v, dict) or not v:
        return b''
    p = b''
    strategy = _node_placement_enum(str(v.get('strategy', '')))
    p = _append_varint_field(p, 1, strategy)
    p = _append_string_field(p, 2, str(v.get('cluster', '')))
    for nid in v.get('node_ids', []):
        p = _append_string_field(p, 3, str(nid))
    rl = v.get('required_labels', {})
    if isinstance(rl, dict) and rl:
        p = _append_string_map(p, 4, {k: str(val) for k, val in rl.items()})
    for nid in v.get('avoid_node_ids', []):
        p = _append_string_field(p, 5, str(nid))
    al = v.get('affinity_labels', {})
    if isinstance(al, dict) and al:
        p = _append_string_map(p, 7, {k: str(val) for k, val in al.items()})
    return p


def _encode_data_parallel_config(
    group_id: str, shard_count: int, part: int, reb: int, placement: Any
) -> bytes:
    """Encode DataParallelConfig proto message."""
    c = b''
    c = _append_string_field(c, 1, group_id)
    c = _append_varint_field(c, 2, shard_count)
    c = _append_varint_field(c, 4, part)
    c = _append_varint_field(c, 5, reb)
    pb = _encode_node_placement(placement)
    if pb:
        c += _encode_length_delimited(6, pb)
    return c


def _cfg_get(m: Dict[str, Any], key: str) -> Any:
    """Get key from dict, falling back to m['config'][key]."""
    if key in m:
        return m[key]
    cfg = m.get('config', {})
    if isinstance(cfg, dict):
        return cfg.get(key)
    return None


def _to_int(v: Any, default: int = 0) -> int:
    if v is None:
        return default
    try:
        return int(v)
    except (TypeError, ValueError):
        return default


# ---------------------------------------------------------------------------
# Shard group request encoders
# ---------------------------------------------------------------------------

def encode_create_shard_group_request(request: Dict[str, Any]) -> bytes:
    """
    Encode CreateShardGroupRequest proto.

    CreateShardGroupRequest {
        DataParallelConfig config = 1;
        string actor_type = 2;
        ActorConfig shard_config = 3;  (not supported)
        bytes initial_state = 4;
        map<string, string> metadata = 5;
    }
    """
    m = request
    group_id = str(_cfg_get(m, 'group_id') or '')
    shard_count = _to_int(_cfg_get(m, 'shard_count'))
    part = _partition_enum(str(_cfg_get(m, 'partition_strategy') or ''))
    reb = _rebalance_enum(str(_cfg_get(m, 'rebalance_policy') or ''))
    cfg = _encode_data_parallel_config(group_id, shard_count, part, reb, _cfg_get(m, 'placement'))
    out = _encode_length_delimited(1, cfg)
    out = _append_string_field(out, 2, str(m.get('actor_type', '')))
    # initial_state (field 4, bytes)
    st = m.get('initial_state')
    if st is not None:
        if isinstance(st, (bytes, bytearray)):
            out += _encode_length_delimited(4, bytes(st))
        elif isinstance(st, str) and st:
            out += _encode_length_delimited(4, st.encode('utf-8'))
        elif isinstance(st, dict):
            out += _encode_length_delimited(4, json.dumps(st).encode('utf-8'))
    # metadata (field 5, map<string, string>)
    meta = m.get('metadata', {})
    if isinstance(meta, dict) and meta:
        out = _append_string_map(out, 5, {k: str(v) for k, v in meta.items()})
    return out


def encode_scatter_gather_request(request: Dict[str, Any]) -> bytes:
    """
    Encode ScatterGatherRequest proto.

    ScatterGatherRequest {
        string group_id = 1;
        Message query = 2;
        Duration timeout = 3;
        ShardGroupAggregationStrategy aggregation = 4;
        uint32 min_responses = 5;
    }
    """
    m = request
    mt = str(m.get('message_type', ''))
    query = m.get('query', {})
    if isinstance(query, dict):
        if not mt:
            mt = str(query.get('op', ''))
        payload = json.dumps(query).encode('utf-8')
    else:
        payload = b'{}'
    qm = _encode_common_message(mt, payload)
    req = b''
    req = _append_string_field(req, 1, str(m.get('group_id', '')))
    req += _encode_length_delimited(2, qm)
    d = _encode_duration_from_ms(_to_int(m.get('timeout_ms')))
    if d:
        req += _encode_length_delimited(3, d)
    req = _append_varint_field(req, 4, _aggregation_enum(str(m.get('aggregation', ''))))
    req = _append_varint_field(req, 5, _to_int(m.get('min_responses')))
    return req


def encode_broadcast_shard_group_request(request: Dict[str, Any]) -> bytes:
    """
    Encode BroadcastShardGroupRequest proto.

    BroadcastShardGroupRequest {
        string group_id = 1;
        Message message = 2;
        Duration timeout = 3;
        uint32 min_acks = 4;
    }
    """
    m = request
    body = m.get('message', {})
    if not isinstance(body, dict):
        body = {}
    mt = str(m.get('message_type', '') or body.get('op', ''))
    payload = json.dumps(body).encode('utf-8')
    msg = _encode_common_message(mt, payload)
    req = b''
    req = _append_string_field(req, 1, str(m.get('group_id', '')))
    req += _encode_length_delimited(2, msg)
    d = _encode_duration_from_ms(_to_int(m.get('timeout_ms')))
    if d:
        req += _encode_length_delimited(3, d)
    req = _append_varint_field(req, 4, _to_int(m.get('min_acks')))
    return req


def _encode_reduce_like_request(request: Dict[str, Any]) -> bytes:
    """
    Shared encoder for ReduceShardGroupRequest and AllReduceShardGroupRequest.

    Both have the same wire layout:
        string group_id = 1;
        Message map_function = 2;
        Duration timeout = 3;
        uint32 min_responses = 4;
        CollectiveReduction reduction = 5;
        CollectiveTargetField target = 6;
    """
    m = request
    mt = str(m.get('message_type', ''))
    body = m.get('map_function', {})
    if not isinstance(body, dict):
        body = {}
    if not mt:
        mt = str(body.get('op', ''))
    payload = json.dumps(body).encode('utf-8')
    mf = _encode_common_message(mt, payload)
    req = b''
    req = _append_string_field(req, 1, str(m.get('group_id', '')))
    req += _encode_length_delimited(2, mf)
    d = _encode_duration_from_ms(_to_int(m.get('timeout_ms')))
    if d:
        req += _encode_length_delimited(3, d)
    req = _append_varint_field(req, 4, _to_int(m.get('min_responses')))
    req = _append_varint_field(req, 5, _reduction_enum(str(m.get('reduction', ''))))
    target = str(m.get('target', ''))
    if target:
        tf = _encode_length_delimited(1, target.encode('utf-8'))
        req += _encode_length_delimited(6, tf)
    return req


def encode_reduce_shard_group_request(request: Dict[str, Any]) -> bytes:
    """Encode ReduceShardGroupRequest proto."""
    return _encode_reduce_like_request(request)


def encode_all_reduce_shard_group_request(request: Dict[str, Any]) -> bytes:
    """Encode AllReduceShardGroupRequest proto."""
    return _encode_reduce_like_request(request)


def encode_map_shard_group_request(request: Dict[str, Any]) -> bytes:
    """
    Encode MapShardGroupRequest proto.

    MapShardGroupRequest {
        string group_id = 1;
        Message map_function = 2;
        Duration timeout = 3;
        uint32 min_responses = 4;
    }
    """
    m = request
    mt = str(m.get('message_type', ''))
    body = m.get('map_function', {})
    if not isinstance(body, dict):
        body = {}
    if not mt:
        mt = str(body.get('op', ''))
    payload = json.dumps(body).encode('utf-8')
    mf = _encode_common_message(mt, payload)
    req = b''
    req = _append_string_field(req, 1, str(m.get('group_id', '')))
    req += _encode_length_delimited(2, mf)
    d = _encode_duration_from_ms(_to_int(m.get('timeout_ms')))
    if d:
        req += _encode_length_delimited(3, d)
    req = _append_varint_field(req, 4, _to_int(m.get('min_responses')))
    return req


def encode_barrier_shard_group_request(request: Dict[str, Any]) -> bytes:
    """
    Encode BarrierShardGroupRequest proto.

    BarrierShardGroupRequest {
        string group_id = 1;
        string barrier_id = 2;
        uint64 round = 3;
        Duration timeout = 4;
        uint32 min_acks = 5;
    }
    """
    m = request
    req = b''
    req = _append_string_field(req, 1, str(m.get('group_id', '')))
    req = _append_string_field(req, 2, str(m.get('barrier_id', '')))
    req = _append_varint_field(req, 3, _to_int(m.get('round')))
    d = _encode_duration_from_ms(_to_int(m.get('timeout_ms')))
    if d:
        req += _encode_length_delimited(4, d)
    req = _append_varint_field(req, 5, _to_int(m.get('min_acks')))
    return req


def encode_application_metrics(metrics: Dict[str, Any]) -> bytes:
    """
    Encode ApplicationMetrics proto.

    ApplicationMetrics {
        map<string, uint64> actor_counts = 1;
        uint32 supervisor_count = 2;
        uint64 uptime_seconds = 3;
        uint64 message_count = 4;
        uint64 error_count = 5;
        map<string, uint64> counter_metrics = 6;
        map<string, uint64> latency_totals_ms = 7;
        map<string, uint64> latency_max_ms = 8;
        map<string, uint64> latency_samples = 9;
    }
    """
    def _u64_map(m: Dict[str, Any]) -> Dict[str, int]:
        return {k: int(v) for k, v in m.items()} if isinstance(m, dict) else {}

    buf = b''
    buf = _append_uint64_map(buf, 1, _u64_map(metrics.get('actor_counts', {})))
    buf = _append_varint_field(buf, 2, _to_int(metrics.get('supervisor_count')))
    buf = _append_varint_field(buf, 3, _to_int(metrics.get('uptime_seconds')))
    buf = _append_varint_field(buf, 4, _to_int(metrics.get('message_count')))
    buf = _append_varint_field(buf, 5, _to_int(metrics.get('error_count')))
    buf = _append_uint64_map(buf, 6, _u64_map(metrics.get('counter_metrics', {})))
    buf = _append_uint64_map(buf, 7, _u64_map(metrics.get('latency_totals_ms', {})))
    buf = _append_uint64_map(buf, 8, _u64_map(metrics.get('latency_max_ms', {})))
    buf = _append_uint64_map(buf, 9, _u64_map(metrics.get('latency_samples', {})))
    return buf


# ---------------------------------------------------------------------------
# Shard group response decoders
# ---------------------------------------------------------------------------

def _read_length_delimited(data: bytes, pos: int) -> Tuple[bytes, int]:
    """Read a length-delimited field. Returns (chunk, new_pos)."""
    length, n = _decode_varint(data, pos)
    pos += n
    end = pos + length
    if end > len(data):
        raise ValueError("length-delimited underflow")
    return data[pos:end], end


def _parse_duration_ms(data: bytes) -> int:
    """Decode google.protobuf.Duration to milliseconds."""
    pos = 0
    sec = 0
    nanos = 0
    while pos < len(data):
        tag_val, n = _decode_varint(data, pos)
        pos += n
        fn = tag_val >> 3
        wt = tag_val & 7
        if wt == 0:
            val, m = _decode_varint(data, pos)
            pos += m
            if fn == 1:
                sec = val
            elif fn == 2:
                nanos = val
        else:
            pos = _skip_field(data, pos, wt)
    return sec * 1000 + nanos // 1_000_000


def _parse_timestamp_ms(data: bytes) -> int:
    """Decode google.protobuf.Timestamp to Unix epoch milliseconds."""
    pos = 0
    sec = 0
    nanos = 0
    while pos < len(data):
        tag_val, n = _decode_varint(data, pos)
        pos += n
        fn = tag_val >> 3
        wt = tag_val & 7
        if wt == 0:
            val, m = _decode_varint(data, pos)
            pos += m
            if fn == 1:
                sec = val
            elif fn == 2:
                nanos = val
        else:
            pos = _skip_field(data, pos, wt)
    return sec * 1000 + nanos // 1_000_000


def _parse_string_map_entry(data: bytes) -> Tuple[str, str]:
    """Decode a single protobuf map<string, string> entry."""
    pos = 0
    key = ""
    value = ""
    while pos < len(data):
        tag_val, n = _decode_varint(data, pos)
        pos += n
        fn = tag_val >> 3
        wt = tag_val & 7
        if wt != 2:
            pos = _skip_field(data, pos, wt)
            continue
        chunk, pos = _read_length_delimited(data, pos)
        decoded = chunk.decode("utf-8", errors="replace")
        if fn == 1:
            key = decoded
        elif fn == 2:
            value = decoded
    return key, value


def decode_lock_response(data: bytes) -> Dict[str, Any]:
    """
    Decode a plexspaces.locks.prv.Lock protobuf into the JSON-friendly shape
    expected by Python lock helpers and examples.
    """
    if not data:
        return {}
    result: Dict[str, Any] = {
        "lock_key": "",
        "holder_id": "",
        "version": "",
        "expires_at_ms": 0,
        "lease_duration_secs": 0,
        "last_heartbeat_ms": 0,
        "metadata": {},
        "locked": False,
    }
    pos = 0
    while pos < len(data):
        tag_val, n = _decode_varint(data, pos)
        pos += n
        fn = tag_val >> 3
        wt = tag_val & 7
        if fn in (1, 2, 3) and wt == 2:
            chunk, pos = _read_length_delimited(data, pos)
            value = chunk.decode("utf-8", errors="replace")
            if fn == 1:
                result["lock_key"] = value
            elif fn == 2:
                result["holder_id"] = value
            else:
                result["version"] = value
        elif fn in (4, 6) and wt == 2:
            chunk, pos = _read_length_delimited(data, pos)
            ts_ms = _parse_timestamp_ms(chunk)
            if fn == 4:
                result["expires_at_ms"] = ts_ms
            else:
                result["last_heartbeat_ms"] = ts_ms
        elif fn == 5 and wt == 0:
            value, n = _decode_varint(data, pos)
            pos += n
            result["lease_duration_secs"] = value
        elif fn == 7 and wt == 2:
            chunk, pos = _read_length_delimited(data, pos)
            key, value = _parse_string_map_entry(chunk)
            result["metadata"][key] = value
        elif fn == 8 and wt == 0:
            value, n = _decode_varint(data, pos)
            pos += n
            result["locked"] = value != 0
        else:
            pos = _skip_field(data, pos, wt)
    return result


def _parse_common_message(data: bytes) -> Tuple[str, bytes]:
    """Decode a plexspaces.common.v1.Message, returning (message_type, payload)."""
    pos = 0
    msg_type = ''
    payload = b''
    while pos < len(data):
        tag_val, n = _decode_varint(data, pos)
        pos += n
        fn = tag_val >> 3
        wt = tag_val & 7
        if fn == 5 and wt == 2:
            chunk, pos = _read_length_delimited(data, pos)
            msg_type = chunk.decode('utf-8', errors='replace')
        elif fn == 6 and wt == 2:
            chunk, pos = _read_length_delimited(data, pos)
            payload = chunk
        else:
            pos = _skip_field(data, pos, wt)
    return msg_type, payload


def _payload_to_any(payload: bytes) -> Any:
    if not payload:
        return None
    try:
        return json.loads(payload.decode('utf-8'))
    except Exception:
        return payload.decode('utf-8', errors='replace')


def _parse_scatter_gather_stats(data: Optional[bytes]) -> Dict[str, Any]:
    out: Dict[str, Any] = {'shards_queried': 0, 'shards_responded': 0, 'shards_failed': 0}
    if not data:
        return out
    pos = 0
    while pos < len(data):
        tag_val, n = _decode_varint(data, pos)
        pos += n
        fn = tag_val >> 3
        wt = tag_val & 7
        if fn == 4 and wt == 2:
            chunk, pos = _read_length_delimited(data, pos)
            out['max_latency'] = {'ms': _parse_duration_ms(chunk)}
        elif wt == 0:
            val, m = _decode_varint(data, pos)
            pos += m
            if fn == 1:
                out['shards_queried'] = val
            elif fn == 2:
                out['shards_responded'] = val
            elif fn == 3:
                out['shards_failed'] = val
        else:
            pos = _skip_field(data, pos, wt)
    return out


def _parse_shard_query_response(data: bytes) -> Dict[str, Any]:
    """Decode a ShardQueryResponse proto message."""
    out: Dict[str, Any] = {}
    pos = 0
    while pos < len(data):
        tag_val, n = _decode_varint(data, pos)
        pos += n
        fn = tag_val >> 3
        wt = tag_val & 7
        if fn == 1 and wt == 0:
            val, m = _decode_varint(data, pos)
            pos += m
            out['shard_id'] = val
        elif fn == 2 and wt == 2:
            chunk, pos = _read_length_delimited(data, pos)
            out['shard_actor_id'] = chunk.decode('utf-8', errors='replace')
        elif fn == 3 and wt == 2:
            chunk, pos = _read_length_delimited(data, pos)
            _, pl = _parse_common_message(chunk)
            p = _payload_to_any(pl)
            out['payload'] = p
            out['response'] = p
        elif fn == 4 and wt == 2:
            chunk, pos = _read_length_delimited(data, pos)
            out['latency_ms'] = _parse_duration_ms(chunk)
        elif fn == 5 and wt == 0:
            val, m = _decode_varint(data, pos)
            pos += m
            out['success'] = val != 0
        elif fn == 6 and wt == 2:
            chunk, pos = _read_length_delimited(data, pos)
            out['error'] = chunk.decode('utf-8', errors='replace')
        else:
            pos = _skip_field(data, pos, wt)
    return out


def _parse_data_parallel_config(data: bytes) -> Dict[str, Any]:
    out: Dict[str, Any] = {}
    pos = 0
    partition_map = {1: 'PARTITION_STRATEGY_HASH', 2: 'PARTITION_STRATEGY_RANGE',
                     3: 'PARTITION_STRATEGY_CONSISTENT_HASH', 99: 'PARTITION_STRATEGY_CUSTOM'}
    rebalance_map = {1: 'REBALANCE_POLICY_NONE', 2: 'REBALANCE_POLICY_ON_SCALE',
                     3: 'REBALANCE_POLICY_LOAD_BASED'}
    while pos < len(data):
        tag_val, n = _decode_varint(data, pos)
        pos += n
        fn = tag_val >> 3
        wt = tag_val & 7
        if fn == 1 and wt == 2:
            chunk, pos = _read_length_delimited(data, pos)
            out['group_id'] = chunk.decode('utf-8', errors='replace')
        elif fn == 2 and wt == 0:
            val, m = _decode_varint(data, pos)
            pos += m
            out['shard_count'] = val
        elif fn == 4 and wt == 0:
            val, m = _decode_varint(data, pos)
            pos += m
            out['partition_strategy'] = partition_map.get(val, 'PARTITION_STRATEGY_UNSPECIFIED')
        elif fn == 5 and wt == 0:
            val, m = _decode_varint(data, pos)
            pos += m
            out['rebalance_policy'] = rebalance_map.get(val, 'REBALANCE_POLICY_UNSPECIFIED')
        else:
            pos = _skip_field(data, pos, wt)
    return out


def _parse_shard_group(data: bytes) -> Dict[str, Any]:
    """Decode a ShardGroup proto message."""
    out: Dict[str, Any] = {'metadata': {}, 'rebalance_status': None}
    shard_ids: List[str] = []
    pos = 0
    while pos < len(data):
        tag_val, n = _decode_varint(data, pos)
        pos += n
        fn = tag_val >> 3
        wt = tag_val & 7
        if fn == 1 and wt == 2:
            chunk, pos = _read_length_delimited(data, pos)
            cfg = _parse_data_parallel_config(chunk)
            out.update(cfg)
        elif fn == 2 and wt == 2:
            chunk, pos = _read_length_delimited(data, pos)
            out['actor_type'] = chunk.decode('utf-8', errors='replace')
        elif fn == 3 and wt == 2:
            chunk, pos = _read_length_delimited(data, pos)
            shard_ids.append(chunk.decode('utf-8', errors='replace'))
        else:
            pos = _skip_field(data, pos, wt)
    out['shard_actor_ids'] = shard_ids
    return out


def decode_create_shard_group_response(data: bytes) -> Dict[str, Any]:
    """
    Decode CreateShardGroupResponse proto.

    CreateShardGroupResponse { ShardGroup group = 1; }
    Returns a dict with shard_actor_ids, group_id, shard_count, etc.
    """
    if not data:
        return {}
    pos = 0
    while pos < len(data):
        tag_val, n = _decode_varint(data, pos)
        pos += n
        fn = tag_val >> 3
        wt = tag_val & 7
        if fn == 1 and wt == 2:
            chunk, _ = _read_length_delimited(data, pos)
            return _parse_shard_group(chunk)
        pos = _skip_field(data, pos, wt)
    return {}


def decode_scatter_gather_response(data: bytes) -> Dict[str, Any]:
    """
    Decode ScatterGatherResponse proto.

    ScatterGatherResponse {
        Message result = 1;
        repeated ShardQueryResponse shard_responses = 2;
        ScatterGatherStats stats = 3;
    }
    """
    out: Dict[str, Any] = {'shard_responses': []}
    if not data:
        out['stats'] = _parse_scatter_gather_stats(None)
        return out
    shards = []
    pos = 0
    while pos < len(data):
        tag_val, n = _decode_varint(data, pos)
        pos += n
        fn = tag_val >> 3
        wt = tag_val & 7
        if fn == 1 and wt == 2:
            chunk, pos = _read_length_delimited(data, pos)
            _, pl = _parse_common_message(chunk)
            out['result'] = _payload_to_any(pl)
        elif fn == 2 and wt == 2:
            chunk, pos = _read_length_delimited(data, pos)
            shards.append(_parse_shard_query_response(chunk))
        elif fn == 3 and wt == 2:
            chunk, pos = _read_length_delimited(data, pos)
            out['stats'] = _parse_scatter_gather_stats(chunk)
        else:
            pos = _skip_field(data, pos, wt)
    out['shard_responses'] = shards
    if 'stats' not in out:
        out['stats'] = _parse_scatter_gather_stats(None)
    return out


def _decode_shards_and_stats_response(data: bytes, result_field: bool = False) -> Dict[str, Any]:
    """
    Shared decoder for broadcast/barrier (shards=field1, stats=field2)
    and reduce/all-reduce (result=field1, shards=field2, stats=field3).
    """
    out: Dict[str, Any] = {}
    shards = []
    if not data:
        out['shard_responses'] = shards
        out['stats'] = _parse_scatter_gather_stats(None)
        return out
    pos = 0
    while pos < len(data):
        tag_val, n = _decode_varint(data, pos)
        pos += n
        fn = tag_val >> 3
        wt = tag_val & 7
        if result_field and fn == 1 and wt == 2:
            chunk, pos = _read_length_delimited(data, pos)
            _, pl = _parse_common_message(chunk)
            out['result'] = _payload_to_any(pl)
        elif fn == (2 if result_field else 1) and wt == 2:
            chunk, pos = _read_length_delimited(data, pos)
            shards.append(_parse_shard_query_response(chunk))
        elif fn == (3 if result_field else 2) and wt == 2:
            chunk, pos = _read_length_delimited(data, pos)
            out['stats'] = _parse_scatter_gather_stats(chunk)
        else:
            pos = _skip_field(data, pos, wt)
    out['shard_responses'] = shards
    if 'stats' not in out:
        out['stats'] = _parse_scatter_gather_stats(None)
    return out


def decode_broadcast_shard_group_response(data: bytes) -> Dict[str, Any]:
    """Decode BroadcastShardGroupResponse proto (shards=field1, stats=field2)."""
    return _decode_shards_and_stats_response(data, result_field=False)


def decode_barrier_shard_group_response(data: bytes) -> Dict[str, Any]:
    """Decode BarrierShardGroupResponse proto (shards=field1, stats=field2)."""
    return _decode_shards_and_stats_response(data, result_field=False)


def decode_reduce_shard_group_response(data: bytes) -> Dict[str, Any]:
    """Decode ReduceShardGroupResponse proto (result=field1, shards=field2, stats=field3)."""
    return _decode_shards_and_stats_response(data, result_field=True)


def decode_all_reduce_shard_group_response(data: bytes) -> Dict[str, Any]:
    """Decode AllReduceShardGroupResponse proto (result=field1, shards=field2, stats=field3)."""
    return _decode_shards_and_stats_response(data, result_field=True)


def decode_map_shard_group_response(data: bytes) -> Dict[str, Any]:
    """
    Decode MapShardGroupResponse proto.

    MapShardGroupResponse {
        repeated ShardQueryResponse shard_results = 1;
        ScatterGatherStats stats = 2;
    }
    """
    out: Dict[str, Any] = {}
    shards = []
    if not data:
        out['shard_results'] = shards
        out['stats'] = _parse_scatter_gather_stats(None)
        return out
    pos = 0
    while pos < len(data):
        tag_val, n = _decode_varint(data, pos)
        pos += n
        fn = tag_val >> 3
        wt = tag_val & 7
        if fn == 1 and wt == 2:
            chunk, pos = _read_length_delimited(data, pos)
            shards.append(_parse_shard_query_response(chunk))
        elif fn == 2 and wt == 2:
            chunk, pos = _read_length_delimited(data, pos)
            out['stats'] = _parse_scatter_gather_stats(chunk)
        else:
            pos = _skip_field(data, pos, wt)
    out['shard_results'] = shards
    if 'stats' not in out:
        out['stats'] = _parse_scatter_gather_stats(None)
    return out


# ---------------------------------------------------------------------------
# ApplicationMetrics response decoder
# ---------------------------------------------------------------------------

def _parse_u64_map_entry(data: bytes) -> Tuple[str, int]:
    """Decode a map<string, uint64> entry."""
    key = ''
    val = 0
    pos = 0
    while pos < len(data):
        tag_val, n = _decode_varint(data, pos)
        pos += n
        fn = tag_val >> 3
        wt = tag_val & 7
        if fn == 1 and wt == 2:
            chunk, pos = _read_length_delimited(data, pos)
            key = chunk.decode('utf-8', errors='replace')
        elif fn == 2 and wt == 0:
            val, m = _decode_varint(data, pos)
            pos += m
        else:
            pos = _skip_field(data, pos, wt)
    return key, val


def _merge_u64_map_field(out: Dict[str, Any], field: str, entry: bytes) -> None:
    k, v = _parse_u64_map_entry(entry)
    if not k:
        return
    acc = out.setdefault(field, {})
    acc[k] = v


def decode_application_metrics_response(data: bytes) -> Dict[str, Any]:
    """
    Decode ApplicationMetrics proto response.

    ApplicationMetrics {
        map<string, uint64> actor_counts = 1;
        uint32 supervisor_count = 2;
        uint64 uptime_seconds = 3;
        uint64 message_count = 4;
        uint64 error_count = 5;
        map<string, uint64> counter_metrics = 6;
        map<string, uint64> latency_totals_ms = 7;
        map<string, uint64> latency_max_ms = 8;
        map<string, uint64> latency_samples = 9;
    }
    """
    out: Dict[str, Any] = {}
    if not data:
        return out
    pos = 0
    map_fields = {1: 'actor_counts', 6: 'counter_metrics', 7: 'latency_totals_ms',
                  8: 'latency_max_ms', 9: 'latency_samples'}
    scalar_fields = {2: 'supervisor_count', 3: 'uptime_seconds', 4: 'message_count',
                     5: 'error_count'}
    while pos < len(data):
        tag_val, n = _decode_varint(data, pos)
        pos += n
        fn = tag_val >> 3
        wt = tag_val & 7
        if fn in map_fields and wt == 2:
            chunk, pos = _read_length_delimited(data, pos)
            _merge_u64_map_field(out, map_fields[fn], chunk)
        elif fn in scalar_fields and wt == 0:
            val, m = _decode_varint(data, pos)
            pos += m
            out[scalar_fields[fn]] = val
        else:
            pos = _skip_field(data, pos, wt)
    return out


def _parse_application_info(data: bytes) -> Dict[str, Any]:
    """Decode ApplicationInfo proto embedded in GetApplicationStatusResponse."""
    out: Dict[str, Any] = {}
    status_map = {0: 'APPLICATION_STATUS_UNSPECIFIED', 1: 'APPLICATION_STATUS_LOADING',
                  2: 'APPLICATION_STATUS_STARTING', 3: 'APPLICATION_STATUS_RUNNING',
                  4: 'APPLICATION_STATUS_STOPPING', 5: 'APPLICATION_STATUS_STOPPED',
                  6: 'APPLICATION_STATUS_FAILED'}
    pos = 0
    while pos < len(data):
        tag_val, n = _decode_varint(data, pos)
        pos += n
        fn = tag_val >> 3
        wt = tag_val & 7
        if fn == 1 and wt == 2:
            chunk, pos = _read_length_delimited(data, pos)
            out['application_id'] = chunk.decode('utf-8', errors='replace')
        elif fn == 2 and wt == 2:
            chunk, pos = _read_length_delimited(data, pos)
            out['name'] = chunk.decode('utf-8', errors='replace')
        elif fn == 3 and wt == 2:
            chunk, pos = _read_length_delimited(data, pos)
            out['version'] = chunk.decode('utf-8', errors='replace')
        elif fn == 4 and wt == 0:
            val, m = _decode_varint(data, pos)
            pos += m
            out['status'] = status_map.get(val, f'APPLICATION_STATUS_{val}')
        elif fn == 6 and wt == 2:
            chunk, pos = _read_length_delimited(data, pos)
            out['metrics'] = decode_application_metrics_response(chunk)
        else:
            pos = _skip_field(data, pos, wt)
    return out


def decode_application_get_status_response(data: bytes) -> Dict[str, Any]:
    """
    Decode GetApplicationStatusResponse proto.

    GetApplicationStatusResponse {
        ApplicationInfo application = 1;
        string error = 3;
        string node_id = 4;
        string node_address = 5;
    }
    """
    out: Dict[str, Any] = {}
    if not data:
        return out
    pos = 0
    while pos < len(data):
        tag_val, n = _decode_varint(data, pos)
        pos += n
        fn = tag_val >> 3
        wt = tag_val & 7
        if fn == 1 and wt == 2:
            chunk, pos = _read_length_delimited(data, pos)
            out['application'] = _parse_application_info(chunk)
        elif fn == 3 and wt == 2:
            chunk, pos = _read_length_delimited(data, pos)
            out['error'] = chunk.decode('utf-8', errors='replace')
        elif fn == 4 and wt == 2:
            chunk, pos = _read_length_delimited(data, pos)
            out['node_id'] = chunk.decode('utf-8', errors='replace')
        elif fn == 5 and wt == 2:
            chunk, pos = _read_length_delimited(data, pos)
            out['node_address'] = chunk.decode('utf-8', errors='replace')
        else:
            pos = _skip_field(data, pos, wt)
    return out
