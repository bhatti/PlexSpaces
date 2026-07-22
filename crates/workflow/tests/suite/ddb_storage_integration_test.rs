// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! Integration tests for DynamoDB WorkflowStorage backend.
//!
//! ## TDD Approach
//! These tests are written FIRST before implementation (RED phase).
//! Implementation will be written to make these tests pass (GREEN phase).

#[cfg(feature = "ddb-backend")]
mod ddb_tests {
    use plexspaces_actor::RequestContext;
    use plexspaces_common::RequestContextExt;
    use plexspaces_workflow::types::{
        ExecutionStatus, StepExecutionExt, StepStatus, WorkflowExecutionExt,
    };
    use plexspaces_workflow::*;
    use serde_json::json;
    use std::collections::HashMap;
    use std::time::Duration;

    use plexspaces_common::test_helpers::dynamodb_local_available;

    /// Helper to check if DynamoDB simulator is available and skip test with warning if not
    async fn check_ddb_available() -> bool {
        if !dynamodb_local_available().await {
            eprintln!("⚠️  WARNING: DynamoDB Local simulator is not running. Skipping DDB test.");
            eprintln!("   To run DDB tests, start DynamoDB Local: docker run -p 8000:8000 amazon/dynamodb-local");
            return false;
        }
        true
    }

    /// Helper to create DynamoDB storage for testing
    async fn create_ddb_storage() -> DynamoDBWorkflowStorage {
        plexspaces_common::test_helpers::setup_aws_local_env();
        let endpoint = plexspaces_common::test_helpers::get_dynamodb_endpoint();

        DynamoDBWorkflowStorage::new(
            "us-east-1".to_string(),
            "plexspaces-workflow-test".to_string(),
            Some(endpoint),
        )
        .await
        .expect("Failed to create DynamoDB workflow storage")
    }

    /// Helper to create test RequestContext with unique tenant IDs for test isolation
    fn tenant1_ctx() -> RequestContext {
        use ulid::Ulid;
        let unique_id = Ulid::new().to_string();
        RequestContext::new_without_auth(format!("tenant1-{}", unique_id), "default".to_string())
    }

    fn tenant2_ctx() -> RequestContext {
        use ulid::Ulid;
        let unique_id = Ulid::new().to_string();
        RequestContext::new_without_auth(format!("tenant2-{}", unique_id), "default".to_string())
    }

    /// Helper to create a default test RequestContext with unique tenant ID
    #[allow(dead_code)]
    fn default_ctx() -> RequestContext {
        use ulid::Ulid;
        let unique_id = Ulid::new().to_string();
        RequestContext::new_without_auth(format!("default-{}", unique_id), "default".to_string())
    }

    // =========================================================================
    // Workflow Definition Tests
    // =========================================================================

    #[tokio::test]
    async fn test_ddb_save_and_get_definition() {
        if !check_ddb_available().await {
            return;
        }
        let storage = create_ddb_storage().await;
        let ctx = tenant1_ctx();

        let definition = WorkflowDefinition {
            id: "test-workflow".to_string(),
            name: "Test Workflow".to_string(),
            version: "1.0".to_string(),
            steps: vec![],
            ..Default::default()
        };

        storage.save_definition(&ctx, &definition).await.unwrap();

        let retrieved = storage
            .get_definition(&ctx, "test-workflow", "1.0")
            .await
            .unwrap();
        assert_eq!(retrieved.id, "test-workflow");
        assert_eq!(retrieved.name, "Test Workflow");
        assert_eq!(retrieved.version, "1.0");
    }

    #[tokio::test]
    async fn test_ddb_save_definition_overwrites() {
        if !check_ddb_available().await {
            return;
        }
        let storage = create_ddb_storage().await;
        let ctx = tenant1_ctx();

        let mut definition = WorkflowDefinition {
            id: "test-workflow".to_string(),
            name: "Test Workflow".to_string(),
            version: "1.0".to_string(),
            steps: vec![],
            ..Default::default()
        };

        storage.save_definition(&ctx, &definition).await.unwrap();

        definition.name = "Updated Workflow".to_string();
        storage.save_definition(&ctx, &definition).await.unwrap();

        let retrieved = storage
            .get_definition(&ctx, "test-workflow", "1.0")
            .await
            .unwrap();
        assert_eq!(retrieved.name, "Updated Workflow");
    }

