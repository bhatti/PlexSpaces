// @generated
/// Generated client implementations.
pub mod process_group_service_client {
    #![allow(unused_variables, dead_code, missing_docs, clippy::let_unit_value)]
    use tonic::codegen::*;
    use tonic::codegen::http::Uri;
    /** ============================================================================
 PROCESS GROUP SERVICE
 ============================================================================
 gRPC service for distributed pub/sub and broadcast messaging.
 Erlang pg/pg2-inspired with PlexSpaces enhancements (multi-tenancy, built-in broadcast).

 ## Usage
 ```rust
 // Create a group for config updates
 client.create_group(CreateGroupRequest { group_name: "config-updates", ... }).await?;

 // Actors join the group
 client.join_group(JoinGroupRequest { group_name: "config-updates", actor_id: "actor-1", ... }).await?;

 // Publish message to all members
 client.publish_to_group(PublishToGroupRequest { group_name: "config-updates", message: msg, ... }).await?;
 ```
*/
    #[derive(Debug, Clone)]
    pub struct ProcessGroupServiceClient<T> {
        inner: tonic::client::Grpc<T>,
    }
    impl ProcessGroupServiceClient<tonic::transport::Channel> {
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
    impl<T> ProcessGroupServiceClient<T>
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
        ) -> ProcessGroupServiceClient<InterceptedService<T, F>>
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
            ProcessGroupServiceClient::new(InterceptedService::new(inner, interceptor))
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
        /** Create a new process group

 ## Purpose
 Creates a named group for pub/sub coordination. Groups must be created before
 actors can join them.

 ## Semantics
 - Group names must be unique within tenant + namespace
 - Empty groups are allowed (no members)
 - Returns error if group already exists

 ## Example
 ```
 CreateGroupRequest { group_name: "config-updates", tenant_id: "acme", namespace: "prod" }
 ```
*/
        pub async fn create_group(
            &mut self,
            request: impl tonic::IntoRequest<super::CreateGroupRequest>,
        ) -> std::result::Result<
            tonic::Response<super::CreateGroupResponse>,
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
                "/plexspaces.processgroups.v1.ProcessGroupService/CreateGroup",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.processgroups.v1.ProcessGroupService",
                        "CreateGroup",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Delete a process group

 ## Purpose
 Removes a group and all its membership data. All members are automatically
 removed from the group.

 ## Semantics
 - Idempotent (deleting non-existent group succeeds)
 - All memberships removed atomically
 - Published messages pending delivery may still be delivered

