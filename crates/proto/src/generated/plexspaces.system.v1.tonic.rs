// @generated
/// Generated client implementations.
pub mod system_service_client {
    #![allow(unused_variables, dead_code, missing_docs, clippy::let_unit_value)]
    use tonic::codegen::*;
    use tonic::codegen::http::Uri;
    /** System Management service
*/
    #[derive(Debug, Clone)]
    pub struct SystemServiceClient<T> {
        inner: tonic::client::Grpc<T>,
    }
    impl SystemServiceClient<tonic::transport::Channel> {
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
    impl<T> SystemServiceClient<T>
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
        ) -> SystemServiceClient<InterceptedService<T, F>>
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
            SystemServiceClient::new(InterceptedService::new(inner, interceptor))
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
        /** Get system information
*/
        pub async fn get_system_info(
            &mut self,
            request: impl tonic::IntoRequest<super::GetSystemInfoRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetSystemInfoResponse>,
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
                "/plexspaces.system.v1.SystemService/GetSystemInfo",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.system.v1.SystemService",
                        "GetSystemInfo",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Health check (custom detailed health, use grpc.health.v1.Health for K8s probes)
*/
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
                "/plexspaces.system.v1.SystemService/GetHealth",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("plexspaces.system.v1.SystemService", "GetHealth"),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Get detailed health with dependency checks

 ## Purpose
 Returns detailed health status including dependency checks for observability.
 Useful for debugging why a node is not ready.

 ## Design Notes
 - Includes both critical and non-critical dependency checks
 - Supports partial failure scenarios (non-critical dependencies can fail)
 - HTTP endpoint: GET /api/v1/system/health/detailed
*/
        pub async fn get_detailed_health(
            &mut self,
            request: impl tonic::IntoRequest<super::GetDetailedHealthRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetDetailedHealthResponse>,
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
                "/plexspaces.system.v1.SystemService/GetDetailedHealth",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.system.v1.SystemService",
                        "GetDetailedHealth",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Liveness probe endpoint (HTTP)

 ## Purpose
 HTTP endpoint for Kubernetes liveness probe.
 Returns 200 if node is alive, 503 if not.

 ## Design Notes
 - Lightweight check (ping, shutdown status)
 - Does not check dependencies (only basic liveness)
 - HTTP endpoint: GET /health/live
*/
        pub async fn liveness_probe(
            &mut self,
            request: impl tonic::IntoRequest<super::LivenessProbeRequest>,
        ) -> std::result::Result<
            tonic::Response<super::LivenessProbeResponse>,
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
                "/plexspaces.system.v1.SystemService/LivenessProbe",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.system.v1.SystemService",
                        "LivenessProbe",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Readiness probe endpoint (HTTP)

 ## Purpose
 HTTP endpoint for Kubernetes readiness probe.
 Returns 200 if node is ready, 503 if not.

 ## Design Notes
 - Checks critical dependencies
 - Non-critical dependencies can fail without blocking readiness
 - HTTP endpoint: GET /health/ready
*/
        pub async fn readiness_probe(
            &mut self,
            request: impl tonic::IntoRequest<super::ReadinessProbeRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ReadinessProbeResponse>,
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
                "/plexspaces.system.v1.SystemService/ReadinessProbe",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.system.v1.SystemService",
                        "ReadinessProbe",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Startup probe endpoint (HTTP)

 ## Purpose
 HTTP endpoint for Kubernetes startup probe.
 Returns 200 if startup is complete, 503 if still starting.

 ## Design Notes
 - Checks if initialization is complete
 - Checks critical dependencies required for startup
 - HTTP endpoint: GET /health/startup
*/
        pub async fn startup_probe(
            &mut self,
            request: impl tonic::IntoRequest<super::StartupProbeRequest>,
        ) -> std::result::Result<
            tonic::Response<super::StartupProbeResponse>,
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
                "/plexspaces.system.v1.SystemService/StartupProbe",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("plexspaces.system.v1.SystemService", "StartupProbe"),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Get node readiness (detailed readiness status for debugging)

 ## Purpose
 Returns detailed readiness information beyond simple SERVING/NOT_SERVING.
 Useful for debugging why a node is not ready.

 ## Design Note
 This is a custom PlexSpaces RPC. For Kubernetes probes, use the standard
 grpc.health.v1.Health service instead.
*/
        pub async fn get_node_readiness(
            &mut self,
            request: impl tonic::IntoRequest<super::GetNodeReadinessRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetNodeReadinessResponse>,
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
                "/plexspaces.system.v1.SystemService/GetNodeReadiness",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.system.v1.SystemService",
                        "GetNodeReadiness",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Mark startup complete (optional control RPC)

 ## Purpose
 Manually trigger startup complete transition (NOT_SERVING → SERVING).
 Normally called automatically by PlexSpacesNode after initialization.

 ## Use Cases
 - Manual testing
 - Custom startup sequences
 - External orchestration
*/
        pub async fn mark_startup_complete(
            &mut self,
            request: impl tonic::IntoRequest<super::MarkStartupCompleteRequest>,
        ) -> std::result::Result<
            tonic::Response<super::MarkStartupCompleteResponse>,
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
                "/plexspaces.system.v1.SystemService/MarkStartupComplete",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.system.v1.SystemService",
                        "MarkStartupComplete",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Begin shutdown (graceful shutdown with draining)

 ## Purpose
 Initiate graceful shutdown sequence:
 1. Set health to NOT_SERVING (K8s removes from service)
 2. Drain in-flight requests (configurable timeout)
 3. Return when ready for final shutdown

 ## Use Cases
 - Manual shutdown testing
 - Custom shutdown orchestration
 - Drain verification
*/
        pub async fn begin_shutdown(
            &mut self,
            request: impl tonic::IntoRequest<super::BeginShutdownRequest>,
        ) -> std::result::Result<
            tonic::Response<super::BeginShutdownResponse>,
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
                "/plexspaces.system.v1.SystemService/BeginShutdown",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.system.v1.SystemService",
                        "BeginShutdown",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Get metrics
*/
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
                "/plexspaces.system.v1.SystemService/GetMetrics",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("plexspaces.system.v1.SystemService", "GetMetrics"),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Get configuration
*/
        pub async fn get_config(
            &mut self,
            request: impl tonic::IntoRequest<super::GetConfigRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetConfigResponse>,
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
                "/plexspaces.system.v1.SystemService/GetConfig",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("plexspaces.system.v1.SystemService", "GetConfig"),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Set configuration
*/
        pub async fn set_config(
            &mut self,
            request: impl tonic::IntoRequest<super::SetConfigRequest>,
        ) -> std::result::Result<
            tonic::Response<super::SetConfigResponse>,
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
                "/plexspaces.system.v1.SystemService/SetConfig",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("plexspaces.system.v1.SystemService", "SetConfig"),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Get logs
*/
        pub async fn get_logs(
            &mut self,
            request: impl tonic::IntoRequest<super::GetLogsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetLogsResponse>,
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
                "/plexspaces.system.v1.SystemService/GetLogs",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("plexspaces.system.v1.SystemService", "GetLogs"),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Create backup
*/
        pub async fn create_backup(
            &mut self,
            request: impl tonic::IntoRequest<super::CreateBackupRequest>,
        ) -> std::result::Result<
            tonic::Response<super::CreateBackupResponse>,
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
                "/plexspaces.system.v1.SystemService/CreateBackup",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("plexspaces.system.v1.SystemService", "CreateBackup"),
                );
            self.inner.unary(req, path, codec).await
        }
        /** List backups
*/
        pub async fn list_backups(
            &mut self,
            request: impl tonic::IntoRequest<super::ListBackupsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListBackupsResponse>,
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
                "/plexspaces.system.v1.SystemService/ListBackups",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("plexspaces.system.v1.SystemService", "ListBackups"),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Restore backup
*/
        pub async fn restore_backup(
            &mut self,
            request: impl tonic::IntoRequest<super::RestoreBackupRequest>,
        ) -> std::result::Result<
            tonic::Response<super::RestoreBackupResponse>,
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
                "/plexspaces.system.v1.SystemService/RestoreBackup",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.system.v1.SystemService",
                        "RestoreBackup",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Shutdown system
*/
        pub async fn shutdown(
            &mut self,
            request: impl tonic::IntoRequest<super::ShutdownRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ShutdownResponse>,
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
                "/plexspaces.system.v1.SystemService/Shutdown",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("plexspaces.system.v1.SystemService", "Shutdown"),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Get shutdown status (observability)

 ## Purpose
 Returns detailed status of ongoing or completed shutdown.

 ## Use Cases
 - Monitor shutdown progress
 - Debug stuck shutdown phases
 - Metrics collection (shutdown duration per phase)
*/
        pub async fn get_shutdown_status(
            &mut self,
            request: impl tonic::IntoRequest<super::GetShutdownStatusRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetShutdownStatusResponse>,
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
                "/plexspaces.system.v1.SystemService/GetShutdownStatus",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.system.v1.SystemService",
                        "GetShutdownStatus",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
    }
}
/// Generated server implementations.
pub mod system_service_server {
    #![allow(unused_variables, dead_code, missing_docs, clippy::let_unit_value)]
    use tonic::codegen::*;
    /// Generated trait containing gRPC methods that should be implemented for use with SystemServiceServer.
    #[async_trait]
    pub trait SystemService: Send + Sync + 'static {
        /** Get system information
*/
        async fn get_system_info(
            &self,
            request: tonic::Request<super::GetSystemInfoRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetSystemInfoResponse>,
            tonic::Status,
        >;
        /** Health check (custom detailed health, use grpc.health.v1.Health for K8s probes)
*/
        async fn get_health(
            &self,
            request: tonic::Request<super::GetHealthRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetHealthResponse>,
            tonic::Status,
        >;
        /** Get detailed health with dependency checks

 ## Purpose
 Returns detailed health status including dependency checks for observability.
 Useful for debugging why a node is not ready.

 ## Design Notes
 - Includes both critical and non-critical dependency checks
 - Supports partial failure scenarios (non-critical dependencies can fail)
 - HTTP endpoint: GET /api/v1/system/health/detailed
*/
        async fn get_detailed_health(
            &self,
            request: tonic::Request<super::GetDetailedHealthRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetDetailedHealthResponse>,
            tonic::Status,
        >;
        /** Liveness probe endpoint (HTTP)

 ## Purpose
 HTTP endpoint for Kubernetes liveness probe.
 Returns 200 if node is alive, 503 if not.

 ## Design Notes
 - Lightweight check (ping, shutdown status)
 - Does not check dependencies (only basic liveness)
 - HTTP endpoint: GET /health/live
*/
        async fn liveness_probe(
            &self,
            request: tonic::Request<super::LivenessProbeRequest>,
        ) -> std::result::Result<
            tonic::Response<super::LivenessProbeResponse>,
            tonic::Status,
        >;
        /** Readiness probe endpoint (HTTP)

 ## Purpose
 HTTP endpoint for Kubernetes readiness probe.
 Returns 200 if node is ready, 503 if not.

 ## Design Notes
 - Checks critical dependencies
 - Non-critical dependencies can fail without blocking readiness
 - HTTP endpoint: GET /health/ready
*/
        async fn readiness_probe(
            &self,
            request: tonic::Request<super::ReadinessProbeRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ReadinessProbeResponse>,
            tonic::Status,
        >;
        /** Startup probe endpoint (HTTP)

 ## Purpose
 HTTP endpoint for Kubernetes startup probe.
 Returns 200 if startup is complete, 503 if still starting.

 ## Design Notes
 - Checks if initialization is complete
 - Checks critical dependencies required for startup
 - HTTP endpoint: GET /health/startup
*/
        async fn startup_probe(
            &self,
            request: tonic::Request<super::StartupProbeRequest>,
        ) -> std::result::Result<
            tonic::Response<super::StartupProbeResponse>,
            tonic::Status,
        >;
        /** Get node readiness (detailed readiness status for debugging)

 ## Purpose
 Returns detailed readiness information beyond simple SERVING/NOT_SERVING.
 Useful for debugging why a node is not ready.

 ## Design Note
 This is a custom PlexSpaces RPC. For Kubernetes probes, use the standard
 grpc.health.v1.Health service instead.
*/
        async fn get_node_readiness(
            &self,
            request: tonic::Request<super::GetNodeReadinessRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetNodeReadinessResponse>,
            tonic::Status,
        >;
        /** Mark startup complete (optional control RPC)

 ## Purpose
 Manually trigger startup complete transition (NOT_SERVING → SERVING).
 Normally called automatically by PlexSpacesNode after initialization.

 ## Use Cases
 - Manual testing
 - Custom startup sequences
 - External orchestration
*/
        async fn mark_startup_complete(
            &self,
            request: tonic::Request<super::MarkStartupCompleteRequest>,
        ) -> std::result::Result<
            tonic::Response<super::MarkStartupCompleteResponse>,
            tonic::Status,
        >;
        /** Begin shutdown (graceful shutdown with draining)

 ## Purpose
 Initiate graceful shutdown sequence:
 1. Set health to NOT_SERVING (K8s removes from service)
 2. Drain in-flight requests (configurable timeout)
 3. Return when ready for final shutdown

 ## Use Cases
 - Manual shutdown testing
 - Custom shutdown orchestration
 - Drain verification
*/
        async fn begin_shutdown(
            &self,
            request: tonic::Request<super::BeginShutdownRequest>,
        ) -> std::result::Result<
            tonic::Response<super::BeginShutdownResponse>,
            tonic::Status,
        >;
        /** Get metrics
*/
        async fn get_metrics(
            &self,
            request: tonic::Request<super::GetMetricsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetMetricsResponse>,
            tonic::Status,
        >;
        /** Get configuration
*/
        async fn get_config(
            &self,
            request: tonic::Request<super::GetConfigRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetConfigResponse>,
            tonic::Status,
        >;
        /** Set configuration
*/
        async fn set_config(
            &self,
            request: tonic::Request<super::SetConfigRequest>,
        ) -> std::result::Result<
            tonic::Response<super::SetConfigResponse>,
            tonic::Status,
        >;
        /** Get logs
*/
        async fn get_logs(
            &self,
            request: tonic::Request<super::GetLogsRequest>,
        ) -> std::result::Result<tonic::Response<super::GetLogsResponse>, tonic::Status>;
        /** Create backup
*/
        async fn create_backup(
            &self,
            request: tonic::Request<super::CreateBackupRequest>,
        ) -> std::result::Result<
            tonic::Response<super::CreateBackupResponse>,
            tonic::Status,
        >;
        /** List backups
*/
        async fn list_backups(
            &self,
            request: tonic::Request<super::ListBackupsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListBackupsResponse>,
            tonic::Status,
        >;
        /** Restore backup
*/
        async fn restore_backup(
            &self,
            request: tonic::Request<super::RestoreBackupRequest>,
        ) -> std::result::Result<
            tonic::Response<super::RestoreBackupResponse>,
            tonic::Status,
        >;
        /** Shutdown system
*/
        async fn shutdown(
            &self,
            request: tonic::Request<super::ShutdownRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ShutdownResponse>,
            tonic::Status,
        >;
        /** Get shutdown status (observability)

 ## Purpose
 Returns detailed status of ongoing or completed shutdown.

 ## Use Cases
 - Monitor shutdown progress
 - Debug stuck shutdown phases
 - Metrics collection (shutdown duration per phase)
*/
        async fn get_shutdown_status(
            &self,
            request: tonic::Request<super::GetShutdownStatusRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetShutdownStatusResponse>,
            tonic::Status,
        >;
    }
    /** System Management service
*/
    #[derive(Debug)]
    pub struct SystemServiceServer<T: SystemService> {
        inner: _Inner<T>,
        accept_compression_encodings: EnabledCompressionEncodings,
        send_compression_encodings: EnabledCompressionEncodings,
        max_decoding_message_size: Option<usize>,
        max_encoding_message_size: Option<usize>,
    }
    struct _Inner<T>(Arc<T>);
    impl<T: SystemService> SystemServiceServer<T> {
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
    impl<T, B> tonic::codegen::Service<http::Request<B>> for SystemServiceServer<T>
    where
        T: SystemService,
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
                "/plexspaces.system.v1.SystemService/GetSystemInfo" => {
                    #[allow(non_camel_case_types)]
                    struct GetSystemInfoSvc<T: SystemService>(pub Arc<T>);
                    impl<
                        T: SystemService,
                    > tonic::server::UnaryService<super::GetSystemInfoRequest>
                    for GetSystemInfoSvc<T> {
                        type Response = super::GetSystemInfoResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::GetSystemInfoRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as SystemService>::get_system_info(&inner, request).await
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
                        let method = GetSystemInfoSvc(inner);
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
                "/plexspaces.system.v1.SystemService/GetHealth" => {
                    #[allow(non_camel_case_types)]
                    struct GetHealthSvc<T: SystemService>(pub Arc<T>);
                    impl<
                        T: SystemService,
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
                                <T as SystemService>::get_health(&inner, request).await
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
                "/plexspaces.system.v1.SystemService/GetDetailedHealth" => {
                    #[allow(non_camel_case_types)]
                    struct GetDetailedHealthSvc<T: SystemService>(pub Arc<T>);
                    impl<
                        T: SystemService,
                    > tonic::server::UnaryService<super::GetDetailedHealthRequest>
                    for GetDetailedHealthSvc<T> {
                        type Response = super::GetDetailedHealthResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::GetDetailedHealthRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as SystemService>::get_detailed_health(&inner, request)
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
                        let method = GetDetailedHealthSvc(inner);
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
                "/plexspaces.system.v1.SystemService/LivenessProbe" => {
                    #[allow(non_camel_case_types)]
                    struct LivenessProbeSvc<T: SystemService>(pub Arc<T>);
                    impl<
                        T: SystemService,
                    > tonic::server::UnaryService<super::LivenessProbeRequest>
                    for LivenessProbeSvc<T> {
                        type Response = super::LivenessProbeResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::LivenessProbeRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as SystemService>::liveness_probe(&inner, request).await
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
                        let method = LivenessProbeSvc(inner);
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
                "/plexspaces.system.v1.SystemService/ReadinessProbe" => {
                    #[allow(non_camel_case_types)]
                    struct ReadinessProbeSvc<T: SystemService>(pub Arc<T>);
                    impl<
                        T: SystemService,
                    > tonic::server::UnaryService<super::ReadinessProbeRequest>
                    for ReadinessProbeSvc<T> {
                        type Response = super::ReadinessProbeResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ReadinessProbeRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as SystemService>::readiness_probe(&inner, request).await
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
                        let method = ReadinessProbeSvc(inner);
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
                "/plexspaces.system.v1.SystemService/StartupProbe" => {
                    #[allow(non_camel_case_types)]
                    struct StartupProbeSvc<T: SystemService>(pub Arc<T>);
                    impl<
                        T: SystemService,
                    > tonic::server::UnaryService<super::StartupProbeRequest>
                    for StartupProbeSvc<T> {
                        type Response = super::StartupProbeResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::StartupProbeRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as SystemService>::startup_probe(&inner, request).await
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
                        let method = StartupProbeSvc(inner);
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
                "/plexspaces.system.v1.SystemService/GetNodeReadiness" => {
                    #[allow(non_camel_case_types)]
                    struct GetNodeReadinessSvc<T: SystemService>(pub Arc<T>);
                    impl<
                        T: SystemService,
                    > tonic::server::UnaryService<super::GetNodeReadinessRequest>
                    for GetNodeReadinessSvc<T> {
                        type Response = super::GetNodeReadinessResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::GetNodeReadinessRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as SystemService>::get_node_readiness(&inner, request)
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
                        let method = GetNodeReadinessSvc(inner);
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
                "/plexspaces.system.v1.SystemService/MarkStartupComplete" => {
                    #[allow(non_camel_case_types)]
                    struct MarkStartupCompleteSvc<T: SystemService>(pub Arc<T>);
                    impl<
                        T: SystemService,
                    > tonic::server::UnaryService<super::MarkStartupCompleteRequest>
                    for MarkStartupCompleteSvc<T> {
                        type Response = super::MarkStartupCompleteResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::MarkStartupCompleteRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as SystemService>::mark_startup_complete(&inner, request)
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
                        let method = MarkStartupCompleteSvc(inner);
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
                "/plexspaces.system.v1.SystemService/BeginShutdown" => {
                    #[allow(non_camel_case_types)]
                    struct BeginShutdownSvc<T: SystemService>(pub Arc<T>);
                    impl<
                        T: SystemService,
                    > tonic::server::UnaryService<super::BeginShutdownRequest>
                    for BeginShutdownSvc<T> {
                        type Response = super::BeginShutdownResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::BeginShutdownRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as SystemService>::begin_shutdown(&inner, request).await
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
                        let method = BeginShutdownSvc(inner);
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
                "/plexspaces.system.v1.SystemService/GetMetrics" => {
                    #[allow(non_camel_case_types)]
                    struct GetMetricsSvc<T: SystemService>(pub Arc<T>);
                    impl<
                        T: SystemService,
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
                                <T as SystemService>::get_metrics(&inner, request).await
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
                "/plexspaces.system.v1.SystemService/GetConfig" => {
                    #[allow(non_camel_case_types)]
                    struct GetConfigSvc<T: SystemService>(pub Arc<T>);
                    impl<
                        T: SystemService,
                    > tonic::server::UnaryService<super::GetConfigRequest>
                    for GetConfigSvc<T> {
                        type Response = super::GetConfigResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::GetConfigRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as SystemService>::get_config(&inner, request).await
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
                        let method = GetConfigSvc(inner);
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
                "/plexspaces.system.v1.SystemService/SetConfig" => {
                    #[allow(non_camel_case_types)]
                    struct SetConfigSvc<T: SystemService>(pub Arc<T>);
                    impl<
                        T: SystemService,
                    > tonic::server::UnaryService<super::SetConfigRequest>
                    for SetConfigSvc<T> {
                        type Response = super::SetConfigResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::SetConfigRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as SystemService>::set_config(&inner, request).await
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
                        let method = SetConfigSvc(inner);
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
                "/plexspaces.system.v1.SystemService/GetLogs" => {
                    #[allow(non_camel_case_types)]
                    struct GetLogsSvc<T: SystemService>(pub Arc<T>);
                    impl<
                        T: SystemService,
                    > tonic::server::UnaryService<super::GetLogsRequest>
                    for GetLogsSvc<T> {
                        type Response = super::GetLogsResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::GetLogsRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as SystemService>::get_logs(&inner, request).await
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
                        let method = GetLogsSvc(inner);
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
                "/plexspaces.system.v1.SystemService/CreateBackup" => {
                    #[allow(non_camel_case_types)]
                    struct CreateBackupSvc<T: SystemService>(pub Arc<T>);
                    impl<
                        T: SystemService,
                    > tonic::server::UnaryService<super::CreateBackupRequest>
                    for CreateBackupSvc<T> {
                        type Response = super::CreateBackupResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::CreateBackupRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as SystemService>::create_backup(&inner, request).await
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
                        let method = CreateBackupSvc(inner);
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
                "/plexspaces.system.v1.SystemService/ListBackups" => {
                    #[allow(non_camel_case_types)]
                    struct ListBackupsSvc<T: SystemService>(pub Arc<T>);
                    impl<
                        T: SystemService,
                    > tonic::server::UnaryService<super::ListBackupsRequest>
                    for ListBackupsSvc<T> {
                        type Response = super::ListBackupsResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ListBackupsRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as SystemService>::list_backups(&inner, request).await
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
                        let method = ListBackupsSvc(inner);
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
                "/plexspaces.system.v1.SystemService/RestoreBackup" => {
                    #[allow(non_camel_case_types)]
                    struct RestoreBackupSvc<T: SystemService>(pub Arc<T>);
                    impl<
                        T: SystemService,
                    > tonic::server::UnaryService<super::RestoreBackupRequest>
                    for RestoreBackupSvc<T> {
                        type Response = super::RestoreBackupResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::RestoreBackupRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as SystemService>::restore_backup(&inner, request).await
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
                        let method = RestoreBackupSvc(inner);
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
                "/plexspaces.system.v1.SystemService/Shutdown" => {
                    #[allow(non_camel_case_types)]
                    struct ShutdownSvc<T: SystemService>(pub Arc<T>);
                    impl<
                        T: SystemService,
                    > tonic::server::UnaryService<super::ShutdownRequest>
                    for ShutdownSvc<T> {
                        type Response = super::ShutdownResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ShutdownRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as SystemService>::shutdown(&inner, request).await
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
                        let method = ShutdownSvc(inner);
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
                "/plexspaces.system.v1.SystemService/GetShutdownStatus" => {
                    #[allow(non_camel_case_types)]
                    struct GetShutdownStatusSvc<T: SystemService>(pub Arc<T>);
                    impl<
                        T: SystemService,
                    > tonic::server::UnaryService<super::GetShutdownStatusRequest>
                    for GetShutdownStatusSvc<T> {
                        type Response = super::GetShutdownStatusResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::GetShutdownStatusRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as SystemService>::get_shutdown_status(&inner, request)
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
                        let method = GetShutdownStatusSvc(inner);
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
    impl<T: SystemService> Clone for SystemServiceServer<T> {
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
    impl<T: SystemService> Clone for _Inner<T> {
        fn clone(&self) -> Self {
            Self(Arc::clone(&self.0))
        }
    }
    impl<T: std::fmt::Debug> std::fmt::Debug for _Inner<T> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{:?}", self.0)
        }
    }
    impl<T: SystemService> tonic::server::NamedService for SystemServiceServer<T> {
        const NAME: &'static str = "plexspaces.system.v1.SystemService";
    }
}
