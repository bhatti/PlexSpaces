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

//! Actor ID Factory - Unified actor ID construction and parsing
//!
//! ## Purpose
//! Provides factory methods for building and parsing actor IDs consistently across the codebase.
//! Follows proto-first design: actor IDs include actor_type from proto Actor message.
//!
//! ## Actor ID Format
//! Format: `{id}//{actor_type}::{namespace}@{node_id}`
//! - `id`: Base actor identifier (can be ULID, client-provided, or empty)
//! - `actor_type`: Actor type from proto (e.g., "read-state-tracker", "GenServer")
//! - `namespace`: Optional namespace for multi-tenancy
//! - `node_id`: Node identifier
//!
//! ## Delimiters
//! - `//`: Separates base ID from actor_type (allows client-provided IDs with slashes)
//! - `::`: Separates actor_type from namespace (allows actor_type with colons)
//! - `@`: Separates namespace from node_id (standard format)
//!
//! ## Examples
//! - `user-123//read-state-tracker::orbit-read-state-ts@node-1`
//! - `//read-state-tracker::orbit-read-state-ts@node-1` (no base ID, ULID generated)
//! - `//GenServer::default@node-1` (no namespace)
//! - `counter@node-1` (legacy format, backward compatible)

use crate::ActorId;

/// Error types for actor ID operations
#[derive(Debug, Clone, thiserror::Error)]
pub enum ActorIdError {
    /// Invalid actor ID format
    #[error("Invalid actor ID format: {0}")]
    InvalidFormat(String),

    /// Missing required component
    #[error("Missing required component: {0}")]
    MissingComponent(String),
}

/// Parsed actor ID components
#[derive(Debug, Clone)]
pub struct ParsedActorId {
    /// Base actor identifier (can be empty, ULID will be generated)
    pub id: String,
    /// Actor type from proto (required)
    pub actor_type: String,
    /// Namespace (optional)
    pub namespace: Option<String>,
    /// Node ID (required)
    pub node_id: String,
}

impl ParsedActorId {
    /// Rebuild actor ID from parsed components
    pub fn to_actor_id(&self) -> ActorId {
        build_actor_id(
            &self.id,
            &self.actor_type,
            self.namespace.as_deref(),
            &self.node_id,
        )
    }
}

/// Build actor ID from components
///
/// ## Purpose
/// Creates a standardized actor ID from individual components.
/// Follows proto-first design: actor_type is required (from proto Actor message).
///
/// ## Arguments
/// * `id` - Base actor identifier (can be empty, ULID will be generated later)
/// * `actor_type` - Actor type from proto (required)
/// * `namespace` - Optional namespace for multi-tenancy
/// * `node_id` - Node identifier (required)
///
/// ## Returns
/// Actor ID in format: `{id}//{actor_type}::{namespace}@{node_id}`
///
/// ## Examples
/// ```rust
/// use plexspaces_core::actor_id::build_actor_id;
///
/// // Full format
/// let actor_id = build_actor_id("user-123", "read-state-tracker", Some("orbit-read-state-ts"), "node-1");
/// // Result: "user-123//read-state-tracker::orbit-read-state-ts@node-1"
///
/// // No base ID (ULID will be generated)
/// let actor_id = build_actor_id("", "read-state-tracker", Some("orbit-read-state-ts"), "node-1");
/// // Result: "//read-state-tracker::orbit-read-state-ts@node-1"
///
/// // No namespace
/// let actor_id = build_actor_id("counter", "GenServer", None, "node-1");
/// // Result: "counter//GenServer@node-1"
/// ```
pub fn build_actor_id(
    id: &str,
    actor_type: &str,
    namespace: Option<&str>,
    node_id: &str,
) -> ActorId {
    if actor_type.is_empty() {
        // Legacy format: just id@node_id (backward compatibility)
        if id.is_empty() {
            format!("@{}", node_id)
        } else {
            format!("{}@{}", id, node_id)
        }
    } else if let Some(ns) = namespace {
        if id.is_empty() {
            format!("//{}::{}@{}", actor_type, ns, node_id)
        } else {
            format!("{}//{}::{}@{}", id, actor_type, ns, node_id)
        }
    } else {
        if id.is_empty() {
            format!("//{}@{}", actor_type, node_id)
        } else {
            format!("{}//{}@{}", id, actor_type, node_id)
        }
    }
}

