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

//! Docker container test environment
//!
//! Actors run as Docker containers with network isolation and resource limits.

use super::{
    ActorConfig, ActorHandle, EnvironmentMetrics, EnvironmentType, TestEnvironment, TestError,
};
use async_trait::async_trait;
use std::collections::HashMap;

pub struct DockerEnvironment {
    _containers: HashMap<String, String>,
    _network: String,
}

impl DockerEnvironment {
    pub async fn new(_config: HashMap<String, String>) -> Result<Self, TestError> {
        Ok(DockerEnvironment {
            _containers: HashMap::new(),
            _network: "plexspaces-test".to_string(),
        })
    }
}

#[async_trait]
impl TestEnvironment for DockerEnvironment {
    fn environment_type(&self) -> EnvironmentType {
        EnvironmentType::Docker
    }

    async fn deploy_actor(&self, _config: ActorConfig) -> Result<ActorHandle, TestError> {
        // TODO: Implement Docker deployment
        Err(TestError::EnvironmentError(
            "Docker not yet implemented".to_string(),
        ))
    }

    async fn send_message(
        &self,
        _actor: &ActorHandle,
        _msg: crate::mailbox::Message,
    ) -> Result<(), TestError> {
        Err(TestError::EnvironmentError(
            "Docker not yet implemented".to_string(),
        ))
    }

    async fn get_state(&self, _actor: &ActorHandle) -> Result<crate::actor::ActorState, TestError> {
        Err(TestError::EnvironmentError(
            "Docker not yet implemented".to_string(),
        ))
    }

    async fn kill_actor(&self, _actor: &ActorHandle) -> Result<(), TestError> {
        Err(TestError::EnvironmentError(
            "Docker not yet implemented".to_string(),
        ))
    }

    async fn create_partition(
        &self,
        _group1: Vec<ActorHandle>,
        _group2: Vec<ActorHandle>,
    ) -> Result<(), TestError> {
        Err(TestError::EnvironmentError(
            "Docker not yet implemented".to_string(),
        ))
    }

    async fn heal_partition(&self) -> Result<(), TestError> {
        Err(TestError::EnvironmentError(
            "Docker not yet implemented".to_string(),
        ))
    }

    async fn collect_metrics(&self) -> Result<EnvironmentMetrics, TestError> {
        Err(TestError::EnvironmentError(
            "Docker not yet implemented".to_string(),
        ))
    }

    async fn cleanup(&self) -> Result<(), TestError> {
        Ok(())
    }
}
