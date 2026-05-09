// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Loader actor: Reads documents (CPU-intensive).

use plexspaces_actor::{Actor as ActorTrait, ActorContext, BehaviorError, BehaviorType};
use plexspaces_sdk::{new_message, Message};
use serde::{Deserialize, Serialize};
use tracing::info;

/// Loader actor behavior
pub struct LoaderBehavior {
    /// Documents to process
    documents: Vec<String>,
}

impl LoaderBehavior {
    pub fn new(documents: Vec<String>) -> Self {
        Self { documents }
    }
}

#[async_trait::async_trait]
impl ActorTrait for LoaderBehavior {
    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }

    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        // Parse request
        let request: LoaderRequest = match serde_json::from_slice(&msg.payload) {
            Ok(req) => req,
            Err(_e) => {
                return Err(BehaviorError::UnsupportedMessage);
            }
        };

        match request {
            LoaderRequest::LoadDocument { document_id } => {
                info!("Loader: Loading document {}", document_id);
                
                // Simulate CPU-intensive document loading
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                
                // Find document (in real app, would read from storage)
                let content = format!("Document {} content: Lorem ipsum...", document_id);
                
                let response = LoaderResponse::DocumentLoaded {
                    document_id,
                    content,
                };
                
                if !msg.sender_id.is_empty() {
                    let reply_value = serde_json::to_value(&response)
                        .map_err(|e| BehaviorError::ProcessingError(format!("Failed to serialize reply: {}", e)))?;
                    let reply = new_message("reply", reply_value);
                    let correlation_id = if msg.correlation_id.is_empty() {
                        None
                    } else {
                        Some(msg.correlation_id.as_str())
                    };
                    ctx.send_reply(correlation_id, &msg.sender_id, msg.receiver_id.clone().into(), reply)
                        .await
                        .map_err(|e| BehaviorError::ProcessingError(format!("Failed to send reply: {}", e)))?;
                }
                
                Ok(())
            }
        }
    }
}

/// Loader request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LoaderRequest {
    LoadDocument { document_id: String },
}

/// Loader response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LoaderResponse {
    DocumentLoaded { document_id: String, content: String },
}
