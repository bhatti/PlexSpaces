// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! gRPC TuplePlexSpace Service Implementation
//!
//! ## Purpose
//! Implements the TuplePlexSpaceService gRPC interface for distributed TupleSpace operations.
//! This enables actors on different nodes to coordinate via shared tuple space.
//!
//! ## Architecture Context (Phase 3 - Distributed Coordination)
//! This service extends PlexSpaces' local TupleSpace to distributed environments:
//! - Actors on Node A can write tuples accessible to actors on Node B
//! - Pattern matching works across node boundaries
//! - Enables distributed coordination patterns (barriers, pub/sub, work queues)
//!
//! ## Integration Points
//! - Uses Node's local TupleSpace for storage
//! - Converts between proto messages and internal TupleSpace types
//! - Handles gRPC errors and validation
//!
//! ## Design Decisions
//! - **No distributed locking yet**: Write-to-local, eventual consistency model
//! - **Template validation**: Reject empty or malformed patterns
//! - **ULID for tuple IDs**: Sortable, time-based identifiers
//! - **Error mapping**: gRPC status codes from TupleSpace errors

use chrono::Utc;
use plexspaces_proto::{
    tuplespace::v1::{
        tuple_field::Value as ProtoValue, tuple_plex_space_service_server::TuplePlexSpaceService,
        CountRequest, CountResponse, ExistsRequest, ExistsResponse, ReadRequest, ReadResponse,
        Tuple as ProtoTuple, TupleField as ProtoTupleField, WriteRequest, WriteResponse,
    },
    v1::common::Empty,
};
use plexspaces_tuplespace::{
    Lease, OrderedFloat, Pattern, PatternField, Tuple, TupleField, TupleSpaceError,
};
use std::sync::Arc;
use tonic::{Request, Response, Status};

use crate::Node;

/// gRPC service implementation for TupleSpace operations
pub struct TuplePlexSpaceServiceImpl {
    /// Node that owns the TupleSpace
    pub node: Arc<Node>,
}

impl TuplePlexSpaceServiceImpl {
    /// Create new TupleSpace service
    pub fn new(node: Arc<Node>) -> Self {
        Self { node }
    }

    /// Convert proto TupleField to internal TupleField
    fn convert_proto_field_to_internal(
        proto_field: &ProtoTupleField,
    ) -> Result<TupleField, Status> {
        match &proto_field.value {
            Some(ProtoValue::Integer(v)) => Ok(TupleField::Integer(*v)),
            Some(ProtoValue::Float(v)) => Ok(TupleField::Float(OrderedFloat::new(*v))),
            Some(ProtoValue::String(v)) => Ok(TupleField::String(v.clone())),
            Some(ProtoValue::Boolean(v)) => Ok(TupleField::Boolean(*v)),
            Some(ProtoValue::Binary(v)) => Ok(TupleField::Binary(v.clone())),
            Some(ProtoValue::Null(_)) => Ok(TupleField::Null),
            Some(ProtoValue::Wildcard(_)) => {
                // Wildcard is not a value, it's a pattern - error if used in write
                Err(Status::invalid_argument(
                    "Wildcard cannot be used as a tuple value (only in patterns)",
                ))
            }
            None => Err(Status::invalid_argument("TupleField must have a value")),
        }
    }

    /// Convert proto Tuple to internal Tuple
    fn convert_proto_tuple_to_internal(proto_tuple: &ProtoTuple) -> Result<Tuple, Status> {
        if proto_tuple.fields.is_empty() {
            return Err(Status::invalid_argument(
                "Tuple must have at least one field",
            ));
        }

        let mut fields = Vec::new();
        for proto_field in &proto_tuple.fields {
            fields.push(Self::convert_proto_field_to_internal(proto_field)?);
        }

        let mut tuple = Tuple::new(fields);

        // Add metadata if present
        // Proto metadata is map<string, Value> (prost_types::Value), but we store map<string, string>
        // Extract string value from Value message
        for (key, value) in &proto_tuple.metadata {
            // Extract string from prost_types::Value
            let value_str = if let Some(kind) = &value.kind {
                match kind {
                    prost_types::value::Kind::StringValue(s) => s.clone(),
                    prost_types::value::Kind::NumberValue(n) => n.to_string(),
                    prost_types::value::Kind::BoolValue(b) => b.to_string(),
                    prost_types::value::Kind::NullValue(_) => String::new(),
                    prost_types::value::Kind::ListValue(_) => format!("{:?}", value),
                    prost_types::value::Kind::StructValue(_) => format!("{:?}", value),
                }
            } else {
                String::new()
            };
            tuple = tuple.with_metadata(key.clone(), value_str);
        }

        // Add lease if present
        if let Some(proto_lease) = &proto_tuple.lease {
            if let Some(ttl) = &proto_lease.ttl {
                use chrono::Duration as ChronoDuration;
                let duration = ChronoDuration::seconds(ttl.seconds as i64)
                    + ChronoDuration::nanoseconds(ttl.nanos as i64);
                let mut lease = Lease::new(duration);
                if !proto_lease.owner.is_empty() {
                    lease = lease.with_owner(proto_lease.owner.clone());
                }
                if proto_lease.renewable {
                    lease = lease.renewable();
                }
                tuple = tuple.with_lease(lease);
            }
        }

        Ok(tuple)
    }

    /// Convert proto template (used in read/take) to internal Pattern
    fn convert_proto_template_to_pattern(proto_tuple: &ProtoTuple) -> Result<Pattern, Status> {
        if proto_tuple.fields.is_empty() {
            return Err(Status::invalid_argument(
                "Template must have at least one field",
            ));
        }

        let mut pattern_fields = Vec::new();
        for proto_field in &proto_tuple.fields {
            match &proto_field.value {
                Some(ProtoValue::Wildcard(_)) => {
                    pattern_fields.push(PatternField::Wildcard);
                }
                Some(ProtoValue::Integer(v)) => {
                    pattern_fields.push(PatternField::Exact(TupleField::Integer(*v)));
                }
                Some(ProtoValue::Float(v)) => {
                    pattern_fields.push(PatternField::Exact(TupleField::Float(OrderedFloat::new(
                        *v,
                    ))));
                }
                Some(ProtoValue::String(v)) => {
                    pattern_fields.push(PatternField::Exact(TupleField::String(v.clone())));
                }
                Some(ProtoValue::Boolean(v)) => {
                    pattern_fields.push(PatternField::Exact(TupleField::Boolean(*v)));
                }
                Some(ProtoValue::Binary(v)) => {
                    pattern_fields.push(PatternField::Exact(TupleField::Binary(v.clone())));
                }
                Some(ProtoValue::Null(_)) => {
                    pattern_fields.push(PatternField::Exact(TupleField::Null));
                }
                None => {
                    return Err(Status::invalid_argument(
                        "Pattern field must have a value or wildcard",
                    ));
                }
            }
        }

        Ok(Pattern::new(pattern_fields))
    }

