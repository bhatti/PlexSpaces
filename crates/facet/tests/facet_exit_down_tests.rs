// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is free software: you can redistribute it and/or modify
// it under the terms of the GNU Lesser General Public License as published by
// the Free Software Foundation, either version 2.1 of the License, or
// (at your option) any later version.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! Tests for facet EXIT/DOWN handling (Phase 4.3)
//!
//! ## Purpose
//! Tests that facets properly implement `on_exit()` and `on_down()` methods
//! and handle lifecycle events correctly.

use plexspaces_facet::{Facet, FacetError, ExitReason};
use serde_json::Value;

/// Test facet that tracks EXIT and DOWN calls
struct TestExitDownFacet {
    exit_calls: std::sync::Arc<tokio::sync::RwLock<Vec<(String, String, ExitReason)>>>,
    down_calls: std::sync::Arc<tokio::sync::RwLock<Vec<(String, String, ExitReason)>>>,
}

impl TestExitDownFacet {
    fn new() -> (Self, std::sync::Arc<tokio::sync::RwLock<Vec<(String, String, ExitReason)>>>, std::sync::Arc<tokio::sync::RwLock<Vec<(String, String, ExitReason)>>>) {
        let exit_calls = std::sync::Arc::new(tokio::sync::RwLock::new(Vec::new()));
        let down_calls = std::sync::Arc::new(tokio::sync::RwLock::new(Vec::new()));
        let facet = Self {
            exit_calls: exit_calls.clone(),
            down_calls: down_calls.clone(),
        };
        (facet, exit_calls, down_calls)
    }
}

#[async_trait::async_trait]
impl Facet for TestExitDownFacet {
    fn facet_type(&self) -> &str {
        "test_exit_down"
    }
    
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    async fn on_attach(&mut self, _actor_id: &str, _config: Value) -> Result<(), FacetError> {
        Ok(())
    }

    async fn on_detach(&mut self, _actor_id: &str) -> Result<(), FacetError> {
        Ok(())
    }

    async fn on_exit(&mut self, actor_id: &str, from: &str, reason: &ExitReason) -> Result<(), FacetError> {
        let mut calls = self.exit_calls.write().await;
        calls.push((actor_id.to_string(), from.to_string(), reason.clone()));
        Ok(())
    }

    async fn on_down(&mut self, actor_id: &str, monitored_id: &str, reason: &ExitReason) -> Result<(), FacetError> {
        let mut calls = self.down_calls.write().await;
        calls.push((actor_id.to_string(), monitored_id.to_string(), reason.clone()));
        Ok(())
    }

    fn get_config(&self) -> Value {
        Value::Null
    }

    fn get_priority(&self) -> i32 {
        100
    }
}

#[tokio::test]
async fn test_facet_on_exit_called() {
    // Test that facet.on_exit() is called when actor receives EXIT signal
    let (mut facet, exit_calls, _down_calls) = TestExitDownFacet::new();
    
    let actor_id = "test-actor";
    let from_actor_id = "linked-actor";
    let reason = ExitReason::Error("test error".to_string());
    
    // Call on_exit
    let result = facet.on_exit(actor_id, from_actor_id, &reason).await;
    assert!(result.is_ok(), "on_exit should succeed");
    
    // Verify call was recorded
    let calls = exit_calls.read().await;
    assert_eq!(calls.len(), 1, "on_exit should be called once");
    assert_eq!(calls[0].0, actor_id, "actor_id should match");
    assert_eq!(calls[0].1, from_actor_id, "from_actor_id should match");
    assert_eq!(calls[0].2, reason, "reason should match");
}

#[tokio::test]
async fn test_facet_on_down_called() {
    // Test that facet.on_down() is called when monitored actor terminates
    let (mut facet, _exit_calls, down_calls) = TestExitDownFacet::new();
    
    let monitoring_actor_id = "monitoring-actor";
    let monitored_actor_id = "monitored-actor";
    let reason = ExitReason::Normal;
    
    // Call on_down
    let result = facet.on_down(monitoring_actor_id, monitored_actor_id, &reason).await;
    assert!(result.is_ok(), "on_down should succeed");
    
    // Verify call was recorded
    let calls = down_calls.read().await;
    assert_eq!(calls.len(), 1, "on_down should be called once");
    assert_eq!(calls[0].0, monitoring_actor_id, "monitoring_actor_id should match");
    assert_eq!(calls[0].1, monitored_actor_id, "monitored_actor_id should match");
    assert_eq!(calls[0].2, reason, "reason should match");
}

#[tokio::test]
async fn test_facet_on_exit_with_different_reasons() {
    // Test that facet.on_exit() handles different exit reasons
    let (mut facet, exit_calls, _down_calls) = TestExitDownFacet::new();
    
    let actor_id = "test-actor";
    let from_actor_id = "linked-actor";
    
    // Test Normal
    facet.on_exit(actor_id, from_actor_id, &ExitReason::Normal).await.unwrap();
    
    // Test Shutdown
    facet.on_exit(actor_id, from_actor_id, &ExitReason::Shutdown).await.unwrap();
    
    // Test Killed
    facet.on_exit(actor_id, from_actor_id, &ExitReason::Killed).await.unwrap();
    
    // Test Error
    facet.on_exit(actor_id, from_actor_id, &ExitReason::Error("test".to_string())).await.unwrap();
    
    // Verify all calls were recorded
    let calls = exit_calls.read().await;
    assert_eq!(calls.len(), 4, "on_exit should be called 4 times");
}

#[tokio::test]
async fn test_facet_on_exit_error_handling() {
    // Test that facet.on_exit() errors don't stop processing
    // This is tested at the FacetContainer level, but we verify the method signature
    let (mut facet, _exit_calls, _down_calls) = TestExitDownFacet::new();
    
    let actor_id = "test-actor";
    let from_actor_id = "linked-actor";
    let reason = ExitReason::Error("test error".to_string());
    
    // Call on_exit - should succeed
    let result = facet.on_exit(actor_id, from_actor_id, &reason).await;
    assert!(result.is_ok(), "on_exit should succeed for test facet");
}

#[tokio::test]
async fn test_facet_on_down_error_handling() {
    // Test that facet.on_down() errors don't stop processing
    // This is tested at the FacetContainer level, but we verify the method signature
    let (mut facet, _exit_calls, _down_calls) = TestExitDownFacet::new();
    
    let monitoring_actor_id = "monitoring-actor";
    let monitored_actor_id = "monitored-actor";
    let reason = ExitReason::Normal;
    
    // Call on_down - should succeed
    let result = facet.on_down(monitoring_actor_id, monitored_actor_id, &reason).await;
    assert!(result.is_ok(), "on_down should succeed for test facet");
}