 ## Example
 ```
 DeleteGroupRequest { group_name: "config-updates", tenant_id: "acme" }
 ```
*/
        pub async fn delete_group(
            &mut self,
            request: impl tonic::IntoRequest<super::DeleteGroupRequest>,
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
                "/plexspaces.processgroups.v1.ProcessGroupService/DeleteGroup",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.processgroups.v1.ProcessGroupService",
                        "DeleteGroup",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Join a process group

 ## Purpose
 Adds an actor to a group. The actor will receive messages published to the group.

 ## Semantics (Erlang pg2 compatible)
 - Actor can join same group multiple times (join_count tracked)
 - Must leave equal number of times to fully remove
 - Topics filter which messages actor receives (empty = all)
 - Returns error if group doesn't exist

 ## Example
 ```
 JoinGroupRequest { group_name: "events", actor_id: "handler-1", topics: ["user.login"] }
 ```
*/
        pub async fn join_group(
            &mut self,
            request: impl tonic::IntoRequest<super::JoinGroupRequest>,
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
                "/plexspaces.processgroups.v1.ProcessGroupService/JoinGroup",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.processgroups.v1.ProcessGroupService",
                        "JoinGroup",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Leave a process group

 ## Purpose
 Removes an actor from a group. Decrements join_count; actor fully removed when
 join_count reaches 0.

 ## Semantics (Erlang pg2 compatible)
 - Decrements join_count by 1
 - Actor removed when join_count reaches 0
 - Returns error if actor not in group

 ## Example
 ```
 LeaveGroupRequest { group_name: "events", actor_id: "handler-1" }
 ```
*/
        pub async fn leave_group(
            &mut self,
            request: impl tonic::IntoRequest<super::LeaveGroupRequest>,
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
                "/plexspaces.processgroups.v1.ProcessGroupService/LeaveGroup",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.processgroups.v1.ProcessGroupService",
                        "LeaveGroup",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Get all members of a group (cluster-wide)

 ## Purpose
 Returns all actor IDs in the group across all nodes in the cluster.

 ## Performance
 O(n) where n = total members across cluster. For large groups, use pagination.

 ## Example
 ```
 GetMembersRequest { group_name: "config-updates", tenant_id: "acme" }
 // Returns: ["actor-1", "actor-2", "actor-3"]
 ```
*/
        pub async fn get_members(
            &mut self,
            request: impl tonic::IntoRequest<super::GetMembersRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetMembersResponse>,
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
                "/plexspaces.processgroups.v1.ProcessGroupService/GetMembers",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.processgroups.v1.ProcessGroupService",
                        "GetMembers",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Get local members of a group (this node only)

 ## Purpose
 Returns only actor IDs hosted on the local node. Faster than GetMembers for
 local-only operations (no network round-trips).

 ## Use Cases
 - Local metrics collection
 - Node-local coordination
 - Optimized local broadcast

 ## Example
 ```
 GetLocalMembersRequest { group_name: "metrics-reporters", tenant_id: "acme" }
 // Returns only actors on this node
 ```
*/
        pub async fn get_local_members(
            &mut self,
            request: impl tonic::IntoRequest<super::GetLocalMembersRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetLocalMembersResponse>,
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
                "/plexspaces.processgroups.v1.ProcessGroupService/GetLocalMembers",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.processgroups.v1.ProcessGroupService",
                        "GetLocalMembers",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        /** List all groups (Erlang pg2 which_groups)

 ## Purpose
 Returns all groups matching the filter criteria. Supports pagination for
 large deployments.

 ## Filters
 - tenant_id: Required (prevents cross-tenant access)
 - namespace: Optional (filter by namespace)
 - name_pattern: Optional (glob pattern, e.g., "config-*")

 ## Example
 ```
 ListGroupsRequest { tenant_id: "acme", namespace: "prod" }
 // Returns: ["config-updates", "user-events", "cluster-nodes"]
 ```
*/
        pub async fn list_groups(
            &mut self,
            request: impl tonic::IntoRequest<super::ListGroupsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListGroupsResponse>,
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
                "/plexspaces.processgroups.v1.ProcessGroupService/ListGroups",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.processgroups.v1.ProcessGroupService",
                        "ListGroups",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Publish a message to all group members

 ## Purpose
 Broadcasts a message to all actors in the group. Handles routing to both
 local and remote actors automatically.

 ## Semantics
 - Best-effort delivery (async, may retry based on TTL)
 - Topic filtering: Only members subscribed to topic receive message
 - Empty topic: All members receive message
 - Returns count of recipients and failures

 ## Performance
 - Messages batched per remote node for efficiency
 - Local delivery bypasses gRPC layer

 ## Example
 ```
 PublishToGroupRequest {
   group_name: "config-updates",
   topic: "database",
   message: Message { payload: config_bytes, ... }
 }
 // Returns: { recipients_count: 5, failures_count: 0 }
 ```
*/
        pub async fn publish_to_group(
            &mut self,
            request: impl tonic::IntoRequest<super::PublishToGroupRequest>,
        ) -> std::result::Result<
            tonic::Response<super::PublishToGroupResponse>,
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
                "/plexspaces.processgroups.v1.ProcessGroupService/PublishToGroup",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.processgroups.v1.ProcessGroupService",
                        "PublishToGroup",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
    }
}
/// Generated server implementations.
pub mod process_group_service_server {
    #![allow(unused_variables, dead_code, missing_docs, clippy::let_unit_value)]
    use tonic::codegen::*;
    /// Generated trait containing gRPC methods that should be implemented for use with ProcessGroupServiceServer.
    #[async_trait]
    pub trait ProcessGroupService: Send + Sync + 'static {
        /** Create a new process group

 ## Purpose
 Creates a named group for pub/sub coordination. Groups must be created before
 actors can join them.

 ## Semantics
 - Group names must be unique within tenant + namespace
 - Empty groups are allowed (no members)
 - Returns error if group already exists

 ## Example
 ```
 CreateGroupRequest { group_name: "config-updates", tenant_id: "acme", namespace: "prod" }
 ```
*/
        async fn create_group(
            &self,
            request: tonic::Request<super::CreateGroupRequest>,
        ) -> std::result::Result<
            tonic::Response<super::CreateGroupResponse>,
            tonic::Status,
        >;
        /** Delete a process group

 ## Purpose
 Removes a group and all its membership data. All members are automatically
 removed from the group.

 ## Semantics
 - Idempotent (deleting non-existent group succeeds)
 - All memberships removed atomically
 - Published messages pending delivery may still be delivered

 ## Example
 ```
 DeleteGroupRequest { group_name: "config-updates", tenant_id: "acme" }
 ```
*/
        async fn delete_group(
            &self,
            request: tonic::Request<super::DeleteGroupRequest>,
        ) -> std::result::Result<
            tonic::Response<super::super::super::common::v1::Empty>,
            tonic::Status,
        >;
        /** Join a process group

 ## Purpose
 Adds an actor to a group. The actor will receive messages published to the group.

 ## Semantics (Erlang pg2 compatible)
 - Actor can join same group multiple times (join_count tracked)
 - Must leave equal number of times to fully remove
 - Topics filter which messages actor receives (empty = all)
 - Returns error if group doesn't exist

 ## Example
 ```
 JoinGroupRequest { group_name: "events", actor_id: "handler-1", topics: ["user.login"] }
 ```
*/
        async fn join_group(
            &self,
            request: tonic::Request<super::JoinGroupRequest>,
        ) -> std::result::Result<
            tonic::Response<super::super::super::common::v1::Empty>,
            tonic::Status,
        >;
        /** Leave a process group

 ## Purpose
 Removes an actor from a group. Decrements join_count; actor fully removed when
 join_count reaches 0.

 ## Semantics (Erlang pg2 compatible)
 - Decrements join_count by 1
 - Actor removed when join_count reaches 0
 - Returns error if actor not in group

 ## Example
 ```
 LeaveGroupRequest { group_name: "events", actor_id: "handler-1" }
 ```
*/
        async fn leave_group(
            &self,
            request: tonic::Request<super::LeaveGroupRequest>,
        ) -> std::result::Result<
            tonic::Response<super::super::super::common::v1::Empty>,
            tonic::Status,
        >;
        /** Get all members of a group (cluster-wide)

 ## Purpose
 Returns all actor IDs in the group across all nodes in the cluster.

 ## Performance
 O(n) where n = total members across cluster. For large groups, use pagination.

 ## Example
 ```
 GetMembersRequest { group_name: "config-updates", tenant_id: "acme" }
 // Returns: ["actor-1", "actor-2", "actor-3"]
 ```
*/
        async fn get_members(
            &self,
            request: tonic::Request<super::GetMembersRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetMembersResponse>,
            tonic::Status,
        >;
        /** Get local members of a group (this node only)

 ## Purpose
 Returns only actor IDs hosted on the local node. Faster than GetMembers for
 local-only operations (no network round-trips).

 ## Use Cases
 - Local metrics collection
 - Node-local coordination
 - Optimized local broadcast

 ## Example
 ```
 GetLocalMembersRequest { group_name: "metrics-reporters", tenant_id: "acme" }
 // Returns only actors on this node
 ```
*/
        async fn get_local_members(
            &self,
            request: tonic::Request<super::GetLocalMembersRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetLocalMembersResponse>,
            tonic::Status,
        >;
        /** List all groups (Erlang pg2 which_groups)

 ## Purpose
 Returns all groups matching the filter criteria. Supports pagination for
 large deployments.

 ## Filters
 - tenant_id: Required (prevents cross-tenant access)
 - namespace: Optional (filter by namespace)
 - name_pattern: Optional (glob pattern, e.g., "config-*")

 ## Example
 ```
 ListGroupsRequest { tenant_id: "acme", namespace: "prod" }
 // Returns: ["config-updates", "user-events", "cluster-nodes"]
 ```
*/
        async fn list_groups(
            &self,
            request: tonic::Request<super::ListGroupsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListGroupsResponse>,
            tonic::Status,
        >;
        /** Publish a message to all group members

 ## Purpose
 Broadcasts a message to all actors in the group. Handles routing to both
 local and remote actors automatically.

 ## Semantics
 - Best-effort delivery (async, may retry based on TTL)
 - Topic filtering: Only members subscribed to topic receive message
 - Empty topic: All members receive message
 - Returns count of recipients and failures

 ## Performance
 - Messages batched per remote node for efficiency
 - Local delivery bypasses gRPC layer

 ## Example
 ```
 PublishToGroupRequest {
   group_name: "config-updates",
   topic: "database",
   message: Message { payload: config_bytes, ... }
 }
 // Returns: { recipients_count: 5, failures_count: 0 }
 ```
*/
        async fn publish_to_group(
            &self,
            request: tonic::Request<super::PublishToGroupRequest>,
        ) -> std::result::Result<
            tonic::Response<super::PublishToGroupResponse>,
            tonic::Status,
        >;
    }
    /** ============================================================================
 PROCESS GROUP SERVICE
 ============================================================================
 gRPC service for distributed pub/sub and broadcast messaging.
 Erlang pg/pg2-inspired with PlexSpaces enhancements (multi-tenancy, built-in broadcast).

 ## Usage
 ```rust
 // Create a group for config updates
 client.create_group(CreateGroupRequest { group_name: "config-updates", ... }).await?;

 // Actors join the group
 client.join_group(JoinGroupRequest { group_name: "config-updates", actor_id: "actor-1", ... }).await?;

 // Publish message to all members
 client.publish_to_group(PublishToGroupRequest { group_name: "config-updates", message: msg, ... }).await?;
 ```
*/
    #[derive(Debug)]
    pub struct ProcessGroupServiceServer<T: ProcessGroupService> {
        inner: _Inner<T>,
        accept_compression_encodings: EnabledCompressionEncodings,
        send_compression_encodings: EnabledCompressionEncodings,
        max_decoding_message_size: Option<usize>,
        max_encoding_message_size: Option<usize>,
    }
    struct _Inner<T>(Arc<T>);
    impl<T: ProcessGroupService> ProcessGroupServiceServer<T> {
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
    impl<T, B> tonic::codegen::Service<http::Request<B>> for ProcessGroupServiceServer<T>
    where
        T: ProcessGroupService,
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
                "/plexspaces.processgroups.v1.ProcessGroupService/CreateGroup" => {
                    #[allow(non_camel_case_types)]
                    struct CreateGroupSvc<T: ProcessGroupService>(pub Arc<T>);
                    impl<
                        T: ProcessGroupService,
                    > tonic::server::UnaryService<super::CreateGroupRequest>
                    for CreateGroupSvc<T> {
                        type Response = super::CreateGroupResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::CreateGroupRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ProcessGroupService>::create_group(&inner, request)
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
                        let method = CreateGroupSvc(inner);
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
                "/plexspaces.processgroups.v1.ProcessGroupService/DeleteGroup" => {
                    #[allow(non_camel_case_types)]
                    struct DeleteGroupSvc<T: ProcessGroupService>(pub Arc<T>);
                    impl<
                        T: ProcessGroupService,
                    > tonic::server::UnaryService<super::DeleteGroupRequest>
                    for DeleteGroupSvc<T> {
                        type Response = super::super::super::common::v1::Empty;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::DeleteGroupRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ProcessGroupService>::delete_group(&inner, request)
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
                        let method = DeleteGroupSvc(inner);
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
                "/plexspaces.processgroups.v1.ProcessGroupService/JoinGroup" => {
                    #[allow(non_camel_case_types)]
                    struct JoinGroupSvc<T: ProcessGroupService>(pub Arc<T>);
                    impl<
                        T: ProcessGroupService,
                    > tonic::server::UnaryService<super::JoinGroupRequest>
                    for JoinGroupSvc<T> {
                        type Response = super::super::super::common::v1::Empty;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::JoinGroupRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ProcessGroupService>::join_group(&inner, request)
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
                        let method = JoinGroupSvc(inner);
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
                "/plexspaces.processgroups.v1.ProcessGroupService/LeaveGroup" => {
                    #[allow(non_camel_case_types)]
                    struct LeaveGroupSvc<T: ProcessGroupService>(pub Arc<T>);
                    impl<
                        T: ProcessGroupService,
                    > tonic::server::UnaryService<super::LeaveGroupRequest>
                    for LeaveGroupSvc<T> {
                        type Response = super::super::super::common::v1::Empty;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::LeaveGroupRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ProcessGroupService>::leave_group(&inner, request)
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
                        let method = LeaveGroupSvc(inner);
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
                "/plexspaces.processgroups.v1.ProcessGroupService/GetMembers" => {
                    #[allow(non_camel_case_types)]
                    struct GetMembersSvc<T: ProcessGroupService>(pub Arc<T>);
                    impl<
                        T: ProcessGroupService,
                    > tonic::server::UnaryService<super::GetMembersRequest>
                    for GetMembersSvc<T> {
                        type Response = super::GetMembersResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::GetMembersRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ProcessGroupService>::get_members(&inner, request)
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
                        let method = GetMembersSvc(inner);
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
                "/plexspaces.processgroups.v1.ProcessGroupService/GetLocalMembers" => {
                    #[allow(non_camel_case_types)]
                    struct GetLocalMembersSvc<T: ProcessGroupService>(pub Arc<T>);
                    impl<
                        T: ProcessGroupService,
                    > tonic::server::UnaryService<super::GetLocalMembersRequest>
                    for GetLocalMembersSvc<T> {
                        type Response = super::GetLocalMembersResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::GetLocalMembersRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ProcessGroupService>::get_local_members(
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
                        let method = GetLocalMembersSvc(inner);
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
                "/plexspaces.processgroups.v1.ProcessGroupService/ListGroups" => {
                    #[allow(non_camel_case_types)]
                    struct ListGroupsSvc<T: ProcessGroupService>(pub Arc<T>);
                    impl<
                        T: ProcessGroupService,
                    > tonic::server::UnaryService<super::ListGroupsRequest>
                    for ListGroupsSvc<T> {
                        type Response = super::ListGroupsResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ListGroupsRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ProcessGroupService>::list_groups(&inner, request)
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
                        let method = ListGroupsSvc(inner);
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
                "/plexspaces.processgroups.v1.ProcessGroupService/PublishToGroup" => {
                    #[allow(non_camel_case_types)]
                    struct PublishToGroupSvc<T: ProcessGroupService>(pub Arc<T>);
                    impl<
                        T: ProcessGroupService,
                    > tonic::server::UnaryService<super::PublishToGroupRequest>
                    for PublishToGroupSvc<T> {
                        type Response = super::PublishToGroupResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::PublishToGroupRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ProcessGroupService>::publish_to_group(
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
                        let method = PublishToGroupSvc(inner);
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
    impl<T: ProcessGroupService> Clone for ProcessGroupServiceServer<T> {
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
    impl<T: ProcessGroupService> Clone for _Inner<T> {
        fn clone(&self) -> Self {
            Self(Arc::clone(&self.0))
        }
    }
    impl<T: std::fmt::Debug> std::fmt::Debug for _Inner<T> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{:?}", self.0)
        }
    }
    impl<T: ProcessGroupService> tonic::server::NamedService
    for ProcessGroupServiceServer<T> {
        const NAME: &'static str = "plexspaces.processgroups.v1.ProcessGroupService";
    }
}
