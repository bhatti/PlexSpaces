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

//! In-memory scheduling state store (for testing).

use crate::state_store::SchedulingStateStore;
use async_trait::async_trait;
use plexspaces_core::RequestContext;
use plexspaces_proto::scheduling::v1::SchedulingRequest;
use std::collections::HashMap;
use std::error::Error;
use std::sync::Arc;
use tokio::sync::RwLock;

/// In-memory scheduling state store (for testing).
#[derive(Clone)]
pub struct MemorySchedulingStateStore {
    requests: Arc<RwLock<HashMap<String, SchedulingRequest>>>,
}

impl MemorySchedulingStateStore {
    /// Create a new in-memory state store.
    pub fn new() -> Self {
        Self {
            requests: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl Default for MemorySchedulingStateStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SchedulingStateStore for MemorySchedulingStateStore {
    async fn store_request(
        &self,
        ctx: &RequestContext,
        request: SchedulingRequest,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        // Validate tenant/namespace match
        if request.tenant_id != ctx.tenant_id() || request.namespace != ctx.namespace() {
            return Err(format!(
                "Request tenant_id/namespace ({}/{}) does not match context ({}/{})",
                request.tenant_id,
                request.namespace,
                ctx.tenant_id(),
                ctx.namespace()
            )
            .into());
        }

        let mut requests = self.requests.write().await;
        requests.insert(request.request_id.clone(), request);
        Ok(())
    }

    async fn get_request(
        &self,
        ctx: &RequestContext,
        request_id: &str,
    ) -> Result<Option<SchedulingRequest>, Box<dyn Error + Send + Sync>> {
        let requests = self.requests.read().await;
        if let Some(request) = requests.get(request_id) {
            // Validate tenant/namespace match
            if request.tenant_id == ctx.tenant_id() && request.namespace == ctx.namespace() {
                Ok(Some(request.clone()))
            } else {
                // Request exists but belongs to different tenant/namespace - return None for security
                Ok(None)
            }
        } else {
            Ok(None)
        }
    }

    async fn update_request(
        &self,
        ctx: &RequestContext,
        request: SchedulingRequest,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        // Validate tenant/namespace match
        if request.tenant_id != ctx.tenant_id() || request.namespace != ctx.namespace() {
            return Err(format!(
                "Request tenant_id/namespace ({}/{}) does not match context ({}/{})",
                request.tenant_id,
                request.namespace,
                ctx.tenant_id(),
                ctx.namespace()
            )
            .into());
        }

        let mut requests = self.requests.write().await;
        requests.insert(request.request_id.clone(), request);
        Ok(())
    }

    async fn query_pending_requests(
        &self,
        ctx: &RequestContext,
    ) -> Result<Vec<SchedulingRequest>, Box<dyn Error + Send + Sync>> {
        use plexspaces_proto::scheduling::v1::SchedulingStatus;

        let requests = self.requests.read().await;
        let pending: Vec<SchedulingRequest> = requests
            .values()
            .filter(|req| {
                // Filter by status AND tenant/namespace
                req.tenant_id == ctx.tenant_id()
                    && req.namespace == ctx.namespace()
                    && SchedulingStatus::try_from(req.status)
                        .map(|s| s == SchedulingStatus::SchedulingStatusPending)
                        .unwrap_or(false)
            })
            .cloned()
            .collect();
        Ok(pending)
    }
}

