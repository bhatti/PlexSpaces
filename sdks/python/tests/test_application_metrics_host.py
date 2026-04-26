import importlib
import json

host_module = importlib.import_module("plexspaces.host")


class _FakeMetricsHost:
    def application_get_metrics(self, application_id, node_id):
        return json.dumps(
            {
                "message_count": 7,
                "error_count": 1,
                "counter_metrics": {"worker_messages": 5},
            }
        )


def test_application_get_metrics_returns_json_dict_from_host():
    previous_impl = host_module._host_impl
    previous_attempted = host_module._host_init_attempted
    previous_wit = host_module._host_is_wit
    try:
        host_module._host_impl = _FakeMetricsHost()
        host_module._host_init_attempted = True
        host_module._host_is_wit = False

        metrics = host_module.host.application_get_metrics(
            "python-parameter-server", "test-node-8093"
        )

        assert metrics["message_count"] == 7
        assert metrics["error_count"] == 1
        assert metrics["counter_metrics"]["worker_messages"] == 5
    finally:
        host_module._host_impl = previous_impl
        host_module._host_init_attempted = previous_attempted
        host_module._host_is_wit = previous_wit
