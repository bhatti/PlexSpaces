from plexspaces.host import host


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