    /// Convert internal TupleField to proto TupleField
    fn convert_internal_field_to_proto(field: &TupleField) -> ProtoTupleField {
        let value = match field {
            TupleField::Integer(v) => Some(ProtoValue::Integer(*v)),
            TupleField::Float(v) => Some(ProtoValue::Float(v.get())),
            TupleField::String(v) => Some(ProtoValue::String(v.clone())),
            TupleField::Boolean(v) => Some(ProtoValue::Boolean(*v)),
            TupleField::Binary(v) => Some(ProtoValue::Binary(v.clone())),
            TupleField::Null => Some(ProtoValue::Null(true)),
        };
        ProtoTupleField { value }
    }

    /// Convert internal Tuple to proto Tuple
    fn convert_internal_tuple_to_proto(tuple: &Tuple) -> ProtoTuple {
        use plexspaces_proto::tuplespace::v1::Lease as ProtoLease;
        use prost_types::{Timestamp, Duration as ProtoDuration};
        
        // Convert lease if present
        let lease = tuple.lease().map(|lease| {
            let expires_at = lease.expires_at();
            let now = Utc::now();
            let ttl_duration = if expires_at > now {
                expires_at - now
            } else {
                chrono::Duration::zero()
            };
            
            ProtoLease {
                ttl: Some(ProtoDuration {
                    seconds: ttl_duration.num_seconds(),
                    nanos: ttl_duration.num_nanoseconds().unwrap_or(0) as i32 % 1_000_000_000,
                }),
                owner: lease.owner().map(|s| s.clone()).unwrap_or_default(),
                renewable: lease.is_renewable(),
                expires_at: Some(Timestamp {
                    seconds: expires_at.timestamp(),
                    nanos: expires_at.timestamp_subsec_nanos() as i32,
                }),
            }
        });
        
        // Convert metadata (proto expects map<string, Value>, but we have map<string, string>)
        // Note: Tuple doesn't expose metadata() method, so we'll leave it empty for now
        // TODO: Add metadata() accessor to Tuple if needed
        // For now, metadata is not exposed, so we'll use empty map
        use prost_types::Value;
        let mut proto_metadata = std::collections::HashMap::new();
        // Metadata conversion will be implemented when Tuple exposes metadata accessor
        
        ProtoTuple {
            id: ulid::Ulid::new().to_string(), // Assign ULID
            fields: tuple
                .fields()
                .iter()
                .map(Self::convert_internal_field_to_proto)
                .collect(),
            timestamp: Some(prost_types::Timestamp {
                seconds: Utc::now().timestamp(),
                nanos: 0,
            }),
            lease,
            metadata: proto_metadata,
            location: None,
        }
    }

    /// Convert TupleSpaceError to gRPC Status
    fn tuplespace_error_to_status(error: TupleSpaceError) -> Status {
        match error {
            TupleSpaceError::NotFound => Status::not_found("Tuple not found"),
            TupleSpaceError::PatternError(msg) => {
                Status::invalid_argument(format!("Pattern error: {}", msg))
            }
            TupleSpaceError::LeaseError(msg) => {
                Status::failed_precondition(format!("Lease error: {}", msg))
            }
            TupleSpaceError::InvalidConfiguration(msg) => {
                Status::invalid_argument(format!("Invalid configuration: {}", msg))
            }
            TupleSpaceError::NotImplemented(msg) => Status::unimplemented(msg),
            _ => Status::internal(format!("TupleSpace error: {}", error)),
        }
    }
}

#[tonic::async_trait]
impl TuplePlexSpaceService for TuplePlexSpaceServiceImpl {
    /// Write tuples to the TupleSpace
    async fn write(
        &self,
        request: Request<WriteRequest>,
    ) -> Result<Response<WriteResponse>, Status> {
        let req = request.into_inner();
        let start = std::time::Instant::now();
        
        // Record metrics
        metrics::counter!("plexspaces_node_tuplespace_write_requests_total").increment(1);
        tracing::debug!("TupleSpace write requested");

        // Validate request
        if req.tuples.is_empty() {
            return Err(Status::invalid_argument("At least one tuple required"));
        }

        let mut tuple_ids = Vec::new();

        // Write each tuple
        for proto_tuple in req.tuples {
            let tuple = Self::convert_proto_tuple_to_internal(&proto_tuple)?;

            // Write to local TupleSpace
            self.node
                .tuplespace()
                .write(tuple.clone())
                .await
                .map_err(Self::tuplespace_error_to_status)?;

            // Generate ULID for tuple
            let tuple_id = ulid::Ulid::new().to_string();
            tuple_ids.push(tuple_id);
        }

        Ok(Response::new(WriteResponse { tuple_ids }))
    }

    /// Read tuples from the TupleSpace (non-destructive)
    async fn read(&self, request: Request<ReadRequest>) -> Result<Response<ReadResponse>, Status> {
        let req = request.into_inner();

        // Validate template
        let proto_template = req
            .template
            .ok_or_else(|| Status::invalid_argument("Template is required for read operation"))?;

        // Convert template to pattern
        let pattern = Self::convert_proto_template_to_pattern(&proto_template)?;

        // Read from TupleSpace
        let max_results = req.max_results.max(1) as usize; // At least 1

        let all_tuples = if max_results == 1 {
            // Single result
            if let Some(tuple) = self
                .node
                .tuplespace()
                .read(pattern)
                .await
                .map_err(Self::tuplespace_error_to_status)?
            {
                vec![tuple]
            } else {
                vec![]
            }
        } else {
            // Multiple results - read all matching tuples
            self.node
                .tuplespace()
                .read_all(pattern)
                .await
                .map_err(Self::tuplespace_error_to_status)?
        };

        // Implement pagination: take only max_results, check if more exist
        let tuples: Vec<_> = all_tuples.iter().take(max_results).collect();
        let has_more = all_tuples.len() > max_results;

        // Convert to proto
        let proto_tuples: Vec<ProtoTuple> = tuples
            .iter()
            .map(|t| Self::convert_internal_tuple_to_proto(t))
            .collect();

        Ok(Response::new(ReadResponse {
            tuples: proto_tuples,
            has_more,
        }))
    }