/// Parse actor ID into components
///
/// ## Purpose
/// Parses an actor ID into its components, supporting both new format and legacy format.
///
/// ## Arguments
/// * `actor_id` - Actor ID to parse
///
/// ## Returns
/// `Ok(ParsedActorId)` with parsed components, `Err(ActorIdError)` if format is invalid
///
/// ## Format Support
/// - New format: `{id}//{actor_type}::{namespace}@{node_id}`
/// - Legacy format: `{actor_name}@{node_id}` (parsed as id="", actor_type=actor_name, namespace=None)
///
/// ## Examples
/// ```rust
/// use plexspaces_core::actor_id::parse_actor_id;
///
/// // New format
/// let parsed = parse_actor_id("user-123//read-state-tracker::orbit-read-state-ts@node-1")?;
/// assert_eq!(parsed.id, "user-123");
/// assert_eq!(parsed.actor_type, "read-state-tracker");
/// assert_eq!(parsed.namespace, Some("orbit-read-state-ts".to_string()));
/// assert_eq!(parsed.node_id, "node-1");
///
/// // Legacy format
/// let parsed = parse_actor_id("counter@node-1")?;
/// assert_eq!(parsed.id, "");
/// assert_eq!(parsed.actor_type, "counter");
/// assert_eq!(parsed.namespace, None);
/// assert_eq!(parsed.node_id, "node-1");
/// ```
pub fn parse_actor_id(actor_id: &str) -> Result<ParsedActorId, ActorIdError> {
    // Split by @ to get node_id
    let (before_node, node_id) = actor_id.rsplit_once('@').ok_or_else(|| {
        ActorIdError::InvalidFormat(format!("Missing @ delimiter in actor ID: {}", actor_id))
    })?;

    if node_id.is_empty() {
        return Err(ActorIdError::InvalidFormat(format!(
            "Empty node_id in actor ID: {}",
            actor_id
        )));
    }

    // Try new format: {id}//{actor_type}::{namespace} or {id}//{actor_type}
    if let Some((before_type, after_type)) = before_node.split_once("//") {
        // Has actor_type separator
        if let Some((actor_type, namespace)) = after_type.split_once("::") {
            // Has namespace separator
            Ok(ParsedActorId {
                id: before_type.to_string(),
                actor_type: actor_type.to_string(),
                namespace: Some(namespace.to_string()),
                node_id: node_id.to_string(),
            })
        } else {
            // No namespace separator, actor_type is everything after //
            Ok(ParsedActorId {
                id: before_type.to_string(),
                actor_type: after_type.to_string(),
                namespace: None,
                node_id: node_id.to_string(),
            })
        }
    } else {
        // Legacy format: {actor_name}@{node_id}
        // Parse as id="", actor_type=actor_name, namespace=None
        Ok(ParsedActorId {
            id: String::new(),
            actor_type: before_node.to_string(),
            namespace: None,
            node_id: node_id.to_string(),
        })
    }
}

/// Extract actor type from actor ID
///
/// ## Purpose
/// Convenience function to extract just the actor_type from an actor ID.
///
/// ## Returns
/// `Some(actor_type)` if parsing succeeds, `None` if format is invalid
pub fn extract_actor_type(actor_id: &str) -> Option<String> {
    parse_actor_id(actor_id).ok().map(|p| p.actor_type)
}

/// Extract namespace from actor ID
///
/// ## Purpose
/// Convenience function to extract just the namespace from an actor ID.
///
/// ## Returns
/// `Some(namespace)` if present, `None` if not present or format is invalid
pub fn extract_namespace(actor_id: &str) -> Option<String> {
    parse_actor_id(actor_id).ok().and_then(|p| p.namespace)
}

