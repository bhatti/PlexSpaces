// @generated
/// Generated client implementations.
pub mod wasm_runtime_service_client {
    #![allow(unused_variables, dead_code, missing_docs, clippy::let_unit_value)]
    use tonic::codegen::*;
    use tonic::codegen::http::Uri;
    /** WASM Runtime Service

 ## Purpose
 gRPC service for WASM module deployment, actor instantiation, and migration.

 ## Why This Exists
 - **Module Lifecycle**: Deploy, cache, evict modules
 - **Actor Lifecycle**: Instantiate, migrate, terminate actors
 - **Cluster Coordination**: Distributed module registry and actor placement

 ## Design Notes
 - Service runs on every node (peer-to-peer, no central coordinator)
 - Module registry can be backed by Redis, etcd, S3
 - Actor placement can use consistent hashing or load-based scheduling
*/
    #[derive(Debug, Clone)]
    pub struct WasmRuntimeServiceClient<T> {
        inner: tonic::client::Grpc<T>,
    }
    impl WasmRuntimeServiceClient<tonic::transport::Channel> {
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
    impl<T> WasmRuntimeServiceClient<T>
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
        ) -> WasmRuntimeServiceClient<InterceptedService<T, F>>
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
            WasmRuntimeServiceClient::new(InterceptedService::new(inner, interceptor))
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
        /** Deploy WASM module to cluster registry
*/
        pub async fn deploy_module(
            &mut self,
            request: impl tonic::IntoRequest<super::DeployWasmModuleRequest>,
        ) -> std::result::Result<
            tonic::Response<super::DeployWasmModuleResponse>,
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
                "/plexspaces.wasm.v1.WasmRuntimeService/DeployModule",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.wasm.v1.WasmRuntimeService",
                        "DeployModule",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Instantiate actor from deployed module
*/
        pub async fn instantiate_actor(
            &mut self,
            request: impl tonic::IntoRequest<super::InstantiateActorRequest>,
        ) -> std::result::Result<
            tonic::Response<super::InstantiateActorResponse>,
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
                "/plexspaces.wasm.v1.WasmRuntimeService/InstantiateActor",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.wasm.v1.WasmRuntimeService",
                        "InstantiateActor",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Migrate actor to different node
*/
        pub async fn migrate_actor(
            &mut self,
            request: impl tonic::IntoRequest<super::MigrateActorRequest>,
        ) -> std::result::Result<
            tonic::Response<super::MigrateActorResponse>,
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
                "/plexspaces.wasm.v1.WasmRuntimeService/MigrateActor",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.wasm.v1.WasmRuntimeService",
                        "MigrateActor",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
    }
}
/// Generated server implementations.
pub mod wasm_runtime_service_server {
    #![allow(unused_variables, dead_code, missing_docs, clippy::let_unit_value)]
    use tonic::codegen::*;
    /// Generated trait containing gRPC methods that should be implemented for use with WasmRuntimeServiceServer.
    #[async_trait]
    pub trait WasmRuntimeService: Send + Sync + 'static {
        /** Deploy WASM module to cluster registry
*/
        async fn deploy_module(
            &self,
            request: tonic::Request<super::DeployWasmModuleRequest>,
        ) -> std::result::Result<
            tonic::Response<super::DeployWasmModuleResponse>,
            tonic::Status,
        >;
        /** Instantiate actor from deployed module
*/
        async fn instantiate_actor(
            &self,
            request: tonic::Request<super::InstantiateActorRequest>,
        ) -> std::result::Result<
            tonic::Response<super::InstantiateActorResponse>,
            tonic::Status,
        >;
        /** Migrate actor to different node
*/
        async fn migrate_actor(
            &self,
            request: tonic::Request<super::MigrateActorRequest>,
        ) -> std::result::Result<
            tonic::Response<super::MigrateActorResponse>,
            tonic::Status,
        >;
    }
    /** WASM Runtime Service

 ## Purpose
 gRPC service for WASM module deployment, actor instantiation, and migration.

 ## Why This Exists
 - **Module Lifecycle**: Deploy, cache, evict modules
 - **Actor Lifecycle**: Instantiate, migrate, terminate actors
 - **Cluster Coordination**: Distributed module registry and actor placement

 ## Design Notes
 - Service runs on every node (peer-to-peer, no central coordinator)
 - Module registry can be backed by Redis, etcd, S3
 - Actor placement can use consistent hashing or load-based scheduling
*/
    #[derive(Debug)]
    pub struct WasmRuntimeServiceServer<T: WasmRuntimeService> {
        inner: _Inner<T>,
        accept_compression_encodings: EnabledCompressionEncodings,
        send_compression_encodings: EnabledCompressionEncodings,
        max_decoding_message_size: Option<usize>,
        max_encoding_message_size: Option<usize>,
    }
    struct _Inner<T>(Arc<T>);
    impl<T: WasmRuntimeService> WasmRuntimeServiceServer<T> {
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
    impl<T, B> tonic::codegen::Service<http::Request<B>> for WasmRuntimeServiceServer<T>
    where
        T: WasmRuntimeService,
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
                "/plexspaces.wasm.v1.WasmRuntimeService/DeployModule" => {
                    #[allow(non_camel_case_types)]
                    struct DeployModuleSvc<T: WasmRuntimeService>(pub Arc<T>);
                    impl<
                        T: WasmRuntimeService,
                    > tonic::server::UnaryService<super::DeployWasmModuleRequest>
                    for DeployModuleSvc<T> {
                        type Response = super::DeployWasmModuleResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::DeployWasmModuleRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as WasmRuntimeService>::deploy_module(&inner, request)
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
                        let method = DeployModuleSvc(inner);
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
                "/plexspaces.wasm.v1.WasmRuntimeService/InstantiateActor" => {
                    #[allow(non_camel_case_types)]
                    struct InstantiateActorSvc<T: WasmRuntimeService>(pub Arc<T>);
                    impl<
                        T: WasmRuntimeService,
                    > tonic::server::UnaryService<super::InstantiateActorRequest>
                    for InstantiateActorSvc<T> {
                        type Response = super::InstantiateActorResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::InstantiateActorRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as WasmRuntimeService>::instantiate_actor(
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
                        let method = InstantiateActorSvc(inner);
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
                "/plexspaces.wasm.v1.WasmRuntimeService/MigrateActor" => {
                    #[allow(non_camel_case_types)]
                    struct MigrateActorSvc<T: WasmRuntimeService>(pub Arc<T>);
                    impl<
                        T: WasmRuntimeService,
                    > tonic::server::UnaryService<super::MigrateActorRequest>
                    for MigrateActorSvc<T> {
                        type Response = super::MigrateActorResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::MigrateActorRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as WasmRuntimeService>::migrate_actor(&inner, request)
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
                        let method = MigrateActorSvc(inner);
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
    impl<T: WasmRuntimeService> Clone for WasmRuntimeServiceServer<T> {
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
    impl<T: WasmRuntimeService> Clone for _Inner<T> {
        fn clone(&self) -> Self {
            Self(Arc::clone(&self.0))
        }
    }
    impl<T: std::fmt::Debug> std::fmt::Debug for _Inner<T> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{:?}", self.0)
        }
    }
    impl<T: WasmRuntimeService> tonic::server::NamedService
    for WasmRuntimeServiceServer<T> {
        const NAME: &'static str = "plexspaces.wasm.v1.WasmRuntimeService";
    }
}
