from parameter_server_actor import (
    ACTOR_ROLES,
    Leader,
    Worker,
    actor_application_id,
    actor_node_id,
    compute_actor_counts,
    normalize_worker_payload,
    worker_seed,
)


def test_worker_seed_is_stable_for_same_actor_id():
    actor_id = "01KM1FJ3AF3BA2BDNKZPGH9K2P//worker::python-parameter-server@test-node-8091"
    assert worker_seed(actor_id) == worker_seed(actor_id)


def test_worker_compute_gradient_returns_expected_shape():
    worker = Worker()
    worker.worker_id = "01KM1FJ3AF3BA2BDNKZPGH9K2P//worker::python-parameter-server@test-node-8091"
    worker.batch_size = 8
    reply = worker.compute_gradient(
        weights={
            "w1": [[0.1, 0.2], [0.3, 0.4]],
            "w2": [0.5, 0.6],
        },
        input_dim=2,
        hidden_dim=2,
    )
    assert reply["status"] == "ok"
    assert reply["samples"] == 8
    assert len(reply["gradients"]["d_w1"]) == 2
    assert len(reply["gradients"]["d_w1"][0]) == 2
    assert len(reply["gradients"]["d_w2"]) == 2


def test_leader_init_applies_numeric_config():
    leader = Leader()
    leader.on_init(
        {
            "actor_id": "01KM1FJ3AF3BA2BDNKZPGH9K2P//leader::python-parameter-server@test-node-8091",
            "args": {
                "learning_rate": "0.05",
                "input_dim": "4",
                "hidden_dim": "3",
                "num_workers": "6",
            },
        }
    )
    assert leader.learning_rate == 0.05
    assert leader.input_dim == 4
    assert leader.hidden_dim == 3
    assert leader.num_workers == 6


def test_actor_roles_match_supervisor_child_types():
    assert ACTOR_ROLES["leader"] is Leader
    assert ACTOR_ROLES["worker"] is Worker


def test_actor_id_helpers_extract_namespace_and_node():
    actor_id = "01KM1FJ3AF3BA2BDNKZPGH9K2P//worker::python-parameter-server@test-node-8093"
    assert actor_application_id(actor_id) == "python-parameter-server"
    assert actor_node_id(actor_id) == "test-node-8093"


def test_normalize_worker_payload_unwraps_nested_payload():
    payload = normalize_worker_payload(
        {"payload": {"response": {"status": "ok", "latency_ms": 12, "samples_processed": 8}}}
    )
    assert payload["status"] == "ok"
    assert payload["latency_ms"] == 12
    assert payload["samples_processed"] == 8


def test_compute_actor_counts_assigns_leader_and_workers_to_nodes():
    counts = compute_actor_counts(
        "test-node-8091",
        [
            "01A//worker::python-parameter-server@test-node-8091",
            "01B//worker::python-parameter-server@test-node-8093",
        ],
    )
    assert counts["test-node-8091"]["leader_actors"] == 1
    assert counts["test-node-8091"]["worker_actors"] == 1
    assert counts["test-node-8093"]["worker_actors"] == 1
