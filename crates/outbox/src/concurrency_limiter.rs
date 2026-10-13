// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! AIMD (Additive Increase Multiplicative Decrease) concurrency limiter.
//!
//! ## Purpose
//! Controls how many messages the outbox relay sends concurrently.
//! On success the limit grows linearly; on rejection it halves (TCP-style).
//!
//! ## Architecture Context
//! Used by `OutboxRelay` to adaptively pace message delivery.
//! Pure logic — no async, no I/O, independently testable.

use plexspaces_proto::outbox::v1::{ConcurrencyConfig, ConcurrencyStrategy};
use std::sync::atomic::{AtomicU32, Ordering};

const DEFAULT_INITIAL_LIMIT: u32 = 100;
const DEFAULT_MIN_LIMIT: u32 = 1;
const DEFAULT_MAX_LIMIT: u32 = 10_000;

/// AIMD concurrency limiter with atomic state.
pub struct ConcurrencyLimiter {
    current_limit: AtomicU32,
    in_flight: AtomicU32,
    min_limit: u32,
    max_limit: u32,
    strategy: ConcurrencyStrategy,
}

/// A permit that must be returned after a send completes (success or failure).
pub struct Permit<'a> {
    limiter: &'a ConcurrencyLimiter,
}

impl Drop for Permit<'_> {
    fn drop(&mut self) {
        self.limiter.in_flight.fetch_sub(1, Ordering::Release);
    }
}

impl ConcurrencyLimiter {
    /// Create a new limiter from proto config.
    pub fn new(config: &ConcurrencyConfig) -> Self {
        let initial = if config.initial_limit == 0 {
            DEFAULT_INITIAL_LIMIT
        } else {
            config.initial_limit
        };
        let min = if config.min_limit == 0 {
            DEFAULT_MIN_LIMIT
        } else {
            config.min_limit
        };
        let max = if config.max_limit == 0 {
            DEFAULT_MAX_LIMIT
        } else {
            config.max_limit
        };
        let strategy = ConcurrencyStrategy::try_from(config.strategy)
            .unwrap_or(ConcurrencyStrategy::ConcurrencyStrategyAimd);
        Self {
            current_limit: AtomicU32::new(initial.clamp(min, max)),
            in_flight: AtomicU32::new(0),
            min_limit: min,
            max_limit: max,
            strategy,
        }
    }

    /// Create a limiter with default settings.
    pub fn with_defaults() -> Self {
        Self {
            current_limit: AtomicU32::new(DEFAULT_INITIAL_LIMIT),
            in_flight: AtomicU32::new(0),
            min_limit: DEFAULT_MIN_LIMIT,
            max_limit: DEFAULT_MAX_LIMIT,
            strategy: ConcurrencyStrategy::ConcurrencyStrategyAimd,
        }
    }

