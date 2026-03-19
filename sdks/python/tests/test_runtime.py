from plexspaces.runtime import normalize_role_actor_id


def test_normalize_role_actor_id_bare_child_id():
    assert normalize_role_actor_id("worker-0") == "worker-0"


def test_normalize_role_actor_id_child_form():
    assert (
        normalize_role_actor_id("worker-0:python-parameter-server@test-node-8091")
        == "worker-0"
    )


def test_normalize_role_actor_id_canonical_form():
    assert (
        normalize_role_actor_id(
            "01KM1FJ3AF3BA2BDNKZPGH9K2P//leader::python-parameter-server@test-node-8091"
        )
        == "leader"
    )