    /// Take tuples from the TupleSpace (destructive)
    async fn take(&self, request: Request<ReadRequest>) -> Result<Response<ReadResponse>, Status> {
        let req = request.into_inner();

        // Validate template
        let proto_template = req
            .template
            .ok_or_else(|| Status::invalid_argument("Template is required for take operation"))?;

        // Convert template to pattern
        let pattern = Self::convert_proto_template_to_pattern(&proto_template)?;

        // Take from TupleSpace
        let max_results = req.max_results.max(1) as usize;

        let tuples = if max_results == 1 {
            // Single result
            if let Some(tuple) = self
                .node
                .tuplespace()
                .take(pattern)
                .await
                .map_err(Self::tuplespace_error_to_status)?
            {
                vec![tuple]
            } else {
                vec![]
            }
        } else {
            // Multiple results
            self.node
                .tuplespace()
                .take_all(pattern)
                .await
                .map_err(Self::tuplespace_error_to_status)?
                .into_iter()
                .take(max_results)
                .collect()
        };

        // Convert to proto
        let proto_tuples: Vec<ProtoTuple> = tuples
            .iter()
            .map(Self::convert_internal_tuple_to_proto)
            .collect();

        Ok(Response::new(ReadResponse {
            tuples: proto_tuples,
            has_more: false,
        }))
    }

    /// Count matching tuples
    async fn count(
        &self,
        request: Request<CountRequest>,
    ) -> Result<Response<CountResponse>, Status> {
        let req = request.into_inner();

        // Validate template
        let proto_template = req
            .template
            .ok_or_else(|| Status::invalid_argument("Template is required for count operation"))?;

        // Convert template to pattern
        let pattern = Self::convert_proto_template_to_pattern(&proto_template)?;

        // Count matching tuples
        let count = self
            .node
            .tuplespace()
            .count(pattern)
            .await
            .map_err(Self::tuplespace_error_to_status)?;

        Ok(Response::new(CountResponse {
            count: count as i64,
        }))
    }

    /// Check if matching tuples exist
    async fn exists(
        &self,
        request: Request<ExistsRequest>,
    ) -> Result<Response<ExistsResponse>, Status> {
        let req = request.into_inner();

        // Validate template
        let proto_template = req
            .template
            .ok_or_else(|| Status::invalid_argument("Template is required for exists operation"))?;

        // Convert template to pattern
        let pattern = Self::convert_proto_template_to_pattern(&proto_template)?;

        // Check existence
        let exists = self
            .node
            .tuplespace()
            .exists(pattern)
            .await
            .map_err(Self::tuplespace_error_to_status)?;

        Ok(Response::new(ExistsResponse { exists }))
    }

    // See docs/BARRIER_REFACTORING_PLAN.md for migration guide and examples.

    /// Subscribe to tuple events (streaming)
    async fn subscribe(
        &self,
        _request: Request<plexspaces_proto::tuplespace::v1::SubscribeRequest>,
    ) -> Result<Response<plexspaces_proto::tuplespace::v1::SubscribeResponse>, Status> {
        // TODO: Implement in Week 6 (Distributed Watchers)
        Err(Status::unimplemented(
            "Subscribe not yet implemented - Phase 3 Week 6",
        ))
    }

    /// Unsubscribe from tuple events
    async fn unsubscribe(
        &self,
        _request: Request<plexspaces_proto::tuplespace::v1::UnsubscribeRequest>,
    ) -> Result<Response<Empty>, Status> {
        // TODO: Implement in Week 6
        Err(Status::unimplemented(
            "Unsubscribe not yet implemented - Phase 3 Week 6",
        ))
    }

    /// Begin transaction
    async fn begin_transaction(
        &self,
        _request: Request<plexspaces_proto::tuplespace::v1::BeginTransactionRequest>,
    ) -> Result<Response<plexspaces_proto::tuplespace::v1::BeginTransactionResponse>, Status> {
        // TODO: Implement transactional support later (post-Phase 3)
        Err(Status::unimplemented("Transactions not yet implemented"))
    }

    /// Commit transaction
    async fn commit_transaction(
        &self,
        _request: Request<plexspaces_proto::tuplespace::v1::CommitTransactionRequest>,
    ) -> Result<Response<plexspaces_proto::tuplespace::v1::CommitTransactionResponse>, Status> {
        Err(Status::unimplemented("Transactions not yet implemented"))
    }

    /// Abort transaction
    async fn abort_transaction(
        &self,
        _request: Request<plexspaces_proto::tuplespace::v1::AbortTransactionRequest>,
    ) -> Result<Response<plexspaces_proto::tuplespace::v1::AbortTransactionResponse>, Status> {
        Err(Status::unimplemented("Transactions not yet implemented"))
    }

    /// Clear all tuples
    async fn clear(&self, _request: Request<Empty>) -> Result<Response<Empty>, Status> {
        self.node.tuplespace().clear().await;
        Ok(Response::new(Empty {}))
    }

    /// Renew tuple lease
    async fn renew_lease(
        &self,
        _request: Request<plexspaces_proto::tuplespace::v1::RenewLeaseRequest>,
    ) -> Result<Response<plexspaces_proto::tuplespace::v1::RenewLeaseResponse>, Status> {
        // TODO: Implement lease renewal (requires storing tuple IDs → tuples mapping)
        Err(Status::unimplemented("Lease renewal not yet implemented"))
    }

