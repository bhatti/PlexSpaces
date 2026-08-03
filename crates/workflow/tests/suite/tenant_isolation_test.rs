// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! Tenant isolation tests for workflow storage.
//!
//! Verifies that workflows created by one tenant cannot be accessed by another.

use plexspaces_actor::{RequestContext, RequestContextExt};
use plexspaces_workflow::storage::sql::make_workflow_definition;
use plexspaces_workflow::storage::WorkflowStorage;
use plexspaces_workflow::types::*;
use serde_json::json;
use std::collections::HashMap;

fn ctx(tenant: &str, ns: &str) -> RequestContext {
    RequestContext::new_without_auth(tenant.into(), ns.into())
}

#[tokio::test]
async fn test_workflow_definitions_isolated_by_tenant() {
    let storage = WorkflowStorage::new_in_memory().await.unwrap();

    let tenant_a = ctx("tenant-a", "ns1");
    let tenant_b = ctx("tenant-b", "ns1");

    let def = make_workflow_definition("shared-name", "Workflow", "1.0", vec![]);

    storage.save_definition(&tenant_a, &def).await.unwrap();

    // Tenant A can access their own definition
    let result = storage
        .get_definition(&tenant_a, "shared-name", "1.0")
        .await;
    assert!(result.is_ok());

    // Tenant B cannot access tenant A's definition
    let result = storage
        .get_definition(&tenant_b, "shared-name", "1.0")
        .await;
    assert!(
        result.is_err(),
        "Tenant B should not see tenant A's workflow"
    );
}

#[tokio::test]
async fn test_workflow_list_definitions_filtered_by_tenant() {
    let storage = WorkflowStorage::new_in_memory().await.unwrap();

    let tenant_a = ctx("tenant-a", "ns1");
    let tenant_b = ctx("tenant-b", "ns1");

    let def_a = make_workflow_definition("wf-a", "Workflow A", "1.0", vec![]);
    let def_b = make_workflow_definition("wf-b", "Workflow B", "1.0", vec![]);

    storage.save_definition(&tenant_a, &def_a).await.unwrap();
    storage.save_definition(&tenant_b, &def_b).await.unwrap();

    // Tenant A sees only their workflow
    let list_a = storage.list_definitions(&tenant_a, None).await.unwrap();
    assert_eq!(list_a.len(), 1);
    assert_eq!(list_a[0].id, "wf-a");

    // Tenant B sees only their workflow
    let list_b = storage.list_definitions(&tenant_b, None).await.unwrap();
    assert_eq!(list_b.len(), 1);
    assert_eq!(list_b[0].id, "wf-b");
}

#[tokio::test]
async fn test_workflow_executions_isolated_by_tenant() {
    let storage = WorkflowStorage::new_in_memory().await.unwrap();

    let tenant_a = ctx("tenant-a", "ns1");
    let tenant_b = ctx("tenant-b", "ns1");

    let def = make_workflow_definition("wf", "Workflow", "1.0", vec![]);
    storage.save_definition(&tenant_a, &def).await.unwrap();

    let exec_id = storage
        .create_execution(&tenant_a, "wf", "1.0", json!({}), HashMap::new())
        .await
        .unwrap();

    // Tenant A can access their execution
    let result = storage.get_execution(&tenant_a, &exec_id).await;
    assert!(result.is_ok());

    // Tenant B cannot access tenant A's execution
    let result = storage.get_execution(&tenant_b, &exec_id).await;
    assert!(
        result.is_err(),
        "Tenant B should not see tenant A's execution"
    );
}

#[tokio::test]
async fn test_workflow_delete_respects_tenant() {
    let storage = WorkflowStorage::new_in_memory().await.unwrap();

    let tenant_a = ctx("tenant-a", "ns1");
    let tenant_b = ctx("tenant-b", "ns1");

    let def = make_workflow_definition("wf", "Workflow", "1.0", vec![]);
    storage.save_definition(&tenant_a, &def).await.unwrap();

    // Tenant B cannot delete tenant A's definition
    let result = storage.delete_definition(&tenant_b, "wf", "1.0").await;
    // Should either fail or silently do nothing
    // Verify tenant A's definition still exists
    let still_exists = storage.get_definition(&tenant_a, "wf", "1.0").await;
    assert!(
        still_exists.is_ok(),
        "Tenant A's definition should still exist after B's delete attempt"
    );
}

#[tokio::test]
async fn test_namespace_isolation_within_tenant() {
    let storage = WorkflowStorage::new_in_memory().await.unwrap();

    let ns1 = ctx("tenant-a", "ns1");
    let ns2 = ctx("tenant-a", "ns2");

    let def = make_workflow_definition("wf", "Workflow", "1.0", vec![]);
    storage.save_definition(&ns1, &def).await.unwrap();

    // Same tenant, different namespace cannot access
    let result = storage.get_definition(&ns2, "wf", "1.0").await;
    assert!(
        result.is_err(),
        "Different namespace should not see the workflow"
    );
}
