// SPDX-License-Identifier: AGPL-3.0-or-later
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

//! Schema validation facet — validates handler input against JSON Schema (draft-7).
//!
//! ## Purpose
//! Intercepts actor method calls **before** they reach the actor and validates the
//! input bytes against a registered JSON Schema for that method.  Invalid inputs are
//! short-circuited at the infrastructure layer; the actor never sees them.
//!
//! This is a **general-purpose** primitive.  It works for any actor handler:
//! payment transfers, API gateways, tool registries, order processors, etc.
//! There is no coupling to agent/LLM semantics.
//!
//! ## Priority
//! Default priority is **95** — runs after `VirtualActorFacet` (100) and before
//! `DurabilityFacet` (90), so invalid messages are never journaled.
//!
//! ## Configuration (app-config.toml)
//! ```toml
//! { type = "schema_validation", priority = 95, config = {
//!   validation_mode = "strict",
//!   method_schemas = {
//!     "transfer"   = "{\"type\":\"object\",\"required\":[\"to\",\"amount\"]}",
//!     "web_search" = "{\"type\":\"object\",\"required\":[\"query\"],\"properties\":{\"query\":{\"type\":\"string\",\"minLength\":1}}}",
//!   }
//! }}
//! ```
//!
//! ## Validation modes
//! - `strict` (default) — invalid input → `ShortCircuit` with structured error JSON
//! - `warn` — invalid input → log warning, continue to actor
//! - `permissive` — always continue (schema is informational only)
//!
//! ## Schema registration at runtime
//! Use the `Facet::configure` extension: not available on the base trait, but the factory
//! accepts schemas in the config map at construction time.  For runtime additions, the
//! `SchemaValidationFacet::register_schema` method is available when you hold a concrete
//! reference (e.g., during testing or factory customisation).

use async_trait::async_trait;
use jsonschema::JSONSchema;
use plexspaces_facet::{ErrorHandling, Facet, FacetError, InterceptResult};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Default facet priority — after VirtualActorFacet (100), before DurabilityFacet (90).
pub const SCHEMA_VALIDATION_FACET_DEFAULT_PRIORITY: i32 = 95;

/// How the facet behaves when schema validation fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationMode {
    /// Reject the message — return a structured error JSON to the caller.
    Strict,
    /// Log a warning and allow the message to continue to the actor.
    Warn,
    /// Always allow the message (schema is informational).
    Permissive,
}

impl ValidationMode {
    fn from_str(s: &str) -> Self {
        match s {
            "warn" => ValidationMode::Warn,
            "permissive" => ValidationMode::Permissive,
            _ => ValidationMode::Strict,
        }
    }
}

/// A compiled JSON Schema entry — the compiled validator plus the original source for display.
struct CompiledSchema {
    validator: JSONSchema,
    source: Value,
}

impl std::fmt::Debug for CompiledSchema {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompiledSchema")
            .field("source", &self.source)
            .finish()
    }
}

/// Schema validation facet.
///
/// Validates handler inputs against registered JSON Schemas (draft-7) using the
/// `jsonschema` crate.  Compiled validators are cached at construction time so
/// hot-path validation has no parsing overhead.
pub struct SchemaValidationFacet {
    /// Raw config stored for `get_config()`.
    config_value: Value,
    /// Facet priority.
    priority: i32,
    /// Actor this facet is attached to (set on `on_attach`, cleared on `on_detach`).
    actor_id: Option<String>,
    /// Validation mode.
    validation_mode: ValidationMode,
    /// Compiled schemas keyed by method name.
    ///
    /// `Arc<RwLock<…>>` only because `register_schema` needs `&self` (the `Facet`
    /// trait takes `&self` on `before_method`).  The write lock is only acquired
    /// during `register_schema`, never during `before_method` under normal operation
    /// once all schemas are registered.
    schemas: Arc<RwLock<HashMap<String, CompiledSchema>>>,
}

