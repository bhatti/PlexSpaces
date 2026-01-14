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

//! TupleSpace gRPC Service Implementation
//!
//! ## Purpose
//! Implements the gRPC TuplePlexSpaceService that delegates to a TupleSpaceProvider.
//! This allows any TupleSpace implementation (InMemory, Redis, SQLite, etc.) to be
//! exposed as a gRPC service for remote access.
//!
//! ## Architecture
//! ```text
//! ┌─────────────────────────────────────┐
//! │   gRPC Client                       │
//! │   (Remote Actor/Service)            │
//! └────────┬────────────────────────────┘
//!          │ gRPC/HTTP2
//!     ┌────▼──────────────────────────┐
//!     │  TuplePlexSpaceServiceImpl    │
//!     │  (This crate)                 │
//!     └────────┬──────────────────────┘
//!              │ Delegates to
//!         ┌────▼─────────────────┐
//!         │  TupleSpaceProvider  │ (trait)
//!         └────────┬─────────────┘
//!                  │
//!    ┌─────────────┼─────────────┐
//!    │             │             │
//! ┌──▼───┐   ┌────▼────┐   ┌───▼────┐
//! │Memory│   │  Redis  │   │ SQLite │
//! └──────┘   └─────────┘   └────────┘
//! ```
//!
//! ## Design Principles
//! - **Provider Delegation**: Service doesn't implement logic, just translates gRPC ↔ Rust
//! - **Multi-Tenancy**: Tenant/namespace from provider ensures isolation
//! - **Proto-First**: All types from protobuf definitions
//! - **Stateless Service**: All state in provider, service is just a facade

#![warn(missing_docs)]
#![warn(clippy::all)]

use async_trait::async_trait;
use chrono::Utc;
use plexspaces_proto::common::v1::Empty;
use plexspaces_proto::tuplespace::v1::{
    tuple_field::Value as ProtoValue, tuple_plex_space_service_server::TuplePlexSpaceService,
    AbortTransactionRequest, AbortTransactionResponse, BeginTransactionRequest,
    BeginTransactionResponse, CommitTransactionRequest, CommitTransactionResponse, CountRequest,
    CountResponse, ExistsRequest, ExistsResponse, GetStatsResponse, ReadRequest, ReadResponse,
    RenewLeaseRequest, RenewLeaseResponse, SubscribeRequest, SubscribeResponse,
    Tuple as ProtoTuple, TupleField as ProtoTupleField, UnsubscribeRequest, WriteRequest,
    WriteResponse,
};
use plexspaces_tuplespace::{
    Lease, OrderedFloat, Pattern, PatternField, Tuple, TupleField, TupleSpaceError,
};
use plexspaces_tuplespace::provider::TupleSpaceProvider;
use std::sync::Arc;
use tonic::{Request, Response, Status};

/// TupleSpace gRPC service implementation that delegates to a provider
///
/// ## Purpose
/// Wraps any TupleSpaceProvider and exposes it as a gRPC service.
///
/// ## Example
/// ```rust,no_run
/// use plexspaces_tuplespace::TupleSpace;
/// use plexspaces_tuplespace_service::TuplePlexSpaceServiceImpl;
/// use std::sync::Arc;
///
/// # async fn example() {
/// // Create a TupleSpace provider (InMemory)
/// let provider = Arc::new(TupleSpace::with_tenant_namespace("acme-corp", "production"));
///
/// // Wrap it in a gRPC service
/// let service = TuplePlexSpaceServiceImpl::new(provider);
///
/// // Serve via tonic
/// // Server::builder()
/// //     .add_service(TuplePlexSpaceServiceServer::new(service))
/// //     .serve(addr).await.unwrap();
/// # }
/// ```
pub struct TuplePlexSpaceServiceImpl<P: TupleSpaceProvider + 'static> {
    /// The underlying TupleSpace provider
    provider: Arc<P>,
}

impl<P: TupleSpaceProvider> TuplePlexSpaceServiceImpl<P> {
    /// Create a new TupleSpace gRPC service
    ///
    /// ## Arguments
    /// * `provider` - Any implementation of TupleSpaceProvider
    ///
    /// ## Returns
    /// gRPC service ready to be registered with tonic Server
    pub fn new(provider: Arc<P>) -> Self {
        Self { provider }
    }

