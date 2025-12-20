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

//! HTTP Client Capability Facet (placeholder)

use crate::{Facet, FacetError};
use async_trait::async_trait;

/// HTTP client facet for making outbound HTTP requests
pub struct HttpClientFacet {
    config: serde_json::Value,
    priority: i32,
}

/// Default priority for HttpClientFacet
pub const HTTP_CLIENT_FACET_DEFAULT_PRIORITY: i32 = 20;

impl HttpClientFacet {
    /// Create a new HTTP client facet
    pub fn new(config: serde_json::Value, priority: i32) -> Self {
        HttpClientFacet {
            config,
            priority,
        }
    }
}

#[async_trait]
impl Facet for HttpClientFacet {
    fn facet_type(&self) -> &str {
        "http_client"
    }
    
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    async fn on_attach(
        &mut self,
        _actor_id: &str,
        _config: serde_json::Value,
    ) -> Result<(), FacetError> {
        Ok(())
    }

    async fn on_detach(&mut self, _actor_id: &str) -> Result<(), FacetError> {
        Ok(())
    }
    
    fn get_config(&self) -> serde_json::Value {
        self.config.clone()
    }
    
    fn get_priority(&self) -> i32 {
        self.priority
    }
}
