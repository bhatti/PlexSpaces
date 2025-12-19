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

//! BehaviorFactory for Byzantine Generals
//!
//! This module provides a BehaviorFactory implementation that can create
//! General behaviors from actor_type and initial_state.

use std::sync::Arc;
use plexspaces_core::{Actor, BehaviorFactoryError, BehaviorRegistry};
use plexspaces::journal::Journal;
use plexspaces::tuplespace::TupleSpace;
use crate::byzantine::General;

/// Register Byzantine Generals behaviors in a BehaviorRegistry
///
/// ## Arguments
/// * `registry` - The BehaviorRegistry to register behaviors in
/// * `journal` - Shared journal instance (not used in simple version, but kept for compatibility)
/// * `tuplespace` - Shared tuplespace instance (not used in simple version, but kept for compatibility)
///
/// ## Behavior Types Registered
/// * `"ByzantineGeneral"` - Creates a General behavior from initial_state JSON
///
/// ## Initial State Format
/// The initial_state should be JSON with:
/// ```json
/// {
///   "id": 0,
///   "source_id": 0,
///   "num_rounds": 1
/// }
/// ```
pub async fn register_byzantine_behaviors(
    registry: &mut BehaviorRegistry,
    _journal: Arc<dyn Journal>,
    _tuplespace: Arc<TupleSpace>,
) {
    registry.register("ByzantineGeneral", move |initial_state: &[u8]| {
        // Parse initial_state JSON
        let state_json: serde_json::Value = serde_json::from_slice(initial_state)
            .map_err(|e| BehaviorFactoryError::InvalidArguments(
                "ByzantineGeneral".to_string(),
                format!("Failed to parse initial_state JSON: {}", e)
            ))?;
        
        let id = state_json.get("id")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| BehaviorFactoryError::InvalidArguments(
                "ByzantineGeneral".to_string(),
                "Missing or invalid 'id' field in initial_state".to_string()
            ))? as usize;
        
        let source_id = state_json.get("source_id")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;
        
        let num_rounds = state_json.get("num_rounds")
            .and_then(|v| v.as_u64())
            .unwrap_or(1) as usize;
        
        // Create General
        let general = General::new(id, source_id, num_rounds);
        
        Ok(Box::new(general) as Box<dyn Actor>)
    }).await;
}
