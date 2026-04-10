// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is free software: you can redistribute it and/or modify
// it under the terms of the GNU Lesser General Public License as published by
// the Free Software Foundation, either version 2.1 of the License, or
// (at your option) any later version.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! Structured actor identity.
//!
//! `ActorId` is constructed from validated fields and owns the canonical
//! string form `{name}//{actor_type}::{namespace}@{node_id}`.
//! Parsing is limited to canonical deserialization boundaries.

use plexspaces_proto::common::v1::ActorId as ProtoActorId;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::borrow::Borrow;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::ops::Deref;

use crate::{TEMP_SENDER_ACTOR_TYPE, TEMP_SENDER_PREFIX};

const NAME_PATTERN: &str = "^[a-zA-Z0-9][a-zA-Z0-9_-]*$";

/// Errors raised while constructing or restoring an [`ActorId`].
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum ActorIdError {
    /// Actor name does not satisfy the canonical validation rules.
    #[error("Invalid actor name '{0}': must match {NAME_PATTERN}")]
    InvalidName(String),

    /// One of the required fields is empty.
    #[error("Missing required field: {0}")]
    MissingField(&'static str),

    /// The canonical string representation is malformed.
    #[error("Invalid canonical format: {0}")]
    InvalidFormat(String),
}

/// Structured actor identity.
#[derive(Clone, Debug)]
pub struct ActorId {
    canonical: String,
    name: String,
    actor_type: String,
    namespace: String,
    node_id: String,
}

impl ActorId {
    /// Build a new structured actor ID.
    pub fn new(
        name: impl Into<String>,
        actor_type: impl Into<String>,
        namespace: impl Into<String>,
        node_id: impl Into<String>,
    ) -> Result<Self, ActorIdError> {
        let name = name.into();
        let actor_type = actor_type.into();
        let namespace = namespace.into();
        let node_id = node_id.into();

        validate_name(&name)?;
        validate_required("actor_type", &actor_type)?;
        validate_required("namespace", &namespace)?;
        validate_required("node_id", &node_id)?;

        let canonical = format!("{name}//{actor_type}::{namespace}@{node_id}");

        Ok(Self {
            canonical,
            name,
            actor_type,
            namespace,
            node_id,
        })
    }

    /// Restore an actor ID from its canonical string representation.
    pub fn from_canonical(canonical: &str) -> Result<Self, ActorIdError> {
        let (before_node, node_id) = canonical
            .rsplit_once('@')
            .ok_or_else(|| ActorIdError::InvalidFormat(format!("missing @ in '{canonical}'")))?;
        let (name, after_name) = before_node
            .split_once("//")
            .ok_or_else(|| ActorIdError::InvalidFormat(format!("missing // in '{canonical}'")))?;
        let (actor_type, namespace) = after_name
            .split_once("::")
            .ok_or_else(|| ActorIdError::InvalidFormat(format!("missing :: in '{canonical}'")))?;

        Self::new(name, actor_type, namespace, node_id)
    }

    /// Convert a proto actor ID into the Rust representation.
    pub fn from_proto(proto: &ProtoActorId) -> Result<Self, ActorIdError> {
        Self::new(
            proto.name.clone(),
            proto.actor_type.clone(),
            proto.namespace.clone(),
            proto.node_id.clone(),
        )
    }

    /// Convert this actor ID to the proto representation.
    pub fn to_proto(&self) -> ProtoActorId {
        ProtoActorId {
            name: self.name.clone(),
            actor_type: self.actor_type.clone(),
            namespace: self.namespace.clone(),
            node_id: self.node_id.clone(),
        }
    }

    /// User-specified actor name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Registered actor type.
    pub fn actor_type(&self) -> &str {
        &self.actor_type
    }

    /// Actor namespace.
    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    /// Hosting node identifier.
    pub fn node_id(&self) -> &str {
        &self.node_id
    }

    /// Canonical string form.
    pub fn as_str(&self) -> &str {
        &self.canonical
    }

    /// Return a cloned actor ID with a different node ID.
    pub fn with_node_id(&self, node_id: impl Into<String>) -> Result<Self, ActorIdError> {
        Self::new(
            self.name.clone(),
            self.actor_type.clone(),
            self.namespace.clone(),
            node_id,
        )
    }

    /// Create a temporary sender actor identity used by ask/reply routing.
    pub fn temporary_sender(
        correlation_id: impl AsRef<str>,
        namespace: impl Into<String>,
        node_id: impl Into<String>,
    ) -> Result<Self, ActorIdError> {
        Self::new(
            format!("{}_{}", TEMP_SENDER_PREFIX, correlation_id.as_ref()),
            TEMP_SENDER_ACTOR_TYPE,
            namespace,
            node_id,
        )
    }

    /// Returns true when this actor ID represents a temporary sender actor.
    pub fn is_temporary_sender(&self) -> bool {
        self.actor_type == TEMP_SENDER_ACTOR_TYPE
            && self.name.starts_with(&format!("{TEMP_SENDER_PREFIX}_"))
    }
}

fn validate_required(field: &'static str, value: &str) -> Result<(), ActorIdError> {
    if value.is_empty() {
        Err(ActorIdError::MissingField(field))
    } else {
        Ok(())
    }
}

fn validate_name(name: &str) -> Result<(), ActorIdError> {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return Err(ActorIdError::InvalidName(name.to_string()));
    };

    if !first.is_ascii_alphanumeric() {
        return Err(ActorIdError::InvalidName(name.to_string()));
    }

    if chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_') {
        Ok(())
    } else {
        Err(ActorIdError::InvalidName(name.to_string()))
    }
}

