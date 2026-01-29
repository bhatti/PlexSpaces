// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Workflow Recovery Service with Robust Node Ownership and Health Monitoring
//!
//! Implements auto-recovery of interrupted workflows with:
//! - Optimistic locking (version-based) to prevent race conditions
//! - Node ownership tracking and transfer
//! - Health monitoring (heartbeat-based stale detection)
//! - Handles node-id changes and dead nodes
//!
//! Based on best practices from Temporal and AWS Step Functions.

use plexspaces_workflow::*;
use tracing::{info, warn, error};
use std::time::Duration;

/// Recovery report summarizing recovery operations
#[derive(Debug)]
pub struct RecoveryReport {
    /// Number of workflows successfully recovered
    pub recovered: usize,
    /// Number of workflows that failed to recover
    pub failed: usize,
    /// Number of workflows transferred from dead nodes
    pub transferred: usize,
    /// Total number of interrupted workflows found
    pub total: usize,
    /// Number of workflows claimed (ownership transferred)
    pub claimed: usize,
}

/// Workflow Recovery Service with robust ownership and health monitoring
///
/// ## Design Principles
/// 1. **Optimistic Locking**: All updates use version checks to prevent race conditions
/// 2. **Node Ownership**: Each workflow owned by exactly one node at a time
/// 3. **Health Monitoring**: Heartbeat-based detection of stale/dead nodes
/// 4. **Ownership Transfer**: Automatic reassignment when node dies or node-id changes
///
/// ## Best Practices (Temporal/Step Functions)
/// - Workflows are "crash-proof" - state persisted before each step
/// - Automatic recovery on startup
/// - Heartbeat mechanism to detect dead workers
/// - Workflow reassignment when worker dies
pub struct WorkflowRecoveryService {
    storage: WorkflowStorage,
    node_id: String,
    stale_threshold: Duration,
}

impl WorkflowRecoveryService {
    /// Create a new recovery service
    pub fn new(storage: WorkflowStorage, node_id: String) -> Self {
        Self {
            storage,
            node_id,
            stale_threshold: Duration::from_secs(300), // 5 minutes default
        }
    }

    /// Create with custom stale threshold
    pub fn with_stale_threshold(
        storage: WorkflowStorage,
        node_id: String,
        stale_threshold: Duration,
    ) -> Self {
        Self {
            storage,
            node_id,
            stale_threshold,
        }
    }