    /// Get the underlying provider (for testing)
    pub fn provider(&self) -> &Arc<P> {
        &self.provider
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
        for (key, value) in &proto_tuple.metadata {
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
        use prost_types::{Duration as ProtoDuration, Timestamp};

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
        let proto_metadata = std::collections::HashMap::new();

        ProtoTuple {
            id: ulid::Ulid::new().to_string(),
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
            TupleSpaceError::NotSupported(msg) => Status::unimplemented(msg),
            _ => Status::internal(format!("TupleSpace error: {}", error)),
        }
    }
}

#[async_trait]
impl<P: TupleSpaceProvider> TuplePlexSpaceService for TuplePlexSpaceServiceImpl<P> {
    async fn write(
        &self,
        request: Request<WriteRequest>,
    ) -> Result<Response<WriteResponse>, Status> {
        let req = request.into_inner();

        // Validate request
        if req.tuples.is_empty() {
            return Err(Status::invalid_argument("At least one tuple required"));
        }

        let mut tuple_ids = Vec::new();

        // Write each tuple
        for proto_tuple in req.tuples {
            let tuple = Self::convert_proto_tuple_to_internal(&proto_tuple)?;

            // Write to provider
            self.provider
                .write(tuple)
                .await
                .map_err(Self::tuplespace_error_to_status)?;

            // Generate ULID for tuple
            let tuple_id = ulid::Ulid::new().to_string();
            tuple_ids.push(tuple_id);
        }

        Ok(Response::new(WriteResponse { tuple_ids }))
    }

    async fn read(&self, request: Request<ReadRequest>) -> Result<Response<ReadResponse>, Status> {
        let req = request.into_inner();

        // Validate template
        let proto_template = req
            .template
            .ok_or_else(|| Status::invalid_argument("Template is required for read operation"))?;

        // Convert template to pattern
        let pattern = Self::convert_proto_template_to_pattern(&proto_template)?;

        // Read from provider (returns Vec<Tuple>)
        let all_tuples = self
            .provider
            .read(&pattern)
            .await
            .map_err(Self::tuplespace_error_to_status)?;

        // Implement pagination: take only max_results, check if more exist
        let max_results = req.max_results.max(1) as usize;
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

    async fn take(&self, request: Request<ReadRequest>) -> Result<Response<ReadResponse>, Status> {
        let req = request.into_inner();

        // Validate template
        let proto_template = req
            .template
            .ok_or_else(|| Status::invalid_argument("Template is required for take operation"))?;

        // Convert template to pattern
        let pattern = Self::convert_proto_template_to_pattern(&proto_template)?;

        // Take from provider (returns Option<Tuple>)
        let max_results = req.max_results.max(1) as usize;

        let mut tuples = Vec::new();
        for _ in 0..max_results {
            if let Some(tuple) = self
                .provider
                .take(&pattern)
                .await
                .map_err(Self::tuplespace_error_to_status)?
            {
                tuples.push(tuple);
            } else {
                break;
            }
        }

        // Convert to proto
        let proto_tuples: Vec<ProtoTuple> = tuples
            .iter()
            .map(|t| Self::convert_internal_tuple_to_proto(t))
            .collect();

        Ok(Response::new(ReadResponse {
            tuples: proto_tuples,
            has_more: false, // Can't determine if more exist without trying
        }))
    }

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
            .provider
            .count(&pattern)
            .await
            .map_err(Self::tuplespace_error_to_status)?;

        Ok(Response::new(CountResponse {
            count: count as i64,
        }))
    }

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

        // Check existence using count
        let count = self
            .provider
            .count(&pattern)
            .await
            .map_err(Self::tuplespace_error_to_status)?;

        Ok(Response::new(ExistsResponse {
            exists: count > 0,
        }))
    }

    async fn subscribe(
        &self,
        _request: Request<SubscribeRequest>,
    ) -> Result<Response<SubscribeResponse>, Status> {
        // TODO: Implement subscribe delegation to provider (Phase 3 Week 6 - Distributed Watchers)
        Err(Status::unimplemented(
            "Subscribe not yet implemented - Phase 3 Week 6",
        ))
    }

    async fn unsubscribe(
        &self,
        _request: Request<UnsubscribeRequest>,
    ) -> Result<Response<Empty>, Status> {
        // TODO: Implement unsubscribe delegation to provider (Phase 3 Week 6 - Distributed Watchers)
        Err(Status::unimplemented(
            "Unsubscribe not yet implemented - Phase 3 Week 6",
        ))
    }

    async fn begin_transaction(
        &self,
        _request: Request<BeginTransactionRequest>,
    ) -> Result<Response<BeginTransactionResponse>, Status> {
        // TODO: Implement transaction support (post-Phase 3)
        Err(Status::unimplemented("Transactions not yet implemented - post-Phase 3"))
    }

