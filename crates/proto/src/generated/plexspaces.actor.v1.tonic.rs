// @generated
/// Generated client implementations.
pub mod actor_service_client {
    #![allow(unused_variables, dead_code, missing_docs, clippy::let_unit_value)]
    use tonic::codegen::*;
    use tonic::codegen::http::Uri;
    /** Actor system service
*/
    #[derive(Debug, Clone)]
    pub struct ActorServiceClient<T> {
        inner: tonic::client::Grpc<T>,
    }
    impl ActorServiceClient<tonic::transport::Channel> {
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
    impl<T> ActorServiceClient<T>
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
        ) -> ActorServiceClient<InterceptedService<T, F>>
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
            ActorServiceClient::new(InterceptedService::new(inner, interceptor))
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
        pub async fn spawn_actor(
            &mut self,
            request: impl tonic::IntoRequest<super::SpawnActorRequest>,
        ) -> std::result::Result<
            tonic::Response<super::SpawnActorResponse>,
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
                "/plexspaces.actor.v1.ActorService/SpawnActor",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("plexspaces.actor.v1.ActorService", "SpawnActor"),
                );
            self.inner.unary(req, path, codec).await
        }
        ///
        pub async fn spawn_actors(
            &mut self,
            request: impl tonic::IntoRequest<super::SpawnActorsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::SpawnActorsResponse>,
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
                "/plexspaces.actor.v1.ActorService/SpawnActors",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("plexspaces.actor.v1.ActorService", "SpawnActors"),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Get an actor by ID
*/
        pub async fn get_actor(
            &mut self,
            request: impl tonic::IntoRequest<super::GetActorRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetActorResponse>,
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
                "/plexspaces.actor.v1.ActorService/GetActor",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("plexspaces.actor.v1.ActorService", "GetActor"));
            self.inner.unary(req, path, codec).await
        }
        /** List actors with filtering
*/
        pub async fn list_actors(
            &mut self,
            request: impl tonic::IntoRequest<super::ListActorsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListActorsResponse>,
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
                "/plexspaces.actor.v1.ActorService/ListActors",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("plexspaces.actor.v1.ActorService", "ListActors"),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Send message to an actor using tell semantics
*/
        pub async fn send_message(
            &mut self,
            request: impl tonic::IntoRequest<super::SendMessageRequest>,
        ) -> std::result::Result<
            tonic::Response<super::SendMessageResponse>,
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
                "/plexspaces.actor.v1.ActorService/SendMessage",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("plexspaces.actor.v1.ActorService", "SendMessage"),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Stream messages for high-throughput scenarios (Erlang-inspired)
 Use this for bulk message passing or event streaming
*/
        pub async fn stream_messages(
            &mut self,
            request: impl tonic::IntoStreamingRequest<
                Message = super::StreamMessageRequest,
            >,
        ) -> std::result::Result<
            tonic::Response<tonic::codec::Streaming<super::StreamMessageResponse>>,
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
                "/plexspaces.actor.v1.ActorService/StreamMessages",
            );
            let mut req = request.into_streaming_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("plexspaces.actor.v1.ActorService", "StreamMessages"),
                );
            self.inner.streaming(req, path, codec).await
        }
        /** Change actor state
*/
        pub async fn set_actor_state(
            &mut self,
            request: impl tonic::IntoRequest<super::SetActorStateRequest>,
        ) -> std::result::Result<
            tonic::Response<super::SetActorStateResponse>,
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
                "/plexspaces.actor.v1.ActorService/SetActorState",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("plexspaces.actor.v1.ActorService", "SetActorState"),
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
                "/plexspaces.actor.v1.ActorService/MigrateActor",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("plexspaces.actor.v1.ActorService", "MigrateActor"),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn delete_actor(
            &mut self,
            request: impl tonic::IntoRequest<super::DeleteActorRequest>,
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
                "/plexspaces.actor.v1.ActorService/DeleteActor",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("plexspaces.actor.v1.ActorService", "DeleteActor"),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn monitor_actor(
            &mut self,
            request: impl tonic::IntoRequest<super::MonitorActorRequest>,
        ) -> std::result::Result<
            tonic::Response<super::MonitorActorResponse>,
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
                "/plexspaces.actor.v1.ActorService/MonitorActor",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("plexspaces.actor.v1.ActorService", "MonitorActor"),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn demonitor_actor(
            &mut self,
            request: impl tonic::IntoRequest<super::DemonitorActorRequest>,
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
                "/plexspaces.actor.v1.ActorService/DemonitorActor",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("plexspaces.actor.v1.ActorService", "DemonitorActor"),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn link_actor(
            &mut self,
            request: impl tonic::IntoRequest<super::LinkActorRequest>,
        ) -> std::result::Result<
            tonic::Response<super::LinkActorResponse>,
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
                "/plexspaces.actor.v1.ActorService/LinkActor",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("plexspaces.actor.v1.ActorService", "LinkActor"),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn unlink_actor(
            &mut self,
            request: impl tonic::IntoRequest<super::UnlinkActorRequest>,
        ) -> std::result::Result<
            tonic::Response<super::UnlinkActorResponse>,
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
                "/plexspaces.actor.v1.ActorService/UnlinkActor",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("plexspaces.actor.v1.ActorService", "UnlinkActor"),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn notify_actor_down(
            &mut self,
            request: impl tonic::IntoRequest<super::ActorDownNotification>,
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
                "/plexspaces.actor.v1.ActorService/NotifyActorDown",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.actor.v1.ActorService",
                        "NotifyActorDown",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn check_actor_exists(
            &mut self,
            request: impl tonic::IntoRequest<super::CheckActorExistsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::CheckActorExistsResponse>,
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
                "/plexspaces.actor.v1.ActorService/CheckActorExists",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.actor.v1.ActorService",
                        "CheckActorExists",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn get_actor_states(
            &mut self,
            request: impl tonic::IntoRequest<super::GetActorStatesRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetActorStatesResponse>,
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
                "/plexspaces.actor.v1.ActorService/GetActorStates",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("plexspaces.actor.v1.ActorService", "GetActorStates"),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn ask_reply(
            &mut self,
            request: impl tonic::IntoRequest<super::AskReplyRequest>,
        ) -> std::result::Result<
            tonic::Response<super::AskReplyResponse>,
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
                "/plexspaces.actor.v1.ActorService/AskReply",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("plexspaces.actor.v1.ActorService", "AskReply"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn create_shard_group(
            &mut self,
            request: impl tonic::IntoRequest<super::CreateShardGroupRequest>,
        ) -> std::result::Result<
            tonic::Response<super::CreateShardGroupResponse>,
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
                "/plexspaces.actor.v1.ActorService/CreateShardGroup",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.actor.v1.ActorService",
                        "CreateShardGroup",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn delete_shard_group(
            &mut self,
            request: impl tonic::IntoRequest<super::DeleteShardGroupRequest>,
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
                "/plexspaces.actor.v1.ActorService/DeleteShardGroup",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.actor.v1.ActorService",
                        "DeleteShardGroup",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn get_shard_group(
            &mut self,
            request: impl tonic::IntoRequest<super::GetShardGroupRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetShardGroupResponse>,
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
                "/plexspaces.actor.v1.ActorService/GetShardGroup",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("plexspaces.actor.v1.ActorService", "GetShardGroup"),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn list_shard_groups(
            &mut self,
            request: impl tonic::IntoRequest<super::ListShardGroupsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListShardGroupsResponse>,
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
                "/plexspaces.actor.v1.ActorService/ListShardGroups",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.actor.v1.ActorService",
                        "ListShardGroups",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn scale_shard_group(
            &mut self,
            request: impl tonic::IntoRequest<super::ScaleShardGroupRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ScaleShardGroupResponse>,
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
                "/plexspaces.actor.v1.ActorService/ScaleShardGroup",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.actor.v1.ActorService",
                        "ScaleShardGroup",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn send_to_shard(
            &mut self,
            request: impl tonic::IntoRequest<super::SendToShardRequest>,
        ) -> std::result::Result<
            tonic::Response<super::SendToShardResponse>,
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
                "/plexspaces.actor.v1.ActorService/SendToShard",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("plexspaces.actor.v1.ActorService", "SendToShard"),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn broadcast_shard_group(
            &mut self,
            request: impl tonic::IntoRequest<super::BroadcastShardGroupRequest>,
        ) -> std::result::Result<
            tonic::Response<super::BroadcastShardGroupResponse>,
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
                "/plexspaces.actor.v1.ActorService/BroadcastShardGroup",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.actor.v1.ActorService",
                        "BroadcastShardGroup",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn reduce_shard_group(
            &mut self,
            request: impl tonic::IntoRequest<super::ReduceShardGroupRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ReduceShardGroupResponse>,
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
                "/plexspaces.actor.v1.ActorService/ReduceShardGroup",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.actor.v1.ActorService",
                        "ReduceShardGroup",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn all_reduce_shard_group(
            &mut self,
            request: impl tonic::IntoRequest<super::AllReduceShardGroupRequest>,
        ) -> std::result::Result<
            tonic::Response<super::AllReduceShardGroupResponse>,
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
                "/plexspaces.actor.v1.ActorService/AllReduceShardGroup",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.actor.v1.ActorService",
                        "AllReduceShardGroup",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn barrier_shard_group(
            &mut self,
            request: impl tonic::IntoRequest<super::BarrierShardGroupRequest>,
        ) -> std::result::Result<
            tonic::Response<super::BarrierShardGroupResponse>,
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
                "/plexspaces.actor.v1.ActorService/BarrierShardGroup",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.actor.v1.ActorService",
                        "BarrierShardGroup",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn scatter_gather(
            &mut self,
            request: impl tonic::IntoRequest<super::ScatterGatherRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ScatterGatherResponse>,
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
                "/plexspaces.actor.v1.ActorService/ScatterGather",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("plexspaces.actor.v1.ActorService", "ScatterGather"),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn bulk_update_shard_group(
            &mut self,
            request: impl tonic::IntoRequest<super::BulkUpdateShardGroupRequest>,
        ) -> std::result::Result<
            tonic::Response<super::BulkUpdateShardGroupResponse>,
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
                "/plexspaces.actor.v1.ActorService/BulkUpdateShardGroup",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.actor.v1.ActorService",
                        "BulkUpdateShardGroup",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn map_shard_group(
            &mut self,
            request: impl tonic::IntoRequest<super::MapShardGroupRequest>,
        ) -> std::result::Result<
            tonic::Response<super::MapShardGroupResponse>,
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
                "/plexspaces.actor.v1.ActorService/MapShardGroup",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("plexspaces.actor.v1.ActorService", "MapShardGroup"),
                );
            self.inner.unary(req, path, codec).await
        }
    }
}
/// Generated server implementations.
pub mod actor_service_server {
    #![allow(unused_variables, dead_code, missing_docs, clippy::let_unit_value)]
    use tonic::codegen::*;
    /// Generated trait containing gRPC methods that should be implemented for use with ActorServiceServer.
    #[async_trait]
    pub trait ActorService: Send + Sync + 'static {
        async fn spawn_actor(
            &self,
            request: tonic::Request<super::SpawnActorRequest>,
        ) -> std::result::Result<
            tonic::Response<super::SpawnActorResponse>,
            tonic::Status,
        >;
        ///
        async fn spawn_actors(
            &self,
            request: tonic::Request<super::SpawnActorsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::SpawnActorsResponse>,
            tonic::Status,
        >;
        /** Get an actor by ID
*/
        async fn get_actor(
            &self,
            request: tonic::Request<super::GetActorRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetActorResponse>,
            tonic::Status,
        >;
        /** List actors with filtering
*/
        async fn list_actors(
            &self,
            request: tonic::Request<super::ListActorsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListActorsResponse>,
            tonic::Status,
        >;
        /** Send message to an actor using tell semantics
*/
        async fn send_message(
            &self,
            request: tonic::Request<super::SendMessageRequest>,
        ) -> std::result::Result<
            tonic::Response<super::SendMessageResponse>,
            tonic::Status,
        >;
        /// Server streaming response type for the StreamMessages method.
        type StreamMessagesStream: tonic::codegen::tokio_stream::Stream<
                Item = std::result::Result<super::StreamMessageResponse, tonic::Status>,
            >
            + Send
            + 'static;
        /** Stream messages for high-throughput scenarios (Erlang-inspired)
 Use this for bulk message passing or event streaming
*/
        async fn stream_messages(
            &self,
            request: tonic::Request<tonic::Streaming<super::StreamMessageRequest>>,
        ) -> std::result::Result<
            tonic::Response<Self::StreamMessagesStream>,
            tonic::Status,
        >;
        /** Change actor state
*/
        async fn set_actor_state(
            &self,
            request: tonic::Request<super::SetActorStateRequest>,
        ) -> std::result::Result<
            tonic::Response<super::SetActorStateResponse>,
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
        async fn delete_actor(
            &self,
            request: tonic::Request<super::DeleteActorRequest>,
        ) -> std::result::Result<
            tonic::Response<super::super::super::common::v1::Empty>,
            tonic::Status,
        >;
        async fn monitor_actor(
            &self,
            request: tonic::Request<super::MonitorActorRequest>,
        ) -> std::result::Result<
            tonic::Response<super::MonitorActorResponse>,
            tonic::Status,
        >;
        async fn demonitor_actor(
            &self,
            request: tonic::Request<super::DemonitorActorRequest>,
        ) -> std::result::Result<
            tonic::Response<super::super::super::common::v1::Empty>,
            tonic::Status,
        >;
        async fn link_actor(
            &self,
            request: tonic::Request<super::LinkActorRequest>,
        ) -> std::result::Result<
            tonic::Response<super::LinkActorResponse>,
            tonic::Status,
        >;
        async fn unlink_actor(
            &self,
            request: tonic::Request<super::UnlinkActorRequest>,
        ) -> std::result::Result<
            tonic::Response<super::UnlinkActorResponse>,
            tonic::Status,
        >;
        async fn notify_actor_down(
            &self,
            request: tonic::Request<super::ActorDownNotification>,
        ) -> std::result::Result<
            tonic::Response<super::super::super::common::v1::Empty>,
            tonic::Status,
        >;
        async fn check_actor_exists(
            &self,
            request: tonic::Request<super::CheckActorExistsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::CheckActorExistsResponse>,
            tonic::Status,
        >;
        async fn get_actor_states(
            &self,
            request: tonic::Request<super::GetActorStatesRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetActorStatesResponse>,
            tonic::Status,
        >;
        async fn ask_reply(
            &self,
            request: tonic::Request<super::AskReplyRequest>,
        ) -> std::result::Result<
            tonic::Response<super::AskReplyResponse>,
            tonic::Status,
        >;
        async fn create_shard_group(
            &self,
            request: tonic::Request<super::CreateShardGroupRequest>,
        ) -> std::result::Result<
            tonic::Response<super::CreateShardGroupResponse>,
            tonic::Status,
        >;
        async fn delete_shard_group(
            &self,
            request: tonic::Request<super::DeleteShardGroupRequest>,
        ) -> std::result::Result<
            tonic::Response<super::super::super::common::v1::Empty>,
            tonic::Status,
        >;
        async fn get_shard_group(
            &self,
            request: tonic::Request<super::GetShardGroupRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetShardGroupResponse>,
            tonic::Status,
        >;
        async fn list_shard_groups(
            &self,
            request: tonic::Request<super::ListShardGroupsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListShardGroupsResponse>,
            tonic::Status,
        >;
        async fn scale_shard_group(
            &self,
            request: tonic::Request<super::ScaleShardGroupRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ScaleShardGroupResponse>,
            tonic::Status,
        >;
        async fn send_to_shard(
            &self,
            request: tonic::Request<super::SendToShardRequest>,
        ) -> std::result::Result<
            tonic::Response<super::SendToShardResponse>,
            tonic::Status,
        >;
        async fn broadcast_shard_group(
            &self,
            request: tonic::Request<super::BroadcastShardGroupRequest>,
        ) -> std::result::Result<
            tonic::Response<super::BroadcastShardGroupResponse>,
            tonic::Status,
        >;
        async fn reduce_shard_group(
            &self,
            request: tonic::Request<super::ReduceShardGroupRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ReduceShardGroupResponse>,
            tonic::Status,
        >;
        async fn all_reduce_shard_group(
            &self,
            request: tonic::Request<super::AllReduceShardGroupRequest>,
        ) -> std::result::Result<
            tonic::Response<super::AllReduceShardGroupResponse>,
            tonic::Status,
        >;
        async fn barrier_shard_group(
            &self,
            request: tonic::Request<super::BarrierShardGroupRequest>,
        ) -> std::result::Result<
            tonic::Response<super::BarrierShardGroupResponse>,
            tonic::Status,
        >;
        async fn scatter_gather(
            &self,
            request: tonic::Request<super::ScatterGatherRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ScatterGatherResponse>,
            tonic::Status,
        >;
        async fn bulk_update_shard_group(
            &self,
            request: tonic::Request<super::BulkUpdateShardGroupRequest>,
        ) -> std::result::Result<
            tonic::Response<super::BulkUpdateShardGroupResponse>,
            tonic::Status,
        >;
        async fn map_shard_group(
            &self,
            request: tonic::Request<super::MapShardGroupRequest>,
        ) -> std::result::Result<
            tonic::Response<super::MapShardGroupResponse>,
            tonic::Status,
        >;
    }
    /** Actor system service
*/
    #[derive(Debug)]
    pub struct ActorServiceServer<T: ActorService> {
        inner: _Inner<T>,
        accept_compression_encodings: EnabledCompressionEncodings,
        send_compression_encodings: EnabledCompressionEncodings,
        max_decoding_message_size: Option<usize>,
        max_encoding_message_size: Option<usize>,
    }
    struct _Inner<T>(Arc<T>);
    impl<T: ActorService> ActorServiceServer<T> {
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
    impl<T, B> tonic::codegen::Service<http::Request<B>> for ActorServiceServer<T>
    where
        T: ActorService,
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
                "/plexspaces.actor.v1.ActorService/SpawnActor" => {
                    #[allow(non_camel_case_types)]
                    struct SpawnActorSvc<T: ActorService>(pub Arc<T>);
                    impl<
                        T: ActorService,
                    > tonic::server::UnaryService<super::SpawnActorRequest>
                    for SpawnActorSvc<T> {
                        type Response = super::SpawnActorResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::SpawnActorRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ActorService>::spawn_actor(&inner, request).await
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
                        let method = SpawnActorSvc(inner);
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
                "/plexspaces.actor.v1.ActorService/SpawnActors" => {
                    #[allow(non_camel_case_types)]
                    struct SpawnActorsSvc<T: ActorService>(pub Arc<T>);
                    impl<
                        T: ActorService,
                    > tonic::server::UnaryService<super::SpawnActorsRequest>
                    for SpawnActorsSvc<T> {
                        type Response = super::SpawnActorsResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::SpawnActorsRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ActorService>::spawn_actors(&inner, request).await
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
                        let method = SpawnActorsSvc(inner);
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
                "/plexspaces.actor.v1.ActorService/GetActor" => {
                    #[allow(non_camel_case_types)]
                    struct GetActorSvc<T: ActorService>(pub Arc<T>);
                    impl<
                        T: ActorService,
                    > tonic::server::UnaryService<super::GetActorRequest>
                    for GetActorSvc<T> {
                        type Response = super::GetActorResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::GetActorRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ActorService>::get_actor(&inner, request).await
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
                        let method = GetActorSvc(inner);
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
                "/plexspaces.actor.v1.ActorService/ListActors" => {
                    #[allow(non_camel_case_types)]
                    struct ListActorsSvc<T: ActorService>(pub Arc<T>);
                    impl<
                        T: ActorService,
                    > tonic::server::UnaryService<super::ListActorsRequest>
                    for ListActorsSvc<T> {
                        type Response = super::ListActorsResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ListActorsRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ActorService>::list_actors(&inner, request).await
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
                        let method = ListActorsSvc(inner);
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
                "/plexspaces.actor.v1.ActorService/SendMessage" => {
                    #[allow(non_camel_case_types)]
                    struct SendMessageSvc<T: ActorService>(pub Arc<T>);
                    impl<
                        T: ActorService,
                    > tonic::server::UnaryService<super::SendMessageRequest>
                    for SendMessageSvc<T> {
                        type Response = super::SendMessageResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::SendMessageRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ActorService>::send_message(&inner, request).await
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
                        let method = SendMessageSvc(inner);
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
                "/plexspaces.actor.v1.ActorService/StreamMessages" => {
                    #[allow(non_camel_case_types)]
                    struct StreamMessagesSvc<T: ActorService>(pub Arc<T>);
                    impl<
                        T: ActorService,
                    > tonic::server::StreamingService<super::StreamMessageRequest>
                    for StreamMessagesSvc<T> {
                        type Response = super::StreamMessageResponse;
                        type ResponseStream = T::StreamMessagesStream;
                        type Future = BoxFuture<
                            tonic::Response<Self::ResponseStream>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<
                                tonic::Streaming<super::StreamMessageRequest>,
                            >,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ActorService>::stream_messages(&inner, request).await
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
                        let method = StreamMessagesSvc(inner);
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
                        let res = grpc.streaming(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/plexspaces.actor.v1.ActorService/SetActorState" => {
                    #[allow(non_camel_case_types)]
                    struct SetActorStateSvc<T: ActorService>(pub Arc<T>);
                    impl<
                        T: ActorService,
                    > tonic::server::UnaryService<super::SetActorStateRequest>
                    for SetActorStateSvc<T> {
                        type Response = super::SetActorStateResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::SetActorStateRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ActorService>::set_actor_state(&inner, request).await
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
                        let method = SetActorStateSvc(inner);
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
                "/plexspaces.actor.v1.ActorService/MigrateActor" => {
                    #[allow(non_camel_case_types)]
                    struct MigrateActorSvc<T: ActorService>(pub Arc<T>);
                    impl<
                        T: ActorService,
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
                                <T as ActorService>::migrate_actor(&inner, request).await
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
                "/plexspaces.actor.v1.ActorService/DeleteActor" => {
                    #[allow(non_camel_case_types)]
                    struct DeleteActorSvc<T: ActorService>(pub Arc<T>);
                    impl<
                        T: ActorService,
                    > tonic::server::UnaryService<super::DeleteActorRequest>
                    for DeleteActorSvc<T> {
                        type Response = super::super::super::common::v1::Empty;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::DeleteActorRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ActorService>::delete_actor(&inner, request).await
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
                        let method = DeleteActorSvc(inner);
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
                "/plexspaces.actor.v1.ActorService/MonitorActor" => {
                    #[allow(non_camel_case_types)]
                    struct MonitorActorSvc<T: ActorService>(pub Arc<T>);
                    impl<
                        T: ActorService,
                    > tonic::server::UnaryService<super::MonitorActorRequest>
                    for MonitorActorSvc<T> {
                        type Response = super::MonitorActorResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::MonitorActorRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ActorService>::monitor_actor(&inner, request).await
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
                        let method = MonitorActorSvc(inner);
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
                "/plexspaces.actor.v1.ActorService/DemonitorActor" => {
                    #[allow(non_camel_case_types)]
                    struct DemonitorActorSvc<T: ActorService>(pub Arc<T>);
                    impl<
                        T: ActorService,
                    > tonic::server::UnaryService<super::DemonitorActorRequest>
                    for DemonitorActorSvc<T> {
                        type Response = super::super::super::common::v1::Empty;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::DemonitorActorRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ActorService>::demonitor_actor(&inner, request).await
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
                        let method = DemonitorActorSvc(inner);
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
                "/plexspaces.actor.v1.ActorService/LinkActor" => {
                    #[allow(non_camel_case_types)]
                    struct LinkActorSvc<T: ActorService>(pub Arc<T>);
                    impl<
                        T: ActorService,
                    > tonic::server::UnaryService<super::LinkActorRequest>
                    for LinkActorSvc<T> {
                        type Response = super::LinkActorResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::LinkActorRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ActorService>::link_actor(&inner, request).await
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
                        let method = LinkActorSvc(inner);
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
                "/plexspaces.actor.v1.ActorService/UnlinkActor" => {
                    #[allow(non_camel_case_types)]
                    struct UnlinkActorSvc<T: ActorService>(pub Arc<T>);
                    impl<
                        T: ActorService,
                    > tonic::server::UnaryService<super::UnlinkActorRequest>
                    for UnlinkActorSvc<T> {
                        type Response = super::UnlinkActorResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::UnlinkActorRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ActorService>::unlink_actor(&inner, request).await
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
                        let method = UnlinkActorSvc(inner);
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
                "/plexspaces.actor.v1.ActorService/NotifyActorDown" => {
                    #[allow(non_camel_case_types)]
                    struct NotifyActorDownSvc<T: ActorService>(pub Arc<T>);
                    impl<
                        T: ActorService,
                    > tonic::server::UnaryService<super::ActorDownNotification>
                    for NotifyActorDownSvc<T> {
                        type Response = super::super::super::common::v1::Empty;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ActorDownNotification>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ActorService>::notify_actor_down(&inner, request)
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
                        let method = NotifyActorDownSvc(inner);
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
                "/plexspaces.actor.v1.ActorService/CheckActorExists" => {
                    #[allow(non_camel_case_types)]
                    struct CheckActorExistsSvc<T: ActorService>(pub Arc<T>);
                    impl<
                        T: ActorService,
                    > tonic::server::UnaryService<super::CheckActorExistsRequest>
                    for CheckActorExistsSvc<T> {
                        type Response = super::CheckActorExistsResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::CheckActorExistsRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ActorService>::check_actor_exists(&inner, request)
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
                        let method = CheckActorExistsSvc(inner);
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
                "/plexspaces.actor.v1.ActorService/GetActorStates" => {
                    #[allow(non_camel_case_types)]
                    struct GetActorStatesSvc<T: ActorService>(pub Arc<T>);
                    impl<
                        T: ActorService,
                    > tonic::server::UnaryService<super::GetActorStatesRequest>
                    for GetActorStatesSvc<T> {
                        type Response = super::GetActorStatesResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::GetActorStatesRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ActorService>::get_actor_states(&inner, request).await
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
                        let method = GetActorStatesSvc(inner);
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
                "/plexspaces.actor.v1.ActorService/AskReply" => {
                    #[allow(non_camel_case_types)]
                    struct AskReplySvc<T: ActorService>(pub Arc<T>);
                    impl<
                        T: ActorService,
                    > tonic::server::UnaryService<super::AskReplyRequest>
                    for AskReplySvc<T> {
                        type Response = super::AskReplyResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::AskReplyRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ActorService>::ask_reply(&inner, request).await
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
                        let method = AskReplySvc(inner);
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
                "/plexspaces.actor.v1.ActorService/CreateShardGroup" => {
                    #[allow(non_camel_case_types)]
                    struct CreateShardGroupSvc<T: ActorService>(pub Arc<T>);
                    impl<
                        T: ActorService,
                    > tonic::server::UnaryService<super::CreateShardGroupRequest>
                    for CreateShardGroupSvc<T> {
                        type Response = super::CreateShardGroupResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::CreateShardGroupRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ActorService>::create_shard_group(&inner, request)
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
                        let method = CreateShardGroupSvc(inner);
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
                "/plexspaces.actor.v1.ActorService/DeleteShardGroup" => {
                    #[allow(non_camel_case_types)]
                    struct DeleteShardGroupSvc<T: ActorService>(pub Arc<T>);
                    impl<
                        T: ActorService,
                    > tonic::server::UnaryService<super::DeleteShardGroupRequest>
                    for DeleteShardGroupSvc<T> {
                        type Response = super::super::super::common::v1::Empty;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::DeleteShardGroupRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ActorService>::delete_shard_group(&inner, request)
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
                        let method = DeleteShardGroupSvc(inner);
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
                "/plexspaces.actor.v1.ActorService/GetShardGroup" => {
                    #[allow(non_camel_case_types)]
                    struct GetShardGroupSvc<T: ActorService>(pub Arc<T>);
                    impl<
                        T: ActorService,
                    > tonic::server::UnaryService<super::GetShardGroupRequest>
                    for GetShardGroupSvc<T> {
                        type Response = super::GetShardGroupResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::GetShardGroupRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ActorService>::get_shard_group(&inner, request).await
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
                        let method = GetShardGroupSvc(inner);
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
                "/plexspaces.actor.v1.ActorService/ListShardGroups" => {
                    #[allow(non_camel_case_types)]
                    struct ListShardGroupsSvc<T: ActorService>(pub Arc<T>);
                    impl<
                        T: ActorService,
                    > tonic::server::UnaryService<super::ListShardGroupsRequest>
                    for ListShardGroupsSvc<T> {
                        type Response = super::ListShardGroupsResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ListShardGroupsRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ActorService>::list_shard_groups(&inner, request)
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
                        let method = ListShardGroupsSvc(inner);
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
                "/plexspaces.actor.v1.ActorService/ScaleShardGroup" => {
                    #[allow(non_camel_case_types)]
                    struct ScaleShardGroupSvc<T: ActorService>(pub Arc<T>);
                    impl<
                        T: ActorService,
                    > tonic::server::UnaryService<super::ScaleShardGroupRequest>
                    for ScaleShardGroupSvc<T> {
                        type Response = super::ScaleShardGroupResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ScaleShardGroupRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ActorService>::scale_shard_group(&inner, request)
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
                        let method = ScaleShardGroupSvc(inner);
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
                "/plexspaces.actor.v1.ActorService/SendToShard" => {
                    #[allow(non_camel_case_types)]
                    struct SendToShardSvc<T: ActorService>(pub Arc<T>);
                    impl<
                        T: ActorService,
                    > tonic::server::UnaryService<super::SendToShardRequest>
                    for SendToShardSvc<T> {
                        type Response = super::SendToShardResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::SendToShardRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ActorService>::send_to_shard(&inner, request).await
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
                        let method = SendToShardSvc(inner);
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
                "/plexspaces.actor.v1.ActorService/BroadcastShardGroup" => {
                    #[allow(non_camel_case_types)]
                    struct BroadcastShardGroupSvc<T: ActorService>(pub Arc<T>);
                    impl<
                        T: ActorService,
                    > tonic::server::UnaryService<super::BroadcastShardGroupRequest>
                    for BroadcastShardGroupSvc<T> {
                        type Response = super::BroadcastShardGroupResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::BroadcastShardGroupRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ActorService>::broadcast_shard_group(&inner, request)
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
                        let method = BroadcastShardGroupSvc(inner);
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
                "/plexspaces.actor.v1.ActorService/ReduceShardGroup" => {
                    #[allow(non_camel_case_types)]
                    struct ReduceShardGroupSvc<T: ActorService>(pub Arc<T>);
                    impl<
                        T: ActorService,
                    > tonic::server::UnaryService<super::ReduceShardGroupRequest>
                    for ReduceShardGroupSvc<T> {
                        type Response = super::ReduceShardGroupResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ReduceShardGroupRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ActorService>::reduce_shard_group(&inner, request)
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
                        let method = ReduceShardGroupSvc(inner);
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
                "/plexspaces.actor.v1.ActorService/AllReduceShardGroup" => {
                    #[allow(non_camel_case_types)]
                    struct AllReduceShardGroupSvc<T: ActorService>(pub Arc<T>);
                    impl<
                        T: ActorService,
                    > tonic::server::UnaryService<super::AllReduceShardGroupRequest>
                    for AllReduceShardGroupSvc<T> {
                        type Response = super::AllReduceShardGroupResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::AllReduceShardGroupRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ActorService>::all_reduce_shard_group(&inner, request)
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
                        let method = AllReduceShardGroupSvc(inner);
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
                "/plexspaces.actor.v1.ActorService/BarrierShardGroup" => {
                    #[allow(non_camel_case_types)]
                    struct BarrierShardGroupSvc<T: ActorService>(pub Arc<T>);
                    impl<
                        T: ActorService,
                    > tonic::server::UnaryService<super::BarrierShardGroupRequest>
                    for BarrierShardGroupSvc<T> {
                        type Response = super::BarrierShardGroupResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::BarrierShardGroupRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ActorService>::barrier_shard_group(&inner, request)
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
                        let method = BarrierShardGroupSvc(inner);
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
                "/plexspaces.actor.v1.ActorService/ScatterGather" => {
                    #[allow(non_camel_case_types)]
                    struct ScatterGatherSvc<T: ActorService>(pub Arc<T>);
                    impl<
                        T: ActorService,
                    > tonic::server::UnaryService<super::ScatterGatherRequest>
                    for ScatterGatherSvc<T> {
                        type Response = super::ScatterGatherResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ScatterGatherRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ActorService>::scatter_gather(&inner, request).await
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
                        let method = ScatterGatherSvc(inner);
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
                "/plexspaces.actor.v1.ActorService/BulkUpdateShardGroup" => {
                    #[allow(non_camel_case_types)]
                    struct BulkUpdateShardGroupSvc<T: ActorService>(pub Arc<T>);
                    impl<
                        T: ActorService,
                    > tonic::server::UnaryService<super::BulkUpdateShardGroupRequest>
                    for BulkUpdateShardGroupSvc<T> {
                        type Response = super::BulkUpdateShardGroupResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::BulkUpdateShardGroupRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ActorService>::bulk_update_shard_group(
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
                        let method = BulkUpdateShardGroupSvc(inner);
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
                "/plexspaces.actor.v1.ActorService/MapShardGroup" => {
                    #[allow(non_camel_case_types)]
                    struct MapShardGroupSvc<T: ActorService>(pub Arc<T>);
                    impl<
                        T: ActorService,
                    > tonic::server::UnaryService<super::MapShardGroupRequest>
                    for MapShardGroupSvc<T> {
                        type Response = super::MapShardGroupResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::MapShardGroupRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ActorService>::map_shard_group(&inner, request).await
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
                        let method = MapShardGroupSvc(inner);
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
    impl<T: ActorService> Clone for ActorServiceServer<T> {
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
    impl<T: ActorService> Clone for _Inner<T> {
        fn clone(&self) -> Self {
            Self(Arc::clone(&self.0))
        }
    }
    impl<T: std::fmt::Debug> std::fmt::Debug for _Inner<T> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{:?}", self.0)
        }
    }
    impl<T: ActorService> tonic::server::NamedService for ActorServiceServer<T> {
        const NAME: &'static str = "plexspaces.actor.v1.ActorService";
    }
}
/// Generated client implementations.
pub mod lifecycle_event_channel_client {
    #![allow(unused_variables, dead_code, missing_docs, clippy::let_unit_value)]
    use tonic::codegen::*;
    use tonic::codegen::http::Uri;
    #[derive(Debug, Clone)]
    pub struct LifecycleEventChannelClient<T> {
        inner: tonic::client::Grpc<T>,
    }
    impl LifecycleEventChannelClient<tonic::transport::Channel> {
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
    impl<T> LifecycleEventChannelClient<T>
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
        ) -> LifecycleEventChannelClient<InterceptedService<T, F>>
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
            LifecycleEventChannelClient::new(InterceptedService::new(inner, interceptor))
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
        pub async fn subscribe_lifecycle_events(
            &mut self,
            request: impl tonic::IntoRequest<super::LifecycleEventFilter>,
        ) -> std::result::Result<
            tonic::Response<tonic::codec::Streaming<super::ActorLifecycleEvent>>,
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
                "/plexspaces.actor.v1.LifecycleEventChannel/SubscribeLifecycleEvents",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.actor.v1.LifecycleEventChannel",
                        "SubscribeLifecycleEvents",
                    ),
                );
            self.inner.server_streaming(req, path, codec).await
        }
        pub async fn publish_lifecycle_event(
            &mut self,
            request: impl tonic::IntoRequest<super::ActorLifecycleEvent>,
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
                "/plexspaces.actor.v1.LifecycleEventChannel/PublishLifecycleEvent",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.actor.v1.LifecycleEventChannel",
                        "PublishLifecycleEvent",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
    }
}
/// Generated server implementations.
pub mod lifecycle_event_channel_server {
    #![allow(unused_variables, dead_code, missing_docs, clippy::let_unit_value)]
    use tonic::codegen::*;
    /// Generated trait containing gRPC methods that should be implemented for use with LifecycleEventChannelServer.
    #[async_trait]
    pub trait LifecycleEventChannel: Send + Sync + 'static {
        /// Server streaming response type for the SubscribeLifecycleEvents method.
        type SubscribeLifecycleEventsStream: tonic::codegen::tokio_stream::Stream<
                Item = std::result::Result<super::ActorLifecycleEvent, tonic::Status>,
            >
            + Send
            + 'static;
        async fn subscribe_lifecycle_events(
            &self,
            request: tonic::Request<super::LifecycleEventFilter>,
        ) -> std::result::Result<
            tonic::Response<Self::SubscribeLifecycleEventsStream>,
            tonic::Status,
        >;
        async fn publish_lifecycle_event(
            &self,
            request: tonic::Request<super::ActorLifecycleEvent>,
        ) -> std::result::Result<
            tonic::Response<super::super::super::common::v1::Empty>,
            tonic::Status,
        >;
    }
    #[derive(Debug)]
    pub struct LifecycleEventChannelServer<T: LifecycleEventChannel> {
        inner: _Inner<T>,
        accept_compression_encodings: EnabledCompressionEncodings,
        send_compression_encodings: EnabledCompressionEncodings,
        max_decoding_message_size: Option<usize>,
        max_encoding_message_size: Option<usize>,
    }
    struct _Inner<T>(Arc<T>);
    impl<T: LifecycleEventChannel> LifecycleEventChannelServer<T> {
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
    for LifecycleEventChannelServer<T>
    where
        T: LifecycleEventChannel,
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
                "/plexspaces.actor.v1.LifecycleEventChannel/SubscribeLifecycleEvents" => {
                    #[allow(non_camel_case_types)]
                    struct SubscribeLifecycleEventsSvc<T: LifecycleEventChannel>(
                        pub Arc<T>,
                    );
                    impl<
                        T: LifecycleEventChannel,
                    > tonic::server::ServerStreamingService<super::LifecycleEventFilter>
                    for SubscribeLifecycleEventsSvc<T> {
                        type Response = super::ActorLifecycleEvent;
                        type ResponseStream = T::SubscribeLifecycleEventsStream;
                        type Future = BoxFuture<
                            tonic::Response<Self::ResponseStream>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::LifecycleEventFilter>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as LifecycleEventChannel>::subscribe_lifecycle_events(
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
                        let method = SubscribeLifecycleEventsSvc(inner);
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
                "/plexspaces.actor.v1.LifecycleEventChannel/PublishLifecycleEvent" => {
                    #[allow(non_camel_case_types)]
                    struct PublishLifecycleEventSvc<T: LifecycleEventChannel>(
                        pub Arc<T>,
                    );
                    impl<
                        T: LifecycleEventChannel,
                    > tonic::server::UnaryService<super::ActorLifecycleEvent>
                    for PublishLifecycleEventSvc<T> {
                        type Response = super::super::super::common::v1::Empty;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ActorLifecycleEvent>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as LifecycleEventChannel>::publish_lifecycle_event(
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
                        let method = PublishLifecycleEventSvc(inner);
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
    impl<T: LifecycleEventChannel> Clone for LifecycleEventChannelServer<T> {
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
    impl<T: LifecycleEventChannel> Clone for _Inner<T> {
        fn clone(&self) -> Self {
            Self(Arc::clone(&self.0))
        }
    }
    impl<T: std::fmt::Debug> std::fmt::Debug for _Inner<T> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{:?}", self.0)
        }
    }
    impl<T: LifecycleEventChannel> tonic::server::NamedService
    for LifecycleEventChannelServer<T> {
        const NAME: &'static str = "plexspaces.actor.v1.LifecycleEventChannel";
    }
}