impl Hash for ActorId {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.canonical.hash(state);
    }
}

impl PartialEq for ActorId {
    fn eq(&self, other: &Self) -> bool {
        self.canonical == other.canonical
    }
}

impl Eq for ActorId {}

impl PartialOrd for ActorId {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ActorId {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.canonical.cmp(&other.canonical)
    }
}

impl fmt::Display for ActorId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.canonical)
    }
}

impl AsRef<str> for ActorId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Deref for ActorId {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl Borrow<str> for ActorId {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl From<ActorId> for String {
    fn from(value: ActorId) -> Self {
        value.canonical
    }
}

impl From<String> for ActorId {
    fn from(value: String) -> Self {
        match Self::from_canonical(&value) {
            Ok(actor_id) => actor_id,
            Err(error) => panic!("invalid canonical ActorId '{value}': {error}"),
        }
    }
}

impl From<&str> for ActorId {
    fn from(value: &str) -> Self {
        Self::from(value.to_string())
    }
}

impl PartialEq<str> for ActorId {
    fn eq(&self, other: &str) -> bool {
        self.canonical == other
    }
}

impl PartialEq<String> for ActorId {
    fn eq(&self, other: &String) -> bool {
        self.canonical == *other
    }
}

impl Serialize for ActorId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ActorId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let canonical = String::deserialize(deserializer)?;
        Self::from_canonical(&canonical).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn creates_valid_actor_id() {
        let actor_id = ActorId::new("counter", "gen_server", "default", "node-1").unwrap();

        assert_eq!(actor_id.name(), "counter");
        assert_eq!(actor_id.actor_type(), "gen_server");
        assert_eq!(actor_id.namespace(), "default");
        assert_eq!(actor_id.node_id(), "node-1");
        assert_eq!(actor_id.as_str(), "counter//gen_server::default@node-1");
    }

    #[test]
    fn rejects_invalid_names() {
        for name in ["", "-counter", "bad@name", "bad//name", "bad::name", "bad space"] {
            let err = ActorId::new(name, "worker", "default", "node-1").unwrap_err();
            assert!(matches!(err, ActorIdError::InvalidName(_)));
        }
    }

    #[test]
    fn rejects_missing_required_fields() {
        assert_eq!(
            ActorId::new("counter", "", "default", "node-1").unwrap_err(),
            ActorIdError::MissingField("actor_type")
        );
        assert_eq!(
            ActorId::new("counter", "worker", "", "node-1").unwrap_err(),
            ActorIdError::MissingField("namespace")
        );
        assert_eq!(
            ActorId::new("counter", "worker", "default", "").unwrap_err(),
            ActorIdError::MissingField("node_id")
        );
    }

    #[test]
    fn display_uses_canonical_form() {
        let actor_id = ActorId::new("counter", "worker", "prod", "node-7").unwrap();
        assert_eq!(actor_id.to_string(), "counter//worker::prod@node-7");
    }

    #[test]
    fn canonical_round_trip() {
        let canonical = "counter//worker::prod@node-7";
        let actor_id = ActorId::from_canonical(canonical).unwrap();
        assert_eq!(actor_id.to_string(), canonical);
    }

    #[test]
    fn hash_and_eq_use_identity_components() {
        let a = ActorId::new("counter", "worker", "prod", "node-7").unwrap();
        let b = ActorId::new("counter", "worker", "prod", "node-7").unwrap();
        let c = ActorId::new("counter", "worker", "prod", "node-8").unwrap();

        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn with_node_id_clones_components() {
        let actor_id = ActorId::new("counter", "worker", "prod", "node-7").unwrap();
        let moved = actor_id.with_node_id("node-8").unwrap();

        assert_eq!(moved.name(), "counter");
        assert_eq!(moved.actor_type(), "worker");
        assert_eq!(moved.namespace(), "prod");
        assert_eq!(moved.node_id(), "node-8");
    }

    #[test]
    fn serde_round_trip_uses_canonical_string() {
        let actor_id = ActorId::new("counter", "worker", "prod", "node-7").unwrap();
        let json = serde_json::to_string(&actor_id).unwrap();
        let restored: ActorId = serde_json::from_str(&json).unwrap();

        assert_eq!(json, "\"counter//worker::prod@node-7\"");
        assert_eq!(restored, actor_id);
    }

    #[test]
    fn string_equality_and_borrow_lookup_work() {
        let actor_id = ActorId::new("counter", "worker", "prod", "node-7").unwrap();
        let mut ids = HashMap::new();
        ids.insert(actor_id.clone(), 42_u8);

        assert_eq!(actor_id.to_string(), "counter//worker::prod@node-7");
        assert_eq!(ids.get("counter//worker::prod@node-7"), Some(&42_u8));
    }

    #[test]
    fn proto_round_trip_preserves_components() {
        let actor_id = ActorId::new("counter", "worker", "prod", "node-7").unwrap();
        let proto = actor_id.to_proto();
        let restored = ActorId::from_proto(&proto).unwrap();

        assert_eq!(restored, actor_id);
    }
}