    async fn commit_transaction(
        &self,
        _request: Request<CommitTransactionRequest>,
    ) -> Result<Response<CommitTransactionResponse>, Status> {
        // TODO: Implement transaction support (post-Phase 3)
        Err(Status::unimplemented("Transactions not yet implemented - post-Phase 3"))
    }

    async fn abort_transaction(
        &self,
        _request: Request<AbortTransactionRequest>,
    ) -> Result<Response<AbortTransactionResponse>, Status> {
        // TODO: Implement transaction support (post-Phase 3)
        Err(Status::unimplemented("Transactions not yet implemented - post-Phase 3"))
    }

    async fn clear(&self, _request: Request<Empty>) -> Result<Response<Empty>, Status> {
        // Clear all tuples from the provider
        self.provider
            .clear()
            .await
            .map_err(Self::tuplespace_error_to_status)?;

        Ok(Response::new(Empty {}))
    }

    async fn renew_lease(
        &self,
        _request: Request<RenewLeaseRequest>,
    ) -> Result<Response<RenewLeaseResponse>, Status> {
        // TODO: Implement lease renewal delegation to provider
        // Note: TupleSpaceProvider trait doesn't have renew_lease() method yet
        // This requires storing tuple IDs → tuples mapping or adding renew_lease() to the trait
        Err(Status::unimplemented(
            "Lease renewal not yet implemented - requires provider trait extension",
        ))
    }

