// @generated
/// Generated client implementations.
pub mod node_service_client {
    #![allow(unused_variables, dead_code, missing_docs, clippy::let_unit_value)]
    use tonic::codegen::*;
    use tonic::codegen::http::Uri;
    /** NodeService provides node management operations

 ## Purpose
 Centralized service for:
 - Node registration and discovery
 - Health and metrics reporting
 - Capacity calculation
 - Application listing on nodes

 ## Design
 - Replaces direct Node methods with gRPC service
 - Uses ServiceLocator for accessing required services
 - Supports pagination and streaming for scalability
*/
    #[derive(Debug, Clone)]
    pub struct NodeServiceClient<T> {
        inner: tonic::client::Grpc<T>,
    }
    impl NodeServiceClient<tonic::transport::Channel> {
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
    impl<T> NodeServiceClient<T>
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
        ) -> NodeServiceClient<InterceptedService<T, F>>
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
            NodeServiceClient::new(InterceptedService::new(inner, interceptor))
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
        /** Get release specification for a node
*/
        pub async fn get_release_spec(
            &mut self,
            request: impl tonic::IntoRequest<super::GetReleaseSpecRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetReleaseSpecResponse>,
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
                "/plexspaces.node.v1.NodeService/GetReleaseSpec",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("plexspaces.node.v1.NodeService", "GetReleaseSpec"),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Register one or more nodes (replaces connect_to)
*/
        pub async fn register_nodes(
            &mut self,
            request: impl tonic::IntoRequest<super::RegisterNodesRequest>,
        ) -> std::result::Result<
            tonic::Response<super::RegisterNodesResponse>,
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
                "/plexspaces.node.v1.NodeService/RegisterNodes",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("plexspaces.node.v1.NodeService", "RegisterNodes"),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Unregister a node
*/
        pub async fn unregister_node(
            &mut self,
            request: impl tonic::IntoRequest<super::UnregisterNodeRequest>,
        ) -> std::result::Result<
            tonic::Response<super::UnregisterNodeResponse>,
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
                "/plexspaces.node.v1.NodeService/UnregisterNode",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("plexspaces.node.v1.NodeService", "UnregisterNode"),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn list_connected_nodes(
            &mut self,
            request: impl tonic::IntoRequest<super::ListConnectedNodesRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListConnectedNodesResponse>,
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
                "/plexspaces.node.v1.NodeService/ListConnectedNodes",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.node.v1.NodeService",
                        "ListConnectedNodes",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn stream_connected_nodes(
            &mut self,
            request: impl tonic::IntoRequest<super::StreamConnectedNodesRequest>,
        ) -> std::result::Result<
            tonic::Response<tonic::codec::Streaming<super::NodeRegistration>>,
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
                "/plexspaces.node.v1.NodeService/StreamConnectedNodes",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.node.v1.NodeService",
                        "StreamConnectedNodes",
                    ),
                );
            self.inner.server_streaming(req, path, codec).await
        }
        pub async fn get_metrics(
            &mut self,
            request: impl tonic::IntoRequest<super::GetMetricsRequest>,
        ) -> std::result::Result<tonic::Response<super::NodeMetrics>, tonic::Status> {
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
                "/plexspaces.node.v1.NodeService/GetMetrics",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("plexspaces.node.v1.NodeService", "GetMetrics"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn calculate_capacity(
            &mut self,
            request: impl tonic::IntoRequest<super::CalculateCapacityRequest>,
        ) -> std::result::Result<tonic::Response<super::NodeCapacity>, tonic::Status> {
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
                "/plexspaces.node.v1.NodeService/CalculateCapacity",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.node.v1.NodeService",
                        "CalculateCapacity",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn list_node_applications(
            &mut self,
            request: impl tonic::IntoRequest<super::ListNodeApplicationsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListNodeApplicationsResponse>,
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
                "/plexspaces.node.v1.NodeService/ListNodeApplications",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.node.v1.NodeService",
                        "ListNodeApplications",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn get_health(
            &mut self,
            request: impl tonic::IntoRequest<super::GetHealthRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetHealthResponse>,
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
                "/plexspaces.node.v1.NodeService/GetHealth",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("plexspaces.node.v1.NodeService", "GetHealth"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn send_heartbeat(
            &mut self,
            request: impl tonic::IntoRequest<super::SendHeartbeatRequest>,
        ) -> std::result::Result<
            tonic::Response<super::SendHeartbeatResponse>,
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
                "/plexspaces.node.v1.NodeService/SendHeartbeat",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("plexspaces.node.v1.NodeService", "SendHeartbeat"),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn ping(
            &mut self,
            request: impl tonic::IntoRequest<super::PingRequest>,
        ) -> std::result::Result<tonic::Response<super::PingResponse>, tonic::Status> {
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
                "/plexspaces.node.v1.NodeService/Ping",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("plexspaces.node.v1.NodeService", "Ping"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn ping_req(
            &mut self,
            request: impl tonic::IntoRequest<super::PingReqRequest>,
        ) -> std::result::Result<
            tonic::Response<super::PingReqResponse>,
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
                "/plexspaces.node.v1.NodeService/PingReq",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("plexspaces.node.v1.NodeService", "PingReq"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn sync_membership(
            &mut self,
            request: impl tonic::IntoRequest<super::SyncMembershipRequest>,
        ) -> std::result::Result<
            tonic::Response<super::SyncMembershipResponse>,
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
                "/plexspaces.node.v1.NodeService/SyncMembership",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("plexspaces.node.v1.NodeService", "SyncMembership"),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn connect_nodes(
            &mut self,
            request: impl tonic::IntoRequest<super::ConnectNodesRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ConnectNodesResponse>,
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
                "/plexspaces.node.v1.NodeService/ConnectNodes",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("plexspaces.node.v1.NodeService", "ConnectNodes"),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn disconnect_nodes(
            &mut self,
            request: impl tonic::IntoRequest<super::DisconnectNodesRequest>,
        ) -> std::result::Result<
            tonic::Response<super::DisconnectNodesResponse>,
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
                "/plexspaces.node.v1.NodeService/DisconnectNodes",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("plexspaces.node.v1.NodeService", "DisconnectNodes"),
                );
            self.inner.unary(req, path, codec).await
        }
    }
}
/// Generated server implementations.
pub mod node_service_server {
    #![allow(unused_variables, dead_code, missing_docs, clippy::let_unit_value)]
    use tonic::codegen::*;
    /// Generated trait containing gRPC methods that should be implemented for use with NodeServiceServer.
    #[async_trait]
    pub trait NodeService: Send + Sync + 'static {
        /** Get release specification for a node
*/
        async fn get_release_spec(
            &self,
            request: tonic::Request<super::GetReleaseSpecRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetReleaseSpecResponse>,
            tonic::Status,
        >;
        /** Register one or more nodes (replaces connect_to)
*/
        async fn register_nodes(
            &self,
            request: tonic::Request<super::RegisterNodesRequest>,
        ) -> std::result::Result<
            tonic::Response<super::RegisterNodesResponse>,
            tonic::Status,
        >;
        /** Unregister a node
*/
        async fn unregister_node(
            &self,
            request: tonic::Request<super::UnregisterNodeRequest>,
        ) -> std::result::Result<
            tonic::Response<super::UnregisterNodeResponse>,
            tonic::Status,
        >;
        async fn list_connected_nodes(
            &self,
            request: tonic::Request<super::ListConnectedNodesRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListConnectedNodesResponse>,
            tonic::Status,
        >;
        /// Server streaming response type for the StreamConnectedNodes method.
        type StreamConnectedNodesStream: tonic::codegen::tokio_stream::Stream<
                Item = std::result::Result<super::NodeRegistration, tonic::Status>,
            >
            + Send
            + 'static;
        async fn stream_connected_nodes(
            &self,
            request: tonic::Request<super::StreamConnectedNodesRequest>,
        ) -> std::result::Result<
            tonic::Response<Self::StreamConnectedNodesStream>,
            tonic::Status,
        >;
        async fn get_metrics(
            &self,
            request: tonic::Request<super::GetMetricsRequest>,
        ) -> std::result::Result<tonic::Response<super::NodeMetrics>, tonic::Status>;
        async fn calculate_capacity(
            &self,
            request: tonic::Request<super::CalculateCapacityRequest>,
        ) -> std::result::Result<tonic::Response<super::NodeCapacity>, tonic::Status>;
        async fn list_node_applications(
            &self,
            request: tonic::Request<super::ListNodeApplicationsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListNodeApplicationsResponse>,
            tonic::Status,
        >;
        async fn get_health(
            &self,
            request: tonic::Request<super::GetHealthRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetHealthResponse>,
            tonic::Status,
        >;
        async fn send_heartbeat(
            &self,
            request: tonic::Request<super::SendHeartbeatRequest>,
        ) -> std::result::Result<
            tonic::Response<super::SendHeartbeatResponse>,
            tonic::Status,
        >;
        async fn ping(
            &self,
            request: tonic::Request<super::PingRequest>,
        ) -> std::result::Result<tonic::Response<super::PingResponse>, tonic::Status>;
        async fn ping_req(
            &self,
            request: tonic::Request<super::PingReqRequest>,
        ) -> std::result::Result<tonic::Response<super::PingReqResponse>, tonic::Status>;
        async fn sync_membership(
            &self,
            request: tonic::Request<super::SyncMembershipRequest>,
        ) -> std::result::Result<
            tonic::Response<super::SyncMembershipResponse>,
            tonic::Status,
        >;
        async fn connect_nodes(
            &self,
            request: tonic::Request<super::ConnectNodesRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ConnectNodesResponse>,
            tonic::Status,
        >;
        async fn disconnect_nodes(
            &self,
            request: tonic::Request<super::DisconnectNodesRequest>,
        ) -> std::result::Result<
            tonic::Response<super::DisconnectNodesResponse>,
            tonic::Status,
        >;
    }
    /** NodeService provides node management operations

 ## Purpose
 Centralized service for:
 - Node registration and discovery
 - Health and metrics reporting
 - Capacity calculation
 - Application listing on nodes

 ## Design
 - Replaces direct Node methods with gRPC service
 - Uses ServiceLocator for accessing required services
 - Supports pagination and streaming for scalability
*/
    #[derive(Debug)]
    pub struct NodeServiceServer<T: NodeService> {
        inner: _Inner<T>,
        accept_compression_encodings: EnabledCompressionEncodings,
        send_compression_encodings: EnabledCompressionEncodings,
        max_decoding_message_size: Option<usize>,
        max_encoding_message_size: Option<usize>,
    }
    struct _Inner<T>(Arc<T>);
    impl<T: NodeService> NodeServiceServer<T> {
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
    impl<T, B> tonic::codegen::Service<http::Request<B>> for NodeServiceServer<T>
    where
        T: NodeService,
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
                "/plexspaces.node.v1.NodeService/GetReleaseSpec" => {
                    #[allow(non_camel_case_types)]
                    struct GetReleaseSpecSvc<T: NodeService>(pub Arc<T>);
                    impl<
                        T: NodeService,
                    > tonic::server::UnaryService<super::GetReleaseSpecRequest>
                    for GetReleaseSpecSvc<T> {
                        type Response = super::GetReleaseSpecResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::GetReleaseSpecRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as NodeService>::get_release_spec(&inner, request).await
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
                        let method = GetReleaseSpecSvc(inner);
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
                "/plexspaces.node.v1.NodeService/RegisterNodes" => {
                    #[allow(non_camel_case_types)]
                    struct RegisterNodesSvc<T: NodeService>(pub Arc<T>);
                    impl<
                        T: NodeService,
                    > tonic::server::UnaryService<super::RegisterNodesRequest>
                    for RegisterNodesSvc<T> {
                        type Response = super::RegisterNodesResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::RegisterNodesRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as NodeService>::register_nodes(&inner, request).await
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
                        let method = RegisterNodesSvc(inner);
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
                "/plexspaces.node.v1.NodeService/UnregisterNode" => {
                    #[allow(non_camel_case_types)]
                    struct UnregisterNodeSvc<T: NodeService>(pub Arc<T>);
                    impl<
                        T: NodeService,
                    > tonic::server::UnaryService<super::UnregisterNodeRequest>
                    for UnregisterNodeSvc<T> {
                        type Response = super::UnregisterNodeResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::UnregisterNodeRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as NodeService>::unregister_node(&inner, request).await
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
                        let method = UnregisterNodeSvc(inner);
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
                "/plexspaces.node.v1.NodeService/ListConnectedNodes" => {
                    #[allow(non_camel_case_types)]
                    struct ListConnectedNodesSvc<T: NodeService>(pub Arc<T>);
                    impl<
                        T: NodeService,
                    > tonic::server::UnaryService<super::ListConnectedNodesRequest>
                    for ListConnectedNodesSvc<T> {
                        type Response = super::ListConnectedNodesResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ListConnectedNodesRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as NodeService>::list_connected_nodes(&inner, request)
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
                        let method = ListConnectedNodesSvc(inner);
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
                "/plexspaces.node.v1.NodeService/StreamConnectedNodes" => {
                    #[allow(non_camel_case_types)]
                    struct StreamConnectedNodesSvc<T: NodeService>(pub Arc<T>);
                    impl<
                        T: NodeService,
                    > tonic::server::ServerStreamingService<
                        super::StreamConnectedNodesRequest,
                    > for StreamConnectedNodesSvc<T> {
                        type Response = super::NodeRegistration;
                        type ResponseStream = T::StreamConnectedNodesStream;
                        type Future = BoxFuture<
                            tonic::Response<Self::ResponseStream>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::StreamConnectedNodesRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as NodeService>::stream_connected_nodes(&inner, request)
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
                        let method = StreamConnectedNodesSvc(inner);
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
                "/plexspaces.node.v1.NodeService/GetMetrics" => {
                    #[allow(non_camel_case_types)]
                    struct GetMetricsSvc<T: NodeService>(pub Arc<T>);
                    impl<
                        T: NodeService,
                    > tonic::server::UnaryService<super::GetMetricsRequest>
                    for GetMetricsSvc<T> {
                        type Response = super::NodeMetrics;
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
                                <T as NodeService>::get_metrics(&inner, request).await
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
                "/plexspaces.node.v1.NodeService/CalculateCapacity" => {
                    #[allow(non_camel_case_types)]
                    struct CalculateCapacitySvc<T: NodeService>(pub Arc<T>);
                    impl<
                        T: NodeService,
                    > tonic::server::UnaryService<super::CalculateCapacityRequest>
                    for CalculateCapacitySvc<T> {
                        type Response = super::NodeCapacity;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::CalculateCapacityRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as NodeService>::calculate_capacity(&inner, request)
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
                        let method = CalculateCapacitySvc(inner);
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
                "/plexspaces.node.v1.NodeService/ListNodeApplications" => {
                    #[allow(non_camel_case_types)]
                    struct ListNodeApplicationsSvc<T: NodeService>(pub Arc<T>);
                    impl<
                        T: NodeService,
                    > tonic::server::UnaryService<super::ListNodeApplicationsRequest>
                    for ListNodeApplicationsSvc<T> {
                        type Response = super::ListNodeApplicationsResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ListNodeApplicationsRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as NodeService>::list_node_applications(&inner, request)
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
                        let method = ListNodeApplicationsSvc(inner);
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
                "/plexspaces.node.v1.NodeService/GetHealth" => {
                    #[allow(non_camel_case_types)]
                    struct GetHealthSvc<T: NodeService>(pub Arc<T>);
                    impl<
                        T: NodeService,
                    > tonic::server::UnaryService<super::GetHealthRequest>
                    for GetHealthSvc<T> {
                        type Response = super::GetHealthResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::GetHealthRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as NodeService>::get_health(&inner, request).await
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
                        let method = GetHealthSvc(inner);
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
                "/plexspaces.node.v1.NodeService/SendHeartbeat" => {
                    #[allow(non_camel_case_types)]
                    struct SendHeartbeatSvc<T: NodeService>(pub Arc<T>);
                    impl<
                        T: NodeService,
                    > tonic::server::UnaryService<super::SendHeartbeatRequest>
                    for SendHeartbeatSvc<T> {
                        type Response = super::SendHeartbeatResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::SendHeartbeatRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as NodeService>::send_heartbeat(&inner, request).await
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
                        let method = SendHeartbeatSvc(inner);
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
                "/plexspaces.node.v1.NodeService/Ping" => {
                    #[allow(non_camel_case_types)]
                    struct PingSvc<T: NodeService>(pub Arc<T>);
                    impl<T: NodeService> tonic::server::UnaryService<super::PingRequest>
                    for PingSvc<T> {
                        type Response = super::PingResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::PingRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as NodeService>::ping(&inner, request).await
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
                        let method = PingSvc(inner);
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
                "/plexspaces.node.v1.NodeService/PingReq" => {
                    #[allow(non_camel_case_types)]
                    struct PingReqSvc<T: NodeService>(pub Arc<T>);
                    impl<
                        T: NodeService,
                    > tonic::server::UnaryService<super::PingReqRequest>
                    for PingReqSvc<T> {
                        type Response = super::PingReqResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::PingReqRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as NodeService>::ping_req(&inner, request).await
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
                        let method = PingReqSvc(inner);
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
                "/plexspaces.node.v1.NodeService/SyncMembership" => {
                    #[allow(non_camel_case_types)]
                    struct SyncMembershipSvc<T: NodeService>(pub Arc<T>);
                    impl<
                        T: NodeService,
                    > tonic::server::UnaryService<super::SyncMembershipRequest>
                    for SyncMembershipSvc<T> {
                        type Response = super::SyncMembershipResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::SyncMembershipRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as NodeService>::sync_membership(&inner, request).await
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
                        let method = SyncMembershipSvc(inner);
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
                "/plexspaces.node.v1.NodeService/ConnectNodes" => {
                    #[allow(non_camel_case_types)]
                    struct ConnectNodesSvc<T: NodeService>(pub Arc<T>);
                    impl<
                        T: NodeService,
                    > tonic::server::UnaryService<super::ConnectNodesRequest>
                    for ConnectNodesSvc<T> {
                        type Response = super::ConnectNodesResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ConnectNodesRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as NodeService>::connect_nodes(&inner, request).await
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
                        let method = ConnectNodesSvc(inner);
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
                "/plexspaces.node.v1.NodeService/DisconnectNodes" => {
                    #[allow(non_camel_case_types)]
                    struct DisconnectNodesSvc<T: NodeService>(pub Arc<T>);
                    impl<
                        T: NodeService,
                    > tonic::server::UnaryService<super::DisconnectNodesRequest>
                    for DisconnectNodesSvc<T> {
                        type Response = super::DisconnectNodesResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::DisconnectNodesRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as NodeService>::disconnect_nodes(&inner, request).await
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
                        let method = DisconnectNodesSvc(inner);
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
    impl<T: NodeService> Clone for NodeServiceServer<T> {
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
    impl<T: NodeService> Clone for _Inner<T> {
        fn clone(&self) -> Self {
            Self(Arc::clone(&self.0))
        }
    }
    impl<T: std::fmt::Debug> std::fmt::Debug for _Inner<T> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{:?}", self.0)
        }
    }
    impl<T: NodeService> tonic::server::NamedService for NodeServiceServer<T> {
        const NAME: &'static str = "plexspaces.node.v1.NodeService";
    }
}
