// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! Helper Functions for Integration Testing

use plexspaces_mailbox::new_message;
use plexspaces_proto::v1::actor::{SendMessageRequest, SendMessageResponse};
use plexspaces_proto::ActorServiceClient;
use tonic::transport::Channel;
use tonic::{Request, Response, Status};

/// Create a message with payload and receiver (uses canonical new_message).
pub fn create_message(payload: &str, receiver: &str) -> plexspaces_proto::common::v1::Message {
    let mut message = new_message(payload.as_bytes().to_vec());
    message.receiver_id = receiver.to_string();
    message
}

/// Send a message via gRPC client
pub async fn send_message(
    client: &mut ActorServiceClient<Channel>,
    payload: &str,
    receiver: &str,
) -> Result<Response<SendMessageResponse>, Status> {
    let message = create_message(payload, receiver);

    let request = Request::new(SendMessageRequest {
        message: Some(message),
        wait_for_response: false,
        timeout: None,
    });

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
