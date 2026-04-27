// SPDX-License-Identifier: AGPL-3.0-or-later
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

//! Virtual actor activation strategy (single definition).
//!
//! Re-exports the proto enum and provides config-string conversion so journaling,
//! actor_factory_impl, and SDK use one type without duplicating parsing logic.

use plexspaces_proto::common::v1::ActivationStrategy as ProtoActivationStrategy;

/// Re-export the single ActivationStrategy enum from proto (common.v1).
pub use plexspaces_proto::common::v1::ActivationStrategy;

/// Parse config string ("lazy", "eager", "prewarm") to proto enum.
/// Unknown or empty defaults to Lazy. Case-insensitive.
#[must_use]
pub fn from_config_str(s: &str) -> ProtoActivationStrategy {
    match s.to_lowercase().as_str() {
        "eager" => ProtoActivationStrategy::ActivationStrategyEager,
        "prewarm" => ProtoActivationStrategy::ActivationStrategyPrewarm,
        "lazy" | _ => ProtoActivationStrategy::ActivationStrategyLazy,
    }
}

/// Format strategy for facet config / logging ("lazy", "eager", "prewarm").
#[must_use]
pub fn to_config_str(s: &ProtoActivationStrategy) -> &'static str {
    match s {
        ProtoActivationStrategy::ActivationStrategyUnspecified
        | ProtoActivationStrategy::ActivationStrategyLazy => "lazy",
        ProtoActivationStrategy::ActivationStrategyEager => "eager",
        ProtoActivationStrategy::ActivationStrategyPrewarm => "prewarm",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_config_str_lazy() {
        assert_eq!(
            from_config_str("lazy"),
            ProtoActivationStrategy::ActivationStrategyLazy
        );
        assert_eq!(
            from_config_str("LAZY"),
            ProtoActivationStrategy::ActivationStrategyLazy
        );
        assert_eq!(
            from_config_str(""),
            ProtoActivationStrategy::ActivationStrategyLazy
        );
    }

    #[test]
    fn from_config_str_eager_prewarm() {
        assert_eq!(
            from_config_str("eager"),
            ProtoActivationStrategy::ActivationStrategyEager
        );
        assert_eq!(
            from_config_str("prewarm"),
            ProtoActivationStrategy::ActivationStrategyPrewarm
        );
    }

    #[test]
    fn to_config_str_roundtrip() {
        assert_eq!(
            to_config_str(&ProtoActivationStrategy::ActivationStrategyLazy),
            "lazy"
        );
        assert_eq!(
            from_config_str(to_config_str(
                &ProtoActivationStrategy::ActivationStrategyLazy
            )),
            ProtoActivationStrategy::ActivationStrategyLazy
        );
    }
}
