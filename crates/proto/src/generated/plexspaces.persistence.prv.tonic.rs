// @generated
/// Generated client implementations.
pub mod persistence_service_client {
    #![allow(unused_variables, dead_code, missing_docs, clippy::let_unit_value)]
    use tonic::codegen::*;
    use tonic::codegen::http::Uri;
    /** Persistence service
*/
    #[derive(Debug, Clone)]
    pub struct PersistenceServiceClient<T> {
        inner: tonic::client::Grpc<T>,
    }
    impl PersistenceServiceClient<tonic::transport::Channel> {
        /// Attempt to create a new client by connecting to a given endpoint.
        pub async fn connect<D>(dst: D) -> Result<Self, tonic::transport::Error>
        where
            D: TryInto<tonic::transport::Endpoint>,
            D::Error: Into<StdError>,
        {
            let conn = tonic::transport::Endpoint::new(dst)?.connect().await?;
            Ok(Self::new(conn))
        }
    }
    impl<T> PersistenceServiceClient<T>
    where
        T: tonic::client::GrpcService<tonic::body::BoxBody>,
        T::Error: Into<StdError>,
        T::ResponseBody: Body<Data = Bytes> + Send + 'static,
        <T::ResponseBody as Body>::Error: Into<StdError> + Send,
    {
        pub fn new(inner: T) -> Self {
            let inner = tonic::client::Grpc::new(inner);
            Self { inner }
        }
        pub fn with_origin(inner: T, origin: Uri) -> Self {
            let inner = tonic::client::Grpc::with_origin(inner, origin);
            Self { inner }
        }
        pub fn with_interceptor<F>(
            inner: T,
            interceptor: F,
        ) -> PersistenceServiceClient<InterceptedService<T, F>>
        where
            F: tonic::service::Interceptor,
            T::ResponseBody: Default,
            T: tonic::codegen::Service<
                http::Request<tonic::body::BoxBody>,
                Response = http::Response<
                    <T as tonic::client::GrpcService<tonic::body::BoxBody>>::ResponseBody,
                >,
            >,
            <T as tonic::codegen::Service<
                http::Request<tonic::body::BoxBody>,
            >>::Error: Into<StdError> + Send + Sync,
        {
            PersistenceServiceClient::new(InterceptedService::new(inner, interceptor))
        }
        /// Compress requests with the given encoding.
        ///
        /// This requires the server to support it otherwise it might respond with an
        /// error.
        #[must_use]
        pub fn send_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.inner = self.inner.send_compressed(encoding);
            self
        }
        /// Enable decompressing responses.
        #[must_use]
        pub fn accept_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.inner = self.inner.accept_compressed(encoding);
            self
        }
        /// Limits the maximum size of a decoded message.
        ///
        /// Default: `4MB`
        #[must_use]
        pub fn max_decoding_message_size(mut self, limit: usize) -> Self {
            self.inner = self.inner.max_decoding_message_size(limit);
            self
        }
        /// Limits the maximum size of an encoded message.
        ///
        /// Default: `usize::MAX`
        #[must_use]
        pub fn max_encoding_message_size(mut self, limit: usize) -> Self {
            self.inner = self.inner.max_encoding_message_size(limit);
            self
        }
        pub async fn append_events(
            &mut self,
            request: impl tonic::IntoRequest<super::AppendEventsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::AppendEventsResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/plexspaces.persistence.prv.PersistenceService/AppendEvents",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.persistence.prv.PersistenceService",
                        "AppendEvents",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Read events from journal
*/
        pub async fn read_events(
            &mut self,
            request: impl tonic::IntoRequest<super::ReadEventsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ReadEventsResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/plexspaces.persistence.prv.PersistenceService/ReadEvents",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.persistence.prv.PersistenceService",
                        "ReadEvents",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Save state snapshot
*/
        pub async fn save_snapshot(
            &mut self,
            request: impl tonic::IntoRequest<super::SaveSnapshotRequest>,
        ) -> std::result::Result<
            tonic::Response<super::SaveSnapshotResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/plexspaces.persistence.prv.PersistenceService/SaveSnapshot",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.persistence.prv.PersistenceService",
                        "SaveSnapshot",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Load state snapshot
*/
        pub async fn load_snapshot(
            &mut self,
            request: impl tonic::IntoRequest<super::LoadSnapshotRequest>,
        ) -> std::result::Result<
            tonic::Response<super::LoadSnapshotResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/plexspaces.persistence.prv.PersistenceService/LoadSnapshot",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.persistence.prv.PersistenceService",
                        "LoadSnapshot",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Create checkpoint
*/
        pub async fn create_checkpoint(
            &mut self,
            request: impl tonic::IntoRequest<super::CreateCheckpointRequest>,
        ) -> std::result::Result<
            tonic::Response<super::CreateCheckpointResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/plexspaces.persistence.prv.PersistenceService/CreateCheckpoint",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.persistence.prv.PersistenceService",
                        "CreateCheckpoint",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Record message received

 ## Purpose
 Journal that a message was received by an actor (before processing).

 ## Why This RPC Exists
 - First step in exactly-once message processing guarantee
 - Enables deduplication on replay (skip already-seen messages)
 - Provides complete message audit trail

 ## How It's Used
 - Called by actor runtime immediately upon message arrival
 - Message is journaled BEFORE handle_message() is invoked
 - On recovery, runtime checks journal to skip duplicate messages
*/
        pub async fn record_message_received(
            &mut self,
            request: impl tonic::IntoRequest<super::RecordMessageReceivedRequest>,
        ) -> std::result::Result<
            tonic::Response<super::RecordMessageReceivedResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/plexspaces.persistence.prv.PersistenceService/RecordMessageReceived",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.persistence.prv.PersistenceService",
                        "RecordMessageReceived",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Record message processed

 ## Purpose
 Journal the outcome of message processing (success/failure).

 ## Why This RPC Exists
 - Completes exactly-once processing cycle
 - Records whether processing succeeded or failed
 - Enables retry logic for failed messages

 ## How It's Used
 - Called after handle_message() completes (successfully or with error)
 - On replay, successful messages are skipped (already processed)
 - Failed messages can be retried or sent to dead letter queue
*/
        pub async fn record_message_processed(
            &mut self,
            request: impl tonic::IntoRequest<super::RecordMessageProcessedRequest>,
        ) -> std::result::Result<
            tonic::Response<super::RecordMessageProcessedResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/plexspaces.persistence.prv.PersistenceService/RecordMessageProcessed",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.persistence.prv.PersistenceService",
                        "RecordMessageProcessed",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Record state change

 ## Purpose
 Journal actor state mutations for recovery.

 ## Why This RPC Exists
 - Enables state reconstruction without full snapshots
 - Provides incremental state updates between snapshots
 - Audit trail for debugging state corruption

 ## How It's Used
 - Called after actor modifies its internal state
 - On recovery, state changes are replayed to reconstruct current state
 - Periodic snapshots reduce replay time
*/
        pub async fn record_state_change(
            &mut self,
            request: impl tonic::IntoRequest<super::RecordStateChangeRequest>,
        ) -> std::result::Result<
            tonic::Response<super::RecordStateChangeResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/plexspaces.persistence.prv.PersistenceService/RecordStateChange",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.persistence.prv.PersistenceService",
                        "RecordStateChange",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Record side effect

 ## Purpose
 Journal non-deterministic operations for deterministic replay.

 ## Why This RPC Exists
 - External calls/timers/random numbers are non-deterministic
 - Replay must use cached results to ensure identical behavior
 - Prevents duplicate external calls on recovery

 ## How It's Used
 - Actor wraps non-deterministic operations with this call
 - First execution: Perform operation, journal request + response
 - Replay: Return journaled response without re-executing

 ## Examples
 - HTTP call to payment gateway (journal response to avoid double-charging)
 - Random number generation (replay uses cached value)
 - Current time access (replay uses journaled timestamp)
*/
        pub async fn record_side_effect(
            &mut self,
            request: impl tonic::IntoRequest<super::RecordSideEffectRequest>,
        ) -> std::result::Result<
            tonic::Response<super::RecordSideEffectResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/plexspaces.persistence.prv.PersistenceService/RecordSideEffect",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.persistence.prv.PersistenceService",
                        "RecordSideEffect",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Record promise created

 ## Purpose
 Create a durable promise that survives actor crashes (Restate-inspired).

 ## Why This RPC Exists
 - Promises enable async actor coordination
 - Must persist across crashes (unlike in-memory futures)
 - Supports idempotent creation with idempotency keys

 ## How It's Used
 - Actor creates promise for async operation (e.g., waiting for external event)
 - Promise persists in journal
 - Completion wakes up waiting actors
 - On crash, promise is restored from journal

 ## Examples
 - Waiting for user approval (promise resolved when user clicks button)
 - Aggregating results from multiple actors (promise resolves when all respond)
 - Timeout-based workflows (promise rejects after duration)
*/
        pub async fn record_promise_created(
            &mut self,
            request: impl tonic::IntoRequest<super::RecordPromiseCreatedRequest>,
        ) -> std::result::Result<
            tonic::Response<super::RecordPromiseCreatedResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/plexspaces.persistence.prv.PersistenceService/RecordPromiseCreated",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.persistence.prv.PersistenceService",
                        "RecordPromiseCreated",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Record promise resolved

 ## Purpose
 Record promise completion (fulfilled or rejected).

 ## Why This RPC Exists
 - Notifies waiting actors of outcome
 - Result is journaled for replay
 - Enables promise-based workflows

 ## How It's Used
 - Called when async operation completes
 - Wakes up actors waiting on promise
 - On replay, promise completes immediately with cached result

 ## Examples
 - User approved request -> promise fulfilled with approval data
 - External service failed -> promise rejected with error
 - Timeout occurred -> promise rejected with timeout
*/
        pub async fn record_promise_resolved(
            &mut self,
            request: impl tonic::IntoRequest<super::RecordPromiseResolvedRequest>,
        ) -> std::result::Result<
            tonic::Response<super::RecordPromiseResolvedResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/plexspaces.persistence.prv.PersistenceService/RecordPromiseResolved",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.persistence.prv.PersistenceService",
                        "RecordPromiseResolved",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Truncate journal

 ## Purpose
 Remove old journal entries after snapshot creation.

 ## Why This RPC Exists
 - Prevents journal from growing unbounded
 - Snapshots make old entries redundant
 - Improves recovery performance (less to replay)

 ## How It's Used
 - Called after successful snapshot creation
 - Removes entries up to snapshot sequence number
 - Keeps recent entries for incremental recovery

 ## Safety
 - Only truncates up to snapshot point (never loses data)
 - Atomic operation (all or nothing)
 - Can be safely called concurrently with other journal ops
*/
        pub async fn truncate_journal(
            &mut self,
            request: impl tonic::IntoRequest<super::TruncateJournalRequest>,
        ) -> std::result::Result<
            tonic::Response<super::TruncateJournalResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/plexspaces.persistence.prv.PersistenceService/TruncateJournal",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.persistence.prv.PersistenceService",
                        "TruncateJournal",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
    }
}
/// Generated server implementations.
pub mod persistence_service_server {
    #![allow(unused_variables, dead_code, missing_docs, clippy::let_unit_value)]
    use tonic::codegen::*;
    /// Generated trait containing gRPC methods that should be implemented for use with PersistenceServiceServer.
    #[async_trait]
    pub trait PersistenceService: Send + Sync + 'static {
        async fn append_events(
            &self,
            request: tonic::Request<super::AppendEventsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::AppendEventsResponse>,
            tonic::Status,
        >;
        /** Read events from journal
*/
        async fn read_events(
            &self,
            request: tonic::Request<super::ReadEventsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ReadEventsResponse>,
            tonic::Status,
        >;
        /** Save state snapshot
*/
        async fn save_snapshot(
            &self,
            request: tonic::Request<super::SaveSnapshotRequest>,
        ) -> std::result::Result<
            tonic::Response<super::SaveSnapshotResponse>,
            tonic::Status,
        >;
        /** Load state snapshot
*/
        async fn load_snapshot(
            &self,
            request: tonic::Request<super::LoadSnapshotRequest>,
        ) -> std::result::Result<
            tonic::Response<super::LoadSnapshotResponse>,
            tonic::Status,
        >;
        /** Create checkpoint
*/
        async fn create_checkpoint(
            &self,
            request: tonic::Request<super::CreateCheckpointRequest>,
        ) -> std::result::Result<
            tonic::Response<super::CreateCheckpointResponse>,
            tonic::Status,
        >;
        /** Record message received

 ## Purpose
 Journal that a message was received by an actor (before processing).

 ## Why This RPC Exists
 - First step in exactly-once message processing guarantee
 - Enables deduplication on replay (skip already-seen messages)
 - Provides complete message audit trail

 ## How It's Used
 - Called by actor runtime immediately upon message arrival
 - Message is journaled BEFORE handle_message() is invoked
 - On recovery, runtime checks journal to skip duplicate messages
*/
        async fn record_message_received(
            &self,
            request: tonic::Request<super::RecordMessageReceivedRequest>,
        ) -> std::result::Result<
            tonic::Response<super::RecordMessageReceivedResponse>,
            tonic::Status,
        >;
        /** Record message processed

 ## Purpose
 Journal the outcome of message processing (success/failure).

 ## Why This RPC Exists
 - Completes exactly-once processing cycle
 - Records whether processing succeeded or failed
 - Enables retry logic for failed messages

 ## How It's Used
 - Called after handle_message() completes (successfully or with error)
 - On replay, successful messages are skipped (already processed)
 - Failed messages can be retried or sent to dead letter queue
*/
        async fn record_message_processed(
            &self,
            request: tonic::Request<super::RecordMessageProcessedRequest>,
        ) -> std::result::Result<
            tonic::Response<super::RecordMessageProcessedResponse>,
            tonic::Status,
        >;
        /** Record state change

 ## Purpose
 Journal actor state mutations for recovery.

 ## Why This RPC Exists
 - Enables state reconstruction without full snapshots
 - Provides incremental state updates between snapshots
 - Audit trail for debugging state corruption

 ## How It's Used
 - Called after actor modifies its internal state
 - On recovery, state changes are replayed to reconstruct current state
 - Periodic snapshots reduce replay time
*/
        async fn record_state_change(
            &self,
            request: tonic::Request<super::RecordStateChangeRequest>,
        ) -> std::result::Result<
            tonic::Response<super::RecordStateChangeResponse>,
            tonic::Status,
        >;
        /** Record side effect

 ## Purpose
 Journal non-deterministic operations for deterministic replay.

 ## Why This RPC Exists
 - External calls/timers/random numbers are non-deterministic
 - Replay must use cached results to ensure identical behavior
 - Prevents duplicate external calls on recovery

 ## How It's Used
 - Actor wraps non-deterministic operations with this call
 - First execution: Perform operation, journal request + response
 - Replay: Return journaled response without re-executing

 ## Examples
 - HTTP call to payment gateway (journal response to avoid double-charging)
 - Random number generation (replay uses cached value)
 - Current time access (replay uses journaled timestamp)
*/
        async fn record_side_effect(
            &self,
            request: tonic::Request<super::RecordSideEffectRequest>,
        ) -> std::result::Result<
            tonic::Response<super::RecordSideEffectResponse>,
            tonic::Status,
        >;
        /** Record promise created

 ## Purpose
 Create a durable promise that survives actor crashes (Restate-inspired).

 ## Why This RPC Exists
 - Promises enable async actor coordination
 - Must persist across crashes (unlike in-memory futures)
 - Supports idempotent creation with idempotency keys

 ## How It's Used
 - Actor creates promise for async operation (e.g., waiting for external event)
 - Promise persists in journal
 - Completion wakes up waiting actors
 - On crash, promise is restored from journal

 ## Examples
 - Waiting for user approval (promise resolved when user clicks button)
 - Aggregating results from multiple actors (promise resolves when all respond)
 - Timeout-based workflows (promise rejects after duration)
*/
        async fn record_promise_created(
            &self,
            request: tonic::Request<super::RecordPromiseCreatedRequest>,
        ) -> std::result::Result<
            tonic::Response<super::RecordPromiseCreatedResponse>,
            tonic::Status,
        >;
        /** Record promise resolved

 ## Purpose
 Record promise completion (fulfilled or rejected).

 ## Why This RPC Exists
 - Notifies waiting actors of outcome
 - Result is journaled for replay
 - Enables promise-based workflows

 ## How It's Used
 - Called when async operation completes
 - Wakes up actors waiting on promise
 - On replay, promise completes immediately with cached result

 ## Examples
 - User approved request -> promise fulfilled with approval data
 - External service failed -> promise rejected with error
 - Timeout occurred -> promise rejected with timeout
*/
        async fn record_promise_resolved(
            &self,
            request: tonic::Request<super::RecordPromiseResolvedRequest>,
        ) -> std::result::Result<
            tonic::Response<super::RecordPromiseResolvedResponse>,
            tonic::Status,
        >;
        /** Truncate journal

 ## Purpose
 Remove old journal entries after snapshot creation.

 ## Why This RPC Exists
 - Prevents journal from growing unbounded
 - Snapshots make old entries redundant
 - Improves recovery performance (less to replay)

 ## How It's Used
 - Called after successful snapshot creation
 - Removes entries up to snapshot sequence number
 - Keeps recent entries for incremental recovery

 ## Safety
 - Only truncates up to snapshot point (never loses data)
 - Atomic operation (all or nothing)
 - Can be safely called concurrently with other journal ops
*/
        async fn truncate_journal(
            &self,
            request: tonic::Request<super::TruncateJournalRequest>,
        ) -> std::result::Result<
            tonic::Response<super::TruncateJournalResponse>,
            tonic::Status,
        >;
    }
    /** Persistence service
*/
    #[derive(Debug)]
    pub struct PersistenceServiceServer<T: PersistenceService> {
        inner: _Inner<T>,
        accept_compression_encodings: EnabledCompressionEncodings,
        send_compression_encodings: EnabledCompressionEncodings,
        max_decoding_message_size: Option<usize>,
        max_encoding_message_size: Option<usize>,
    }
    struct _Inner<T>(Arc<T>);
    impl<T: PersistenceService> PersistenceServiceServer<T> {
        pub fn new(inner: T) -> Self {
            Self::from_arc(Arc::new(inner))
        }
        pub fn from_arc(inner: Arc<T>) -> Self {
            let inner = _Inner(inner);
            Self {
                inner,
                accept_compression_encodings: Default::default(),
                send_compression_encodings: Default::default(),
                max_decoding_message_size: None,
                max_encoding_message_size: None,
            }
        }
        pub fn with_interceptor<F>(
            inner: T,
            interceptor: F,
        ) -> InterceptedService<Self, F>
        where
            F: tonic::service::Interceptor,
        {
            InterceptedService::new(Self::new(inner), interceptor)
        }
        /// Enable decompressing requests with the given encoding.
        #[must_use]
        pub fn accept_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.accept_compression_encodings.enable(encoding);
            self
        }
        /// Compress responses with the given encoding, if the client supports it.
        #[must_use]
        pub fn send_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.send_compression_encodings.enable(encoding);
            self
        }
        /// Limits the maximum size of a decoded message.
        ///
        /// Default: `4MB`
        #[must_use]
        pub fn max_decoding_message_size(mut self, limit: usize) -> Self {
            self.max_decoding_message_size = Some(limit);
            self
        }
        /// Limits the maximum size of an encoded message.
        ///
        /// Default: `usize::MAX`
        #[must_use]
        pub fn max_encoding_message_size(mut self, limit: usize) -> Self {
            self.max_encoding_message_size = Some(limit);
            self
        }
    }
    impl<T, B> tonic::codegen::Service<http::Request<B>> for PersistenceServiceServer<T>
    where
        T: PersistenceService,
        B: Body + Send + 'static,
        B::Error: Into<StdError> + Send + 'static,
    {
        type Response = http::Response<tonic::body::BoxBody>;
        type Error = std::convert::Infallible;
        type Future = BoxFuture<Self::Response, Self::Error>;
        fn poll_ready(
            &mut self,
            _cx: &mut Context<'_>,
        ) -> Poll<std::result::Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }
        fn call(&mut self, req: http::Request<B>) -> Self::Future {
            let inner = self.inner.clone();
            match req.uri().path() {
                "/plexspaces.persistence.prv.PersistenceService/AppendEvents" => {
                    #[allow(non_camel_case_types)]
                    struct AppendEventsSvc<T: PersistenceService>(pub Arc<T>);
                    impl<
                        T: PersistenceService,
                    > tonic::server::UnaryService<super::AppendEventsRequest>
                    for AppendEventsSvc<T> {
                        type Response = super::AppendEventsResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::AppendEventsRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as PersistenceService>::append_events(&inner, request)
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = AppendEventsSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/plexspaces.persistence.prv.PersistenceService/ReadEvents" => {
                    #[allow(non_camel_case_types)]
                    struct ReadEventsSvc<T: PersistenceService>(pub Arc<T>);
                    impl<
                        T: PersistenceService,
                    > tonic::server::UnaryService<super::ReadEventsRequest>
                    for ReadEventsSvc<T> {
                        type Response = super::ReadEventsResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ReadEventsRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as PersistenceService>::read_events(&inner, request)
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = ReadEventsSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/plexspaces.persistence.prv.PersistenceService/SaveSnapshot" => {
                    #[allow(non_camel_case_types)]
                    struct SaveSnapshotSvc<T: PersistenceService>(pub Arc<T>);
                    impl<
                        T: PersistenceService,
                    > tonic::server::UnaryService<super::SaveSnapshotRequest>
                    for SaveSnapshotSvc<T> {
                        type Response = super::SaveSnapshotResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::SaveSnapshotRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as PersistenceService>::save_snapshot(&inner, request)
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = SaveSnapshotSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/plexspaces.persistence.prv.PersistenceService/LoadSnapshot" => {
                    #[allow(non_camel_case_types)]
                    struct LoadSnapshotSvc<T: PersistenceService>(pub Arc<T>);
                    impl<
                        T: PersistenceService,
                    > tonic::server::UnaryService<super::LoadSnapshotRequest>
                    for LoadSnapshotSvc<T> {
                        type Response = super::LoadSnapshotResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::LoadSnapshotRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as PersistenceService>::load_snapshot(&inner, request)
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = LoadSnapshotSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/plexspaces.persistence.prv.PersistenceService/CreateCheckpoint" => {
                    #[allow(non_camel_case_types)]
                    struct CreateCheckpointSvc<T: PersistenceService>(pub Arc<T>);
                    impl<
                        T: PersistenceService,
                    > tonic::server::UnaryService<super::CreateCheckpointRequest>
                    for CreateCheckpointSvc<T> {
                        type Response = super::CreateCheckpointResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::CreateCheckpointRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as PersistenceService>::create_checkpoint(
                                        &inner,
                                        request,
                                    )
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = CreateCheckpointSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/plexspaces.persistence.prv.PersistenceService/RecordMessageReceived" => {
                    #[allow(non_camel_case_types)]
                    struct RecordMessageReceivedSvc<T: PersistenceService>(pub Arc<T>);
                    impl<
                        T: PersistenceService,
                    > tonic::server::UnaryService<super::RecordMessageReceivedRequest>
                    for RecordMessageReceivedSvc<T> {
                        type Response = super::RecordMessageReceivedResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::RecordMessageReceivedRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as PersistenceService>::record_message_received(
                                        &inner,
                                        request,
                                    )
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = RecordMessageReceivedSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/plexspaces.persistence.prv.PersistenceService/RecordMessageProcessed" => {
                    #[allow(non_camel_case_types)]
                    struct RecordMessageProcessedSvc<T: PersistenceService>(pub Arc<T>);
                    impl<
                        T: PersistenceService,
                    > tonic::server::UnaryService<super::RecordMessageProcessedRequest>
                    for RecordMessageProcessedSvc<T> {
                        type Response = super::RecordMessageProcessedResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::RecordMessageProcessedRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as PersistenceService>::record_message_processed(
                                        &inner,
                                        request,
                                    )
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = RecordMessageProcessedSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/plexspaces.persistence.prv.PersistenceService/RecordStateChange" => {
                    #[allow(non_camel_case_types)]
                    struct RecordStateChangeSvc<T: PersistenceService>(pub Arc<T>);
                    impl<
                        T: PersistenceService,
                    > tonic::server::UnaryService<super::RecordStateChangeRequest>
                    for RecordStateChangeSvc<T> {
                        type Response = super::RecordStateChangeResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::RecordStateChangeRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as PersistenceService>::record_state_change(
                                        &inner,
                                        request,
                                    )
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = RecordStateChangeSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/plexspaces.persistence.prv.PersistenceService/RecordSideEffect" => {
                    #[allow(non_camel_case_types)]
                    struct RecordSideEffectSvc<T: PersistenceService>(pub Arc<T>);
                    impl<
                        T: PersistenceService,
                    > tonic::server::UnaryService<super::RecordSideEffectRequest>
                    for RecordSideEffectSvc<T> {
                        type Response = super::RecordSideEffectResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::RecordSideEffectRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as PersistenceService>::record_side_effect(
                                        &inner,
                                        request,
                                    )
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = RecordSideEffectSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/plexspaces.persistence.prv.PersistenceService/RecordPromiseCreated" => {
                    #[allow(non_camel_case_types)]
                    struct RecordPromiseCreatedSvc<T: PersistenceService>(pub Arc<T>);
                    impl<
                        T: PersistenceService,
                    > tonic::server::UnaryService<super::RecordPromiseCreatedRequest>
                    for RecordPromiseCreatedSvc<T> {
                        type Response = super::RecordPromiseCreatedResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::RecordPromiseCreatedRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as PersistenceService>::record_promise_created(
                                        &inner,
                                        request,
                                    )
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = RecordPromiseCreatedSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/plexspaces.persistence.prv.PersistenceService/RecordPromiseResolved" => {
                    #[allow(non_camel_case_types)]
                    struct RecordPromiseResolvedSvc<T: PersistenceService>(pub Arc<T>);
                    impl<
                        T: PersistenceService,
                    > tonic::server::UnaryService<super::RecordPromiseResolvedRequest>
                    for RecordPromiseResolvedSvc<T> {
                        type Response = super::RecordPromiseResolvedResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::RecordPromiseResolvedRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as PersistenceService>::record_promise_resolved(
                                        &inner,
                                        request,
                                    )
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = RecordPromiseResolvedSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/plexspaces.persistence.prv.PersistenceService/TruncateJournal" => {
                    #[allow(non_camel_case_types)]
                    struct TruncateJournalSvc<T: PersistenceService>(pub Arc<T>);
                    impl<
                        T: PersistenceService,
                    > tonic::server::UnaryService<super::TruncateJournalRequest>
                    for TruncateJournalSvc<T> {
                        type Response = super::TruncateJournalResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::TruncateJournalRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as PersistenceService>::truncate_journal(&inner, request)
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = TruncateJournalSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                _ => {
                    Box::pin(async move {
                        Ok(
                            http::Response::builder()
                                .status(200)
                                .header("grpc-status", "12")
                                .header("content-type", "application/grpc")
                                .body(empty_body())
                                .unwrap(),
                        )
                    })
                }
            }
        }
    }
    impl<T: PersistenceService> Clone for PersistenceServiceServer<T> {
        fn clone(&self) -> Self {
            let inner = self.inner.clone();
            Self {
                inner,
                accept_compression_encodings: self.accept_compression_encodings,
                send_compression_encodings: self.send_compression_encodings,
                max_decoding_message_size: self.max_decoding_message_size,
                max_encoding_message_size: self.max_encoding_message_size,
            }
        }
    }
    impl<T: PersistenceService> Clone for _Inner<T> {
        fn clone(&self) -> Self {
            Self(Arc::clone(&self.0))
        }
    }
    impl<T: std::fmt::Debug> std::fmt::Debug for _Inner<T> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{:?}", self.0)
        }
    }
    impl<T: PersistenceService> tonic::server::NamedService
    for PersistenceServiceServer<T> {
        const NAME: &'static str = "plexspaces.persistence.prv.PersistenceService";
    }
}
