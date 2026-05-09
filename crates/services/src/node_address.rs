// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Re-export shared node-address helpers for service-layer callers.


pub use plexspaces_common::node_address::{
    canonical_node_address_key, dialable_node_address, node_addresses_equivalent,
};
