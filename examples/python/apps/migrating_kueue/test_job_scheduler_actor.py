import importlib.util
import json
from pathlib import Path


def _load_module():
    path = Path(__file__).with_name("job_scheduler_actor.py")
    spec = importlib.util.spec_from_file_location(path.stem, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class FakeHost:
    def __init__(self):
        self.kv = {}
        self.now = 1000

    def kv_get(self, key):
        return self.kv.get(key, "")

    def kv_put(self, key, value):
        self.kv[key] = value
        return ""

    def now_ms(self):
        self.now += 10
        return self.now

    def self_id(self):
        return "sched-1//migrating_kueue_wasm::migrating-kueue-scheduler-py@test-node-8091"

    def lock_acquire(self, tenant, namespace, holder, name, ttl_seconds, timeout_ms):
        return json.dumps({"lock_key": name, "version": "v1"})

    def lock_release(self, lock_id, tenant, namespace, holder, version):
        return ""

    def log(self, level, message):
        return None


def test_allocate_then_complete_uses_persisted_job_state():
    module = _load_module()
    module.host = FakeHost()

    actor = module.JobSchedulerActor()
    actor.on_init({})
    actor.submit("j1", 5, 2)
    actor.submit("j2", 10, 1)
    actor.submit("j3", 3, 4)

    allocated = actor.allocate()
    assert allocated["ok"] is True
    assert allocated["job_id"] == "j2"

    restored = module.JobSchedulerActor()
    restored.on_init({})
    completed = restored.complete("j2")
    assert completed == {"ok": True, "job_id": "j2"}

    quotas = restored.get_quotas()
    assert quotas["processed_count"] == 1
    assert quotas["used_gpus"] == 0
    assert quotas["peak_used_gpus"] == 1
    assert quotas["total_compute_ms"] == 10.0
    assert quotas["total_coord_ms"] == 5.0

    queue = restored.list_queue()
    assert queue["allocated_count"] == 0
    assert queue["pending_count"] == 2