impl std::fmt::Debug for SchemaValidationFacet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SchemaValidationFacet")
            .field("priority", &self.priority)
            .field("actor_id", &self.actor_id)
            .field("validation_mode", &self.validation_mode)
            .finish()
    }
}

impl SchemaValidationFacet {
    /// Create a new `SchemaValidationFacet`.
    ///
    /// ## Arguments
    /// * `config` — JSON configuration (see module-level docs for shape).
    /// * `priority` — Facet priority; use `SCHEMA_VALIDATION_FACET_DEFAULT_PRIORITY`.
    ///
    /// ## Errors
    /// Returns `Err(FacetError::InvalidConfig)` if any schema string in
    /// `method_schemas` is not valid JSON or cannot be compiled as JSON Schema.
    /// **Schema errors fail loudly — they are never silently swallowed.**
    pub fn new(config: Value, priority: i32) -> Result<Self, FacetError> {
        let mode_str = config
            .get("validation_mode")
            .and_then(|v| v.as_str())
            .unwrap_or("strict");
        let validation_mode = ValidationMode::from_str(mode_str);

        let mut compiled: HashMap<String, CompiledSchema> = HashMap::new();

        if let Some(schemas_obj) = config.get("method_schemas").and_then(|v| v.as_object()) {
            for (method, schema_val) in schemas_obj {
                // Parse the schema — it may be an inline JSON object or a JSON string.
                let schema_json: Value = match schema_val {
                    Value::String(s) => serde_json::from_str(s).map_err(|e| {
                        FacetError::InvalidConfig(format!(
                            "schema_validation: method '{}' has invalid JSON schema string: {}",
                            method, e
                        ))
                    })?,
                    other => other.clone(),
                };

                // Compile — full draft-7 validation via jsonschema crate.
                let validator = JSONSchema::compile(&schema_json).map_err(|e| {
                    FacetError::InvalidConfig(format!(
                        "schema_validation: method '{}' schema failed to compile: {}",
                        method, e
                    ))
                })?;

                compiled.insert(
                    method.clone(),
                    CompiledSchema {
                        validator,
                        source: schema_json,
                    },
                );
            }
        }

        Ok(Self {
            config_value: config,
            priority,
            actor_id: None,
            validation_mode,
            schemas: Arc::new(RwLock::new(compiled)),
        })
    }

    /// Register (or replace) a schema for a method at runtime.
    ///
    /// ## Errors
    /// Returns `Err` if the schema string is not valid JSON or does not compile.
    pub async fn register_schema(
        &self,
        method: &str,
        schema_json: &str,
    ) -> Result<(), FacetError> {
        let schema_val: Value = serde_json::from_str(schema_json).map_err(|e| {
            FacetError::InvalidConfig(format!(
                "schema_validation: register_schema '{}': invalid JSON: {}",
                method, e
            ))
        })?;

        let validator = JSONSchema::compile(&schema_val).map_err(|e| {
            FacetError::InvalidConfig(format!(
                "schema_validation: register_schema '{}': schema failed to compile: {}",
                method, e
            ))
        })?;

        let mut guard = self.schemas.write().await;
        guard.insert(
            method.to_string(),
            CompiledSchema {
                validator,
                source: schema_val,
            },
        );
        Ok(())
    }

    /// Validate `input_bytes` against the schema registered for `method`.
    ///
    /// Returns `Ok(())` if valid, no schema registered, or mode is permissive.
    /// Returns `Err(String)` with a human-readable error list if validation fails.
    async fn validate(&self, method: &str, input_bytes: &[u8]) -> Result<(), String> {
        let guard = self.schemas.read().await;
        let entry = match guard.get(method) {
            Some(e) => e,
            None => return Ok(()), // No schema registered for this method → allow
        };

        // Parse the incoming payload.
        let instance: Value = serde_json::from_slice(input_bytes).map_err(|e| {
            format!(
                "schema_validation: method '{}' — input is not valid JSON: {}",
                method, e
            )
        })?;

        // Run the compiled JSON Schema validator (full draft-7).
        if let Err(errors) = entry.validator.validate(&instance) {
            let messages: Vec<String> = errors
                .map(|e| format!("{} at {}", e, e.instance_path))
                .collect();
            return Err(format!(
                "schema_validation: method '{}' failed ({} error(s)): {}",
                method,
                messages.len(),
                messages.join("; ")
            ));
        }

        Ok(())
    }

