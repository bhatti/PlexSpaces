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

//! Tests for service wrappers in core crate
//!
//! These tests verify that service wrappers correctly adapt existing service types
//! to the traits defined in actor_context.rs.
//!
//! Note: Most service wrappers are in the `node` crate to avoid circular dependencies.
//! Only TupleSpaceProviderWrapper is tested here since TupleSpace is in core.

mod tests {
    use plexspaces_core::service_wrappers::TupleSpaceProviderWrapper;
    use plexspaces_core::actor_context::TupleSpaceProvider;
    use plexspaces_tuplespace::{Pattern, PatternField, Tuple, TupleField};
    use std::sync::Arc;
    use plexspaces_tuplespace::TupleSpace;

    #[tokio::test]
    async fn test_tuplespace_provider_wrapper() {
        let tuplespace = Arc::new(TupleSpace::default());
        let wrapper = TupleSpaceProviderWrapper::new(tuplespace.clone());

        // Test write
        let tuple = Tuple::new(vec![
            TupleField::String("test".to_string()),
            TupleField::Integer(42),
        ]);
        wrapper.write(tuple.clone()).await.unwrap();

        // Test read
        let pattern = Pattern::new(vec![
            PatternField::Exact(TupleField::String("test".to_string())),
            PatternField::Wildcard,
        ]);
        let results = wrapper.read(&pattern).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].fields()[0], TupleField::String("test".to_string()));

        // Test take
        let taken = wrapper.take(&pattern).await.unwrap();
        assert!(taken.is_some());

        // Test count
        let count = wrapper.count(&pattern).await.unwrap();
        assert_eq!(count, 0);
    }
}

