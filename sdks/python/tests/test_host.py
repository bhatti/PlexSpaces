import importlib
import json

host_module = importlib.import_module("plexspaces.host")
host = host_module.host
pg_first = host_module.pg_first
ServiceHttpClient = host_module.ServiceHttpClient


def test_kv_list_normalizes_wasm_native_list():
    """WIT/component hosts may return list[str] for kv_list; Host must expose JSON str."""

    class ListReturningHost:
        def kv_list(self, prefix: str):
            return [f"{prefix}a", f"{prefix}b"]

    previous = host_module._host_impl
    host_module._host_impl = ListReturningHost()
    try:
        out = host.kv_list("pre/")
        assert json.loads(out) == ["pre/a", "pre/b"]
    finally:
        host_module._host_impl = previous


def test_http_fetch_returns_response_dict():
    resp = host.http_fetch("test-link", "GET", "/v1/items")
    assert resp["status"] == 200
    assert isinstance(resp["headers"], dict)
    assert "body" in resp


def test_http_fetch_post_with_body():
    import json
    resp = host.http_fetch("test-link", "POST", "/v1/items", {"Content-Type": "application/json"}, '{"name":"test"}')
    assert resp["status"] == 200


def test_service_http_client_get():
    client = ServiceHttpClient("test-api")
    resp = client.get("/v1/items")
    assert resp["status"] == 200


def test_service_http_client_post():
    client = ServiceHttpClient("test-api")
    resp = client.post("/v1/items", {"name": "test"})
    assert resp["status"] == 200


def test_service_http_client_put():
    client = ServiceHttpClient("test-api")
    resp = client.put("/v1/items/1", {"name": "updated"})
    assert resp["status"] == 200


def test_service_http_client_delete():
    client = ServiceHttpClient("test-api")
    resp = client.delete("/v1/items/1")
    assert resp["status"] == 200


def test_create_shard_group_uses_host_wrapper_shape():
    response = host.create_shard_group(
        {
            "group_id": "group-a",
            "actor_type": "worker",
            "shard_count": 2,
        }
    )
    assert response["group_id"] == "mock-group"
    assert response["actor_type"] == "worker"


def test_application_get_status_returns_dict():
    response = host.application_get_status("app-a", "node-a")
    assert response["node_id"] == "node-a"
    assert response["application"]["application_id"] == "app-a"


# ========================================================================
# ProcessGroups.first / first_or_raise
# ========================================================================

class _StubPGHost:
    def __init__(self, groups=None):
        self._groups = groups or {}
        self._kv = {}
        self._metrics = []

    def pg_join(self, group):
        self._groups.setdefault(group, []).append("test@node")
        return ""

    def pg_members(self, group):
        return json.dumps(self._groups.get(group, []))

    def pg_leave(self, group):
        self._groups.pop(group, None)
        return ""

    def pg_broadcast(self, group, msg_type, payload):
        return ""

    def kv_get(self, key):
        return self._kv.get(key, b"")

    def kv_put(self, key, value):
        if isinstance(value, (bytes, bytearray)):
            value = value.decode("utf-8")
        self._kv[key] = value
        return b""

    def application_metrics_add(self, application_id, metrics_bytes):
        import json
        if isinstance(metrics_bytes, (bytes, bytearray)):
            self._metrics.append(json.loads(metrics_bytes.decode("utf-8")))
        else:
            self._metrics.append(json.loads(metrics_bytes))
        return json.dumps({}).encode()

    def log(self, level, message):
        pass

    def now_ms(self):
        return 0


def _install_stub(stub):
    prev = (
        host_module._host_impl,
        host_module._host_init_attempted,
        host_module._host_is_wit,
    )
    host_module._host_impl = stub
    host_module._host_init_attempted = True
    host_module._host_is_wit = False
    return prev


def _restore_stub(prev):
    host_module._host_impl, host_module._host_init_attempted, host_module._host_is_wit = prev


def test_process_groups_first_returns_member():
    stub = _StubPGHost({"svc:test": ["actor1@node", "actor2@node"]})
    prev = _install_stub(stub)
    try:
        result = host.process_groups.first("svc:test")
        assert result == "actor1@node"
    finally:
        _restore_stub(prev)


def test_process_groups_first_returns_none_when_empty():
    stub = _StubPGHost()
    prev = _install_stub(stub)
    try:
        result = host.process_groups.first("svc:empty")
        assert result is None
    finally:
        _restore_stub(prev)


def test_process_groups_first_or_raise_returns_member():
    stub = _StubPGHost({"svc:llm": ["llm@node"]})
    prev = _install_stub(stub)
    try:
        result = host.process_groups.first_or_raise("svc:llm")
        assert result == "llm@node"
    finally:
        _restore_stub(prev)