    /// Build the short-circuit error response bytes.
    fn rejection_bytes(method: &str, error_detail: &str) -> Vec<u8> {
        let v = serde_json::json!({
            "status": "error",
            "code": "schema_validation_failed",
            "method": method,
            "detail": error_detail,
        });
        serde_json::to_vec(&v).unwrap_or_default()
    }
}

#[async_trait]
impl Facet for SchemaValidationFacet {
    fn facet_type(&self) -> &str {
        "schema_validation"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    async fn on_attach(&mut self, actor_id: &str, _config: Value) -> Result<(), FacetError> {
        self.actor_id = Some(actor_id.to_string());
        tracing::debug!(
            actor_id = actor_id,
            mode = ?self.validation_mode,
            "SchemaValidationFacet attached"
        );
        Ok(())
    }

    async fn on_detach(&mut self, _actor_id: &str) -> Result<(), FacetError> {
        self.actor_id = None;
        Ok(())
    }

    /// Validate the input before the actor processes it.
    ///
    /// - `strict` mode: invalid → `ShortCircuit` with error JSON (actor never called)
    /// - `warn` mode: invalid → log warning, return `Continue`
    /// - `permissive` mode: always `Continue`
    async fn before_method(
        &self,
        method: &str,
        args: &[u8],
        _headers: &HashMap<String, String>,
    ) -> Result<InterceptResult, FacetError> {
        if self.validation_mode == ValidationMode::Permissive {
            return Ok(InterceptResult::Continue);
        }

        match self.validate(method, args).await {
            Ok(()) => Ok(InterceptResult::Continue),
            Err(detail) => {
                let actor_id = self.actor_id.as_deref().unwrap_or("<unknown>");
                match self.validation_mode {
                    ValidationMode::Strict => {
                        tracing::warn!(
                            actor_id,
                            method,
                            detail = %detail,
                            "SchemaValidationFacet: rejecting invalid input"
                        );
                        metrics::counter!("schema_validation_rejections_total",
                            "method" => method.to_string(),
                            "actor_id" => actor_id.to_string()
                        ).increment(1);
                        Ok(InterceptResult::ShortCircuit(Self::rejection_bytes(
                            method, &detail,
                        )))
                    }
                    ValidationMode::Warn => {
                        tracing::warn!(
                            actor_id,
                            method,
                            detail = %detail,
                            "SchemaValidationFacet: validation warning (warn mode — continuing)"
                        );
                        metrics::counter!("schema_validation_warnings_total",
                            "method" => method.to_string(),
                            "actor_id" => actor_id.to_string()
                        ).increment(1);
                        Ok(InterceptResult::Continue)
                    }
                    ValidationMode::Permissive => Ok(InterceptResult::Continue),
                }
            }
        }
    }

    async fn on_error(&self, _method: &str, _error: &str) -> Result<ErrorHandling, FacetError> {
        Ok(ErrorHandling::Propagate)
    }

    fn get_config(&self) -> Value {
        self.config_value.clone()
    }

    fn get_priority(&self) -> i32 {
        self.priority
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use plexspaces_facet::InterceptResult;
    use serde_json::json;

    fn make_facet(mode: &str, schemas: serde_json::Map<String, Value>) -> SchemaValidationFacet {
        let config = json!({
            "validation_mode": mode,
            "method_schemas": schemas,
        });
        SchemaValidationFacet::new(config, SCHEMA_VALIDATION_FACET_DEFAULT_PRIORITY)
            .expect("facet construction should succeed with valid schemas")
    }

    fn args(v: Value) -> Vec<u8> {
        serde_json::to_vec(&v).unwrap()
    }

    fn empty_headers() -> HashMap<String, String> {
        HashMap::new()
    }

    // ── Construction ──────────────────────────────────────────────────────────

    #[test]
    fn test_construction_default_priority() {
        let facet = make_facet("strict", serde_json::Map::new());
        assert_eq!(facet.get_priority(), SCHEMA_VALIDATION_FACET_DEFAULT_PRIORITY);
        assert_eq!(facet.facet_type(), "schema_validation");
    }

    #[test]
    fn test_construction_invalid_schema_fails_loudly() {
        let config = json!({
            "method_schemas": { "transfer": "not-valid-json{{" }
        });
        let result = SchemaValidationFacet::new(config, 95);
        assert!(result.is_err(), "invalid JSON schema must fail at construction");
        let err = result.unwrap_err();
        assert!(
            format!("{:?}", err).contains("invalid JSON schema"),
            "error should mention 'invalid JSON schema'"
        );
    }

    #[test]
    fn test_construction_schema_as_object() {
        // Schema as inline JSON object (not a string) must also compile correctly
        let config = json!({
            "method_schemas": {
                "transfer": {
                    "type": "object",
                    "required": ["amount"],
                    "properties": {"amount": {"type": "number"}}
                }
            }
        });
        assert!(SchemaValidationFacet::new(config, 95).is_ok());
    }

    // ── Strict mode ───────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_strict_valid_input_passes() {
        let mut schemas = serde_json::Map::new();
        schemas.insert(
            "transfer".to_string(),
            json!(r#"{"type":"object","required":["to","amount"],"properties":{"to":{"type":"string"},"amount":{"type":"number","minimum":0}}}"#),
        );
        let mut facet = make_facet("strict", schemas);
        facet.on_attach("actor-1", json!({})).await.unwrap();

        let input = args(json!({"to": "alice", "amount": 100.0}));
        let result = facet.before_method("transfer", &input, &empty_headers()).await.unwrap();
        assert!(matches!(result, InterceptResult::Continue));
    }

    #[tokio::test]
    async fn test_strict_missing_required_field_rejected() {
        let mut schemas = serde_json::Map::new();
        schemas.insert(
            "transfer".to_string(),
            json!(r#"{"type":"object","required":["to","amount"]}"#),
        );
        let mut facet = make_facet("strict", schemas);
        facet.on_attach("actor-1", json!({})).await.unwrap();

        // Missing "amount"
        let input = args(json!({"to": "alice"}));
        let result = facet.before_method("transfer", &input, &empty_headers()).await.unwrap();
        match result {
            InterceptResult::ShortCircuit(bytes) => {
                let v: Value = serde_json::from_slice(&bytes).unwrap();
                assert_eq!(v["code"], "schema_validation_failed");
                assert_eq!(v["method"], "transfer");
            }
            other => panic!("expected ShortCircuit, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_strict_integer_type_correct() {
        // A float must FAIL when schema says "integer"
        let mut schemas = serde_json::Map::new();
        schemas.insert(
            "set_count".to_string(),
            json!(r#"{"type":"object","properties":{"count":{"type":"integer"}}}"#),
        );
        let mut facet = make_facet("strict", schemas);
        facet.on_attach("actor-1", json!({})).await.unwrap();

        // 3.14 is NOT an integer
        let input = args(json!({"count": 3.14}));
        let result = facet.before_method("set_count", &input, &empty_headers()).await.unwrap();
        assert!(
            matches!(result, InterceptResult::ShortCircuit(_)),
            "float 3.14 must fail type:integer"
        );

        // 3 (integer) must pass
        let input_int = args(json!({"count": 3}));
        let result_int = facet
            .before_method("set_count", &input_int, &empty_headers())
            .await
            .unwrap();
        assert!(matches!(result_int, InterceptResult::Continue));
    }

    #[tokio::test]
    async fn test_strict_min_length_enforced() {
        let mut schemas = serde_json::Map::new();
        schemas.insert(
            "search".to_string(),
            json!(r#"{"type":"object","properties":{"query":{"type":"string","minLength":1}}}"#),
        );
        let mut facet = make_facet("strict", schemas);
        facet.on_attach("actor-1", json!({})).await.unwrap();

        // Empty string → rejected
        let input = args(json!({"query": ""}));
        let result = facet.before_method("search", &input, &empty_headers()).await.unwrap();
        assert!(matches!(result, InterceptResult::ShortCircuit(_)));

        // Non-empty → allowed
        let input_ok = args(json!({"query": "hello"}));
        let result_ok = facet
            .before_method("search", &input_ok, &empty_headers())
            .await
            .unwrap();
        assert!(matches!(result_ok, InterceptResult::Continue));
    }

    #[tokio::test]
    async fn test_strict_enum_enforced() {
        let mut schemas = serde_json::Map::new();
        schemas.insert(
            "set_mode".to_string(),
            json!(r#"{"type":"object","properties":{"mode":{"enum":["fast","slow","off"]}}}"#),
        );
        let mut facet = make_facet("strict", schemas);
        facet.on_attach("actor-1", json!({})).await.unwrap();

        let bad = args(json!({"mode": "turbo"}));
        assert!(matches!(
            facet.before_method("set_mode", &bad, &empty_headers()).await.unwrap(),
            InterceptResult::ShortCircuit(_)
        ));

        let good = args(json!({"mode": "fast"}));
        assert!(matches!(
            facet.before_method("set_mode", &good, &empty_headers()).await.unwrap(),
            InterceptResult::Continue
        ));
    }

    #[tokio::test]
    async fn test_strict_pattern_enforced() {
        let mut schemas = serde_json::Map::new();
        schemas.insert(
            "set_code".to_string(),
            json!(r#"{"type":"object","properties":{"code":{"type":"string","pattern":"^[A-Z]{3}-\\d{4}$"}}}"#),
        );
        let mut facet = make_facet("strict", schemas);
        facet.on_attach("actor-1", json!({})).await.unwrap();

        let bad = args(json!({"code": "abc-123"}));
        assert!(matches!(
            facet.before_method("set_code", &bad, &empty_headers()).await.unwrap(),
            InterceptResult::ShortCircuit(_)
        ));

        let good = args(json!({"code": "ABC-1234"}));
        assert!(matches!(
            facet.before_method("set_code", &good, &empty_headers()).await.unwrap(),
            InterceptResult::Continue
        ));
    }

    #[tokio::test]
    async fn test_no_schema_for_method_passes() {
        // No schema registered for "unknown_method" → always pass
        let facet = make_facet("strict", serde_json::Map::new());
        let input = args(json!({"anything": "goes"}));
        let result = facet.before_method("unknown_method", &input, &empty_headers()).await.unwrap();
        assert!(matches!(result, InterceptResult::Continue));
    }

    // ── Warn mode ─────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_warn_invalid_still_continues() {
        let mut schemas = serde_json::Map::new();
        schemas.insert(
            "transfer".to_string(),
            json!(r#"{"type":"object","required":["amount"]}"#),
        );
        let mut facet = make_facet("warn", schemas);
        facet.on_attach("actor-1", json!({})).await.unwrap();

        // Missing "amount" — in warn mode this must still Continue
        let input = args(json!({"irrelevant": true}));
        let result = facet.before_method("transfer", &input, &empty_headers()).await.unwrap();
        assert!(matches!(result, InterceptResult::Continue));
    }

    // ── Permissive mode ───────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_permissive_always_continues() {
        let mut schemas = serde_json::Map::new();
        schemas.insert(
            "transfer".to_string(),
            json!(r#"{"type":"object","required":["amount"]}"#),
        );
        let facet = make_facet("permissive", schemas);

        // Missing "amount" — permissive always passes
        let input = args(json!({}));
        let result = facet.before_method("transfer", &input, &empty_headers()).await.unwrap();
        assert!(matches!(result, InterceptResult::Continue));
    }

    // ── Runtime schema registration ───────────────────────────────────────────

    #[tokio::test]
    async fn test_register_schema_runtime() {
        let facet = make_facet("strict", serde_json::Map::new());

        // No schema yet → passes
        let input = args(json!({"x": 1}));
        assert!(matches!(
            facet.before_method("dynamic", &input, &empty_headers()).await.unwrap(),
            InterceptResult::Continue
        ));

        // Register a schema that requires field "x" to be a string
        facet
            .register_schema("dynamic", r#"{"type":"object","properties":{"x":{"type":"string"}}}"#)
            .await
            .unwrap();

        // Integer 1 now fails
        assert!(matches!(
            facet.before_method("dynamic", &input, &empty_headers()).await.unwrap(),
            InterceptResult::ShortCircuit(_)
        ));

        // String passes
        let input_str = args(json!({"x": "hello"}));
        assert!(matches!(
            facet.before_method("dynamic", &input_str, &empty_headers()).await.unwrap(),
            InterceptResult::Continue
        ));
    }

    #[tokio::test]
    async fn test_register_schema_invalid_fails() {
        let facet = make_facet("strict", serde_json::Map::new());
        let result = facet.register_schema("method", "not-json{{").await;
        assert!(result.is_err());
    }

    // ── Priority and config ───────────────────────────────────────────────────

    #[test]
    fn test_custom_priority() {
        let config = json!({ "priority": 80 });
        let facet = SchemaValidationFacet::new(config, 80).unwrap();
        assert_eq!(facet.get_priority(), 80);
    }

    #[tokio::test]
    async fn test_on_attach_sets_actor_id() {
        let mut facet = make_facet("strict", serde_json::Map::new());
        assert!(facet.actor_id.is_none());
        facet.on_attach("my-actor", json!({})).await.unwrap();
        assert_eq!(facet.actor_id.as_deref(), Some("my-actor"));
        facet.on_detach("my-actor").await.unwrap();
        assert!(facet.actor_id.is_none());
    }

    // ── Non-JSON input ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_non_json_input_rejected_in_strict() {
        let mut schemas = serde_json::Map::new();
        schemas.insert(
            "m".to_string(),
            json!(r#"{"type":"object"}"#),
        );
        let mut facet = make_facet("strict", schemas);
        facet.on_attach("actor-1", json!({})).await.unwrap();

        let bad_bytes = b"not json at all {{";
        let result = facet.before_method("m", bad_bytes, &empty_headers()).await.unwrap();
        assert!(matches!(result, InterceptResult::ShortCircuit(_)));
    }

    // ── Multiple schemas on same facet ────────────────────────────────────────

    #[tokio::test]
    async fn test_multiple_methods_independent_schemas() {
        let mut schemas = serde_json::Map::new();
        schemas.insert(
            "deposit".to_string(),
            json!(r#"{"type":"object","required":["amount"],"properties":{"amount":{"type":"number","minimum":0}}}"#),
        );
        schemas.insert(
            "withdraw".to_string(),
            json!(r#"{"type":"object","required":["amount"],"properties":{"amount":{"type":"number","maximum":10000}}}"#),
        );
        let mut facet = make_facet("strict", schemas);
        facet.on_attach("bank", json!({})).await.unwrap();

        // deposit: negative fails
        assert!(matches!(
            facet.before_method("deposit", &args(json!({"amount": -1})), &empty_headers()).await.unwrap(),
            InterceptResult::ShortCircuit(_)
        ));
        // deposit: positive passes
        assert!(matches!(
            facet.before_method("deposit", &args(json!({"amount": 50})), &empty_headers()).await.unwrap(),
            InterceptResult::Continue
        ));
        // withdraw: over max fails
        assert!(matches!(
            facet.before_method("withdraw", &args(json!({"amount": 99999})), &empty_headers()).await.unwrap(),
            InterceptResult::ShortCircuit(_)
        ));
        // withdraw: valid passes
        assert!(matches!(
            facet.before_method("withdraw", &args(json!({"amount": 500})), &empty_headers()).await.unwrap(),
            InterceptResult::Continue
        ));
    }
}
