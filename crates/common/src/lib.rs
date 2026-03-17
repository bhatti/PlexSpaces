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

//! Common types shared across PlexSpaces crates
//!
//! This crate contains fundamental types that are used by multiple crates
//! to avoid circular dependencies.

#![warn(missing_docs)]
#![warn(clippy::all)]

pub mod activation_strategy;
pub mod aws_config;
pub mod config_manager;
pub mod keyvalue_store;
pub mod node_address;
pub mod release_config;
pub mod release_parser;
pub mod request_context;
pub mod security_validator;
pub mod virtual_actor_config;

#[cfg(any(test, feature = "test-helpers"))]
pub mod test_helpers;

pub use activation_strategy::{from_config_str, to_config_str, ActivationStrategy};
pub use aws_config::{AWSConfig, DLQConfig, DynamoDBConfig, S3Config, SQSConfig};
pub use config_manager::{
    get_env, get_env_bool, get_env_or, get_env_u32, get_env_u64, initialize, EnvConfig,
};
pub use keyvalue_store::{KeyValueStore, KeyValueStoreError, KeyValueStoreResult};
pub use node_address::{
    canonical_node_address_key, dialable_node_address, node_addresses_equivalent,
};
pub use release_config::create_default_release_config;
pub use release_parser::{Release, ReleaseError};
pub use request_context::{RequestContext, RequestContextError, AUTH_REQUIRED_HINT};
pub use security_validator::{validate_security_config, SecurityValidationError};
pub use virtual_actor_config::{
    duration_to_proto, format_duration, get_activation_strategy, get_idle_timeout,
    get_max_pool_per_actor_type, DEFAULT_ACTIVATION_STRATEGY, DEFAULT_IDLE_TIMEOUT_SECONDS,
    DEFAULT_MAX_POOL_PER_ACTOR_TYPE,
};