    /// Get storage statistics
    ///
    /// ## Purpose
    /// Retrieves performance and usage statistics from the TupleSpace storage backend.
    ///
    /// ## Returns
    /// - StorageStats with tuple count, memory usage, operation counts, latency metrics
    ///
    /// ## Errors
    /// - This method does not return errors (stats are always available)
    async fn get_stats(
        &self,
        _request: Request<Empty>,
    ) -> Result<Response<plexspaces_proto::tuplespace::v1::GetStatsResponse>, Status> {
        // Get stats from TupleSpace (returns TupleSpaceStats directly, not Result)
        let ts_stats = self.node.tuplespace().stats().await;

        // Convert TupleSpaceStats to proto StorageStats
        // TupleSpaceStats fields: total_writes, total_reads, total_takes, current_size
        let total_ops = ts_stats.total_writes() + ts_stats.total_reads() + ts_stats.total_takes();

        let storage_stats = plexspaces_proto::tuplespace::v1::StorageStats {
            tuple_count: ts_stats.current_size() as u64,
            memory_bytes: 0, // Not tracked by in-memory TupleSpace
            total_operations: total_ops,
            read_operations: ts_stats.total_reads(),
            write_operations: ts_stats.total_writes(),
            take_operations: ts_stats.total_takes(),
            avg_latency_ms: 0.0, // Not tracked yet
        };

        // Create response
        let response = plexspaces_proto::tuplespace::v1::GetStatsResponse {
            stats: Some(storage_stats),
        };

        Ok(Response::new(response))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tonic::Code;

    // Helper to create proto field
    fn proto_int(v: i64) -> ProtoTupleField {
        ProtoTupleField {
            value: Some(ProtoValue::Integer(v)),
        }
    }

    fn proto_float(v: f64) -> ProtoTupleField {
        ProtoTupleField {
            value: Some(ProtoValue::Float(v)),
        }
    }

    fn proto_string(v: &str) -> ProtoTupleField {
        ProtoTupleField {
            value: Some(ProtoValue::String(v.to_string())),
        }
    }

    fn proto_bool(v: bool) -> ProtoTupleField {
        ProtoTupleField {
            value: Some(ProtoValue::Boolean(v)),
        }
    }

    fn proto_binary(v: Vec<u8>) -> ProtoTupleField {
        ProtoTupleField {
            value: Some(ProtoValue::Binary(v)),
        }
    }

    fn proto_null() -> ProtoTupleField {
        ProtoTupleField {
            value: Some(ProtoValue::Null(true)),
        }
    }

    fn proto_wildcard() -> ProtoTupleField {
        ProtoTupleField {
            value: Some(ProtoValue::Wildcard(true)),
        }
    }

    fn proto_none() -> ProtoTupleField {
        ProtoTupleField { value: None }
    }

    #[test]
    fn test_convert_proto_field_integer() {
        let field = proto_int(42);
        let result = TuplePlexSpaceServiceImpl::convert_proto_field_to_internal(&field);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), TupleField::Integer(42));
    }

    #[test]
    fn test_convert_proto_field_float() {
        let field = proto_float(3.14);
        let result = TuplePlexSpaceServiceImpl::convert_proto_field_to_internal(&field);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), TupleField::Float(OrderedFloat::new(3.14)));
    }

    #[test]
    fn test_convert_proto_field_string() {
        let field = proto_string("test");
        let result = TuplePlexSpaceServiceImpl::convert_proto_field_to_internal(&field);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), TupleField::String("test".to_string()));
    }

    #[test]
    fn test_convert_proto_field_boolean() {
        let field = proto_bool(true);
        let result = TuplePlexSpaceServiceImpl::convert_proto_field_to_internal(&field);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), TupleField::Boolean(true));
    }

    #[test]
    fn test_convert_proto_field_binary() {
        let field = proto_binary(vec![1, 2, 3]);
        let result = TuplePlexSpaceServiceImpl::convert_proto_field_to_internal(&field);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), TupleField::Binary(vec![1, 2, 3]));
    }

    #[test]
    fn test_convert_proto_field_null() {
        let field = proto_null();
        let result = TuplePlexSpaceServiceImpl::convert_proto_field_to_internal(&field);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), TupleField::Null);
    }

    #[test]
    fn test_convert_proto_field_wildcard_error() {
        let field = proto_wildcard();
        let result = TuplePlexSpaceServiceImpl::convert_proto_field_to_internal(&field);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.code(), Code::InvalidArgument);
        assert!(err.message().contains("Wildcard"));
    }

    #[test]
    fn test_convert_proto_field_none_error() {
        let field = proto_none();
        let result = TuplePlexSpaceServiceImpl::convert_proto_field_to_internal(&field);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.code(), Code::InvalidArgument);
        assert!(err.message().contains("must have a value"));
    }

    #[test]
    fn test_convert_proto_tuple_success() {
        let proto_tuple = ProtoTuple {
            id: String::new(),
            fields: vec![proto_int(1), proto_string("test")],
            timestamp: None,
            lease: None,
            metadata: std::collections::HashMap::new(),
            location: None,
        };

        let result = TuplePlexSpaceServiceImpl::convert_proto_tuple_to_internal(&proto_tuple);
        assert!(result.is_ok());
        let tuple = result.unwrap();
        assert_eq!(tuple.fields().len(), 2);
        assert_eq!(tuple.fields()[0], TupleField::Integer(1));
        assert_eq!(tuple.fields()[1], TupleField::String("test".to_string()));
    }

    #[test]
    fn test_convert_proto_tuple_empty_error() {
        let proto_tuple = ProtoTuple {
            id: String::new(),
            fields: vec![],
            timestamp: None,
            lease: None,
            metadata: std::collections::HashMap::new(),
            location: None,
        };

        let result = TuplePlexSpaceServiceImpl::convert_proto_tuple_to_internal(&proto_tuple);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.code(), Code::InvalidArgument);
        assert!(err.message().contains("at least one field"));
    }

    #[test]
    fn test_convert_proto_tuple_with_metadata() {
        let mut metadata = std::collections::HashMap::new();
        metadata.insert("key1".to_string(), prost_types::Value::default());

        let proto_tuple = ProtoTuple {
            id: String::new(),
            fields: vec![proto_int(42)],
            timestamp: None,
            lease: None,
            metadata,
            location: None,
        };

        let result = TuplePlexSpaceServiceImpl::convert_proto_tuple_to_internal(&proto_tuple);
        assert!(result.is_ok());
    }

    #[test]
    fn test_convert_proto_tuple_with_lease() {
        let proto_tuple = ProtoTuple {
            id: String::new(),
            fields: vec![proto_int(42)],
            timestamp: None,
            lease: Some(plexspaces_proto::tuplespace::v1::Lease {
                ttl: Some(prost_types::Duration {
                    seconds: 60,
                    nanos: 0,
                }),
                owner: String::new(),
                renewable: false,
                expires_at: Some(prost_types::Timestamp {
                    seconds: chrono::Utc::now().timestamp() + 60,
                    nanos: 0,
                }),
            }),
            metadata: std::collections::HashMap::new(),
            location: None,
        };

        let result = TuplePlexSpaceServiceImpl::convert_proto_tuple_to_internal(&proto_tuple);
        assert!(result.is_ok());
        let tuple = result.unwrap();
        assert!(tuple.lease().is_some());
    }

    #[test]
    fn test_convert_proto_template_to_pattern_wildcard() {
        let proto_tuple = ProtoTuple {
            id: String::new(),
            fields: vec![proto_int(1), proto_wildcard()],
            timestamp: None,
            lease: None,
            metadata: std::collections::HashMap::new(),
            location: None,
        };

        let result = TuplePlexSpaceServiceImpl::convert_proto_template_to_pattern(&proto_tuple);
        assert!(result.is_ok());
    }

    #[test]
    fn test_convert_proto_template_all_types() {
        // Test pattern with all supported field types
        let proto_tuple = ProtoTuple {
            id: String::new(),
            fields: vec![
                proto_int(42),
                proto_float(3.14),
                proto_string("test"),
                proto_bool(true),
                proto_binary(vec![1, 2, 3]),
                proto_null(),
                proto_wildcard(),
            ],
            timestamp: None,
            lease: None,
            metadata: std::collections::HashMap::new(),
            location: None,
        };

        let result = TuplePlexSpaceServiceImpl::convert_proto_template_to_pattern(&proto_tuple);
        assert!(result.is_ok());
        // Pattern created successfully with all 7 field types
    }

    #[test]
    fn test_convert_proto_template_empty_error() {
        let proto_tuple = ProtoTuple {
            id: String::new(),
            fields: vec![],
            timestamp: None,
            lease: None,
            metadata: std::collections::HashMap::new(),
            location: None,
        };

        let result = TuplePlexSpaceServiceImpl::convert_proto_template_to_pattern(&proto_tuple);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.code(), Code::InvalidArgument);
    }

    #[test]
    fn test_convert_proto_template_none_field_error() {
        let proto_tuple = ProtoTuple {
            id: String::new(),
            fields: vec![proto_int(1), proto_none()],
            timestamp: None,
            lease: None,
            metadata: std::collections::HashMap::new(),
            location: None,
        };

        let result = TuplePlexSpaceServiceImpl::convert_proto_template_to_pattern(&proto_tuple);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.code(), Code::InvalidArgument);
        assert!(err.message().contains("must have a value or wildcard"));
    }

    #[test]
    fn test_convert_internal_field_to_proto_integer() {
        let field = TupleField::Integer(42);
        let proto = TuplePlexSpaceServiceImpl::convert_internal_field_to_proto(&field);
        assert_eq!(proto.value, Some(ProtoValue::Integer(42)));
    }

    #[test]
    fn test_convert_internal_field_to_proto_float() {
        let field = TupleField::Float(OrderedFloat::new(3.14));
        let proto = TuplePlexSpaceServiceImpl::convert_internal_field_to_proto(&field);
        assert_eq!(proto.value, Some(ProtoValue::Float(3.14)));
    }

    #[test]
    fn test_convert_internal_field_to_proto_string() {
        let field = TupleField::String("test".to_string());
        let proto = TuplePlexSpaceServiceImpl::convert_internal_field_to_proto(&field);
        assert_eq!(proto.value, Some(ProtoValue::String("test".to_string())));
    }

    #[test]
    fn test_convert_internal_field_to_proto_boolean() {
        let field = TupleField::Boolean(true);
        let proto = TuplePlexSpaceServiceImpl::convert_internal_field_to_proto(&field);
        assert_eq!(proto.value, Some(ProtoValue::Boolean(true)));
    }

    #[test]
    fn test_convert_internal_field_to_proto_binary() {
        let field = TupleField::Binary(vec![1, 2, 3]);
        let proto = TuplePlexSpaceServiceImpl::convert_internal_field_to_proto(&field);
        assert_eq!(proto.value, Some(ProtoValue::Binary(vec![1, 2, 3])));
    }

    #[test]
    fn test_convert_internal_field_to_proto_null() {
        let field = TupleField::Null;
        let proto = TuplePlexSpaceServiceImpl::convert_internal_field_to_proto(&field);
        assert_eq!(proto.value, Some(ProtoValue::Null(true)));
    }

    #[test]
    fn test_convert_internal_tuple_to_proto() {
        let tuple = Tuple::new(vec![
            TupleField::Integer(1),
            TupleField::String("test".to_string()),
        ]);

        let proto = TuplePlexSpaceServiceImpl::convert_internal_tuple_to_proto(&tuple);
        assert_eq!(proto.fields.len(), 2);
        assert_eq!(proto.fields[0].value, Some(ProtoValue::Integer(1)));
        assert_eq!(
            proto.fields[1].value,
            Some(ProtoValue::String("test".to_string()))
        );
        assert!(!proto.id.is_empty());
        assert!(proto.timestamp.is_some());
    }

    #[test]
    fn test_tuplespace_error_to_status_not_found() {
        let err = TupleSpaceError::NotFound;
        let status = TuplePlexSpaceServiceImpl::tuplespace_error_to_status(err);
        assert_eq!(status.code(), Code::NotFound);
    }

    #[test]
    fn test_tuplespace_error_to_status_pattern_error() {
        let err = TupleSpaceError::PatternError("invalid pattern".to_string());
        let status = TuplePlexSpaceServiceImpl::tuplespace_error_to_status(err);
        assert_eq!(status.code(), Code::InvalidArgument);
        assert!(status.message().contains("Pattern error"));
    }

    #[test]
    fn test_tuplespace_error_to_status_lease_error() {
        let err = TupleSpaceError::LeaseError("expired".to_string());
        let status = TuplePlexSpaceServiceImpl::tuplespace_error_to_status(err);
        assert_eq!(status.code(), Code::FailedPrecondition);
        assert!(status.message().contains("Lease error"));
    }

    #[test]
    fn test_tuplespace_error_to_status_invalid_configuration() {
        let err = TupleSpaceError::InvalidConfiguration("bad config".to_string());
        let status = TuplePlexSpaceServiceImpl::tuplespace_error_to_status(err);
        assert_eq!(status.code(), Code::InvalidArgument);
        assert!(status.message().contains("Invalid configuration"));
    }

    #[test]
    fn test_tuplespace_error_to_status_not_implemented() {
        let err = TupleSpaceError::NotImplemented("future feature".to_string());
        let status = TuplePlexSpaceServiceImpl::tuplespace_error_to_status(err);
        assert_eq!(status.code(), Code::Unimplemented);
    }

    #[test]
    fn test_tuplespace_error_to_status_other() {
        let err = TupleSpaceError::IoError("disk error".to_string());
        let status = TuplePlexSpaceServiceImpl::tuplespace_error_to_status(err);
        assert_eq!(status.code(), Code::Internal);
    }

    // Integration tests for implemented RPC methods

    #[tokio::test]
    async fn test_write_and_read_single_tuple() {
        use crate::NodeBuilder;
        let node = Arc::new(NodeBuilder::new("test-write-read").build());
        let service = TuplePlexSpaceServiceImpl::new(node);

        // Write a tuple
        let write_req = Request::new(WriteRequest {
            tuples: vec![ProtoTuple {
                id: String::new(),
                fields: vec![proto_int(42), proto_string("test")],
                timestamp: None,
                lease: None,
                metadata: std::collections::HashMap::new(),
                location: None,
            }],
            transaction_id: String::new(),
        });

        let write_resp = service.write(write_req).await;
        assert!(write_resp.is_ok());
        let write_result = write_resp.unwrap().into_inner();
        assert_eq!(write_result.tuple_ids.len(), 1);

        // Read the tuple back
        let read_req = Request::new(ReadRequest {
            template: Some(ProtoTuple {
                id: String::new(),
                fields: vec![proto_int(42), proto_wildcard()],
                timestamp: None,
                lease: None,
                metadata: std::collections::HashMap::new(),
                location: None,
            }),
            timeout: None,
            blocking: false,
            take: false,
            max_results: 1,
            transaction_id: String::new(),
            spatial_filter: None,
        });

        let read_resp = service.read(read_req).await;
        assert!(read_resp.is_ok());
        let read_result = read_resp.unwrap().into_inner();
        assert_eq!(read_result.tuples.len(), 1);
        assert_eq!(
            read_result.tuples[0].fields[0].value,
            Some(ProtoValue::Integer(42))
        );
        assert_eq!(
            read_result.tuples[0].fields[1].value,
            Some(ProtoValue::String("test".to_string()))
        );
    }

    #[tokio::test]
    async fn test_write_multiple_tuples() {
        use crate::NodeBuilder;
        let node = Arc::new(NodeBuilder::new("test-write-multiple").build());
        let service = TuplePlexSpaceServiceImpl::new(node);

        // Write multiple tuples
        let write_req = Request::new(WriteRequest {
            tuples: vec![
                ProtoTuple {
                    id: String::new(),
                    fields: vec![proto_string("sensor"), proto_int(1), proto_float(23.5)],
                    timestamp: None,
                    lease: None,
                    metadata: std::collections::HashMap::new(),
                    location: None,
                },
                ProtoTuple {
                    id: String::new(),
                    fields: vec![proto_string("sensor"), proto_int(2), proto_float(24.1)],
                    timestamp: None,
                    lease: None,
                    metadata: std::collections::HashMap::new(),
                    location: None,
                },
                ProtoTuple {
                    id: String::new(),
                    fields: vec![proto_string("sensor"), proto_int(3), proto_float(22.8)],
                    timestamp: None,
                    lease: None,
                    metadata: std::collections::HashMap::new(),
                    location: None,
                },
            ],
            transaction_id: String::new(),
        });

        let write_resp = service.write(write_req).await;
        assert!(write_resp.is_ok());
        let write_result = write_resp.unwrap().into_inner();
        assert_eq!(write_result.tuple_ids.len(), 3);
    }

    #[tokio::test]
    async fn test_read_with_pattern_matching() {
        use crate::NodeBuilder;
        let node = Arc::new(NodeBuilder::new("test-pattern-match").build());
        let service = TuplePlexSpaceServiceImpl::new(node);

        // Write tuples
        let write_req = Request::new(WriteRequest {
            tuples: vec![
                ProtoTuple {
                    id: String::new(),
                    fields: vec![proto_string("sensor"), proto_int(1), proto_float(23.5)],
                    timestamp: None,
                    lease: None,
                    metadata: std::collections::HashMap::new(),
                    location: None,
                },
                ProtoTuple {
                    id: String::new(),
                    fields: vec![proto_string("sensor"), proto_int(2), proto_float(24.1)],
                    timestamp: None,
                    lease: None,
                    metadata: std::collections::HashMap::new(),
                    location: None,
                },
                ProtoTuple {
                    id: String::new(),
                    fields: vec![proto_string("actuator"), proto_int(1), proto_bool(true)],
                    timestamp: None,
                    lease: None,
                    metadata: std::collections::HashMap::new(),
                    location: None,
                },
            ],
            transaction_id: String::new(),
        });
        service.write(write_req).await.unwrap();

        // Read with wildcard pattern - should match sensors only
        let read_req = Request::new(ReadRequest {
            template: Some(ProtoTuple {
                id: String::new(),
                fields: vec![proto_string("sensor"), proto_wildcard(), proto_wildcard()],
                timestamp: None,
                lease: None,
                metadata: std::collections::HashMap::new(),
                location: None,
            }),
            timeout: None,
            blocking: false,
            take: false,
            max_results: 10,
            transaction_id: String::new(),
            spatial_filter: None,
        });

        let read_resp = service.read(read_req).await;
        assert!(read_resp.is_ok());
        let read_result = read_resp.unwrap().into_inner();
        assert_eq!(read_result.tuples.len(), 2); // Only sensor tuples
    }

    #[tokio::test]
    async fn test_take_removes_tuple() {
        use crate::NodeBuilder;
        let node = Arc::new(NodeBuilder::new("test-take").build());
        let service = TuplePlexSpaceServiceImpl::new(node);

        // Write a tuple
        let write_req = Request::new(WriteRequest {
            tuples: vec![ProtoTuple {
                id: String::new(),
                fields: vec![proto_string("task"), proto_int(1)],
                timestamp: None,
                lease: None,
                metadata: std::collections::HashMap::new(),
                location: None,
            }],
            transaction_id: String::new(),
        });
        service.write(write_req).await.unwrap();

        // Take the tuple (destructive read)
        let take_req = Request::new(ReadRequest {
            template: Some(ProtoTuple {
                id: String::new(),
                fields: vec![proto_string("task"), proto_wildcard()],
                timestamp: None,
                lease: None,
                metadata: std::collections::HashMap::new(),
                location: None,
            }),
            timeout: None,
            blocking: false,
            take: false,
            max_results: 1,
            transaction_id: String::new(),
            spatial_filter: None,
        });

        let take_resp = service.take(take_req).await;
        assert!(take_resp.is_ok());
        let take_result = take_resp.unwrap().into_inner();
        assert_eq!(take_result.tuples.len(), 1);

        // Try to read again - should be empty
        let read_req = Request::new(ReadRequest {
            template: Some(ProtoTuple {
                id: String::new(),
                fields: vec![proto_string("task"), proto_wildcard()],
                timestamp: None,
                lease: None,
                metadata: std::collections::HashMap::new(),
                location: None,
            }),
            timeout: None,
            blocking: false,
            take: false,
            max_results: 1,
            transaction_id: String::new(),
            spatial_filter: None,
        });

        let read_resp = service.read(read_req).await;
        assert!(read_resp.is_ok());
        let read_result = read_resp.unwrap().into_inner();
        assert_eq!(read_result.tuples.len(), 0); // Tuple was removed by take
    }

    #[tokio::test]
    async fn test_count_matching_tuples() {
        use crate::NodeBuilder;
        let node = Arc::new(NodeBuilder::new("test-count").build());
        let service = TuplePlexSpaceServiceImpl::new(node);

        // Write tuples
        let write_req = Request::new(WriteRequest {
            tuples: vec![
                ProtoTuple {
                    id: String::new(),
                    fields: vec![proto_string("event"), proto_string("login")],
                    timestamp: None,
                    lease: None,
                    metadata: std::collections::HashMap::new(),
                    location: None,
                },
                ProtoTuple {
                    id: String::new(),
                    fields: vec![proto_string("event"), proto_string("logout")],
                    timestamp: None,
                    lease: None,
                    metadata: std::collections::HashMap::new(),
                    location: None,
                },
                ProtoTuple {
                    id: String::new(),
                    fields: vec![proto_string("event"), proto_string("login")],
                    timestamp: None,
                    lease: None,
                    metadata: std::collections::HashMap::new(),
                    location: None,
                },
            ],
            transaction_id: String::new(),
        });
        service.write(write_req).await.unwrap();

        // Count all events
        let count_req = Request::new(CountRequest {
            template: Some(ProtoTuple {
                id: String::new(),
                fields: vec![proto_string("event"), proto_wildcard()],
                timestamp: None,
                lease: None,
                metadata: std::collections::HashMap::new(),
                location: None,
            }),
            transaction_id: String::new(),
            spatial_filter: None,
        });

        let count_resp = service.count(count_req).await;
        assert!(count_resp.is_ok());
        let count_result = count_resp.unwrap().into_inner();
        assert_eq!(count_result.count, 3);
    }

    #[tokio::test]
    async fn test_exists_check() {
        use crate::NodeBuilder;
        let node = Arc::new(NodeBuilder::new("test-exists").build());
        let service = TuplePlexSpaceServiceImpl::new(node);

        // Check exists before write - should be false
        let exists_req = Request::new(ExistsRequest {
            template: Some(ProtoTuple {
                id: String::new(),
                fields: vec![proto_string("config"), proto_string("timeout")],
                timestamp: None,
                lease: None,
                metadata: std::collections::HashMap::new(),
                location: None,
            }),
            transaction_id: String::new(),
        });

        let exists_resp = service.exists(exists_req).await;
        assert!(exists_resp.is_ok());
        assert!(!exists_resp.unwrap().into_inner().exists);

        // Write tuple
        let write_req = Request::new(WriteRequest {
            tuples: vec![ProtoTuple {
                id: String::new(),
                fields: vec![proto_string("config"), proto_string("timeout")],
                timestamp: None,
                lease: None,
                metadata: std::collections::HashMap::new(),
                location: None,
            }],
            transaction_id: String::new(),
        });
        service.write(write_req).await.unwrap();

        // Check exists after write - should be true
        let exists_req = Request::new(ExistsRequest {
            template: Some(ProtoTuple {
                id: String::new(),
                fields: vec![proto_string("config"), proto_wildcard()],
                timestamp: None,
                lease: None,
                metadata: std::collections::HashMap::new(),
                location: None,
            }),
            transaction_id: String::new(),
        });

        let exists_resp = service.exists(exists_req).await;
        assert!(exists_resp.is_ok());
        assert!(exists_resp.unwrap().into_inner().exists);
    }

    #[tokio::test]
    async fn test_clear_all_tuples() {
        use crate::NodeBuilder;
        let node = Arc::new(NodeBuilder::new("test-clear").build());
        let service = TuplePlexSpaceServiceImpl::new(node);

        // Write tuples
        let write_req = Request::new(WriteRequest {
            tuples: vec![
                ProtoTuple {
                    id: String::new(),
                    fields: vec![proto_int(1)],
                    timestamp: None,
                    lease: None,
                    metadata: std::collections::HashMap::new(),
                    location: None,
                },
                ProtoTuple {
                    id: String::new(),
                    fields: vec![proto_int(2)],
                    timestamp: None,
                    lease: None,
                    metadata: std::collections::HashMap::new(),
                    location: None,
                },
            ],
            transaction_id: String::new(),
        });
        service.write(write_req).await.unwrap();

        // Clear all
        let clear_req = Request::new(Empty {});
        let clear_resp = service.clear(clear_req).await;
        assert!(clear_resp.is_ok());

        // Verify empty
        let count_req = Request::new(CountRequest {
            template: Some(ProtoTuple {
                id: String::new(),
                fields: vec![proto_wildcard()],
                timestamp: None,
                lease: None,
                metadata: std::collections::HashMap::new(),
                location: None,
            }),
            transaction_id: String::new(),
            spatial_filter: None,
        });

        let count_resp = service.count(count_req).await;
        assert!(count_resp.is_ok());
        assert_eq!(count_resp.unwrap().into_inner().count, 0);
    }

    #[tokio::test]
    async fn test_get_stats() {
        use crate::NodeBuilder;
        let node = Arc::new(NodeBuilder::new("test-stats").build());
        let service = TuplePlexSpaceServiceImpl::new(node);

        // Get initial stats
        let stats_req = Request::new(Empty {});
        let stats_resp = service.get_stats(stats_req).await;
        assert!(stats_resp.is_ok());
        let stats_result = stats_resp.unwrap().into_inner();
        assert!(stats_result.stats.is_some());

        let initial_stats = stats_result.stats.unwrap();
        assert_eq!(initial_stats.tuple_count, 0);
        assert_eq!(initial_stats.write_operations, 0);
        assert_eq!(initial_stats.read_operations, 0);
        assert_eq!(initial_stats.take_operations, 0);

        // Write some tuples
        let write_req = Request::new(WriteRequest {
            tuples: vec![
                ProtoTuple {
                    id: String::new(),
                    fields: vec![proto_int(1)],
                    timestamp: None,
                    lease: None,
                    metadata: std::collections::HashMap::new(),
                    location: None,
                },
                ProtoTuple {
                    id: String::new(),
                    fields: vec![proto_int(2)],
                    timestamp: None,
                    lease: None,
                    metadata: std::collections::HashMap::new(),
                    location: None,
                },
            ],
            transaction_id: String::new(),
        });
        service.write(write_req).await.unwrap();

        // Read a tuple
        let read_req = Request::new(ReadRequest {
            template: Some(ProtoTuple {
                id: String::new(),
                fields: vec![proto_wildcard()],
                timestamp: None,
                lease: None,
                metadata: std::collections::HashMap::new(),
                location: None,
            }),
            timeout: None,
            blocking: false,
            take: false,
            max_results: 1,
            transaction_id: String::new(),
            spatial_filter: None,
        });
        service.read(read_req).await.unwrap();

        // Get stats after operations
        let stats_req = Request::new(Empty {});
        let stats_resp = service.get_stats(stats_req).await;
        assert!(stats_resp.is_ok());
        let stats_result = stats_resp.unwrap().into_inner();
        assert!(stats_result.stats.is_some());

        let updated_stats = stats_result.stats.unwrap();
        assert_eq!(updated_stats.tuple_count, 2); // 2 tuples written
        assert_eq!(updated_stats.write_operations, 2); // 2 writes
        assert_eq!(updated_stats.read_operations, 1); // 1 read
        assert!(updated_stats.total_operations >= 3); // At least write + read ops
    }

    #[tokio::test]
    async fn test_write_validation_empty_tuples() {
        use crate::NodeBuilder;
        let node = Arc::new(NodeBuilder::new("test-validation").build());
        let service = TuplePlexSpaceServiceImpl::new(node);

        // Try to write empty tuples array
        let write_req = Request::new(WriteRequest {
            tuples: vec![],
            transaction_id: String::new(),
        });

        let write_resp = service.write(write_req).await;
        assert!(write_resp.is_err());
        assert_eq!(write_resp.unwrap_err().code(), Code::InvalidArgument);
    }

    #[tokio::test]
    async fn test_read_validation_missing_template() {
        use crate::NodeBuilder;
        let node = Arc::new(NodeBuilder::new("test-validation-read").build());
        let service = TuplePlexSpaceServiceImpl::new(node);

        // Try to read without template
        let read_req = Request::new(ReadRequest {
            template: None,
            timeout: None,
            blocking: false,
            take: false,
            max_results: 1,
            transaction_id: String::new(),
            spatial_filter: None,
        });

        let read_resp = service.read(read_req).await;
        assert!(read_resp.is_err());
        assert_eq!(read_resp.unwrap_err().code(), Code::InvalidArgument);
    }

    #[tokio::test]
    async fn test_unimplemented_methods() {
        use crate::NodeBuilder;
        let node = Arc::new(NodeBuilder::new("test-unimplemented").build());
        let service = TuplePlexSpaceServiceImpl::new(node);

        // Test subscribe (unimplemented)
        let subscribe_req = Request::new(plexspaces_proto::tuplespace::v1::SubscribeRequest {
            template: None,
            qos: 0,
            actions: 0,
            callback_url: String::new(),
        });
        let result = service.subscribe(subscribe_req).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code(), Code::Unimplemented);

        // Test unsubscribe (unimplemented)
        let unsubscribe_req = Request::new(plexspaces_proto::tuplespace::v1::UnsubscribeRequest {
            subscription_id: String::new(),
        });
        let result = service.unsubscribe(unsubscribe_req).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code(), Code::Unimplemented);

        // Test begin_transaction (unimplemented)
        let begin_req = Request::new(plexspaces_proto::tuplespace::v1::BeginTransactionRequest {
            isolation_level: 0,
            timeout: None,
        });
        let result = service.begin_transaction(begin_req).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code(), Code::Unimplemented);

        // Test commit_transaction (unimplemented)
        let commit_req = Request::new(plexspaces_proto::tuplespace::v1::CommitTransactionRequest {
            transaction_id: String::new(),
        });
        let result = service.commit_transaction(commit_req).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code(), Code::Unimplemented);

        // Test abort_transaction (unimplemented)
        let abort_req = Request::new(plexspaces_proto::tuplespace::v1::AbortTransactionRequest {
            transaction_id: String::new(),
        });
        let result = service.abort_transaction(abort_req).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code(), Code::Unimplemented);

        // Test renew_lease (unimplemented)
        let renew_req = Request::new(plexspaces_proto::tuplespace::v1::RenewLeaseRequest {
            tuple_id: String::new(),
            new_ttl: None,
        });
        let result = service.renew_lease(renew_req).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code(), Code::Unimplemented);
    }
}
