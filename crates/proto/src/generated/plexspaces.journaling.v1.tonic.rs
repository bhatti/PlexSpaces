// @generated
/// Generated client implementations.
pub mod journal_service_client {
    #![allow(unused_variables, dead_code, missing_docs, clippy::let_unit_value)]
    use tonic::codegen::*;
    use tonic::codegen::http::Uri;
    /** / Journal service for distributed journaling
/
/ ## Purpose
/ Provides gRPC API for remote journal operations (Restate-style distributed journaling).
/
/ ## Why This Exists
/ - Distributed actors: Actors on different nodes share journal
/ - Remote replay: Replay journal from remote storage
/ - Centralized journaling: Dedicated journal service for multiple nodes
/
/ ## Design Notes
/ - Streaming: ReplayFrom uses streaming for large journals
/ - Batching: AppendBatch for atomic multi-entry writes
/ - Statistics: GetStats for observability
/
/ ## Usage
/ ```rust
/ // Local journaling (single node)
/ let storage = PostgresJournalStorage::new(config);
/ let facet = DurabilityFacet::new_local(storage);
/
/ // Distributed journaling (multi-node)
/ let client = JournalServiceClient::connect("http://journal-service:9090");
/ let facet = DurabilityFacet::new_remote(client);
/ ```
*/
    #[derive(Debug, Clone)]
    pub struct JournalServiceClient<T> {
        inner: tonic::client::Grpc<T>,
    }
    impl JournalServiceClient<tonic::transport::Channel> {
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
    impl<T> JournalServiceClient<T>
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
        ) -> JournalServiceClient<InterceptedService<T, F>>
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
            JournalServiceClient::new(InterceptedService::new(inner, interceptor))
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
        pub async fn append(
            &mut self,
            request: impl tonic::IntoRequest<super::AppendRequest>,
        ) -> std::result::Result<tonic::Response<super::AppendResponse>, tonic::Status> {
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
                "/plexspaces.journaling.v1.JournalService/Append",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("plexspaces.journaling.v1.JournalService", "Append"),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn append_batch(
            &mut self,
            request: impl tonic::IntoRequest<super::AppendBatchRequest>,
        ) -> std::result::Result<
            tonic::Response<super::AppendBatchResponse>,
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
                "/plexspaces.journaling.v1.JournalService/AppendBatch",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.journaling.v1.JournalService",
                        "AppendBatch",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn replay_from(
            &mut self,
            request: impl tonic::IntoRequest<super::ReplayFromRequest>,
        ) -> std::result::Result<
            tonic::Response<tonic::codec::Streaming<super::JournalEntry>>,
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
                "/plexspaces.journaling.v1.JournalService/ReplayFrom",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.journaling.v1.JournalService",
                        "ReplayFrom",
                    ),
                );
            self.inner.server_streaming(req, path, codec).await
        }
        pub async fn get_latest_checkpoint(
            &mut self,
            request: impl tonic::IntoRequest<super::GetLatestCheckpointRequest>,
        ) -> std::result::Result<tonic::Response<super::Checkpoint>, tonic::Status> {
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
                "/plexspaces.journaling.v1.JournalService/GetLatestCheckpoint",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.journaling.v1.JournalService",
                        "GetLatestCheckpoint",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn save_checkpoint(
            &mut self,
            request: impl tonic::IntoRequest<super::SaveCheckpointRequest>,
        ) -> std::result::Result<
            tonic::Response<super::SaveCheckpointResponse>,
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
                "/plexspaces.journaling.v1.JournalService/SaveCheckpoint",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.journaling.v1.JournalService",
                        "SaveCheckpoint",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn truncate_to(
            &mut self,
            request: impl tonic::IntoRequest<super::TruncateToRequest>,
        ) -> std::result::Result<
            tonic::Response<super::TruncateToResponse>,
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
                "/plexspaces.journaling.v1.JournalService/TruncateTo",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.journaling.v1.JournalService",
                        "TruncateTo",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn get_stats(
            &mut self,
            request: impl tonic::IntoRequest<super::GetStatsRequest>,
        ) -> std::result::Result<tonic::Response<super::JournalStats>, tonic::Status> {
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
                "/plexspaces.journaling.v1.JournalService/GetStats",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.journaling.v1.JournalService",
                        "GetStats",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn append_event(
            &mut self,
            request: impl tonic::IntoRequest<super::AppendEventRequest>,
        ) -> std::result::Result<
            tonic::Response<super::AppendEventResponse>,
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
                "/plexspaces.journaling.v1.JournalService/AppendEvent",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.journaling.v1.JournalService",
                        "AppendEvent",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn replay_events_from(
            &mut self,
            request: impl tonic::IntoRequest<super::ReplayEventsFromRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ReplayEventsFromResponse>,
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
                "/plexspaces.journaling.v1.JournalService/ReplayEventsFrom",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.journaling.v1.JournalService",
                        "ReplayEventsFrom",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn get_actor_history(
            &mut self,
            request: impl tonic::IntoRequest<super::GetActorHistoryRequest>,
        ) -> std::result::Result<tonic::Response<super::ActorHistory>, tonic::Status> {
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
                "/plexspaces.journaling.v1.JournalService/GetActorHistory",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.journaling.v1.JournalService",
                        "GetActorHistory",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
    }
}
/// Generated server implementations.
pub mod journal_service_server {
    #![allow(unused_variables, dead_code, missing_docs, clippy::let_unit_value)]
    use tonic::codegen::*;
    /// Generated trait containing gRPC methods that should be implemented for use with JournalServiceServer.
    #[async_trait]
    pub trait JournalService: Send + Sync + 'static {
        async fn append(
            &self,
            request: tonic::Request<super::AppendRequest>,
        ) -> std::result::Result<tonic::Response<super::AppendResponse>, tonic::Status>;
        async fn append_batch(
            &self,
            request: tonic::Request<super::AppendBatchRequest>,
        ) -> std::result::Result<
            tonic::Response<super::AppendBatchResponse>,
            tonic::Status,
        >;
        /// Server streaming response type for the ReplayFrom method.
        type ReplayFromStream: tonic::codegen::tokio_stream::Stream<
                Item = std::result::Result<super::JournalEntry, tonic::Status>,
            >
            + Send
            + 'static;
        async fn replay_from(
            &self,
            request: tonic::Request<super::ReplayFromRequest>,
        ) -> std::result::Result<tonic::Response<Self::ReplayFromStream>, tonic::Status>;
        async fn get_latest_checkpoint(
            &self,
            request: tonic::Request<super::GetLatestCheckpointRequest>,
        ) -> std::result::Result<tonic::Response<super::Checkpoint>, tonic::Status>;
        async fn save_checkpoint(
            &self,
            request: tonic::Request<super::SaveCheckpointRequest>,
        ) -> std::result::Result<
            tonic::Response<super::SaveCheckpointResponse>,
            tonic::Status,
        >;
        async fn truncate_to(
            &self,
            request: tonic::Request<super::TruncateToRequest>,
        ) -> std::result::Result<
            tonic::Response<super::TruncateToResponse>,
            tonic::Status,
        >;
        async fn get_stats(
            &self,
            request: tonic::Request<super::GetStatsRequest>,
        ) -> std::result::Result<tonic::Response<super::JournalStats>, tonic::Status>;
        async fn append_event(
            &self,
            request: tonic::Request<super::AppendEventRequest>,
        ) -> std::result::Result<
            tonic::Response<super::AppendEventResponse>,
            tonic::Status,
        >;
        async fn replay_events_from(
            &self,
            request: tonic::Request<super::ReplayEventsFromRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ReplayEventsFromResponse>,
            tonic::Status,
        >;
        async fn get_actor_history(
            &self,
            request: tonic::Request<super::GetActorHistoryRequest>,
        ) -> std::result::Result<tonic::Response<super::ActorHistory>, tonic::Status>;
    }
    /** / Journal service for distributed journaling
/
/ ## Purpose
/ Provides gRPC API for remote journal operations (Restate-style distributed journaling).
/
/ ## Why This Exists
/ - Distributed actors: Actors on different nodes share journal
/ - Remote replay: Replay journal from remote storage
/ - Centralized journaling: Dedicated journal service for multiple nodes
/
/ ## Design Notes
/ - Streaming: ReplayFrom uses streaming for large journals
/ - Batching: AppendBatch for atomic multi-entry writes
/ - Statistics: GetStats for observability
/
/ ## Usage
/ ```rust
/ // Local journaling (single node)
/ let storage = PostgresJournalStorage::new(config);
/ let facet = DurabilityFacet::new_local(storage);
/
/ // Distributed journaling (multi-node)
/ let client = JournalServiceClient::connect("http://journal-service:9090");
/ let facet = DurabilityFacet::new_remote(client);
/ ```
*/
    #[derive(Debug)]
    pub struct JournalServiceServer<T: JournalService> {
        inner: _Inner<T>,
        accept_compression_encodings: EnabledCompressionEncodings,
        send_compression_encodings: EnabledCompressionEncodings,
        max_decoding_message_size: Option<usize>,
        max_encoding_message_size: Option<usize>,
    }
    struct _Inner<T>(Arc<T>);
    impl<T: JournalService> JournalServiceServer<T> {
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
    impl<T, B> tonic::codegen::Service<http::Request<B>> for JournalServiceServer<T>
    where
        T: JournalService,
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
                "/plexspaces.journaling.v1.JournalService/Append" => {
                    #[allow(non_camel_case_types)]
                    struct AppendSvc<T: JournalService>(pub Arc<T>);
                    impl<
                        T: JournalService,
                    > tonic::server::UnaryService<super::AppendRequest>
                    for AppendSvc<T> {
                        type Response = super::AppendResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::AppendRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as JournalService>::append(&inner, request).await
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
                        let method = AppendSvc(inner);
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
                "/plexspaces.journaling.v1.JournalService/AppendBatch" => {
                    #[allow(non_camel_case_types)]
                    struct AppendBatchSvc<T: JournalService>(pub Arc<T>);
                    impl<
                        T: JournalService,
                    > tonic::server::UnaryService<super::AppendBatchRequest>
                    for AppendBatchSvc<T> {
                        type Response = super::AppendBatchResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::AppendBatchRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as JournalService>::append_batch(&inner, request).await
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
                        let method = AppendBatchSvc(inner);
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
                "/plexspaces.journaling.v1.JournalService/ReplayFrom" => {
                    #[allow(non_camel_case_types)]
                    struct ReplayFromSvc<T: JournalService>(pub Arc<T>);
                    impl<
                        T: JournalService,
                    > tonic::server::ServerStreamingService<super::ReplayFromRequest>
                    for ReplayFromSvc<T> {
                        type Response = super::JournalEntry;
                        type ResponseStream = T::ReplayFromStream;
                        type Future = BoxFuture<
                            tonic::Response<Self::ResponseStream>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ReplayFromRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as JournalService>::replay_from(&inner, request).await
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
                        let method = ReplayFromSvc(inner);
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
                        let res = grpc.server_streaming(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/plexspaces.journaling.v1.JournalService/GetLatestCheckpoint" => {
                    #[allow(non_camel_case_types)]
                    struct GetLatestCheckpointSvc<T: JournalService>(pub Arc<T>);
                    impl<
                        T: JournalService,
                    > tonic::server::UnaryService<super::GetLatestCheckpointRequest>
                    for GetLatestCheckpointSvc<T> {
                        type Response = super::Checkpoint;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::GetLatestCheckpointRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as JournalService>::get_latest_checkpoint(
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
                        let method = GetLatestCheckpointSvc(inner);
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
                "/plexspaces.journaling.v1.JournalService/SaveCheckpoint" => {
                    #[allow(non_camel_case_types)]
                    struct SaveCheckpointSvc<T: JournalService>(pub Arc<T>);
                    impl<
                        T: JournalService,
                    > tonic::server::UnaryService<super::SaveCheckpointRequest>
                    for SaveCheckpointSvc<T> {
                        type Response = super::SaveCheckpointResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::SaveCheckpointRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as JournalService>::save_checkpoint(&inner, request)
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
                        let method = SaveCheckpointSvc(inner);
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
                "/plexspaces.journaling.v1.JournalService/TruncateTo" => {
                    #[allow(non_camel_case_types)]
                    struct TruncateToSvc<T: JournalService>(pub Arc<T>);
                    impl<
                        T: JournalService,
                    > tonic::server::UnaryService<super::TruncateToRequest>
                    for TruncateToSvc<T> {
                        type Response = super::TruncateToResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::TruncateToRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as JournalService>::truncate_to(&inner, request).await
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
                        let method = TruncateToSvc(inner);
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
                "/plexspaces.journaling.v1.JournalService/GetStats" => {
                    #[allow(non_camel_case_types)]
                    struct GetStatsSvc<T: JournalService>(pub Arc<T>);
                    impl<
                        T: JournalService,
                    > tonic::server::UnaryService<super::GetStatsRequest>
                    for GetStatsSvc<T> {
                        type Response = super::JournalStats;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::GetStatsRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as JournalService>::get_stats(&inner, request).await
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
                "/plexspaces.journaling.v1.JournalService/AppendEvent" => {
                    #[allow(non_camel_case_types)]
                    struct AppendEventSvc<T: JournalService>(pub Arc<T>);
                    impl<
                        T: JournalService,
                    > tonic::server::UnaryService<super::AppendEventRequest>
                    for AppendEventSvc<T> {
                        type Response = super::AppendEventResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::AppendEventRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as JournalService>::append_event(&inner, request).await
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
                        let method = AppendEventSvc(inner);
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
                "/plexspaces.journaling.v1.JournalService/ReplayEventsFrom" => {
                    #[allow(non_camel_case_types)]
                    struct ReplayEventsFromSvc<T: JournalService>(pub Arc<T>);
                    impl<
                        T: JournalService,
                    > tonic::server::UnaryService<super::ReplayEventsFromRequest>
                    for ReplayEventsFromSvc<T> {
                        type Response = super::ReplayEventsFromResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ReplayEventsFromRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as JournalService>::replay_events_from(&inner, request)
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
                        let method = ReplayEventsFromSvc(inner);
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
                "/plexspaces.journaling.v1.JournalService/GetActorHistory" => {
                    #[allow(non_camel_case_types)]
                    struct GetActorHistorySvc<T: JournalService>(pub Arc<T>);
                    impl<
                        T: JournalService,
                    > tonic::server::UnaryService<super::GetActorHistoryRequest>
                    for GetActorHistorySvc<T> {
                        type Response = super::ActorHistory;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::GetActorHistoryRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as JournalService>::get_actor_history(&inner, request)
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
                        let method = GetActorHistorySvc(inner);
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
    impl<T: JournalService> Clone for JournalServiceServer<T> {
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
    impl<T: JournalService> Clone for _Inner<T> {
        fn clone(&self) -> Self {
            Self(Arc::clone(&self.0))
        }
    }
    impl<T: std::fmt::Debug> std::fmt::Debug for _Inner<T> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{:?}", self.0)
        }
    }
    impl<T: JournalService> tonic::server::NamedService for JournalServiceServer<T> {
        const NAME: &'static str = "plexspaces.journaling.v1.JournalService";
    }
}
