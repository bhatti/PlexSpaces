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

use plexspaces_core::ApplicationManager;
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
}

impl ApplicationManagerExt for Arc<ApplicationManager> {
    async fn get_application_spec(&self, name: &str) -> Option<ApplicationSpec> {
        use crate::application_impl::SpecApplication;
        use crate::wasm_application::WasmApplication;
        
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
}