def test_process_groups_first_or_raise_raises_when_empty():
    stub = _StubPGHost()
    prev = _install_stub(stub)
    try:
        try:
            host.process_groups.first_or_raise("svc:missing")
            assert False, "expected RuntimeError"
        except RuntimeError as e:
            assert "svc:missing" in str(e)
    finally:
        _restore_stub(prev)


def test_top_level_pg_first_returns_member():
    stub = _StubPGHost({"svc:test": ["actor1@node", "actor2@node"]})
    prev = _install_stub(stub)
    try:
        result = pg_first("svc:test")
        assert result == "actor1@node"
    finally:
        _restore_stub(prev)


def test_top_level_pg_first_returns_none_when_empty():
    stub = _StubPGHost()
    prev = _install_stub(stub)
    try:
        result = pg_first("svc:empty")
        assert result is None
    finally:
        _restore_stub(prev)


# ========================================================================
# Host.kv_get_json / kv_put_json
# ========================================================================

def test_kv_put_json_and_get_json_round_trip():
    stub = _StubPGHost()
    prev = _install_stub(stub)
    try:
        data = {"seq": 42, "task_type": "summarize"}
        host.kv_put_json("test:task:42", data)
        result = host.kv_get_json("test:task:42")
        assert result == data
    finally:
        _restore_stub(prev)


def test_kv_get_json_missing_key_returns_none():
    stub = _StubPGHost()
    prev = _install_stub(stub)
    try:
        result = host.kv_get_json("nonexistent:key")
        assert result is None
    finally:
        _restore_stub(prev)


def test_kv_get_json_corrupt_data_returns_none():
    stub = _StubPGHost()
    stub._kv["bad:json"] = "not-json{"
    prev = _install_stub(stub)
    try:
        result = host.kv_get_json("bad:json")
        assert result is None
    finally:
        _restore_stub(prev)


def test_kv_put_json_raises_on_unmarshalable_value():
    stub = _StubPGHost()
    prev = _install_stub(stub)
    try:
        try:
            host.kv_put_json("bad:val", object())  # bare object() has no JSON representation
            assert False, "expected ValueError or TypeError"
        except (ValueError, TypeError):
            pass
    finally:
        _restore_stub(prev)


# ========================================================================
# Host.incr_counter / incr_counters
# ========================================================================

def test_incr_counter_records_metric():
    stub = _StubPGHost()
    prev = _install_stub(stub)
    try:
        host.incr_counter("myapp", "my_op")
        assert len(stub._metrics) == 1
        assert stub._metrics[0]["counter_metrics"]["my_op"] == 1
    finally:
        _restore_stub(prev)


def test_incr_counters_multiple():
    stub = _StubPGHost()
    prev = _install_stub(stub)
    try:
        host.incr_counters("myapp", {"cache_hits": 5, "cache_misses": 2})
        assert len(stub._metrics) == 1
        m = stub._metrics[0]
        assert m["message_count"] == 2
        assert m["counter_metrics"]["cache_hits"] == 5
        assert m["counter_metrics"]["cache_misses"] == 2
    finally:
        _restore_stub(prev)


def test_incr_counter_swallows_errors():
    class _FailingHost(_StubPGHost):
        def application_metrics_add(self, application_id, metrics_bytes):
            raise RuntimeError("metrics unavailable")

    stub = _FailingHost()
    prev = _install_stub(stub)
    try:
        host.incr_counter("myapp", "op")  # must not raise
    finally:
        _restore_stub(prev)


# ========================================================================
# EventLog
# ========================================================================

def test_event_log_append_and_poll():
    from plexspaces.host import EventLog
    stub = _StubPGHost()
    prev = _install_stub(stub)
    try:
        log = EventLog()
        seq = log.append(host, "audit:", {"action": "login"})
        assert seq == 1
        seq = log.append(host, "audit:", {"action": "logout"})
        assert seq == 2

        events, cursor = log.poll(host, "audit:", "consumer-1", limit=10)
        assert len(events) == 2
        assert cursor == 2
    finally:
        _restore_stub(prev)


def test_event_log_poll_idempotent():
    from plexspaces.host import EventLog
    stub = _StubPGHost()
    prev = _install_stub(stub)
    try:
        log = EventLog()
        log.append(host, "ev:", {"x": 1})

        events, cursor = log.poll(host, "ev:", "c1", limit=10)
        assert len(events) == 1 and cursor == 1

        events2, cursor2 = log.poll(host, "ev:", "c1", limit=10)
        assert len(events2) == 0
        assert cursor2 == 1
    finally:
        _restore_stub(prev)


