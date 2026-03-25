// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! Helper Functions for Integration Testing

use plexspaces_proto::v1::actor::{SendMessageRequest, SendMessageResponse};
use plexspaces_proto::ActorServiceClient;
use tonic::transport::Channel;
use tonic::{Request, Response, Status};

/// Create a tell request with payload and target actor id/type.
pub fn create_send_message_request(payload: &str, receiver: &str) -> SendMessageRequest {
    SendMessageRequest {
        namespace: String::new(),
        actor_type: receiver.to_string(),
        http_method: "POST".to_string(),
        payload: payload.as_bytes().to_vec(),
        headers: Default::default(),
        query_params: Default::default(),
        path: String::new(),
        subpath: String::new(),
        sender_id: String::new(),
        message_type: "cast".to_string(),
        correlation_id: String::new(),
        reply_to: String::new(),
        message_id: String::new(),
    }
}

/// Send a message via gRPC client
pub async fn send_message(
    client: &mut ActorServiceClient<Channel>,
    payload: &str,
    receiver: &str,
) -> Result<Response<SendMessageResponse>, Status> {
    let request = Request::new(create_send_message_request(payload, receiver));

    client.send_message(request).await
}

/// Send a message and assert it succeeds
pub async fn send_message_ok(
    client: &mut ActorServiceClient<Channel>,
    payload: &str,
    receiver: &str,
) {
    let result = send_message(client, payload, receiver).await;
    assert!(result.is_ok(), "Failed to send message: {:?}", result.err());
}

/// Send a message and expect it to fail
pub async fn send_message_err(
    client: &mut ActorServiceClient<Channel>,
    payload: &str,
    receiver: &str,
) -> Status {
    let result = send_message(client, payload, receiver).await;
    assert!(result.is_err(), "Expected error but got success");
    result.unwrap_err()
}
