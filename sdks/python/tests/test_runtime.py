from plexspaces.runtime import build_class_alias_map, normalize_role_actor_id, select_actor_class


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
            "leader//parameter_server_wasm::python-parameter-server@test-node-8091"
        )
        == "leader"
    )


class ToolRegistryActor:
    pass


class CalculatorToolActor:
    pass


def test_build_class_alias_map_prefers_actor_roles_for_role_names():
    alias_map = build_class_alias_map(
        [ToolRegistryActor, CalculatorToolActor],
        actor_roles={
            "tool_registry": ToolRegistryActor,
            "calculator_tool": CalculatorToolActor,
        },
    )

    assert alias_map["tool_registry"] is ToolRegistryActor
    assert alias_map["calculator_tool"] is CalculatorToolActor
    assert alias_map["tool-registry"] is ToolRegistryActor
    assert alias_map["calculator-tool"] is CalculatorToolActor


def test_build_class_alias_map_adds_trimmed_class_name_aliases():
    alias_map = build_class_alias_map([ToolRegistryActor])

    assert alias_map["tool_registry_actor"] is ToolRegistryActor
    assert alias_map["tool_registry"] is ToolRegistryActor
    assert alias_map["tool-registry"] is ToolRegistryActor


# ── select_actor_class dispatch tests ────────────────────────────────────────


class LeaderActor:
    pass


class WorkerActor:
    pass


def _two_actor_map():
    return build_class_alias_map(
        [LeaderActor, WorkerActor],
        actor_roles={"leader": LeaderActor, "worker": WorkerActor},
    )


def test_select_actor_class_dispatches_by_role():
    class_map = _two_actor_map()
    config = {"role": "worker", "actor_type": "shared_wasm", "actor_id": "leader//shared_wasm::ns@node"}
    assert select_actor_class(config, class_map, LeaderActor) is WorkerActor


def test_select_actor_class_role_takes_priority_over_actor_id():
    class_map = _two_actor_map()
    # role says "leader" but actor_id would resolve to "worker" — role wins.
    config = {"role": "leader", "actor_id": "worker//shared_wasm::ns@node"}
    assert select_actor_class(config, class_map, WorkerActor) is LeaderActor


def test_select_actor_class_falls_back_to_actor_id_when_no_role():
    class_map = _two_actor_map()
    config = {"actor_id": "worker//shared_wasm::ns@node", "actor_type": "shared_wasm"}
    assert select_actor_class(config, class_map, LeaderActor) is WorkerActor


def test_select_actor_class_falls_back_to_actor_type():
    class_map = build_class_alias_map([WorkerActor])
    config = {"actor_type": "worker_actor", "actor_id": "unknown//worker_actor::ns@node"}
    assert select_actor_class(config, class_map, LeaderActor) is WorkerActor


def test_select_actor_class_returns_default_when_nothing_matches():
    class_map = _two_actor_map()
    config = {"role": "unknown_role", "actor_id": "unknown//x::ns@node", "actor_type": "unknown_type"}
    assert select_actor_class(config, class_map, LeaderActor) is LeaderActor


def test_select_actor_class_role_with_dash_normalized():
    class_map = _two_actor_map()
    # Framework sends "role": "worker" — kebab/underscore normalisation must match.
    config = {"role": "worker"}
    assert select_actor_class(config, class_map, LeaderActor) is WorkerActor


def test_select_actor_class_empty_class_map_returns_default():
    config = {"role": "worker"}
    assert select_actor_class(config, {}, WorkerActor) is WorkerActor


def test_select_actor_class_non_dict_config_returns_default():
    class_map = _two_actor_map()
    assert select_actor_class(None, class_map, LeaderActor) is LeaderActor  # type: ignore