    /// Recover interrupted workflows on startup
    ///
    /// ## Implementation (Following Temporal/Step Functions Best Practices)
    /// 1. **Claim Ownership First**: Update node_id for workflows owned by this node
    ///    - Prevents race conditions where multiple nodes try to recover same workflow
    ///    - Handles node-id changes (reclaims workflows with old node-id)
    /// 2. **Recover Owned Workflows**: Resume workflows we successfully claimed
    /// 3. **Transfer Stale Workflows**: Claim and recover workflows from dead nodes
    ///
    /// ## Key Design
    /// - **Optimistic Locking**: All ownership transfers use version checks
    /// - **Claim Before Recover**: Always update node_id before recovery to prevent conflicts
    /// - **Idempotent**: Safe to call multiple times (version checks prevent double-recovery)
    ///
    /// ## Returns
    /// RecoveryReport with statistics
    pub async fn recover_on_startup(&self) -> Result<RecoveryReport, WorkflowError> {
        info!("🔄 Starting workflow recovery for node: {}", self.node_id);

        // Step 1: Find all RUNNING/PENDING workflows (owned by any node or unowned)
        let all_interrupted = self
            .storage
            .list_executions_by_status(
                vec![ExecutionStatus::Running, ExecutionStatus::Pending],
                None, // Get all, not just owned by this node
            )
            .await?;

        info!("Found {} interrupted workflows total", all_interrupted.len());

        // Step 2: Check for stale workflows from dead nodes
        let stale = self
            .storage
            .list_stale_executions(
                self.stale_threshold.as_secs(),
                vec![ExecutionStatus::Running, ExecutionStatus::Pending],
            )
            .await?;

        info!("Found {} stale workflows (from dead nodes)", stale.len());

        let mut recovered = 0;
        let mut failed = 0;
        let mut transferred = 0;
        let mut claimed = 0;

        // Step 3: Claim ownership and recover workflows
        // Process in order: owned by this node first, then stale, then unowned
        for execution in &all_interrupted {
            let is_owned = execution.node_id.as_ref() == Some(&self.node_id);
            let is_stale = stale.iter().any(|s| s.execution_id == execution.execution_id);

            // Skip if already owned by this node and not stale
            if is_owned && !is_stale {
                // Already owned - just recover (but still update heartbeat to claim)
                info!(
                    "Recovering owned workflow: {} (status: {:?}, version: {})",
                    execution.execution_id, execution.status, execution.version
                );

                // Update heartbeat first (claims ownership and indicates we're alive)
                if let Err(e) = self
                    .storage
                    .update_heartbeat(&execution.execution_id, &self.node_id)
                    .await
                {
                    warn!("Failed to update heartbeat for {}: {}", execution.execution_id, e);
                }

                match self.recover_workflow(execution).await {
                    Ok(_) => {
                        info!("✓ Successfully recovered workflow: {}", execution.execution_id);
                        recovered += 1;
                    }
                    Err(WorkflowError::ConcurrentUpdate(_)) => {
                        warn!(
                            "⚠ Concurrent update detected for {} (another node may have recovered it)",
                            execution.execution_id
                        );
                        // Not a failure - another node recovered it
                    }
                    Err(e) => {
                        error!("❌ Failed to recover workflow {}: {}", execution.execution_id, e);
                        failed += 1;
                    }
                }
                continue;
            }

            // Need to claim ownership (node-id changed, stale, or unowned)
            info!(
                "Claiming ownership of workflow: {} (current owner: {:?}, version: {})",
                execution.execution_id, execution.node_id, execution.version
            );

            // Claim ownership with optimistic locking (prevents race conditions)
            match self
                .storage
                .transfer_ownership(
                    &execution.execution_id,
                    &self.node_id,
                    execution.version,
                )
                .await
            {
                Ok(_) => {
                    info!("✓ Claimed ownership of workflow: {}", execution.execution_id);
                    claimed += 1;

                    if is_stale {
                        transferred += 1;
                    }

                    // Now recover it (we own it now)
                    match self.recover_workflow(execution).await {
                        Ok(_) => {
                            info!("✓ Recovered claimed workflow: {}", execution.execution_id);
                            recovered += 1;
                        }
                        Err(e) => {
                            error!(
                                "❌ Failed to recover claimed workflow {}: {}",
                                execution.execution_id, e
                            );
                            failed += 1;
                        }
                    }
                }
                Err(WorkflowError::ConcurrentUpdate(_)) => {
                    warn!(
                        "⚠ Ownership claim failed for {} (concurrent update - another node may have taken it)",
                        execution.execution_id
                    );
                    // Not a failure - another node took it (this is expected in multi-node)
                }
                Err(e) => {
                    error!(
                        "❌ Failed to claim ownership of workflow {}: {}",
                        execution.execution_id, e
                    );
                    failed += 1;
                }
            }
        }

        let total = all_interrupted.len();

        info!(
            "Recovery complete: {}/{} workflows recovered, {} claimed, {} transferred, {} failed",
            recovered, total, claimed, transferred, failed
        );

        Ok(RecoveryReport {
            recovered,
            failed,
            transferred,
            total,
            claimed,
        })
    }

    /// Recover a single workflow (internal helper)
    async fn recover_workflow(
        &self,
        execution: &WorkflowExecution,
    ) -> Result<(), WorkflowError> {
        // Update heartbeat before recovery (indicates we're alive)
        if let Some(ref node_id) = execution.node_id {
            self.storage
                .update_heartbeat(&execution.execution_id, node_id)
                .await?;
        }

        // Resume workflow execution
        WorkflowExecutor::execute_from_state(&self.storage, &execution.execution_id).await
    }

    /// Update heartbeat for all workflows owned by this node
    ///
    /// ## Purpose
    /// Called periodically to indicate node is alive.
    /// Prevents workflows from being marked as stale.
    pub async fn update_heartbeats(&self) -> Result<usize, WorkflowError> {
        let owned = self
            .storage
            .list_executions_by_status(
                vec![ExecutionStatus::Running, ExecutionStatus::Pending],
                Some(&self.node_id),
            )
            .await?;

        let mut updated = 0;
        for execution in &owned {
            if let Err(e) = self
                .storage
                .update_heartbeat(&execution.execution_id, &self.node_id)
                .await
            {
                warn!("Failed to update heartbeat for {}: {}", execution.execution_id, e);
            } else {
                updated += 1;
            }
        }

        Ok(updated)
    }

    /// Check for stale workflows (health check)
    ///
    /// ## Purpose
    /// Detects workflows that haven't been updated recently, indicating
    /// the node may have crashed without updating status.
    ///
    /// ## Returns
    /// Vector of stale workflow execution IDs
    pub async fn check_stale_workflows(
        &self,
    ) -> Result<Vec<String>, WorkflowError> {
        let stale = self
            .storage
            .list_stale_executions(
                self.stale_threshold.as_secs(),
                vec![ExecutionStatus::Running], // Only check RUNNING workflows
            )
            .await?;

        if !stale.is_empty() {
            warn!(
                "Found {} stale workflows (not updated in {} seconds)",
                stale.len(),
                self.stale_threshold.as_secs()
            );
        }

        Ok(stale.iter().map(|e| e.execution_id.clone()).collect())
    }
}
