// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Loader actor: Reads documents (CPU-intensive).

use plexspaces_core::{ActorBehavior, ActorContext, BehaviorError, BehaviorType};
use plexspaces_mailbox::Message;
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
impl ActorBehavior for LoaderBehavior {
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
                
                // Send reply via context
                if let Ok(reply_data) = serde_json::to_vec(&response) {
                    let reply = Message::new(reply_data);
                    let _ = ctx.reply(reply).await; // Ignore reply errors for now
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

