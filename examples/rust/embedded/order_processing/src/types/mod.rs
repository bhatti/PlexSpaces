// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Domain types for the order processing microservices example.

pub mod order;
pub mod payment;
pub mod inventory;
pub mod shipping;

// Re-export main types for convenience
pub use order::{Order, OrderState, OrderItem, OrderError};
pub use payment::{PaymentRequest, PaymentResult, PaymentMethod, ChargeRequest, PaymentError};
pub use inventory::{InventoryItem, ReservationRequest, ReservationItem, StockLevel, InventoryError};
pub use shipping::{ShipmentRequest, ShippingLabel, Address, Carrier, ServiceLevel, PackageInfo, CarrierInfo, ShippingError};