    /// Try to acquire a permit. Returns `None` if at capacity.
    pub fn try_acquire(&self) -> Option<Permit<'_>> {
        loop {
            let current = self.in_flight.load(Ordering::Acquire);
            let limit = self.current_limit.load(Ordering::Acquire);
            if current >= limit {
                metrics::counter!("plexspaces_outbox_limiter_acquire_rejected_total").increment(1);
                return None;
            }
            if self
                .in_flight
                .compare_exchange_weak(current, current + 1, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                metrics::gauge!("plexspaces_outbox_limiter_in_flight").set((current + 1) as f64);
                return Some(Permit { limiter: self });
            }
        }
    }

    /// Signal that a send succeeded. Increases limit by 1 (additive increase).
    /// The permit is consumed.
    pub fn on_success(&self, _permit: Permit<'_>) {
        metrics::counter!("plexspaces_outbox_limiter_success_total").increment(1);
        if matches!(self.strategy, ConcurrencyStrategy::ConcurrencyStrategyAimd) {
            loop {
                let current = self.current_limit.load(Ordering::Acquire);
                let new = (current + 1).min(self.max_limit);
                if self
                    .current_limit
                    .compare_exchange_weak(current, new, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
                {
                    metrics::gauge!("plexspaces_outbox_limiter_current_limit").set(new as f64);
                    break;
                }
            }
        }
    }

    /// Signal that a send was rejected. Halves the limit (multiplicative decrease).
    /// The permit is consumed.
    pub fn on_rejection(&self, _permit: Permit<'_>) {
        metrics::counter!("plexspaces_outbox_limiter_rejection_total").increment(1);
        if matches!(self.strategy, ConcurrencyStrategy::ConcurrencyStrategyAimd) {
            loop {
                let current = self.current_limit.load(Ordering::Acquire);
                let new = (current / 2).max(self.min_limit);
                if self
                    .current_limit
                    .compare_exchange_weak(current, new, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
                {
                    metrics::gauge!("plexspaces_outbox_limiter_current_limit").set(new as f64);
                    break;
                }
            }
        }
    }

    /// Current concurrency limit.
    pub fn current_limit(&self) -> u32 {
        self.current_limit.load(Ordering::Acquire)
    }

    /// Number of permits currently in flight.
    pub fn in_flight(&self) -> u32 {
        self.in_flight.load(Ordering::Acquire)
    }

    /// Available capacity (limit - in_flight).
    pub fn available(&self) -> u32 {
        let limit = self.current_limit.load(Ordering::Acquire);
        let flight = self.in_flight.load(Ordering::Acquire);
        limit.saturating_sub(flight)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_config() -> ConcurrencyConfig {
        ConcurrencyConfig {
            initial_limit: 10,
            min_limit: 1,
            max_limit: 100,
            strategy: ConcurrencyStrategy::ConcurrencyStrategyAimd as i32,
        }
    }

    #[test]
    fn test_new_from_config() {
        let limiter = ConcurrencyLimiter::new(&default_config());
        assert_eq!(limiter.current_limit(), 10);
        assert_eq!(limiter.in_flight(), 0);
        assert_eq!(limiter.available(), 10);
    }

    #[test]
    fn test_try_acquire_within_limit() {
        let limiter = ConcurrencyLimiter::new(&default_config());
        let permit = limiter.try_acquire();
        assert!(permit.is_some());
        assert_eq!(limiter.in_flight(), 1);
        assert_eq!(limiter.available(), 9);
    }

    #[test]
    fn test_try_acquire_at_capacity_fails() {
        let config = ConcurrencyConfig {
            initial_limit: 2,
            min_limit: 1,
            max_limit: 100,
            strategy: ConcurrencyStrategy::ConcurrencyStrategyAimd as i32,
        };
        let limiter = ConcurrencyLimiter::new(&config);
        let _p1 = limiter.try_acquire().unwrap();
        let _p2 = limiter.try_acquire().unwrap();
        assert!(limiter.try_acquire().is_none());
        assert_eq!(limiter.in_flight(), 2);
    }

    #[test]
    fn test_permit_drop_releases_slot() {
        let limiter = ConcurrencyLimiter::new(&default_config());
        {
            let _permit = limiter.try_acquire().unwrap();
            assert_eq!(limiter.in_flight(), 1);
        }
        assert_eq!(limiter.in_flight(), 0);
    }

    #[test]
    fn test_on_success_increases_limit() {
        let limiter = ConcurrencyLimiter::new(&default_config());
        let permit = limiter.try_acquire().unwrap();
        limiter.on_success(permit);
        assert_eq!(limiter.current_limit(), 11);
        assert_eq!(limiter.in_flight(), 0);
    }

    #[test]
    fn test_on_rejection_halves_limit() {
        let limiter = ConcurrencyLimiter::new(&default_config());
        let permit = limiter.try_acquire().unwrap();
        limiter.on_rejection(permit);
        assert_eq!(limiter.current_limit(), 5);
    }

    #[test]
    fn test_limit_capped_at_max() {
        let config = ConcurrencyConfig {
            initial_limit: 99,
            min_limit: 1,
            max_limit: 100,
            strategy: ConcurrencyStrategy::ConcurrencyStrategyAimd as i32,
        };
        let limiter = ConcurrencyLimiter::new(&config);
        let p1 = limiter.try_acquire().unwrap();
        limiter.on_success(p1);
        assert_eq!(limiter.current_limit(), 100);
        let p2 = limiter.try_acquire().unwrap();
        limiter.on_success(p2);
        assert_eq!(limiter.current_limit(), 100);
    }

    #[test]
    fn test_limit_floored_at_min() {
        let config = ConcurrencyConfig {
            initial_limit: 2,
            min_limit: 1,
            max_limit: 100,
            strategy: ConcurrencyStrategy::ConcurrencyStrategyAimd as i32,
        };
        let limiter = ConcurrencyLimiter::new(&config);
        let p = limiter.try_acquire().unwrap();
        limiter.on_rejection(p);
        assert_eq!(limiter.current_limit(), 1);
        let p = limiter.try_acquire().unwrap();
        limiter.on_rejection(p);
        assert_eq!(limiter.current_limit(), 1);
    }

    #[test]
    fn test_fixed_strategy_no_adjustment() {
        let config = ConcurrencyConfig {
            initial_limit: 10,
            min_limit: 1,
            max_limit: 100,
            strategy: ConcurrencyStrategy::ConcurrencyStrategyFixed as i32,
        };
        let limiter = ConcurrencyLimiter::new(&config);
        let p1 = limiter.try_acquire().unwrap();
        limiter.on_success(p1);
        assert_eq!(limiter.current_limit(), 10);
        let p2 = limiter.try_acquire().unwrap();
        limiter.on_rejection(p2);
        assert_eq!(limiter.current_limit(), 10);
    }

    #[test]
    fn test_aimd_convergence() {
        let config = ConcurrencyConfig {
            initial_limit: 50,
            min_limit: 1,
            max_limit: 100,
            strategy: ConcurrencyStrategy::ConcurrencyStrategyAimd as i32,
        };
        let limiter = ConcurrencyLimiter::new(&config);
        for _ in 0..10 {
            let p = limiter.try_acquire().unwrap();
            limiter.on_success(p);
        }
        assert_eq!(limiter.current_limit(), 60);
        let p = limiter.try_acquire().unwrap();
        limiter.on_rejection(p);
        assert_eq!(limiter.current_limit(), 30);
    }

    #[test]
    fn test_with_defaults() {
        let limiter = ConcurrencyLimiter::with_defaults();
        assert_eq!(limiter.current_limit(), 100);
        assert_eq!(limiter.in_flight(), 0);
    }

    #[test]
    fn test_zero_config_uses_defaults() {
        let config = ConcurrencyConfig {
            initial_limit: 0,
            min_limit: 0,
            max_limit: 0,
            strategy: 0,
        };
        let limiter = ConcurrencyLimiter::new(&config);
        assert_eq!(limiter.current_limit(), 100);
    }

    #[test]
    fn test_initial_clamped_to_range() {
        let config = ConcurrencyConfig {
            initial_limit: 200,
            min_limit: 5,
            max_limit: 50,
            strategy: ConcurrencyStrategy::ConcurrencyStrategyAimd as i32,
        };
        let limiter = ConcurrencyLimiter::new(&config);
        assert_eq!(limiter.current_limit(), 50);
    }

    #[test]
    fn test_concurrent_acquire_respects_limit() {
        let config = ConcurrencyConfig {
            initial_limit: 5,
            min_limit: 1,
            max_limit: 100,
            strategy: ConcurrencyStrategy::ConcurrencyStrategyAimd as i32,
        };
        let limiter = ConcurrencyLimiter::new(&config);
        let mut permits = Vec::new();
        for _ in 0..5 {
            permits.push(limiter.try_acquire().unwrap());
        }
        assert!(limiter.try_acquire().is_none());
        assert_eq!(limiter.in_flight(), 5);
        drop(permits);
        assert_eq!(limiter.in_flight(), 0);
    }
}
