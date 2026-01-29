// @generated
/// Generated client implementations.
pub mod tuple_space_service_client {
    #![allow(unused_variables, dead_code, missing_docs, clippy::let_unit_value)]
    use tonic::codegen::*;
    use tonic::codegen::http::Uri;
    /** TupleSpace service
*/
    #[derive(Debug, Clone)]
    pub struct TupleSpaceServiceClient<T> {
        inner: tonic::client::Grpc<T>,
    }
    impl TupleSpaceServiceClient<tonic::transport::Channel> {
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
    impl<T> TupleSpaceServiceClient<T>
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
        ) -> TupleSpaceServiceClient<InterceptedService<T, F>>
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
            TupleSpaceServiceClient::new(InterceptedService::new(inner, interceptor))
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
        pub async fn write(
            &mut self,
            request: impl tonic::IntoRequest<super::WriteRequest>,
        ) -> std::result::Result<tonic::Response<super::WriteResponse>, tonic::Status> {
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
                "/plexspaces.tuplespace.v1.TupleSpaceService/Write",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.tuplespace.v1.TupleSpaceService",
                        "Write",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Read tuples (blocking and non-blocking)
*/
        pub async fn read(
            &mut self,
            request: impl tonic::IntoRequest<super::ReadRequest>,
        ) -> std::result::Result<tonic::Response<super::ReadResponse>, tonic::Status> {
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
                "/plexspaces.tuplespace.v1.TupleSpaceService/Read",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("plexspaces.tuplespace.v1.TupleSpaceService", "Read"),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Take tuples (read and remove)
*/
        pub async fn take(
            &mut self,
            request: impl tonic::IntoRequest<super::ReadRequest>,
        ) -> std::result::Result<tonic::Response<super::ReadResponse>, tonic::Status> {
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
                "/plexspaces.tuplespace.v1.TupleSpaceService/Take",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("plexspaces.tuplespace.v1.TupleSpaceService", "Take"),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Count matching tuples
*/
        pub async fn count(
            &mut self,
            request: impl tonic::IntoRequest<super::CountRequest>,
        ) -> std::result::Result<tonic::Response<super::CountResponse>, tonic::Status> {
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
                "/plexspaces.tuplespace.v1.TupleSpaceService/Count",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.tuplespace.v1.TupleSpaceService",
                        "Count",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Check if tuples exist
*/
        pub async fn exists(
            &mut self,
            request: impl tonic::IntoRequest<super::ExistsRequest>,
        ) -> std::result::Result<tonic::Response<super::ExistsResponse>, tonic::Status> {
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
                "/plexspaces.tuplespace.v1.TupleSpaceService/Exists",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.tuplespace.v1.TupleSpaceService",
                        "Exists",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Subscribe to tuple events
*/
        pub async fn subscribe(
            &mut self,
            request: impl tonic::IntoRequest<super::SubscribeRequest>,
        ) -> std::result::Result<
            tonic::Response<super::SubscribeResponse>,
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
                "/plexspaces.tuplespace.v1.TupleSpaceService/Subscribe",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.tuplespace.v1.TupleSpaceService",
                        "Subscribe",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Unsubscribe from tuple events
*/
        pub async fn unsubscribe(
            &mut self,
            request: impl tonic::IntoRequest<super::UnsubscribeRequest>,
        ) -> std::result::Result<
            tonic::Response<super::super::super::common::v1::Empty>,
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
                "/plexspaces.tuplespace.v1.TupleSpaceService/Unsubscribe",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.tuplespace.v1.TupleSpaceService",
                        "Unsubscribe",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Begin transaction
*/
        pub async fn begin_transaction(
            &mut self,
            request: impl tonic::IntoRequest<super::BeginTransactionRequest>,
        ) -> std::result::Result<
            tonic::Response<super::BeginTransactionResponse>,
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
                "/plexspaces.tuplespace.v1.TupleSpaceService/BeginTransaction",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.tuplespace.v1.TupleSpaceService",
                        "BeginTransaction",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Commit transaction
*/
        pub async fn commit_transaction(
            &mut self,
            request: impl tonic::IntoRequest<super::CommitTransactionRequest>,
        ) -> std::result::Result<
            tonic::Response<super::CommitTransactionResponse>,
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
                "/plexspaces.tuplespace.v1.TupleSpaceService/CommitTransaction",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.tuplespace.v1.TupleSpaceService",
                        "CommitTransaction",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Abort transaction
*/
        pub async fn abort_transaction(
            &mut self,
            request: impl tonic::IntoRequest<super::AbortTransactionRequest>,
        ) -> std::result::Result<
            tonic::Response<super::AbortTransactionResponse>,
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
                "/plexspaces.tuplespace.v1.TupleSpaceService/AbortTransaction",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.tuplespace.v1.TupleSpaceService",
                        "AbortTransaction",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Clear tuplespace
*/
        pub async fn clear(
            &mut self,
            request: impl tonic::IntoRequest<super::super::super::common::v1::Empty>,
        ) -> std::result::Result<
            tonic::Response<super::super::super::common::v1::Empty>,
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
                "/plexspaces.tuplespace.v1.TupleSpaceService/Clear",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.tuplespace.v1.TupleSpaceService",
                        "Clear",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Renew tuple lease
*/
        pub async fn renew_lease(
            &mut self,
            request: impl tonic::IntoRequest<super::RenewLeaseRequest>,
        ) -> std::result::Result<
            tonic::Response<super::RenewLeaseResponse>,
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
                "/plexspaces.tuplespace.v1.TupleSpaceService/RenewLease",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.tuplespace.v1.TupleSpaceService",
                        "RenewLease",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Get storage statistics

 ## Purpose
 Retrieve performance and usage statistics from the TupleSpace storage backend.

 ## Why This Exists
 - **Monitoring**: Track tuple count, memory usage, operation counts
 - **Performance**: Measure average latency for optimization
 - **Capacity Planning**: Know when to scale storage
 - **Debugging**: Identify performance bottlenecks

 ## How It's Used
 - Called by monitoring dashboards (Prometheus, Grafana)
 - Used by auto-scaling logic to determine when to add nodes
 - Debugging tool for investigating slow TupleSpace operations

 ## Design Notes
 - Returns StorageStats message (defined in tuplespace_storage.proto)
 - Statistics include: tuple_count, memory_bytes, operation counts, avg_latency_ms
 - All storage backends (Memory, Redis, PostgreSQL, SQLite) implement this

 ## Examples
 ```
 GetStatsRequest {} -> GetStatsResponse {
   stats: {
     tuple_count: 10000,
     memory_bytes: 5242880,
     total_operations: 50000,
     read_operations: 30000,
     write_operations: 15000,
     take_operations: 5000,
     avg_latency_ms: 2.5
   }
 }
 ```
*/
        pub async fn get_stats(
            &mut self,
            request: impl tonic::IntoRequest<super::super::super::common::v1::Empty>,
        ) -> std::result::Result<
            tonic::Response<super::GetStatsResponse>,
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
                "/plexspaces.tuplespace.v1.TupleSpaceService/GetStats",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.tuplespace.v1.TupleSpaceService",
                        "GetStats",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
    }
}
/// Generated server implementations.
pub mod tuple_space_service_server {
    #![allow(unused_variables, dead_code, missing_docs, clippy::let_unit_value)]
    use tonic::codegen::*;
    /// Generated trait containing gRPC methods that should be implemented for use with TupleSpaceServiceServer.
    #[async_trait]
    pub trait TupleSpaceService: Send + Sync + 'static {
        async fn write(
            &self,
            request: tonic::Request<super::WriteRequest>,
        ) -> std::result::Result<tonic::Response<super::WriteResponse>, tonic::Status>;
        /** Read tuples (blocking and non-blocking)
*/
        async fn read(
            &self,
            request: tonic::Request<super::ReadRequest>,
        ) -> std::result::Result<tonic::Response<super::ReadResponse>, tonic::Status>;
        /** Take tuples (read and remove)
*/
        async fn take(
            &self,
            request: tonic::Request<super::ReadRequest>,
        ) -> std::result::Result<tonic::Response<super::ReadResponse>, tonic::Status>;
        /** Count matching tuples
*/
        async fn count(
            &self,
            request: tonic::Request<super::CountRequest>,
        ) -> std::result::Result<tonic::Response<super::CountResponse>, tonic::Status>;
        /** Check if tuples exist
*/
        async fn exists(
            &self,
            request: tonic::Request<super::ExistsRequest>,
        ) -> std::result::Result<tonic::Response<super::ExistsResponse>, tonic::Status>;
        /** Subscribe to tuple events
*/
        async fn subscribe(
            &self,
            request: tonic::Request<super::SubscribeRequest>,
        ) -> std::result::Result<
            tonic::Response<super::SubscribeResponse>,
            tonic::Status,
        >;
        /** Unsubscribe from tuple events
*/
        async fn unsubscribe(
            &self,
            request: tonic::Request<super::UnsubscribeRequest>,
        ) -> std::result::Result<
            tonic::Response<super::super::super::common::v1::Empty>,
            tonic::Status,
        >;
        /** Begin transaction
*/
        async fn begin_transaction(
            &self,
            request: tonic::Request<super::BeginTransactionRequest>,
        ) -> std::result::Result<
            tonic::Response<super::BeginTransactionResponse>,
            tonic::Status,
        >;
        /** Commit transaction
*/
        async fn commit_transaction(
            &self,
            request: tonic::Request<super::CommitTransactionRequest>,
        ) -> std::result::Result<
            tonic::Response<super::CommitTransactionResponse>,
            tonic::Status,
        >;
        /** Abort transaction
*/
        async fn abort_transaction(
            &self,
            request: tonic::Request<super::AbortTransactionRequest>,
        ) -> std::result::Result<
            tonic::Response<super::AbortTransactionResponse>,
            tonic::Status,
        >;
        /** Clear tuplespace
*/
        async fn clear(
            &self,
            request: tonic::Request<super::super::super::common::v1::Empty>,
        ) -> std::result::Result<
            tonic::Response<super::super::super::common::v1::Empty>,
            tonic::Status,
        >;
        /** Renew tuple lease
*/
        async fn renew_lease(
            &self,
            request: tonic::Request<super::RenewLeaseRequest>,
        ) -> std::result::Result<
            tonic::Response<super::RenewLeaseResponse>,
            tonic::Status,
        >;
        /** Get storage statistics

 ## Purpose
 Retrieve performance and usage statistics from the TupleSpace storage backend.

 ## Why This Exists
 - **Monitoring**: Track tuple count, memory usage, operation counts
 - **Performance**: Measure average latency for optimization
 - **Capacity Planning**: Know when to scale storage
 - **Debugging**: Identify performance bottlenecks

 ## How It's Used
 - Called by monitoring dashboards (Prometheus, Grafana)
 - Used by auto-scaling logic to determine when to add nodes
 - Debugging tool for investigating slow TupleSpace operations

 ## Design Notes
 - Returns StorageStats message (defined in tuplespace_storage.proto)
 - Statistics include: tuple_count, memory_bytes, operation counts, avg_latency_ms
 - All storage backends (Memory, Redis, PostgreSQL, SQLite) implement this

 ## Examples
 ```
 GetStatsRequest {} -> GetStatsResponse {
   stats: {
     tuple_count: 10000,
     memory_bytes: 5242880,
     total_operations: 50000,
     read_operations: 30000,
     write_operations: 15000,
     take_operations: 5000,
     avg_latency_ms: 2.5
   }
 }
 ```
*/
        async fn get_stats(
            &self,
            request: tonic::Request<super::super::super::common::v1::Empty>,
        ) -> std::result::Result<
            tonic::Response<super::GetStatsResponse>,
            tonic::Status,
        >;
    }
    /** TupleSpace service
*/
    #[derive(Debug)]
    pub struct TupleSpaceServiceServer<T: TupleSpaceService> {
        inner: _Inner<T>,
        accept_compression_encodings: EnabledCompressionEncodings,
        send_compression_encodings: EnabledCompressionEncodings,
        max_decoding_message_size: Option<usize>,
        max_encoding_message_size: Option<usize>,
    }
    struct _Inner<T>(Arc<T>);
    impl<T: TupleSpaceService> TupleSpaceServiceServer<T> {
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
    impl<T, B> tonic::codegen::Service<http::Request<B>> for TupleSpaceServiceServer<T>
    where
        T: TupleSpaceService,
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
                "/plexspaces.tuplespace.v1.TupleSpaceService/Write" => {
                    #[allow(non_camel_case_types)]
                    struct WriteSvc<T: TupleSpaceService>(pub Arc<T>);
                    impl<
                        T: TupleSpaceService,
                    > tonic::server::UnaryService<super::WriteRequest> for WriteSvc<T> {
                        type Response = super::WriteResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::WriteRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as TupleSpaceService>::write(&inner, request).await
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
                        let method = WriteSvc(inner);
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
                "/plexspaces.tuplespace.v1.TupleSpaceService/Read" => {
                    #[allow(non_camel_case_types)]
                    struct ReadSvc<T: TupleSpaceService>(pub Arc<T>);
                    impl<
                        T: TupleSpaceService,
                    > tonic::server::UnaryService<super::ReadRequest> for ReadSvc<T> {
                        type Response = super::ReadResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ReadRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as TupleSpaceService>::read(&inner, request).await
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
                        let method = ReadSvc(inner);
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
                "/plexspaces.tuplespace.v1.TupleSpaceService/Take" => {
                    #[allow(non_camel_case_types)]
                    struct TakeSvc<T: TupleSpaceService>(pub Arc<T>);
                    impl<
                        T: TupleSpaceService,
                    > tonic::server::UnaryService<super::ReadRequest> for TakeSvc<T> {
                        type Response = super::ReadResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ReadRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as TupleSpaceService>::take(&inner, request).await
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
                        let method = TakeSvc(inner);
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
                "/plexspaces.tuplespace.v1.TupleSpaceService/Count" => {
                    #[allow(non_camel_case_types)]
                    struct CountSvc<T: TupleSpaceService>(pub Arc<T>);
                    impl<
                        T: TupleSpaceService,
                    > tonic::server::UnaryService<super::CountRequest> for CountSvc<T> {
                        type Response = super::CountResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::CountRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as TupleSpaceService>::count(&inner, request).await
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
                        let method = CountSvc(inner);
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
                "/plexspaces.tuplespace.v1.TupleSpaceService/Exists" => {
                    #[allow(non_camel_case_types)]
                    struct ExistsSvc<T: TupleSpaceService>(pub Arc<T>);
                    impl<
                        T: TupleSpaceService,
                    > tonic::server::UnaryService<super::ExistsRequest>
                    for ExistsSvc<T> {
                        type Response = super::ExistsResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ExistsRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as TupleSpaceService>::exists(&inner, request).await
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
                        let method = ExistsSvc(inner);
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
                "/plexspaces.tuplespace.v1.TupleSpaceService/Subscribe" => {
                    #[allow(non_camel_case_types)]
                    struct SubscribeSvc<T: TupleSpaceService>(pub Arc<T>);
                    impl<
                        T: TupleSpaceService,
                    > tonic::server::UnaryService<super::SubscribeRequest>
                    for SubscribeSvc<T> {
                        type Response = super::SubscribeResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::SubscribeRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as TupleSpaceService>::subscribe(&inner, request).await
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
                        let method = SubscribeSvc(inner);
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
                "/plexspaces.tuplespace.v1.TupleSpaceService/Unsubscribe" => {
                    #[allow(non_camel_case_types)]
                    struct UnsubscribeSvc<T: TupleSpaceService>(pub Arc<T>);
                    impl<
                        T: TupleSpaceService,
                    > tonic::server::UnaryService<super::UnsubscribeRequest>
                    for UnsubscribeSvc<T> {
                        type Response = super::super::super::common::v1::Empty;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::UnsubscribeRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as TupleSpaceService>::unsubscribe(&inner, request).await
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
                        let method = UnsubscribeSvc(inner);
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
                "/plexspaces.tuplespace.v1.TupleSpaceService/BeginTransaction" => {
                    #[allow(non_camel_case_types)]
                    struct BeginTransactionSvc<T: TupleSpaceService>(pub Arc<T>);
                    impl<
                        T: TupleSpaceService,
                    > tonic::server::UnaryService<super::BeginTransactionRequest>
                    for BeginTransactionSvc<T> {
                        type Response = super::BeginTransactionResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::BeginTransactionRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as TupleSpaceService>::begin_transaction(&inner, request)
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
                        let method = BeginTransactionSvc(inner);
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
                "/plexspaces.tuplespace.v1.TupleSpaceService/CommitTransaction" => {
                    #[allow(non_camel_case_types)]
                    struct CommitTransactionSvc<T: TupleSpaceService>(pub Arc<T>);
                    impl<
                        T: TupleSpaceService,
                    > tonic::server::UnaryService<super::CommitTransactionRequest>
                    for CommitTransactionSvc<T> {
                        type Response = super::CommitTransactionResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::CommitTransactionRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as TupleSpaceService>::commit_transaction(
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
                        let method = CommitTransactionSvc(inner);
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
                "/plexspaces.tuplespace.v1.TupleSpaceService/AbortTransaction" => {
                    #[allow(non_camel_case_types)]
                    struct AbortTransactionSvc<T: TupleSpaceService>(pub Arc<T>);
                    impl<
                        T: TupleSpaceService,
                    > tonic::server::UnaryService<super::AbortTransactionRequest>
                    for AbortTransactionSvc<T> {
                        type Response = super::AbortTransactionResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::AbortTransactionRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as TupleSpaceService>::abort_transaction(&inner, request)
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
                        let method = AbortTransactionSvc(inner);
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
                "/plexspaces.tuplespace.v1.TupleSpaceService/Clear" => {
                    #[allow(non_camel_case_types)]
                    struct ClearSvc<T: TupleSpaceService>(pub Arc<T>);
                    impl<
                        T: TupleSpaceService,
                    > tonic::server::UnaryService<super::super::super::common::v1::Empty>
                    for ClearSvc<T> {
                        type Response = super::super::super::common::v1::Empty;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<
                                super::super::super::common::v1::Empty,
                            >,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as TupleSpaceService>::clear(&inner, request).await
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
                        let method = ClearSvc(inner);
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
                "/plexspaces.tuplespace.v1.TupleSpaceService/RenewLease" => {
                    #[allow(non_camel_case_types)]
                    struct RenewLeaseSvc<T: TupleSpaceService>(pub Arc<T>);
                    impl<
                        T: TupleSpaceService,
                    > tonic::server::UnaryService<super::RenewLeaseRequest>
                    for RenewLeaseSvc<T> {
                        type Response = super::RenewLeaseResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::RenewLeaseRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as TupleSpaceService>::renew_lease(&inner, request).await
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
                        let method = RenewLeaseSvc(inner);
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
                "/plexspaces.tuplespace.v1.TupleSpaceService/GetStats" => {
                    #[allow(non_camel_case_types)]
                    struct GetStatsSvc<T: TupleSpaceService>(pub Arc<T>);
                    impl<
                        T: TupleSpaceService,
                    > tonic::server::UnaryService<super::super::super::common::v1::Empty>
                    for GetStatsSvc<T> {
                        type Response = super::GetStatsResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<
                                super::super::super::common::v1::Empty,
                            >,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as TupleSpaceService>::get_stats(&inner, request).await
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
                        let method = GetStatsSvc(inner);
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
    impl<T: TupleSpaceService> Clone for TupleSpaceServiceServer<T> {
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
    impl<T: TupleSpaceService> Clone for _Inner<T> {
        fn clone(&self) -> Self {
            Self(Arc::clone(&self.0))
        }
    }
    impl<T: std::fmt::Debug> std::fmt::Debug for _Inner<T> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{:?}", self.0)
        }
    }
    impl<T: TupleSpaceService> tonic::server::NamedService
    for TupleSpaceServiceServer<T> {
        const NAME: &'static str = "plexspaces.tuplespace.v1.TupleSpaceService";
    }
}
