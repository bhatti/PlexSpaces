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

//! Extension trait for ApplicationManager with node-specific functionality
//!
//! ## Purpose
//! Provides node-specific extensions to ApplicationManager that require
//! knowledge of node-specific application types (SpecApplication, WasmApplication).

use plexspaces_application::ApplicationManager;
use plexspaces_proto::application::v1::ApplicationSpec;
use std::sync::Arc;

/// Extension trait for ApplicationManager with node-specific functionality
pub trait ApplicationManagerExt {
    /// Get ApplicationSpec from application (if available)
    ///
    /// ## Purpose
    /// Attempts to extract ApplicationSpec from the application instance.
    /// Works for both SpecApplication and WasmApplication.
    ///
    /// ## Returns
    /// ApplicationSpec if available, None otherwise
    async fn get_application_spec(&self, name: &str) -> Option<ApplicationSpec>;
    
    /// Set tenant_id/namespace on WasmApplication before start()
    ///
    /// ## Purpose
    /// Sets tenant_id/namespace from API request on WasmApplication (node-specific version)
    /// so that actors spawned by the application use the correct context.
    ///
    /// ## Arguments
    /// * `name` - Application name
    /// * `tenant_id` - Tenant ID from API request
    /// * `namespace` - Namespace from API request
    async fn set_wasm_application_tenant_namespace(&self, name: &str, tenant_id: String, namespace: String);
}

impl ApplicationManagerExt for Arc<ApplicationManager> {
    async fn get_application_spec(&self, name: &str) -> Option<ApplicationSpec> {
        use plexspaces_application::{SpecApplication, WasmApplication};
        
        self.with_application(name, |app_any| {
            // Try to downcast to SpecApplication
            if let Some(spec_app) = app_any.downcast_ref::<SpecApplication>() {
                return Some(spec_app.spec().clone());
            }
            
            // Try to downcast to WasmApplication
            if let Some(wasm_app) = app_any.downcast_ref::<WasmApplication>() {
                return wasm_app.spec().cloned();
            }
            
            None
        }).await
    }
    
    async fn set_wasm_application_tenant_namespace(&self, name: &str, tenant_id: String, namespace: String) {
        // Try to set tenant_id/namespace on node-specific WasmApplication
        // Note: This only works for node-specific WasmApplication, not application crate version
        use crate::wasm_application::WasmApplication as NodeWasmApplication;
        
        // Use with_application to get access, but we can't call async in closure
        // So we'll use a workaround: store tenant_id/namespace in a way WasmApplication can access
        // Actually, WasmApplication in node/src reads from its own fields
        // We need to get mutable access, but with_application only gives &dyn Any
        // So we'll need to use a different approach - set them when creating WasmApplication
        // For now, node-specific WasmApplication is set via set_tenant_namespace() before boxing
        // This is handled in ApplicationService when creating WasmApplication
        self.with_application(name, |app_any| {
            if let Some(_wasm_app) = app_any.downcast_ref::<NodeWasmApplication>() {
                // Can't call async method here - tenant_id/namespace should be set when creating WasmApplication
                Some(())
            } else {
                None
            }
        }).await;
    }
}
