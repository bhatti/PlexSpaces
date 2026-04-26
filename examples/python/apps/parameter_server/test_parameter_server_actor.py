from parameter_server_actor import (
    ACTOR_ROLES,
    Leader,
    Worker,
    apply_metrics_delta,
    actor_application_id,
    actor_node_id,
    compute_actor_counts,
    metrics_map,
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


def test_metrics_map_filters_non_numeric_values():
    metrics = {"counter_metrics": {"worker_messages": 4, "skip": "x", "samples_processed": 12.0}}
    assert metrics_map(metrics, "counter_metrics") == {
        "worker_messages": 4,
        "samples_processed": 12,
    }


def test_apply_metrics_delta_uses_application_metrics_snapshots():
    node_metrics = {}
    role_metrics = {}
    start_metrics = {
        "message_count": 3,
        "error_count": 0,
        "counter_metrics": {"worker_messages": 2, "gradient_operation_count": 1, "samples_processed": 256},
        "latency_totals_ms": {"worker": 10, "worker.compute": 9, "worker.coordination": 1},
        "latency_max_ms": {"worker": 10},
        "latency_samples": {"worker": 1},
    }
    end_metrics = {
        "message_count": 9,
        "error_count": 1,
        "counter_metrics": {"worker_messages": 6, "gradient_operation_count": 4, "samples_processed": 1024},
        "latency_totals_ms": {"worker": 55, "worker.compute": 48, "worker.coordination": 7},
        "latency_max_ms": {"worker": 25},
        "latency_samples": {"worker": 4},
    }

    apply_metrics_delta(node_metrics, role_metrics, start_metrics, end_metrics, "test-node-8093")

    assert node_metrics["test-node-8093"]["messages"] == 6
    assert node_metrics["test-node-8093"]["worker_messages"] == 4
    assert node_metrics["test-node-8093"]["gradient_operations"] == 3
    assert node_metrics["test-node-8093"]["samples_processed"] == 768
    assert node_metrics["test-node-8093"]["compute_time_ms"] == 39
    assert node_metrics["test-node-8093"]["coordination_time_ms"] == 6
    assert node_metrics["test-node-8093"]["responses"] == 3
    assert node_metrics["test-node-8093"]["errors"] == 1
    assert role_metrics["worker"]["messages"] == 4
    assert role_metrics["worker"]["gradient_operations"] == 3
    assert role_metrics["worker"]["samples_processed"] == 768
