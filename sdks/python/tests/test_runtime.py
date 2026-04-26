from plexspaces.runtime import build_class_alias_map, normalize_role_actor_id


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


def test_build_class_alias_map_prefers_actor_roles_for_declaration_names():
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
