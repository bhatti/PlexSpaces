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

//! Workflow spawning utilities
//!
//! ## Example
//! ```ignore
//! use plexspaces_workflow::{spawn_workflow, WorkflowRef};
//!
//! let workflow: WorkflowRef = spawn_workflow(
//!     &ctx,
//!     "workflow-123",
//!     MyWorkflow::new(),
//!     vec![],
//!     service_locator.clone(),
//! ).await?;
//!
//! let result: Output = workflow.run(&input).await?;
//! ```

use std::sync::Arc;
use plexspaces_actor::{ActorFactoryImpl, WorkflowRef};
use plexspaces_core::{RequestContext, ServiceLocator as ServiceLocatorTrait};
use crate::WorkflowError;

/// Spawn a workflow actor and return a WorkflowRef for interacting with it.
///
/// Delegates to `ActorFactoryImpl::spawn_workflow` for centralized actor creation.
///
/// ## Example
/// ```ignore
/// use plexspaces_workflow::{spawn_workflow, WorkflowRef};
///
/// let workflow: WorkflowRef = spawn_workflow(
///     &ctx,
///     "approval-workflow-123",
///     ApprovalWorkflow::new(),
///     vec![Box::new(DurabilityFacet::new(...))],
///     service_locator.clone(),
/// ).await?;
///
/// let result: ApprovalResult = workflow.run(&request).await?;
/// workflow.signal("approve", &approval_data).await?;
/// ```
pub async fn spawn_workflow<B>(
    ctx: &RequestContext,
    workflow_id: impl Into<plexspaces_core::ActorId>,
    behavior: B,
    facets: Vec<Box<dyn plexspaces_facet::Facet>>,
    service_locator: Arc<dyn ServiceLocatorTrait>,
) -> Result<WorkflowRef, WorkflowError>
where
    B: plexspaces_core::Actor + Send + 'static,
{
    // Get ActorFactory from service locator and downcast to ActorFactoryImpl
    let factory_dyn = service_locator
        .get_actor_factory()
        .await
        .ok_or_else(|| WorkflowError::Execution("ActorFactory not found in ServiceLocator".to_string()))?;
    
    let factory = factory_dyn
        .as_any()
        .downcast_ref::<ActorFactoryImpl>()
        .ok_or_else(|| WorkflowError::Execution("ActorFactory is not ActorFactoryImpl".to_string()))?;
    
    // Delegate to factory's typed spawn method
    factory.spawn_workflow(ctx, workflow_id, behavior, facets).await
        .map_err(|e: plexspaces_actor::WorkflowRefError| WorkflowError::Execution(e.to_string()))
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_workflow_ref_id() {
        // Integration tests in tests/workflow_ref_test.rs cover the full flow.
    }
}