    #[tokio::test]
    async fn test_ddb_get_definition_not_found() {
        if !check_ddb_available().await {
            return;
        }
        let storage = create_ddb_storage().await;
        let ctx = tenant1_ctx();

        let result = storage.get_definition(&ctx, "nonexistent", "1.0").await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), WorkflowError::NotFound(_)));
    }

    #[tokio::test]
    async fn test_ddb_list_definitions() {
        if !check_ddb_available().await {
            return;
        }
        let storage = create_ddb_storage().await;
        let ctx = tenant1_ctx();

        let def1 = WorkflowDefinition {
            id: "workflow-1".to_string(),
            name: "Workflow One".to_string(),
            version: "1.0".to_string(),
            steps: vec![],
            ..Default::default()
        };

        let def2 = WorkflowDefinition {
            id: "workflow-2".to_string(),
            name: "Workflow Two".to_string(),
            version: "1.0".to_string(),
            steps: vec![],
            ..Default::default()
        };

        storage.save_definition(&ctx, &def1).await.unwrap();
        storage.save_definition(&ctx, &def2).await.unwrap();

        let definitions = storage.list_definitions(&ctx, None).await.unwrap();
        assert_eq!(definitions.len(), 2);
    }

    #[tokio::test]
    async fn test_ddb_list_definitions_with_prefix() {
        if !check_ddb_available().await {
            return;
        }
        let storage = create_ddb_storage().await;
        let ctx = tenant1_ctx();

        let def1 = WorkflowDefinition {
            id: "credit-workflow".to_string(),
            name: "Credit Workflow".to_string(),
            version: "1.0".to_string(),
            steps: vec![],
            ..Default::default()
        };

        let def2 = WorkflowDefinition {
            id: "payment-workflow".to_string(),
            name: "Payment Workflow".to_string(),
            version: "1.0".to_string(),
            steps: vec![],
            ..Default::default()
        };

        storage.save_definition(&ctx, &def1).await.unwrap();
        storage.save_definition(&ctx, &def2).await.unwrap();

        let definitions = storage
            .list_definitions(&ctx, Some("Credit"))
            .await
            .unwrap();
        assert_eq!(definitions.len(), 1);
        assert_eq!(definitions[0].name, "Credit Workflow");
    }

    #[tokio::test]
    async fn test_ddb_delete_definition() {
        if !check_ddb_available().await {
            return;
        }
        let storage = create_ddb_storage().await;
        let ctx = tenant1_ctx();

        let definition = WorkflowDefinition {
            id: "test-workflow".to_string(),
            name: "Test Workflow".to_string(),
            version: "1.0".to_string(),
            steps: vec![],
            ..Default::default()
        };

        storage.save_definition(&ctx, &definition).await.unwrap();
        storage
            .delete_definition(&ctx, "test-workflow", "1.0")
            .await
            .unwrap();

        let result = storage.get_definition(&ctx, "test-workflow", "1.0").await;
        assert!(result.is_err());
    }

    // =========================================================================
    // Workflow Execution Tests
    // =========================================================================

    #[tokio::test]
    async fn test_ddb_create_and_get_execution() {
        if !check_ddb_available().await {
            return;
        }
        let storage = create_ddb_storage().await;
        let ctx = tenant1_ctx();

        // Create definition first
        let definition = WorkflowDefinition {
            id: "test-workflow".to_string(),
            name: "Test Workflow".to_string(),
            version: "1.0".to_string(),
            steps: vec![],
            ..Default::default()
        };
        storage.save_definition(&ctx, &definition).await.unwrap();

        // Create execution
        let execution_id = storage
            .create_execution(
                &ctx,
                "test-workflow",
                "1.0",
                json!({"input": "test"}),
                HashMap::new(),
            )
            .await
            .unwrap();

        // Get execution
        let execution = storage.get_execution(&ctx, &execution_id).await.unwrap();

        assert_eq!(execution.execution_id, execution_id);
        assert_eq!(execution.definition_id, "test-workflow");
        assert_eq!(
            execution.status,
            ExecutionStatus::ExecutionStatusPending as i32
        );
    }

    #[tokio::test]
    async fn test_ddb_create_execution_with_node() {
        if !check_ddb_available().await {
            return;
        }
        let storage = create_ddb_storage().await;
        let ctx = tenant1_ctx();

        let definition = WorkflowDefinition {
            id: "test-workflow".to_string(),
            name: "Test Workflow".to_string(),
            version: "1.0".to_string(),
            steps: vec![],
            ..Default::default()
        };
        storage.save_definition(&ctx, &definition).await.unwrap();

        let execution_id = storage
            .create_execution_with_node(
                &ctx,
                "test-workflow",
                "1.0",
                json!({"input": "test"}),
                HashMap::new(),
                Some("node-1"),
            )
            .await
            .unwrap();

        let execution = storage.get_execution(&ctx, &execution_id).await.unwrap();
        assert_eq!(execution.node_id, "node-1".to_string());
    }

    #[tokio::test]
    async fn test_ddb_update_execution_status() {
        if !check_ddb_available().await {
            return;
        }
        let storage = create_ddb_storage().await;
        let ctx = tenant1_ctx();

        let definition = WorkflowDefinition {
            id: "test-workflow".to_string(),
            name: "Test Workflow".to_string(),
            version: "1.0".to_string(),
            steps: vec![],
            ..Default::default()
        };
        storage.save_definition(&ctx, &definition).await.unwrap();

        let execution_id = storage
            .create_execution(&ctx, "test-workflow", "1.0", json!({}), HashMap::new())
            .await
            .unwrap();

        storage
            .update_execution_status(&ctx, &execution_id, ExecutionStatus::ExecutionStatusRunning)
            .await
            .unwrap();

        let execution = storage.get_execution(&ctx, &execution_id).await.unwrap();
        assert_eq!(
            execution.status,
            ExecutionStatus::ExecutionStatusRunning as i32
        );
    }

    #[tokio::test]
    async fn test_ddb_update_execution_status_with_version() {
        if !check_ddb_available().await {
            return;
        }
        let storage = create_ddb_storage().await;
        let ctx = tenant1_ctx();

        let definition = WorkflowDefinition {
            id: "test-workflow".to_string(),
            name: "Test Workflow".to_string(),
            version: "1.0".to_string(),
            steps: vec![],
            ..Default::default()
        };
        storage.save_definition(&ctx, &definition).await.unwrap();

        let execution_id = storage
            .create_execution(&ctx, "test-workflow", "1.0", json!({}), HashMap::new())
            .await
            .unwrap();

        // Update with correct version
        storage
            .update_execution_status_with_version(
                &ctx,
                &execution_id,
                ExecutionStatus::ExecutionStatusRunning,
                Some(1),
            )
            .await
            .unwrap();

        // Try to update with wrong version (should fail)
        let result = storage
            .update_execution_status_with_version(
                &ctx,
                &execution_id,
                ExecutionStatus::ExecutionStatusCompleted,
                Some(1),
            )
            .await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            WorkflowError::ConcurrentUpdate(_)
        ));
    }

    #[tokio::test]
    async fn test_ddb_update_execution_output() {
        if !check_ddb_available().await {
            return;
        }
        let storage = create_ddb_storage().await;
        let ctx = tenant1_ctx();

        let definition = WorkflowDefinition {
            id: "test-workflow".to_string(),
            name: "Test Workflow".to_string(),
            version: "1.0".to_string(),
            steps: vec![],
            ..Default::default()
        };
        storage.save_definition(&ctx, &definition).await.unwrap();

        let execution_id = storage
            .create_execution(&ctx, "test-workflow", "1.0", json!({}), HashMap::new())
            .await
            .unwrap();

        storage
            .update_execution_output(&ctx, &execution_id, json!({"result": "success"}))
            .await
            .unwrap();

        let execution = storage.get_execution(&ctx, &execution_id).await.unwrap();
        assert_eq!(execution.output_value(), Some(json!({"result": "success"})));
    }

    #[tokio::test]
    async fn test_ddb_transfer_ownership() {
        if !check_ddb_available().await {
            return;
        }
        let storage = create_ddb_storage().await;
        let ctx = tenant1_ctx();

        let definition = WorkflowDefinition {
            id: "test-workflow".to_string(),
            name: "Test Workflow".to_string(),
            version: "1.0".to_string(),
            steps: vec![],
            ..Default::default()
        };
        storage.save_definition(&ctx, &definition).await.unwrap();

        let execution_id = storage
            .create_execution_with_node(
                &ctx,
                "test-workflow",
                "1.0",
                json!({}),
                HashMap::new(),
                Some("node-1"),
            )
            .await
            .unwrap();

        storage
            .transfer_ownership(&ctx, &execution_id, "node-2", 1)
            .await
            .unwrap();

        let execution = storage.get_execution(&ctx, &execution_id).await.unwrap();
        assert_eq!(execution.node_id, "node-2".to_string());
    }

    #[tokio::test]
    async fn test_ddb_update_heartbeat() {
        if !check_ddb_available().await {
            return;
        }
        let storage = create_ddb_storage().await;
        let ctx = tenant1_ctx();

        let definition = WorkflowDefinition {
            id: "test-workflow".to_string(),
            name: "Test Workflow".to_string(),
            version: "1.0".to_string(),
            steps: vec![],
            ..Default::default()
        };
        storage.save_definition(&ctx, &definition).await.unwrap();

        let execution_id = storage
            .create_execution_with_node(
                &ctx,
                "test-workflow",
                "1.0",
                json!({}),
                HashMap::new(),
                Some("node-1"),
            )
            .await
            .unwrap();

        let initial_execution = storage.get_execution(&ctx, &execution_id).await.unwrap();
        let initial_heartbeat = initial_execution
            .last_heartbeat
            .expect("Initial heartbeat should be set when creating execution with node");
        let initial_timestamp = initial_heartbeat.seconds;

        // Sleep long enough to ensure timestamp difference (at least 1 second for Unix timestamps)
        tokio::time::sleep(Duration::from_millis(1100)).await;

        storage
            .update_heartbeat(&ctx, &execution_id, "node-1")
            .await
            .unwrap();

        // Retry getting execution in case of eventual consistency
        let mut execution = storage.get_execution(&ctx, &execution_id).await.unwrap();
        let mut retries = 0;
        while execution.last_heartbeat.map(|h| h.seconds) == Some(initial_timestamp) && retries < 5
        {
            tokio::time::sleep(Duration::from_millis(100)).await;
            execution = storage.get_execution(&ctx, &execution_id).await.unwrap();
            retries += 1;
        }

        let updated_heartbeat = execution
            .last_heartbeat
            .expect("Heartbeat should be updated");
        let updated_timestamp = updated_heartbeat.seconds;
        assert!(
            updated_timestamp > initial_timestamp,
            "Heartbeat should be updated after sleep and update_heartbeat call (initial: {}, updated: {})",
            initial_timestamp,
            updated_timestamp
        );
    }

    // =========================================================================
    // Step Execution Tests
    // =========================================================================

    #[tokio::test]
    async fn test_ddb_create_and_get_step_execution() {
        if !check_ddb_available().await {
            return;
        }
        let storage = create_ddb_storage().await;
        let ctx = tenant1_ctx();

        let definition = WorkflowDefinition {
            id: "test-workflow".to_string(),
            name: "Test Workflow".to_string(),
            version: "1.0".to_string(),
            steps: vec![],
            ..Default::default()
        };
        storage.save_definition(&ctx, &definition).await.unwrap();

        let execution_id = storage
            .create_execution(&ctx, "test-workflow", "1.0", json!({}), HashMap::new())
            .await
            .unwrap();

        let step_exec_id = storage
            .create_step_execution(&ctx, &execution_id, "step-1", json!({"input": "test"}))
            .await
            .unwrap();

        let step_exec = storage
            .get_step_execution(&ctx, &step_exec_id)
            .await
            .unwrap();
        assert_eq!(step_exec.step_execution_id, step_exec_id);
        assert_eq!(step_exec.execution_id, execution_id);
        assert_eq!(step_exec.step_id, "step-1");
        assert_eq!(step_exec.status, StepStatus::StepStatusRunning as i32);
        assert_eq!(step_exec.attempt, 1);
    }

    #[tokio::test]
    async fn test_ddb_complete_step_execution() {
        if !check_ddb_available().await {
            return;
        }
        let storage = create_ddb_storage().await;
        let ctx = tenant1_ctx();

        let definition = WorkflowDefinition {
            id: "test-workflow".to_string(),
            name: "Test Workflow".to_string(),
            version: "1.0".to_string(),
            steps: vec![],
            ..Default::default()
        };
        storage.save_definition(&ctx, &definition).await.unwrap();

        let execution_id = storage
            .create_execution(&ctx, "test-workflow", "1.0", json!({}), HashMap::new())
            .await
            .unwrap();

        let step_exec_id = storage
            .create_step_execution(&ctx, &execution_id, "step-1", json!({}))
            .await
            .unwrap();

        storage
            .complete_step_execution(
                &ctx,
                &step_exec_id,
                StepStatus::StepStatusCompleted,
                Some(json!({"result": "success"})),
                None,
            )
            .await
            .unwrap();

        let step_exec = storage
            .get_step_execution(&ctx, &step_exec_id)
            .await
            .unwrap();
        assert_eq!(step_exec.status, StepStatus::StepStatusCompleted as i32);
        let output_val = step_exec.output_value();
        assert_eq!(output_val, Some(json!({"result": "success"})));
    }

    #[tokio::test]
    async fn test_ddb_get_step_execution_history() {
        if !check_ddb_available().await {
            return;
        }
        let storage = create_ddb_storage().await;
        let ctx = tenant1_ctx();

        let definition = WorkflowDefinition {
            id: "test-workflow".to_string(),
            name: "Test Workflow".to_string(),
            version: "1.0".to_string(),
            steps: vec![],
            ..Default::default()
        };
        storage.save_definition(&ctx, &definition).await.unwrap();

        let execution_id = storage
            .create_execution(&ctx, "test-workflow", "1.0", json!({}), HashMap::new())
            .await
            .unwrap();

        storage
            .create_step_execution(&ctx, &execution_id, "step-1", json!({}))
            .await
            .unwrap();

        storage
            .create_step_execution(&ctx, &execution_id, "step-2", json!({}))
            .await
            .unwrap();

        let history = storage
            .get_step_execution_history(&ctx, &execution_id)
            .await
            .unwrap();
        assert_eq!(history.len(), 2);
    }

    // =========================================================================
    // Signal Tests
    // =========================================================================

    #[tokio::test]
    async fn test_ddb_send_and_check_signal() {
        if !check_ddb_available().await {
            return;
        }
        let storage = create_ddb_storage().await;
        let ctx = tenant1_ctx();

        let definition = WorkflowDefinition {
            id: "test-workflow".to_string(),
            name: "Test Workflow".to_string(),
            version: "1.0".to_string(),
            steps: vec![],
            ..Default::default()
        };
        storage.save_definition(&ctx, &definition).await.unwrap();

        let execution_id = storage
            .create_execution(&ctx, "test-workflow", "1.0", json!({}), HashMap::new())
            .await
            .unwrap();

        storage
            .send_signal(&ctx, &execution_id, "approval", json!({"approved": true}))
            .await
            .unwrap();

        let signal = storage
            .check_signal(&ctx, &execution_id, "approval")
            .await
            .unwrap();
        assert!(signal.is_some());
        assert_eq!(signal.unwrap(), json!({"approved": true}));

        // Signal should be consumed (deleted)
        let signal_again = storage
            .check_signal(&ctx, &execution_id, "approval")
            .await
            .unwrap();
        assert!(signal_again.is_none());
    }

    // =========================================================================
    // Query Tests
    // =========================================================================

    #[tokio::test]
    async fn test_ddb_list_executions_by_status() {
        if !check_ddb_available().await {
            return;
        }
        let storage = create_ddb_storage().await;
        let ctx = tenant1_ctx();

        let definition = WorkflowDefinition {
            id: "test-workflow".to_string(),
            name: "Test Workflow".to_string(),
            version: "1.0".to_string(),
            steps: vec![],
            ..Default::default()
        };
        storage.save_definition(&ctx, &definition).await.unwrap();

        let exec1 = storage
            .create_execution(&ctx, "test-workflow", "1.0", json!({}), HashMap::new())
            .await
            .unwrap();

        let exec2 = storage
            .create_execution(&ctx, "test-workflow", "1.0", json!({}), HashMap::new())
            .await
            .unwrap();

        storage
            .update_execution_status(&ctx, &exec1, ExecutionStatus::ExecutionStatusRunning)
            .await
            .unwrap();
        storage
            .update_execution_status(&ctx, &exec2, ExecutionStatus::ExecutionStatusCompleted)
            .await
            .unwrap();

        let running = storage
            .list_executions_by_status(&ctx, vec![ExecutionStatus::ExecutionStatusRunning], None)
            .await
            .unwrap();
        assert_eq!(running.len(), 1);
        assert_eq!(running[0].execution_id, exec1);

        let completed = storage
            .list_executions_by_status(&ctx, vec![ExecutionStatus::ExecutionStatusCompleted], None)
            .await
            .unwrap();
        assert_eq!(completed.len(), 1);
        assert_eq!(completed[0].execution_id, exec2);
    }

    #[tokio::test]
    async fn test_ddb_list_stale_executions() {
        if !check_ddb_available().await {
            return;
        }
        let storage = create_ddb_storage().await;
        let ctx = tenant1_ctx();

        let definition = WorkflowDefinition {
            id: "test-workflow".to_string(),
            name: "Test Workflow".to_string(),
            version: "1.0".to_string(),
            steps: vec![],
            ..Default::default()
        };
        storage.save_definition(&ctx, &definition).await.unwrap();

        let execution_id = storage
            .create_execution_with_node(
                &ctx,
                "test-workflow",
                "1.0",
                json!({}),
                HashMap::new(),
                Some("node-1"),
            )
            .await
            .unwrap();

        storage
            .update_execution_status(&ctx, &execution_id, ExecutionStatus::ExecutionStatusRunning)
            .await
            .unwrap();

        // Wait a bit
        tokio::time::sleep(Duration::from_millis(100)).await;

        // List stale executions (threshold = 1 second, but we only waited 100ms, so should be empty)
        let stale = storage
            .list_stale_executions(&ctx, 1, vec![ExecutionStatus::ExecutionStatusRunning])
            .await
            .unwrap();
        assert_eq!(stale.len(), 0);

        // Wait longer
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Now should be stale
        let stale = storage
            .list_stale_executions(&ctx, 1, vec![ExecutionStatus::ExecutionStatusRunning])
            .await
            .unwrap();
        assert_eq!(stale.len(), 1);
    }

    // =========================================================================
    // Tenant Isolation Tests (CRITICAL FOR SECURITY)
    // =========================================================================

    #[tokio::test]
    async fn test_ddb_tenant_isolation_definitions() {
        if !check_ddb_available().await {
            return;
        }
        let storage = create_ddb_storage().await;
        let ctx1 = tenant1_ctx();
        let ctx2 = tenant2_ctx();

        let def1 = WorkflowDefinition {
            id: "workflow-1".to_string(),
            name: "Workflow 1".to_string(),
            version: "1.0".to_string(),
            steps: vec![],
            ..Default::default()
        };

        let def2 = WorkflowDefinition {
            id: "workflow-1".to_string(), // Same ID
            name: "Workflow 1".to_string(),
            version: "1.0".to_string(),
            steps: vec![],
            ..Default::default()
        };

        storage.save_definition(&ctx1, &def1).await.unwrap();
        storage.save_definition(&ctx2, &def2).await.unwrap();

        // Each tenant should see their own definition
        let def1_retrieved = storage
            .get_definition(&ctx1, "workflow-1", "1.0")
            .await
            .unwrap();
        let def2_retrieved = storage
            .get_definition(&ctx2, "workflow-1", "1.0")
            .await
            .unwrap();

        assert_eq!(def1_retrieved.id, "workflow-1");
        assert_eq!(def2_retrieved.id, "workflow-1");

        // Tenant1 should not see tenant2's definitions
        let all_defs = storage.list_definitions(&ctx1, None).await.unwrap();
        assert_eq!(all_defs.len(), 1);
    }

    #[tokio::test]
    async fn test_ddb_tenant_isolation_executions() {
        if !check_ddb_available().await {
            return;
        }
        let storage = create_ddb_storage().await;
        let ctx1 = tenant1_ctx();
        let ctx2 = tenant2_ctx();

        let definition = WorkflowDefinition {
            id: "test-workflow".to_string(),
            name: "Test Workflow".to_string(),
            version: "1.0".to_string(),
            steps: vec![],
            ..Default::default()
        };
        storage.save_definition(&ctx1, &definition).await.unwrap();
        storage.save_definition(&ctx2, &definition).await.unwrap();

        let exec1 = storage
            .create_execution(&ctx1, "test-workflow", "1.0", json!({}), HashMap::new())
            .await
            .unwrap();

        let exec2 = storage
            .create_execution(&ctx2, "test-workflow", "1.0", json!({}), HashMap::new())
            .await
            .unwrap();

        // Each tenant should only see their own executions
        let running1 = storage
            .list_executions_by_status(&ctx1, vec![ExecutionStatus::ExecutionStatusPending], None)
            .await
            .unwrap();
        assert_eq!(running1.len(), 1);
        assert_eq!(running1[0].execution_id, exec1);

        let running2 = storage
            .list_executions_by_status(&ctx2, vec![ExecutionStatus::ExecutionStatusPending], None)
            .await
            .unwrap();
        assert_eq!(running2.len(), 1);
        assert_eq!(running2[0].execution_id, exec2);

        // Tenant1 should not be able to access tenant2's execution
        let result = storage.get_execution(&ctx1, &exec2).await;
        assert!(result.is_err());
    }
}
