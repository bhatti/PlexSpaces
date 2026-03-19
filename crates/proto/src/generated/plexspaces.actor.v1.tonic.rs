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
        /** Send message to an actor
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
        /** Delete an actor
*/
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
        /** Monitor an actor (Erlang-style location transparent monitoring)

 ## Purpose
 Establishes a monitoring link from supervisor to actor. When the actor
 terminates (normally or abnormally), the remote node will notify the
 supervisor via NotifyActorDown.

 ## Erlang Philosophy
 In Erlang, monitor(process, Pid) works the same for local and remote processes.
 The runtime handles location transparency - same API whether the process is
 in the same node or a different node.

 ## Design Notes
 - supervisor_id: The actor that wants to be notified (usually a supervisor)
 - supervisor_callback: gRPC address where to send NotifyActorDown
 - actor_id can be local or remote (format: "actor@node")
 - Returns monitor_ref for potential demonitor() in future
*/
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
        /** Link two actors (Erlang link/1 equivalent)

 ## Purpose
 Creates a bidirectional link between two actors. When one actor dies abnormally,
 the linked actor automatically dies (cascading failure).

 ## Erlang Philosophy
 Equivalent to Erlang's `link(Pid)` - creates bidirectional link.
 If either process dies abnormally, the other dies too.

 ## Design Notes
 - Links are bidirectional (if A links to B, B is linked to A)
 - Links only propagate abnormal deaths (not "normal" shutdowns)
 - Links are used internally by supervision (parent-child relationships)
 - Links can be created explicitly via this API
*/
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
        /** Unlink two actors (Erlang unlink/1 equivalent)

 ## Purpose
 Removes the bidirectional link between two actors. After unlinking,
 actors can die independently without cascading failures.

 ## Erlang Philosophy
 Equivalent to Erlang's `unlink(Pid)` - removes bidirectional link.
*/
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
        /** Internal: Notify supervisor of actor termination

 ## Purpose
 Called by the remote node hosting the actor when it terminates. The remote
 node sends this notification to the supervisor_callback address that was
 provided in MonitorActor.

 ## Erlang Philosophy
 Equivalent to receiving {'DOWN', Ref, process, Pid, Reason} message in Erlang.
 The supervisor receives this asynchronously when the monitored actor exits.

 ## Design Notes
 - This is an internal RPC, not typically called by user code
 - Supervisor uses this to implement restart strategies
 - reason: "normal" for graceful shutdown, error message for crashes
*/
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
        /** Activate a virtual actor (load into memory)

 ## Purpose
 Activates a virtual actor that exists virtually but is not yet in memory.
 Virtual actors are always addressable but activated on-demand.

 ## When Used
 Only works for actors with VirtualActorFacet attached (opt-in pattern).
 Regular actors are always active after creation.

 ## Behavior
 - Loads actor state from storage (if persisted)
 - Instantiates actor in memory
 - Processes pending messages (queued during activation)
 - Updates VirtualActorLifecycle (last_activated, activation_count)
*/
        pub async fn activate_actor(
            &mut self,
            request: impl tonic::IntoRequest<super::ActivateActorRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ActivateActorResponse>,
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
                "/plexspaces.actor.v1.ActorService/ActivateActor",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("plexspaces.actor.v1.ActorService", "ActivateActor"),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Deactivate a virtual actor (remove from memory)

 ## Purpose
 Deactivates a virtual actor that has been idle, freeing memory while
 maintaining addressability.

 ## When Used
 Only works for actors with VirtualActorFacet attached.
 Regular actors cannot be deactivated (must be deleted).

 ## Behavior
 - Persists actor state to storage (if persist_on_deactivation enabled)
 - Removes actor from memory
 - Updates VirtualActorLifecycle (last_accessed, is_activating = false)
 - Queues any new messages for later activation
*/
        pub async fn deactivate_actor(
            &mut self,
            request: impl tonic::IntoRequest<super::DeactivateActorRequest>,
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
                "/plexspaces.actor.v1.ActorService/DeactivateActor",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.actor.v1.ActorService",
                        "DeactivateActor",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Check if virtual actor exists (without activating)

 ## Purpose
 Query whether a virtual actor exists without triggering activation.
 Useful for existence checks, health monitoring, and discovery.

 ## Returns
 - exists: Actor exists (virtual or active)
 - is_active: Actor is currently active (in memory)
 - is_virtual: Actor has VirtualActorFacet (is virtual)
*/
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
        /** Get or activate a virtual actor (Orleans-style)

 ## Purpose
 Gets existing actor if active, or activates virtual actor if inactive.
 This is the primary API for virtual actors (Orleans grains pattern).

 ## Orleans Comparison
 Orleans: `IGrainFactory.GetGrain<T>(key)` - always returns grain reference
 PlexSpaces: `GetOrActivateActor(actor_id, actor_type?, initial_state?, config?)` - activates if needed

 ## Behavior
 1. If actor exists and is active → Return existing ActorRef
 2. If actor exists but is inactive (virtual) → Activate and return ActorRef
 3. If actor doesn't exist → Create new actor (if actor_type provided) and return ActorRef

 ## Virtual Actor Pattern
 - Actor ID must be client-specified (e.g., "user/123", "session/abc")
 - Actor must have VirtualActorFacet attached (enables lazy activation)
 - First message triggers activation automatically

 ## Design Notes
 - actor_id: Client-specified, required (format: "{actor_type}/{key}" or "{actor_type}@{key}")
 - actor_type: Required if actor doesn't exist (used to create new actor)
 - initial_state, config: Used if creating new actor
*/
        pub async fn get_or_activate_actor(
            &mut self,
            request: impl tonic::IntoRequest<super::GetOrActivateActorRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetOrActivateActorResponse>,
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
                "/plexspaces.actor.v1.ActorService/GetOrActivateActor",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "plexspaces.actor.v1.ActorService",
                        "GetOrActivateActor",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Invoke an actor via HTTP-like interface (FaaS-style)

 ## Purpose
 Provides a FaaS-like interface for invoking actors via HTTP GET/POST requests.
 This enables actors to be invoked like serverless functions while maintaining
 the actor model's stateful, message-driven architecture.

 ## HTTP Method Behavior
 - **GET**: Converts query parameters to JSON payload, calls actor.ask() (request-reply)
 - **POST**: Converts request body to payload and headers to headers, calls actor.tell() (fire-and-forget)

 ## Actor Lookup
 - Looks up actors by actor_type using ObjectRegistry discover with object_category filter
 - If multiple actors of same type found, randomly selects one (load balancing)
 - Returns 404 if no actor of requested type found

 ## Security
 - Extracts tenant_id from JWT claims if authentication enabled
 - Verifies JWT tenant_id matches requested tenant_id in path
 - Default tenant_id is "default" when no authentication provided
 - All actors must have tenant_id (default if no auth)

 ## Path Format
 `/api/v1/actors/{tenant_id}/{namespace}/{actor_type}`
 - tenant_id: Tenant identifier (default: "default")
 - namespace: Namespace identifier (default: "default" if not specified)
 - actor_type: Type of actor to invoke (used for lookup)
*/
        pub async fn invoke_actor(
            &mut self,
            request: impl tonic::IntoRequest<super::InvokeActorRequest>,
        ) -> std::result::Result<
            tonic::Response<super::InvokeActorResponse>,
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
                "/plexspaces.actor.v1.ActorService/InvokeActor",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("plexspaces.actor.v1.ActorService", "InvokeActor"),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Terminate an actor gracefully by ID

 ## Purpose
 Gracefully terminates an actor, allowing it to complete pending work and clean up.
 This is the HTTP DELETE endpoint for actors (mapped from DELETE /api/v1/actors/{namespace}/{actor_id}).
 Pairs with SpawnActor for complete actor lifecycle management.

 ## Behavior
 - Sends graceful shutdown signal to actor
 - Actor completes pending messages (with timeout)
 - Actor state is persisted (if DurabilityFacet attached)
 - Actor is removed from registry

 ## Difference from DeactivateActor
 - TerminateActor: Permanent termination (actor removed from system)
 - DeactivateActor: Temporary passivation (virtual actor can reactivate on next message)

 ## Security
 - Requires permission to terminate actors in the namespace
 - Tenant isolation enforced via JWT claims
*/
        pub async fn terminate_actor(
            &mut self,
            request: impl tonic::IntoRequest<super::TerminateActorRequest>,
        ) -> std::result::Result<
            tonic::Response<super::TerminateActorResponse>,
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
                "/plexspaces.actor.v1.ActorService/TerminateActor",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("plexspaces.actor.v1.ActorService", "TerminateActor"),
                );
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
        /** Send message to an actor
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
        /** Delete an actor
*/
        async fn delete_actor(
            &self,
            request: tonic::Request<super::DeleteActorRequest>,
        ) -> std::result::Result<
            tonic::Response<super::super::super::common::v1::Empty>,
            tonic::Status,
        >;
        /** Monitor an actor (Erlang-style location transparent monitoring)

 ## Purpose
 Establishes a monitoring link from supervisor to actor. When the actor
 terminates (normally or abnormally), the remote node will notify the
 supervisor via NotifyActorDown.

 ## Erlang Philosophy
 In Erlang, monitor(process, Pid) works the same for local and remote processes.
 The runtime handles location transparency - same API whether the process is
 in the same node or a different node.

 ## Design Notes
 - supervisor_id: The actor that wants to be notified (usually a supervisor)
 - supervisor_callback: gRPC address where to send NotifyActorDown
 - actor_id can be local or remote (format: "actor@node")
 - Returns monitor_ref for potential demonitor() in future
*/
        async fn monitor_actor(
            &self,
            request: tonic::Request<super::MonitorActorRequest>,
        ) -> std::result::Result<
            tonic::Response<super::MonitorActorResponse>,
            tonic::Status,
        >;
        /** Link two actors (Erlang link/1 equivalent)

 ## Purpose
 Creates a bidirectional link between two actors. When one actor dies abnormally,
 the linked actor automatically dies (cascading failure).

 ## Erlang Philosophy
 Equivalent to Erlang's `link(Pid)` - creates bidirectional link.
 If either process dies abnormally, the other dies too.

 ## Design Notes
 - Links are bidirectional (if A links to B, B is linked to A)
 - Links only propagate abnormal deaths (not "normal" shutdowns)
 - Links are used internally by supervision (parent-child relationships)
 - Links can be created explicitly via this API
*/
        async fn link_actor(
            &self,
            request: tonic::Request<super::LinkActorRequest>,
        ) -> std::result::Result<
            tonic::Response<super::LinkActorResponse>,
            tonic::Status,
        >;
        /** Unlink two actors (Erlang unlink/1 equivalent)

 ## Purpose
 Removes the bidirectional link between two actors. After unlinking,
 actors can die independently without cascading failures.

 ## Erlang Philosophy
 Equivalent to Erlang's `unlink(Pid)` - removes bidirectional link.
*/
        async fn unlink_actor(
            &self,
            request: tonic::Request<super::UnlinkActorRequest>,
        ) -> std::result::Result<
            tonic::Response<super::UnlinkActorResponse>,
            tonic::Status,
        >;
        /** Internal: Notify supervisor of actor termination

 ## Purpose
 Called by the remote node hosting the actor when it terminates. The remote
 node sends this notification to the supervisor_callback address that was
 provided in MonitorActor.

 ## Erlang Philosophy
 Equivalent to receiving {'DOWN', Ref, process, Pid, Reason} message in Erlang.
 The supervisor receives this asynchronously when the monitored actor exits.

 ## Design Notes
 - This is an internal RPC, not typically called by user code
 - Supervisor uses this to implement restart strategies
 - reason: "normal" for graceful shutdown, error message for crashes
*/
        async fn notify_actor_down(
            &self,
            request: tonic::Request<super::ActorDownNotification>,
        ) -> std::result::Result<
            tonic::Response<super::super::super::common::v1::Empty>,
            tonic::Status,
        >;
        /** Activate a virtual actor (load into memory)

 ## Purpose
 Activates a virtual actor that exists virtually but is not yet in memory.
 Virtual actors are always addressable but activated on-demand.

 ## When Used
 Only works for actors with VirtualActorFacet attached (opt-in pattern).
 Regular actors are always active after creation.

 ## Behavior
 - Loads actor state from storage (if persisted)
 - Instantiates actor in memory
 - Processes pending messages (queued during activation)
 - Updates VirtualActorLifecycle (last_activated, activation_count)
*/
        async fn activate_actor(
            &self,
            request: tonic::Request<super::ActivateActorRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ActivateActorResponse>,
            tonic::Status,
        >;
        /** Deactivate a virtual actor (remove from memory)

 ## Purpose
 Deactivates a virtual actor that has been idle, freeing memory while
 maintaining addressability.

 ## When Used
 Only works for actors with VirtualActorFacet attached.
 Regular actors cannot be deactivated (must be deleted).

 ## Behavior
 - Persists actor state to storage (if persist_on_deactivation enabled)
 - Removes actor from memory
 - Updates VirtualActorLifecycle (last_accessed, is_activating = false)
 - Queues any new messages for later activation
*/
        async fn deactivate_actor(
            &self,
            request: tonic::Request<super::DeactivateActorRequest>,
        ) -> std::result::Result<
            tonic::Response<super::super::super::common::v1::Empty>,
            tonic::Status,
        >;
        /** Check if virtual actor exists (without activating)

 ## Purpose
 Query whether a virtual actor exists without triggering activation.
 Useful for existence checks, health monitoring, and discovery.

 ## Returns
 - exists: Actor exists (virtual or active)
 - is_active: Actor is currently active (in memory)
 - is_virtual: Actor has VirtualActorFacet (is virtual)
*/
        async fn check_actor_exists(
            &self,
            request: tonic::Request<super::CheckActorExistsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::CheckActorExistsResponse>,
            tonic::Status,
        >;
        /** Get or activate a virtual actor (Orleans-style)

 ## Purpose
 Gets existing actor if active, or activates virtual actor if inactive.
 This is the primary API for virtual actors (Orleans grains pattern).

 ## Orleans Comparison
 Orleans: `IGrainFactory.GetGrain<T>(key)` - always returns grain reference
 PlexSpaces: `GetOrActivateActor(actor_id, actor_type?, initial_state?, config?)` - activates if needed

 ## Behavior
 1. If actor exists and is active → Return existing ActorRef
 2. If actor exists but is inactive (virtual) → Activate and return ActorRef
 3. If actor doesn't exist → Create new actor (if actor_type provided) and return ActorRef

 ## Virtual Actor Pattern
 - Actor ID must be client-specified (e.g., "user/123", "session/abc")
 - Actor must have VirtualActorFacet attached (enables lazy activation)
 - First message triggers activation automatically

 ## Design Notes
 - actor_id: Client-specified, required (format: "{actor_type}/{key}" or "{actor_type}@{key}")
 - actor_type: Required if actor doesn't exist (used to create new actor)
 - initial_state, config: Used if creating new actor
*/
        async fn get_or_activate_actor(
            &self,
            request: tonic::Request<super::GetOrActivateActorRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetOrActivateActorResponse>,
            tonic::Status,
        >;
        /** Invoke an actor via HTTP-like interface (FaaS-style)

 ## Purpose
 Provides a FaaS-like interface for invoking actors via HTTP GET/POST requests.
 This enables actors to be invoked like serverless functions while maintaining
 the actor model's stateful, message-driven architecture.

 ## HTTP Method Behavior
 - **GET**: Converts query parameters to JSON payload, calls actor.ask() (request-reply)
 - **POST**: Converts request body to payload and headers to headers, calls actor.tell() (fire-and-forget)

 ## Actor Lookup
 - Looks up actors by actor_type using ObjectRegistry discover with object_category filter
 - If multiple actors of same type found, randomly selects one (load balancing)
 - Returns 404 if no actor of requested type found

 ## Security
 - Extracts tenant_id from JWT claims if authentication enabled
 - Verifies JWT tenant_id matches requested tenant_id in path
 - Default tenant_id is "default" when no authentication provided
 - All actors must have tenant_id (default if no auth)

 ## Path Format
 `/api/v1/actors/{tenant_id}/{namespace}/{actor_type}`
 - tenant_id: Tenant identifier (default: "default")
 - namespace: Namespace identifier (default: "default" if not specified)
 - actor_type: Type of actor to invoke (used for lookup)
*/
        async fn invoke_actor(
            &self,
            request: tonic::Request<super::InvokeActorRequest>,
        ) -> std::result::Result<
            tonic::Response<super::InvokeActorResponse>,
            tonic::Status,
        >;
        /** Terminate an actor gracefully by ID

 ## Purpose
 Gracefully terminates an actor, allowing it to complete pending work and clean up.
 This is the HTTP DELETE endpoint for actors (mapped from DELETE /api/v1/actors/{namespace}/{actor_id}).
 Pairs with SpawnActor for complete actor lifecycle management.

 ## Behavior
 - Sends graceful shutdown signal to actor
 - Actor completes pending messages (with timeout)
 - Actor state is persisted (if DurabilityFacet attached)
 - Actor is removed from registry

 ## Difference from DeactivateActor
 - TerminateActor: Permanent termination (actor removed from system)
 - DeactivateActor: Temporary passivation (virtual actor can reactivate on next message)

 ## Security
 - Requires permission to terminate actors in the namespace
 - Tenant isolation enforced via JWT claims
*/
        async fn terminate_actor(
            &self,
            request: tonic::Request<super::TerminateActorRequest>,
        ) -> std::result::Result<
            tonic::Response<super::TerminateActorResponse>,
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
                "/plexspaces.actor.v1.ActorService/ActivateActor" => {
                    #[allow(non_camel_case_types)]
                    struct ActivateActorSvc<T: ActorService>(pub Arc<T>);
                    impl<
                        T: ActorService,
                    > tonic::server::UnaryService<super::ActivateActorRequest>
                    for ActivateActorSvc<T> {
                        type Response = super::ActivateActorResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ActivateActorRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ActorService>::activate_actor(&inner, request).await
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
                        let method = ActivateActorSvc(inner);
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
                "/plexspaces.actor.v1.ActorService/DeactivateActor" => {
                    #[allow(non_camel_case_types)]
                    struct DeactivateActorSvc<T: ActorService>(pub Arc<T>);
                    impl<
                        T: ActorService,
                    > tonic::server::UnaryService<super::DeactivateActorRequest>
                    for DeactivateActorSvc<T> {
                        type Response = super::super::super::common::v1::Empty;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::DeactivateActorRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ActorService>::deactivate_actor(&inner, request).await
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
                        let method = DeactivateActorSvc(inner);
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
                "/plexspaces.actor.v1.ActorService/GetOrActivateActor" => {
                    #[allow(non_camel_case_types)]
                    struct GetOrActivateActorSvc<T: ActorService>(pub Arc<T>);
                    impl<
                        T: ActorService,
                    > tonic::server::UnaryService<super::GetOrActivateActorRequest>
                    for GetOrActivateActorSvc<T> {
                        type Response = super::GetOrActivateActorResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::GetOrActivateActorRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ActorService>::get_or_activate_actor(&inner, request)
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
                        let method = GetOrActivateActorSvc(inner);
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
                "/plexspaces.actor.v1.ActorService/InvokeActor" => {
                    #[allow(non_camel_case_types)]
                    struct InvokeActorSvc<T: ActorService>(pub Arc<T>);
                    impl<
                        T: ActorService,
                    > tonic::server::UnaryService<super::InvokeActorRequest>
                    for InvokeActorSvc<T> {
                        type Response = super::InvokeActorResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::InvokeActorRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ActorService>::invoke_actor(&inner, request).await
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
                        let method = InvokeActorSvc(inner);
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
                "/plexspaces.actor.v1.ActorService/TerminateActor" => {
                    #[allow(non_camel_case_types)]
                    struct TerminateActorSvc<T: ActorService>(pub Arc<T>);
                    impl<
                        T: ActorService,
                    > tonic::server::UnaryService<super::TerminateActorRequest>
                    for TerminateActorSvc<T> {
                        type Response = super::TerminateActorResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::TerminateActorRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ActorService>::terminate_actor(&inner, request).await
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
                        let method = TerminateActorSvc(inner);
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
