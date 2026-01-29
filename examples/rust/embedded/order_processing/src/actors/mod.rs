// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Actor implementations for order processing microservices.

// Order processor using new framework features (hybrid behavior approach)
pub mod order_processor;

// Re-export
pub use order_processor::{OrderProcessorBehavior, OrderMessage, OrderConfig};
