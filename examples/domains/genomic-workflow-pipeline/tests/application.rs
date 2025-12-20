// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

use plexspaces_core::application::{Application, ApplicationNode};
use plexspaces_node::{NodeBuilder, NodeId};
use genomic_workflow_pipeline::application::GenomicsPipelineApplication;
use std::sync::Arc;

#[tokio::test]
async fn test_genomic_pipeline_application_starts_and_stops() {
    let node = Arc::new(NodeBuilder::new("test-node".to_string()).build());

    let mut app = GenomicsPipelineApplication::new();

    // Start the application
    let result = app.start(node.clone()).await;
    assert!(result.is_ok());

    // Stop the application
    let result = app.stop().await;
    assert!(result.is_ok());
}
