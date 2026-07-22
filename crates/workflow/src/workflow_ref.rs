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

use crate::WorkflowError;
use plexspaces_actor::{ActorBuilder, WorkflowRef};
use plexspaces_actor::{RequestContext, RequestContextExt, ServiceLocator as ServiceLocatorTrait};
use std::sync::Arc;

/// Spawn a workflow actor and return a WorkflowRef for interacting with it.
///
/// Delegates to the framework-owned actor spawn path and returns a typed workflow reference.
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
    workflow_name: impl Into<String>,
    behavior: B,
    facets: Vec<Box<dyn plexspaces_facet::Facet>>,
    service_locator: Arc<dyn ServiceLocatorTrait>,
) -> Result<WorkflowRef, WorkflowError>
where
    B: plexspaces_actor::actor_types::Actor + Send + 'static,
{
    let mut builder = ActorBuilder::new(Box::new(behavior))
        .with_name(workflow_name.into())
        .with_namespace(ctx.namespace().to_string());
    for facet in facets {
        builder = builder.with_facet(facet);
    }

    let actor_ref = builder
        .spawn(ctx, service_locator)
        .await
        .map_err(|e| WorkflowError::Execution(e.to_string()))?;

    Ok(WorkflowRef::new(actor_ref))
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_workflow_ref_id() {
        // Integration tests in tests/workflow_ref_test.rs cover the full flow.
    }
}
