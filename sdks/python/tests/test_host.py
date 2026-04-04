from plexspaces.host import host, ServiceHttpClient


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