/// Extract base ID from actor ID
///
/// ## Purpose
/// Convenience function to extract just the base ID from an actor ID.
///
/// ## Returns
/// Base ID (can be empty), or `None` if format is invalid
pub fn extract_base_id(actor_id: &str) -> Option<String> {
    parse_actor_id(actor_id).ok().map(|p| p.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_actor_id_full() {
        let actor_id = build_actor_id(
            "user-123",
            "read-state-tracker",
            Some("orbit-read-state-ts"),
            "node-1",
        );
        assert_eq!(
            actor_id,
            "user-123//read-state-tracker::orbit-read-state-ts@node-1"
        );
    }

    #[test]
    fn test_build_actor_id_no_base_id() {
        let actor_id = build_actor_id(
            "",
            "read-state-tracker",
            Some("orbit-read-state-ts"),
            "node-1",
        );
        assert_eq!(actor_id, "//read-state-tracker::orbit-read-state-ts@node-1");
    }

    #[test]
    fn test_build_actor_id_no_namespace() {
        let actor_id = build_actor_id("counter", "GenServer", None, "node-1");
        assert_eq!(actor_id, "counter//GenServer@node-1");
    }

    #[test]
    fn test_build_actor_id_legacy() {
        let actor_id = build_actor_id("counter", "", None, "node-1");
        assert_eq!(actor_id, "counter@node-1");
    }

    #[test]
    fn test_parse_actor_id_new_format() {
        let parsed =
            parse_actor_id("user-123//read-state-tracker::orbit-read-state-ts@node-1").unwrap();
        assert_eq!(parsed.id, "user-123");
        assert_eq!(parsed.actor_type, "read-state-tracker");
        assert_eq!(parsed.namespace, Some("orbit-read-state-ts".to_string()));
        assert_eq!(parsed.node_id, "node-1");
    }

    #[test]
    fn test_parse_actor_id_legacy_format() {
        let parsed = parse_actor_id("counter@node-1").unwrap();
        assert_eq!(parsed.id, "");
        assert_eq!(parsed.actor_type, "counter");
        assert_eq!(parsed.namespace, None);
        assert_eq!(parsed.node_id, "node-1");
    }

    #[test]
    fn test_parse_actor_id_no_namespace() {
        let parsed = parse_actor_id("counter//GenServer@node-1").unwrap();
        assert_eq!(parsed.id, "counter");
        assert_eq!(parsed.actor_type, "GenServer");
        assert_eq!(parsed.namespace, None);
        assert_eq!(parsed.node_id, "node-1");
    }

    #[test]
    fn test_extract_actor_type() {
        assert_eq!(
            extract_actor_type("user-123//read-state-tracker::orbit-read-state-ts@node-1"),
            Some("read-state-tracker".to_string())
        );
        assert_eq!(
            extract_actor_type("counter@node-1"),
            Some("counter".to_string())
        );
    }

    #[test]
    fn test_extract_namespace() {
        assert_eq!(
            extract_namespace("user-123//read-state-tracker::orbit-read-state-ts@node-1"),
            Some("orbit-read-state-ts".to_string())
        );
        assert_eq!(extract_namespace("counter@node-1"), None);
    }

    #[test]
    fn test_round_trip() {
        let original = "user-123//read-state-tracker::orbit-read-state-ts@node-1";
        let parsed = parse_actor_id(original).unwrap();
        let rebuilt = parsed.to_actor_id();
        assert_eq!(original, rebuilt);
    }

    #[test]
    fn test_parse_actor_id_invalid_format() {
        // Missing @ delimiter
        assert!(parse_actor_id("invalid").is_err());

        // Empty node_id
        assert!(parse_actor_id("actor@").is_err());
    }

    #[test]
    fn test_build_actor_id_edge_cases() {
        // Empty everything except node_id
        let actor_id = build_actor_id("", "", None, "node-1");
        assert_eq!(actor_id, "@node-1");

        // Empty base_id, has actor_type and namespace
        let actor_id = build_actor_id("", "test-type", Some("ns"), "node-1");
        assert_eq!(actor_id, "//test-type::ns@node-1");
    }

    #[test]
    fn test_extract_functions() {
        // Test extract_actor_type
        assert_eq!(
            extract_actor_type("user-123//read-state-tracker::orbit-read-state-ts@node-1"),
            Some("read-state-tracker".to_string())
        );
        assert_eq!(
            extract_actor_type("counter@node-1"),
            Some("counter".to_string())
        );
        assert_eq!(extract_actor_type("invalid"), None);

        // Test extract_namespace
        assert_eq!(
            extract_namespace("user-123//read-state-tracker::orbit-read-state-ts@node-1"),
            Some("orbit-read-state-ts".to_string())
        );
        assert_eq!(extract_namespace("counter@node-1"), None);
        assert_eq!(extract_namespace("invalid"), None);

        // Test extract_base_id
        assert_eq!(
            extract_base_id("user-123//read-state-tracker::orbit-read-state-ts@node-1"),
            Some("user-123".to_string())
        );
        assert_eq!(extract_base_id("counter@node-1"), Some("".to_string()));
        assert_eq!(extract_base_id("invalid"), None);
    }

    #[test]
    fn test_parsed_actor_id_to_actor_id() {
        let parsed = ParsedActorId {
            id: "user-123".to_string(),
            actor_type: "read-state-tracker".to_string(),
            namespace: Some("orbit-read-state-ts".to_string()),
            node_id: "node-1".to_string(),
        };

        let actor_id = parsed.to_actor_id();
        assert_eq!(
            actor_id,
            "user-123//read-state-tracker::orbit-read-state-ts@node-1"
        );
    }
}
