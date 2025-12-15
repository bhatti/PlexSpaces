// @generated
/// Generated client implementations.
pub mod circuit_breaker_service_client {
    #![allow(unused_variables, dead_code, missing_docs, clippy::let_unit_value)]
    use tonic::codegen::*;
    use tonic::codegen::http::Uri;
    /** Circuit breaker service
*/
    #[derive(Debug, Clone)]
    pub struct CircuitBreakerServiceClient<T> {
        inner: tonic::client::Grpc<T>,
    }
    impl CircuitBreakerServiceClient<tonic::transport::Channel> {
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
    impl<T> CircuitBreakerServiceClient<T>
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
        ) -> CircuitBreakerServiceClient<InterceptedService<T, F>>
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
            CircuitBreakerServiceClient::new(InterceptedService::new(inner, interceptor))
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
        pub async fn create_circuit_breaker(
            &mut self,
            request: impl tonic::IntoRequest<super::CreateCircuitBreakerRequest>,
        ) -> std::result::Result<
            tonic::Response<super::CreateCircuitBreakerResponse>,
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
                "/plexspaces.circuitbreaker.prv.CircuitBreakerService/CreateCircuitBreaker",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.circuitbreaker.prv.CircuitBreakerService",
                        "CreateCircuitBreaker",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn execute_request(
            &mut self,
            request: impl tonic::IntoRequest<super::ExecuteRequestRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ExecuteRequestResponse>,
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
                "/plexspaces.circuitbreaker.prv.CircuitBreakerService/ExecuteRequest",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.circuitbreaker.prv.CircuitBreakerService",
                        "ExecuteRequest",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn record_result(
            &mut self,
            request: impl tonic::IntoRequest<super::RecordResultRequest>,
        ) -> std::result::Result<
            tonic::Response<super::RecordResultResponse>,
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
                "/plexspaces.circuitbreaker.prv.CircuitBreakerService/RecordResult",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.circuitbreaker.prv.CircuitBreakerService",
                        "RecordResult",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn trip_circuit(
            &mut self,
            request: impl tonic::IntoRequest<super::TripCircuitRequest>,
        ) -> std::result::Result<
            tonic::Response<super::TripCircuitResponse>,
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
                "/plexspaces.circuitbreaker.prv.CircuitBreakerService/TripCircuit",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.circuitbreaker.prv.CircuitBreakerService",
                        "TripCircuit",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn reset_circuit(
            &mut self,
            request: impl tonic::IntoRequest<super::ResetCircuitRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ResetCircuitResponse>,
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
                "/plexspaces.circuitbreaker.prv.CircuitBreakerService/ResetCircuit",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.circuitbreaker.prv.CircuitBreakerService",
                        "ResetCircuit",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn get_state(
            &mut self,
            request: impl tonic::IntoRequest<super::GetStateRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetStateResponse>,
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
                "/plexspaces.circuitbreaker.prv.CircuitBreakerService/GetState",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.circuitbreaker.prv.CircuitBreakerService",
                        "GetState",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn get_metrics(
            &mut self,
            request: impl tonic::IntoRequest<super::GetMetricsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetMetricsResponse>,
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
                "/plexspaces.circuitbreaker.prv.CircuitBreakerService/GetMetrics",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.circuitbreaker.prv.CircuitBreakerService",
                        "GetMetrics",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn watch_events(
            &mut self,
            request: impl tonic::IntoRequest<super::WatchEventsRequest>,
        ) -> std::result::Result<
            tonic::Response<tonic::codec::Streaming<super::CircuitBreakerEvent>>,
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
                "/plexspaces.circuitbreaker.prv.CircuitBreakerService/WatchEvents",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.circuitbreaker.prv.CircuitBreakerService",
                        "WatchEvents",
                    ),
                );
            self.inner.server_streaming(req, path, codec).await
        }
        pub async fn list_circuit_breakers(
            &mut self,
            request: impl tonic::IntoRequest<super::ListCircuitBreakersRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListCircuitBreakersResponse>,
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
                "/plexspaces.circuitbreaker.prv.CircuitBreakerService/ListCircuitBreakers",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.circuitbreaker.prv.CircuitBreakerService",
                        "ListCircuitBreakers",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn delete_circuit_breaker(
            &mut self,
            request: impl tonic::IntoRequest<super::DeleteCircuitBreakerRequest>,
        ) -> std::result::Result<
            tonic::Response<super::DeleteCircuitBreakerResponse>,
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
                "/plexspaces.circuitbreaker.prv.CircuitBreakerService/DeleteCircuitBreaker",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.circuitbreaker.prv.CircuitBreakerService",
                        "DeleteCircuitBreaker",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
    }
}
/// Generated server implementations.
pub mod circuit_breaker_service_server {
    #![allow(unused_variables, dead_code, missing_docs, clippy::let_unit_value)]
    use tonic::codegen::*;
    /// Generated trait containing gRPC methods that should be implemented for use with CircuitBreakerServiceServer.
    #[async_trait]
    pub trait CircuitBreakerService: Send + Sync + 'static {
        async fn create_circuit_breaker(
            &self,
            request: tonic::Request<super::CreateCircuitBreakerRequest>,
        ) -> std::result::Result<
            tonic::Response<super::CreateCircuitBreakerResponse>,
            tonic::Status,
        >;
        async fn execute_request(
            &self,
            request: tonic::Request<super::ExecuteRequestRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ExecuteRequestResponse>,
            tonic::Status,
        >;
        async fn record_result(
            &self,
            request: tonic::Request<super::RecordResultRequest>,
        ) -> std::result::Result<
            tonic::Response<super::RecordResultResponse>,
            tonic::Status,
        >;
        async fn trip_circuit(
            &self,
            request: tonic::Request<super::TripCircuitRequest>,
        ) -> std::result::Result<
            tonic::Response<super::TripCircuitResponse>,
            tonic::Status,
        >;
        async fn reset_circuit(
            &self,
            request: tonic::Request<super::ResetCircuitRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ResetCircuitResponse>,
            tonic::Status,
        >;
        async fn get_state(
            &self,
            request: tonic::Request<super::GetStateRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetStateResponse>,
            tonic::Status,
        >;
        async fn get_metrics(
            &self,
            request: tonic::Request<super::GetMetricsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetMetricsResponse>,
            tonic::Status,
        >;
        /// Server streaming response type for the WatchEvents method.
        type WatchEventsStream: tonic::codegen::tokio_stream::Stream<
                Item = std::result::Result<super::CircuitBreakerEvent, tonic::Status>,
            >
            + Send
            + 'static;
        async fn watch_events(
            &self,
            request: tonic::Request<super::WatchEventsRequest>,
        ) -> std::result::Result<
            tonic::Response<Self::WatchEventsStream>,
            tonic::Status,
        >;
        async fn list_circuit_breakers(
            &self,
            request: tonic::Request<super::ListCircuitBreakersRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListCircuitBreakersResponse>,
            tonic::Status,
        >;
        async fn delete_circuit_breaker(
            &self,
            request: tonic::Request<super::DeleteCircuitBreakerRequest>,
        ) -> std::result::Result<
            tonic::Response<super::DeleteCircuitBreakerResponse>,
            tonic::Status,
        >;
    }
    /** Circuit breaker service
*/
    #[derive(Debug)]
    pub struct CircuitBreakerServiceServer<T: CircuitBreakerService> {
        inner: _Inner<T>,
        accept_compression_encodings: EnabledCompressionEncodings,
        send_compression_encodings: EnabledCompressionEncodings,
        max_decoding_message_size: Option<usize>,
        max_encoding_message_size: Option<usize>,
    }
    struct _Inner<T>(Arc<T>);
    impl<T: CircuitBreakerService> CircuitBreakerServiceServer<T> {
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
    impl<T, B> tonic::codegen::Service<http::Request<B>>
    for CircuitBreakerServiceServer<T>
    where
        T: CircuitBreakerService,
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
                "/plexspaces.circuitbreaker.prv.CircuitBreakerService/CreateCircuitBreaker" => {
                    #[allow(non_camel_case_types)]
                    struct CreateCircuitBreakerSvc<T: CircuitBreakerService>(pub Arc<T>);
                    impl<
                        T: CircuitBreakerService,
                    > tonic::server::UnaryService<super::CreateCircuitBreakerRequest>
                    for CreateCircuitBreakerSvc<T> {
                        type Response = super::CreateCircuitBreakerResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::CreateCircuitBreakerRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as CircuitBreakerService>::create_circuit_breaker(
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
                        let method = CreateCircuitBreakerSvc(inner);
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
                "/plexspaces.circuitbreaker.prv.CircuitBreakerService/ExecuteRequest" => {
                    #[allow(non_camel_case_types)]
                    struct ExecuteRequestSvc<T: CircuitBreakerService>(pub Arc<T>);
                    impl<
                        T: CircuitBreakerService,
                    > tonic::server::UnaryService<super::ExecuteRequestRequest>
                    for ExecuteRequestSvc<T> {
                        type Response = super::ExecuteRequestResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ExecuteRequestRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as CircuitBreakerService>::execute_request(
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
                        let method = ExecuteRequestSvc(inner);
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
                "/plexspaces.circuitbreaker.prv.CircuitBreakerService/RecordResult" => {
                    #[allow(non_camel_case_types)]
                    struct RecordResultSvc<T: CircuitBreakerService>(pub Arc<T>);
                    impl<
                        T: CircuitBreakerService,
                    > tonic::server::UnaryService<super::RecordResultRequest>
                    for RecordResultSvc<T> {
                        type Response = super::RecordResultResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::RecordResultRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as CircuitBreakerService>::record_result(&inner, request)
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
                        let method = RecordResultSvc(inner);
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
                "/plexspaces.circuitbreaker.prv.CircuitBreakerService/TripCircuit" => {
                    #[allow(non_camel_case_types)]
                    struct TripCircuitSvc<T: CircuitBreakerService>(pub Arc<T>);
                    impl<
                        T: CircuitBreakerService,
                    > tonic::server::UnaryService<super::TripCircuitRequest>
                    for TripCircuitSvc<T> {
                        type Response = super::TripCircuitResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::TripCircuitRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as CircuitBreakerService>::trip_circuit(&inner, request)
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
                        let method = TripCircuitSvc(inner);
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
                "/plexspaces.circuitbreaker.prv.CircuitBreakerService/ResetCircuit" => {
                    #[allow(non_camel_case_types)]
                    struct ResetCircuitSvc<T: CircuitBreakerService>(pub Arc<T>);
                    impl<
                        T: CircuitBreakerService,
                    > tonic::server::UnaryService<super::ResetCircuitRequest>
                    for ResetCircuitSvc<T> {
                        type Response = super::ResetCircuitResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ResetCircuitRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as CircuitBreakerService>::reset_circuit(&inner, request)
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
                        let method = ResetCircuitSvc(inner);
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
                "/plexspaces.circuitbreaker.prv.CircuitBreakerService/GetState" => {
                    #[allow(non_camel_case_types)]
                    struct GetStateSvc<T: CircuitBreakerService>(pub Arc<T>);
                    impl<
                        T: CircuitBreakerService,
                    > tonic::server::UnaryService<super::GetStateRequest>
                    for GetStateSvc<T> {
                        type Response = super::GetStateResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::GetStateRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as CircuitBreakerService>::get_state(&inner, request)
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
                        let method = GetStateSvc(inner);
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
                "/plexspaces.circuitbreaker.prv.CircuitBreakerService/GetMetrics" => {
                    #[allow(non_camel_case_types)]
                    struct GetMetricsSvc<T: CircuitBreakerService>(pub Arc<T>);
                    impl<
                        T: CircuitBreakerService,
                    > tonic::server::UnaryService<super::GetMetricsRequest>
                    for GetMetricsSvc<T> {
                        type Response = super::GetMetricsResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::GetMetricsRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as CircuitBreakerService>::get_metrics(&inner, request)
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
                        let method = GetMetricsSvc(inner);
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
                "/plexspaces.circuitbreaker.prv.CircuitBreakerService/WatchEvents" => {
                    #[allow(non_camel_case_types)]
                    struct WatchEventsSvc<T: CircuitBreakerService>(pub Arc<T>);
                    impl<
                        T: CircuitBreakerService,
                    > tonic::server::ServerStreamingService<super::WatchEventsRequest>
                    for WatchEventsSvc<T> {
                        type Response = super::CircuitBreakerEvent;
                        type ResponseStream = T::WatchEventsStream;
                        type Future = BoxFuture<
                            tonic::Response<Self::ResponseStream>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::WatchEventsRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as CircuitBreakerService>::watch_events(&inner, request)
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
                        let method = WatchEventsSvc(inner);
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
                "/plexspaces.circuitbreaker.prv.CircuitBreakerService/ListCircuitBreakers" => {
                    #[allow(non_camel_case_types)]
                    struct ListCircuitBreakersSvc<T: CircuitBreakerService>(pub Arc<T>);
                    impl<
                        T: CircuitBreakerService,
                    > tonic::server::UnaryService<super::ListCircuitBreakersRequest>
                    for ListCircuitBreakersSvc<T> {
                        type Response = super::ListCircuitBreakersResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ListCircuitBreakersRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as CircuitBreakerService>::list_circuit_breakers(
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
                        let method = ListCircuitBreakersSvc(inner);
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
                "/plexspaces.circuitbreaker.prv.CircuitBreakerService/DeleteCircuitBreaker" => {
                    #[allow(non_camel_case_types)]
                    struct DeleteCircuitBreakerSvc<T: CircuitBreakerService>(pub Arc<T>);
                    impl<
                        T: CircuitBreakerService,
                    > tonic::server::UnaryService<super::DeleteCircuitBreakerRequest>
                    for DeleteCircuitBreakerSvc<T> {
                        type Response = super::DeleteCircuitBreakerResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::DeleteCircuitBreakerRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as CircuitBreakerService>::delete_circuit_breaker(
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
                        let method = DeleteCircuitBreakerSvc(inner);
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
    impl<T: CircuitBreakerService> Clone for CircuitBreakerServiceServer<T> {
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
    impl<T: CircuitBreakerService> Clone for _Inner<T> {
        fn clone(&self) -> Self {
            Self(Arc::clone(&self.0))
        }
    }
    impl<T: std::fmt::Debug> std::fmt::Debug for _Inner<T> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{:?}", self.0)
        }
    }
    impl<T: CircuitBreakerService> tonic::server::NamedService
    for CircuitBreakerServiceServer<T> {
        const NAME: &'static str = "plexspaces.circuitbreaker.prv.CircuitBreakerService";
    }
}