    async fn get_stats(
        &self,
        _request: Request<Empty>,
    ) -> Result<Response<GetStatsResponse>, Status> {
        use plexspaces_proto::tuplespace::v1::StorageStats;

        // Get stats from provider
        let stats = self
            .provider
            .stats()
            .await
            .map_err(Self::tuplespace_error_to_status)?;

        // Convert internal stats to proto StorageStats
        let proto_stats = StorageStats {
            tuple_count: stats.current_size() as u64,
            memory_bytes: 0, // Not available from TupleSpaceStats
            total_operations: stats.total_writes() + stats.total_reads() + stats.total_takes(),
            read_operations: stats.total_reads(),
            write_operations: stats.total_writes(),
            take_operations: stats.total_takes(),
            avg_latency_ms: 0.0, // Not available from TupleSpaceStats
        };

        Ok(Response::new(GetStatsResponse {
            stats: Some(proto_stats),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plexspaces_tuplespace::TupleSpace;

    #[test]
    fn test_service_creation() {
        // Test creating service with in-memory provider
        let provider = Arc::new(TupleSpace::with_tenant_namespace("test-tenant", "test-ns"));
        let service = TuplePlexSpaceServiceImpl::new(provider.clone());

        // Verify provider is stored
        assert!(Arc::ptr_eq(service.provider(), &provider));
    }

    #[tokio::test]
    async fn test_clear() {
        // Create service
        let provider = Arc::new(TupleSpace::with_tenant_namespace("test", "test"));
        let service = TuplePlexSpaceServiceImpl::new(provider.clone());

        // Write some tuples
        let tuple1 = ProtoTuple {
            id: "1".to_string(),
            fields: vec![ProtoTupleField {
                value: Some(ProtoValue::String("test1".to_string())),
            }],
            timestamp: None,
            lease: None,
            metadata: std::collections::HashMap::new(),
            location: None,
        };
        let mut write_req = WriteRequest::default();
        write_req.tuples = vec![tuple1];
        let _ = service.write(Request::new(write_req)).await.unwrap();

        // Verify tuple exists
        let pattern = ProtoTuple {
            id: "".to_string(),
            fields: vec![ProtoTupleField {
                value: Some(ProtoValue::String("test1".to_string())),
            }],
            timestamp: None,
            lease: None,
            metadata: std::collections::HashMap::new(),
            location: None,
        };
        let mut read_req = ReadRequest::default();
        read_req.template = Some(pattern.clone());
        read_req.max_results = 10;
        let read_result = service.read(Request::new(read_req)).await.unwrap();
        assert!(!read_result.into_inner().tuples.is_empty());

        // Clear all tuples
        let clear_req = Request::new(Empty::default());
        let clear_result = service.clear(clear_req).await;
        assert!(clear_result.is_ok());

        // Verify tuples are gone
        let mut read_req2 = ReadRequest::default();
        read_req2.template = Some(pattern);
        read_req2.max_results = 10;
        let read_result2 = service.read(Request::new(read_req2)).await.unwrap();
        assert!(read_result2.into_inner().tuples.is_empty());
    }

    #[tokio::test]
    async fn test_get_stats() {
        // Create service
        let provider = Arc::new(TupleSpace::with_tenant_namespace("test", "test"));
        let service = TuplePlexSpaceServiceImpl::new(provider);

        // Get stats
        let stats_req = Request::new(Empty::default());
        let stats_result = service.get_stats(stats_req).await;
        assert!(stats_result.is_ok());

        let stats = stats_result.unwrap().into_inner();
        assert!(stats.stats.is_some());
        let storage_stats = stats.stats.unwrap();
        assert_eq!(storage_stats.tuple_count, 0); // Initially empty
        assert_eq!(storage_stats.total_operations, 0);
    }

    #[tokio::test]
    async fn test_get_stats_empty() {
        // Create service
        let provider = Arc::new(TupleSpace::with_tenant_namespace("test", "test"));
        let service = TuplePlexSpaceServiceImpl::new(provider);

        // Get stats for empty space
        let stats_req = Request::new(Empty::default());
        let result = service.get_stats(stats_req).await;
        assert!(result.is_ok());

        let response = result.unwrap().into_inner();
        assert!(response.stats.is_some());
        let stats = response.stats.unwrap();
        assert_eq!(stats.tuple_count, 0);
        assert_eq!(stats.total_operations, 0);
    }

    #[tokio::test]
    async fn test_get_stats_with_operations() {
        // Create service
        let provider = Arc::new(TupleSpace::with_tenant_namespace("test", "test"));
        let service = TuplePlexSpaceServiceImpl::new(provider.clone());

        // Write some tuples
        let tuple1 = ProtoTuple {
            id: "1".to_string(),
            fields: vec![ProtoTupleField {
                value: Some(ProtoValue::String("test1".to_string())),
            }],
            timestamp: None,
            lease: None,
            metadata: std::collections::HashMap::new(),
            location: None,
        };
        let mut write_req = WriteRequest::default();
        write_req.tuples = vec![tuple1];
        service.write(Request::new(write_req)).await.unwrap();

        // Read a tuple
        let pattern = ProtoTuple {
            id: "".to_string(),
            fields: vec![ProtoTupleField {
                value: Some(ProtoValue::String("test1".to_string())),
            }],
            timestamp: None,
            lease: None,
            metadata: std::collections::HashMap::new(),
            location: None,
        };
        let mut read_req = ReadRequest::default();
        read_req.template = Some(pattern);
        read_req.max_results = 10;
        service.read(Request::new(read_req)).await.unwrap();

        // Get stats
        let stats_req = Request::new(Empty::default());
        let result = service.get_stats(stats_req).await;
        assert!(result.is_ok());

        let response = result.unwrap().into_inner();
        assert!(response.stats.is_some());
        let stats = response.stats.unwrap();
        assert!(stats.total_operations > 0);
    }

    #[tokio::test]
    async fn test_unimplemented_methods() {
        // Create service
        let provider = Arc::new(TupleSpace::with_tenant_namespace("test", "test"));
        let service = TuplePlexSpaceServiceImpl::new(provider);

        // Test that unimplemented methods return Unimplemented status
        // Note: write(), read(), take(), count(), exists(), clear(), and get_stats() are now implemented
        // Only subscribe, unsubscribe, transactions, and renew_lease are still unimplemented

        let subscribe_req = Request::new(SubscribeRequest::default());
        let result = service.subscribe(subscribe_req).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code(), tonic::Code::Unimplemented);

        let unsubscribe_req = Request::new(UnsubscribeRequest::default());
        let result = service.unsubscribe(unsubscribe_req).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code(), tonic::Code::Unimplemented);

        let begin_tx_req = Request::new(BeginTransactionRequest::default());
        let result = service.begin_transaction(begin_tx_req).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code(), tonic::Code::Unimplemented);

        let commit_tx_req = Request::new(CommitTransactionRequest::default());
        let result = service.commit_transaction(commit_tx_req).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code(), tonic::Code::Unimplemented);

        let abort_tx_req = Request::new(AbortTransactionRequest::default());
        let result = service.abort_transaction(abort_tx_req).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code(), tonic::Code::Unimplemented);

        let renew_lease_req = Request::new(RenewLeaseRequest {
            tuple_id: "test-id".to_string(),
            new_ttl: None,
        });
        let result = service.renew_lease(renew_lease_req).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code(), tonic::Code::Unimplemented);
    }
}
