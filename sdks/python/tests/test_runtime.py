# SPDX-License-Identifier: AGPL-3.0-or-later
from plexspaces.runtime import build_class_map, select_actor_class


class SessionActor:
    pass


class ChannelActor:
    pass


class GuildActor:
    pass


class AbstractionsActor:
    pass


# ── build_class_map ──────────────────────────────────────────────────────────


def test_build_class_map_registers_exact_class_names():
    cm = build_class_map([SessionActor, ChannelActor, GuildActor])
    assert cm["SessionActor"] is SessionActor
    assert cm["ChannelActor"] is ChannelActor
    assert cm["GuildActor"] is GuildActor


def test_build_class_map_includes_snake_case():
    cm = build_class_map([SessionActor])
    # Class name + automatic snake_case alias for framework TOML convention
    assert set(cm.keys()) == {"SessionActor", "session_actor"}
    assert cm["session_actor"] is SessionActor


def test_build_class_map_actor_roles_override_for_same_class():
    # Same class, three role names — classic same-actor-type multi-instance case
    cm = build_class_map(
        [AbstractionsActor],
        actor_roles={"ephemeral": AbstractionsActor, "channel": AbstractionsActor},
    )
    assert cm["AbstractionsActor"] is AbstractionsActor
    assert cm["ephemeral"] is AbstractionsActor
    assert cm["channel"] is AbstractionsActor


def test_build_class_map_ignores_roles_for_unknown_classes():
    class UnrelatedActor:
        pass

    cm = build_class_map(
        [SessionActor],
        actor_roles={"some_role": UnrelatedActor},
    )
    assert "some_role" not in cm


# ── select_actor_class ───────────────────────────────────────────────────────


def _multi_map():
    return build_class_map([SessionActor, ChannelActor, GuildActor])


def test_select_dispatches_by_actor_type_exact():
    cm = _multi_map()
    assert (
        select_actor_class(
            {"actor_type": "ChannelActor", "actor_id": "guild-acme__general//ChannelActor::ns@node"},
            cm,
            SessionActor,
        )
        is ChannelActor
    )


def test_select_actor_type_wins_over_role():
    cm = _multi_map()
    # actor_type always wins when present — role is not consulted
    assert (
        select_actor_class(
            {"actor_type": "GuildActor", "role": "session_fallback"},
            cm,
            SessionActor,
        )
        is GuildActor
    )


def test_select_role_used_for_same_class_dispatch():
    cm = build_class_map(
        [AbstractionsActor],
        actor_roles={"ephemeral": AbstractionsActor, "channel": AbstractionsActor},
    )
    # actor_type matches AbstractionsActor directly
    assert (
        select_actor_class({"actor_type": "AbstractionsActor", "role": "ephemeral"}, cm, AbstractionsActor)
        is AbstractionsActor
    )
    # role only — no actor_type or actor_type not in map
    assert (
        select_actor_class({"role": "ephemeral"}, cm, AbstractionsActor)
        is AbstractionsActor
    )


def test_select_role_fallback_when_actor_type_absent():
    cm = build_class_map(
        [SessionActor, ChannelActor],
        actor_roles={"my_role": ChannelActor},
    )
    assert (
        select_actor_class({"role": "my_role"}, cm, SessionActor)
        is ChannelActor
    )


def test_select_returns_default_when_nothing_matches():
    cm = _multi_map()
    assert (
        select_actor_class(
            {"actor_type": "UnknownActor", "role": "unknown_role"},
            cm,
            SessionActor,
        )
        is SessionActor
    )


def test_select_empty_class_map_returns_default():
    assert select_actor_class({"actor_type": "SessionActor"}, {}, ChannelActor) is ChannelActor


def test_select_non_dict_config_returns_default():
    cm = _multi_map()
    assert select_actor_class(None, cm, SessionActor) is SessionActor  # type: ignore


def test_select_no_greedy_prefix_on_instance_name():
    # Previously broken: "guild-acme__general" actor_id prefix-matched "guild" -> GuildActor.
    # Now actor_id is never used for dispatch — only actor_type and role.
    cm = _multi_map()
    assert (
        select_actor_class(
            {
                "actor_type": "ChannelActor",
                "actor_id": "guild-acme__general//ChannelActor::ns@node",
            },
            cm,
            SessionActor,
        )
        is ChannelActor
    )
