// @generated
/// Generated client implementations.
pub mod blob_service_client {
    #![allow(unused_variables, dead_code, missing_docs, clippy::let_unit_value)]
    use tonic::codegen::*;
    use tonic::codegen::http::Uri;
    /** Blob Storage Service
*/
    #[derive(Debug, Clone)]
    pub struct BlobServiceClient<T> {
        inner: tonic::client::Grpc<T>,
    }
    impl BlobServiceClient<tonic::transport::Channel> {
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
    impl<T> BlobServiceClient<T>
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
        ) -> BlobServiceClient<InterceptedService<T, F>>
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
            BlobServiceClient::new(InterceptedService::new(inner, interceptor))
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
        /** Upload a blob (HTTP: POST /api/v1/blobs)
*/
        pub async fn upload_blob(
            &mut self,
            request: impl tonic::IntoRequest<super::UploadBlobRequest>,
        ) -> std::result::Result<
            tonic::Response<super::UploadBlobResponse>,
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
                "/plexspaces.storage.v1.BlobService/UploadBlob",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("plexspaces.storage.v1.BlobService", "UploadBlob"),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Download a blob (HTTP: GET /api/v1/blobs/{blob_id}/download)
*/
        pub async fn download_blob(
            &mut self,
            request: impl tonic::IntoRequest<super::DownloadBlobRequest>,
        ) -> std::result::Result<
            tonic::Response<tonic::codec::Streaming<super::DownloadBlobResponse>>,
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
                "/plexspaces.storage.v1.BlobService/DownloadBlob",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("plexspaces.storage.v1.BlobService", "DownloadBlob"),
                );
            self.inner.server_streaming(req, path, codec).await
        }
        /** Get blob metadata (HTTP: GET /api/v1/blobs/{blob_id})
*/
        pub async fn get_blob_metadata(
            &mut self,
            request: impl tonic::IntoRequest<super::GetBlobMetadataRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetBlobMetadataResponse>,
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
                "/plexspaces.storage.v1.BlobService/GetBlobMetadata",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.storage.v1.BlobService",
                        "GetBlobMetadata",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        /** List blobs (HTTP: GET /api/v1/blobs)
*/
        pub async fn list_blobs(
            &mut self,
            request: impl tonic::IntoRequest<super::ListBlobsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListBlobsResponse>,
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
                "/plexspaces.storage.v1.BlobService/ListBlobs",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("plexspaces.storage.v1.BlobService", "ListBlobs"),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Delete a blob (HTTP: DELETE /api/v1/blobs/{blob_id})
*/
        pub async fn delete_blob(
            &mut self,
            request: impl tonic::IntoRequest<super::DeleteBlobRequest>,
        ) -> std::result::Result<
            tonic::Response<super::DeleteBlobResponse>,
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
                "/plexspaces.storage.v1.BlobService/DeleteBlob",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("plexspaces.storage.v1.BlobService", "DeleteBlob"),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Generate presigned URL (HTTP: POST /api/v1/blobs/{blob_id}/presigned-url)
*/
        pub async fn generate_presigned_url(
            &mut self,
            request: impl tonic::IntoRequest<super::GeneratePresignedUrlRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GeneratePresignedUrlResponse>,
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
                "/plexspaces.storage.v1.BlobService/GeneratePresignedUrl",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.storage.v1.BlobService",
                        "GeneratePresignedUrl",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
    }
}
/// Generated server implementations.
pub mod blob_service_server {
    #![allow(unused_variables, dead_code, missing_docs, clippy::let_unit_value)]
    use tonic::codegen::*;
    /// Generated trait containing gRPC methods that should be implemented for use with BlobServiceServer.
    #[async_trait]
    pub trait BlobService: Send + Sync + 'static {
        /** Upload a blob (HTTP: POST /api/v1/blobs)
*/
        async fn upload_blob(
            &self,
            request: tonic::Request<super::UploadBlobRequest>,
        ) -> std::result::Result<
            tonic::Response<super::UploadBlobResponse>,
            tonic::Status,
        >;
        /// Server streaming response type for the DownloadBlob method.
        type DownloadBlobStream: tonic::codegen::tokio_stream::Stream<
                Item = std::result::Result<super::DownloadBlobResponse, tonic::Status>,
            >
            + Send
            + 'static;
        /** Download a blob (HTTP: GET /api/v1/blobs/{blob_id}/download)
*/
        async fn download_blob(
            &self,
            request: tonic::Request<super::DownloadBlobRequest>,
        ) -> std::result::Result<
            tonic::Response<Self::DownloadBlobStream>,
            tonic::Status,
        >;
        /** Get blob metadata (HTTP: GET /api/v1/blobs/{blob_id})
*/
        async fn get_blob_metadata(
            &self,
            request: tonic::Request<super::GetBlobMetadataRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetBlobMetadataResponse>,
            tonic::Status,
        >;
        /** List blobs (HTTP: GET /api/v1/blobs)
*/
        async fn list_blobs(
            &self,
            request: tonic::Request<super::ListBlobsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListBlobsResponse>,
            tonic::Status,
        >;
        /** Delete a blob (HTTP: DELETE /api/v1/blobs/{blob_id})
*/
        async fn delete_blob(
            &self,
            request: tonic::Request<super::DeleteBlobRequest>,
        ) -> std::result::Result<
            tonic::Response<super::DeleteBlobResponse>,
            tonic::Status,
        >;
        /** Generate presigned URL (HTTP: POST /api/v1/blobs/{blob_id}/presigned-url)
*/
        async fn generate_presigned_url(
            &self,
            request: tonic::Request<super::GeneratePresignedUrlRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GeneratePresignedUrlResponse>,
            tonic::Status,
        >;
    }
    /** Blob Storage Service
*/
    #[derive(Debug)]
    pub struct BlobServiceServer<T: BlobService> {
        inner: _Inner<T>,
        accept_compression_encodings: EnabledCompressionEncodings,
        send_compression_encodings: EnabledCompressionEncodings,
        max_decoding_message_size: Option<usize>,
        max_encoding_message_size: Option<usize>,
    }
    struct _Inner<T>(Arc<T>);
    impl<T: BlobService> BlobServiceServer<T> {
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
    impl<T, B> tonic::codegen::Service<http::Request<B>> for BlobServiceServer<T>
    where
        T: BlobService,
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
                "/plexspaces.storage.v1.BlobService/UploadBlob" => {
                    #[allow(non_camel_case_types)]
                    struct UploadBlobSvc<T: BlobService>(pub Arc<T>);
                    impl<
                        T: BlobService,
                    > tonic::server::UnaryService<super::UploadBlobRequest>
                    for UploadBlobSvc<T> {
                        type Response = super::UploadBlobResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::UploadBlobRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as BlobService>::upload_blob(&inner, request).await
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
                        let method = UploadBlobSvc(inner);
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
                "/plexspaces.storage.v1.BlobService/DownloadBlob" => {
                    #[allow(non_camel_case_types)]
                    struct DownloadBlobSvc<T: BlobService>(pub Arc<T>);
                    impl<
                        T: BlobService,
                    > tonic::server::ServerStreamingService<super::DownloadBlobRequest>
                    for DownloadBlobSvc<T> {
                        type Response = super::DownloadBlobResponse;
                        type ResponseStream = T::DownloadBlobStream;
                        type Future = BoxFuture<
                            tonic::Response<Self::ResponseStream>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::DownloadBlobRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as BlobService>::download_blob(&inner, request).await
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
                        let method = DownloadBlobSvc(inner);
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
                "/plexspaces.storage.v1.BlobService/GetBlobMetadata" => {
                    #[allow(non_camel_case_types)]
                    struct GetBlobMetadataSvc<T: BlobService>(pub Arc<T>);
                    impl<
                        T: BlobService,
                    > tonic::server::UnaryService<super::GetBlobMetadataRequest>
                    for GetBlobMetadataSvc<T> {
                        type Response = super::GetBlobMetadataResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::GetBlobMetadataRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as BlobService>::get_blob_metadata(&inner, request).await
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
                        let method = GetBlobMetadataSvc(inner);
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
                "/plexspaces.storage.v1.BlobService/ListBlobs" => {
                    #[allow(non_camel_case_types)]
                    struct ListBlobsSvc<T: BlobService>(pub Arc<T>);
                    impl<
                        T: BlobService,
                    > tonic::server::UnaryService<super::ListBlobsRequest>
                    for ListBlobsSvc<T> {
                        type Response = super::ListBlobsResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ListBlobsRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as BlobService>::list_blobs(&inner, request).await
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
                        let method = ListBlobsSvc(inner);
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
                "/plexspaces.storage.v1.BlobService/DeleteBlob" => {
                    #[allow(non_camel_case_types)]
                    struct DeleteBlobSvc<T: BlobService>(pub Arc<T>);
                    impl<
                        T: BlobService,
                    > tonic::server::UnaryService<super::DeleteBlobRequest>
                    for DeleteBlobSvc<T> {
                        type Response = super::DeleteBlobResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::DeleteBlobRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as BlobService>::delete_blob(&inner, request).await
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
                        let method = DeleteBlobSvc(inner);
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
                "/plexspaces.storage.v1.BlobService/GeneratePresignedUrl" => {
                    #[allow(non_camel_case_types)]
                    struct GeneratePresignedUrlSvc<T: BlobService>(pub Arc<T>);
                    impl<
                        T: BlobService,
                    > tonic::server::UnaryService<super::GeneratePresignedUrlRequest>
                    for GeneratePresignedUrlSvc<T> {
                        type Response = super::GeneratePresignedUrlResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::GeneratePresignedUrlRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as BlobService>::generate_presigned_url(&inner, request)
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
                        let method = GeneratePresignedUrlSvc(inner);
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
    impl<T: BlobService> Clone for BlobServiceServer<T> {
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
    impl<T: BlobService> Clone for _Inner<T> {
        fn clone(&self) -> Self {
            Self(Arc::clone(&self.0))
        }
    }
    impl<T: std::fmt::Debug> std::fmt::Debug for _Inner<T> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{:?}", self.0)
        }
    }
    impl<T: BlobService> tonic::server::NamedService for BlobServiceServer<T> {
        const NAME: &'static str = "plexspaces.storage.v1.BlobService";
    }
}
