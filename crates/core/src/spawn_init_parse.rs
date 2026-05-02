// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Parse **legacy JSON byte** spawn-init payloads into role hints and flat argument maps.
//!
//! [`crate::virtual_actor_manager::wasm_init_payload`] builds the canonical JSON shape for
//! WASM and BehaviorRegistry (`actor_id`, `actor_type`, `declaration_name`, `behavior_kind`,
//! nested `args`, plus promoted scalar fields). A few bridges still receive that JSON as raw
//! bytes (for example WASM host `initial_state` or older clients). This module extracts
//! **`role`** (from `role`, `declaration_name`, or `config.role`) and **`args`** so callers can
//! fill [`plexspaces_proto::actor::v1::ActorSpawnSpec::role`] and [`ActorSpawnSpec::args`].
//!
//! When the caller already has structured data, set `spec.args` (and `role`) directly—do not
//! round-trip through JSON.

use std::collections::HashMap;

/// Parses legacy JSON spawn-init **bytes** into an optional role/declaration hint and string args map.
///
/// Returns `(role_hint, args)` where `role_hint` is used for virtual-definition lookup when
/// present, and `args` merges explicit `"args"` object entries with top-level scalar fields
/// (excluding framework meta-keys).
pub fn legacy_spawn_init_json_to_role_and_args(
    initial_state: &[u8],
) -> (Option<String>, HashMap<String, String>) {
    let init_payload = serde_json::from_slice::<serde_json::Value>(initial_state).ok();
    let requested_role = init_payload.as_ref().and_then(|value| {
        value
            .get("role")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .or_else(|| {
                value
                    .get("declaration_name")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
            })
            .or_else(|| {
                value
                    .get("config")
                    .and_then(|cfg| cfg.get("role"))
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
            })
    });
    let args = init_payload
        .as_ref()
        .and_then(|value| {
            value
                .get("args")
                .and_then(|args| args.as_object())
                .map(|args| {
                    args.iter()
                        .map(|(k, v)| {
                            let s = match v {
                                serde_json::Value::String(s) => s.clone(),
                                _ => v.to_string(),
                            };
                            (k.clone(), s)
                        })
                        .collect()
                })
                .or_else(|| {
                    value.as_object().map(|obj| {
                        obj.iter()
                            .filter(|(k, _)| {
                                !matches!(
                                    k.as_str(),
                                    "actor_id"
                                        | "actor_type"
                                        | "declaration_name"
                                        | "behavior_kind"
                                        | "args"
                                        | "config"
                                ) && !k.starts_with("__")
                            })
                            .filter_map(|(k, v)| match v {
                                serde_json::Value::String(s) => Some((k.clone(), s.clone())),
                                serde_json::Value::Number(n) => Some((k.clone(), n.to_string())),
                                serde_json::Value::Bool(b) => Some((k.clone(), b.to_string())),
                                _ => None,
                            })
                            .collect()
                    })
                })
        })
        .unwrap_or_default();
    (requested_role, args)
}

#[cfg(test)]
mod tests {
    use super::legacy_spawn_init_json_to_role_and_args;

    #[test]
    fn empty_input_yields_empty_maps() {
        let (role, args) = legacy_spawn_init_json_to_role_and_args(&[]);
        assert!(role.is_none());
        assert!(args.is_empty());
    }

    #[test]
    fn parses_nested_args_and_role() {
        let json = br#"{"role":"worker","args":{"x":"1"}}"#;
        let (role, args) = legacy_spawn_init_json_to_role_and_args(json);
        assert_eq!(role.as_deref(), Some("worker"));
        assert_eq!(args.get("x").map(String::as_str), Some("1"));
    }

    #[test]
    fn declaration_name_fills_role_when_role_absent() {
        let json = br#"{"declaration_name":"ephemeral","args":{}}"#;
        let (role, _) = legacy_spawn_init_json_to_role_and_args(json);
        assert_eq!(role.as_deref(), Some("ephemeral"));
    }

    #[test]
    fn top_level_scalars_promoted_to_args() {
        let json = br#"{"initial_count":"7"}"#;
        let (_, args) = legacy_spawn_init_json_to_role_and_args(json);
        assert_eq!(args.get("initial_count").map(String::as_str), Some("7"));
    }
}