def test_event_log_two_independent_consumers():
    from plexspaces.host import EventLog
    stub = _StubPGHost()
    prev = _install_stub(stub)
    try:
        log = EventLog()
        for i in range(3):
            log.append(host, "ev:", {"i": i})

        ev_a, cur_a = log.poll(host, "ev:", "consumer-A", limit=10)
        ev_b, cur_b = log.poll(host, "ev:", "consumer-B", limit=2)
        assert len(ev_a) == 3 and cur_a == 3
        assert len(ev_b) == 2 and cur_b == 2
    finally:
        _restore_stub(prev)


def test_event_log_append_rolls_back_on_error():
    from plexspaces.host import EventLog

    class _FailKvPut(_StubPGHost):
        def kv_put(self, key, value):
            return "ERROR: disk full"

    stub = _FailKvPut()
    prev = _install_stub(stub)
    try:
        log = EventLog()
        try:
            log.append(host, "ev:", {"x": 1})
            assert False, "expected error"
        except RuntimeError:
            pass
        assert log.watermark == 0
    finally:
        _restore_stub(prev)


# ── Channel tests ─────────────────────────────────────────────────────────────

def _make_channel_host():
    """Return a fresh _MockHost installed as the active host impl."""
    mock = host_module._MockHost()
    prev = _install_stub(mock)
    return mock, prev


def test_channel_send_receive():
    mock, prev = _make_channel_host()
    try:
        msg_id = host.channel.send("", "tasks:work", "process", {"doc": "d1"})
        assert msg_id, "expected non-empty message ID"

        msg, ok, err = host.channel.receive("", "tasks:work", timeout_ms=0)
        assert ok, "expected message, got empty"
        assert err is None
        assert msg["id"] == msg_id
        assert msg["delivery_count"] == 1
    finally:
        _restore_stub(prev)


def test_channel_receive_empty_returns_not_ok():
    mock, prev = _make_channel_host()
    try:
        msg, ok, err = host.channel.receive("", "empty:channel", timeout_ms=0)
        assert not ok
        assert msg is None
        assert err is None
    finally:
        _restore_stub(prev)


def test_channel_ack_tracked():
    mock, prev = _make_channel_host()
    try:
        msg_id = host.channel.send("", "q", "x", None)
        msg, _, _ = host.channel.receive("", "q", 0)
        host.channel.ack("", "q", msg["id"])
        assert mock._channel_acked.get(msg_id), f"expected {msg_id} acked"
    finally:
        _restore_stub(prev)


def test_channel_nack_tracked():
    mock, prev = _make_channel_host()
    try:
        msg_id = host.channel.send("", "q", "x", None)
        msg, _, _ = host.channel.receive("", "q", 0)
        host.channel.nack("", "q", msg["id"], requeue=False)
        assert mock._channel_nacked.get(msg_id), f"expected {msg_id} nacked"
    finally:
        _restore_stub(prev)


def test_channel_subscribe_unique_ids():
    mock, prev = _make_channel_host()
    try:
        id1 = host.channel.subscribe("", "events:a", "")
        id2 = host.channel.subscribe("", "events:b", "")
        assert id1 != id2, f"expected unique subscription IDs, got {id1} and {id2}"
        assert mock._channel_subs[id1] == "events:a"
        assert mock._channel_subs[id2] == "events:b"
    finally:
        _restore_stub(prev)


def test_channel_unsubscribe_removes_entry():
    mock, prev = _make_channel_host()
    try:
        sub_id = host.channel.subscribe("", "events:login", "")
        assert sub_id in mock._channel_subs
        host.channel.unsubscribe(sub_id)
        assert sub_id not in mock._channel_subs
    finally:
        _restore_stub(prev)


def test_channel_publish():
    mock, prev = _make_channel_host()
    try:
        msg_id = host.channel.publish("", "events:login", "user_login", {"user": "alice"})
        assert msg_id, "expected non-empty message ID"
        assert len(mock._channel_topics.get("events:login", [])) == 1
    finally:
        _restore_stub(prev)


def test_channel_depth():
    mock, prev = _make_channel_host()
    try:
        host.channel.send("", "tasks:depth", "t", None)
        host.channel.send("", "tasks:depth", "t", None)
        assert host.channel.depth("", "tasks:depth") == 2
    finally:
        _restore_stub(prev)


def test_channel_depth_after_receive():
    mock, prev = _make_channel_host()
    try:
        host.channel.send("", "tasks:dr", "t", None)
        host.channel.send("", "tasks:dr", "t", None)
        host.channel.receive("", "tasks:dr", 0)
        assert host.channel.depth("", "tasks:dr") == 1
    finally:
        _restore_stub(prev)


def test_channel_create_delete():
    mock, prev = _make_channel_host()
    try:
        host.channel.create("", "managed:q", max_size=100, message_ttl_ms=60000)
        host.channel.send("", "managed:q", "x", None)
        assert host.channel.depth("", "managed:q") == 1
        host.channel.delete("", "managed:q")
        assert host.channel.depth("", "managed:q") == 0
    finally:
        _restore_stub(prev)
