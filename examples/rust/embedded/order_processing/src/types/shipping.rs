// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Address {
    pub name: String,
    pub street1: String,
    pub street2: Option<String>,
    pub city: String,
    pub state: String,
    pub postal_code: String,
    pub country: String,
    pub phone: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Carrier {
    FedEx,
    UPS,
    USPS,
    DHL,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServiceLevel {
    Standard,
    Express,
    Overnight,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageInfo {
    pub length: f64,
    pub width: f64,
    pub height: f64,
    pub weight: f64,
}

impl PackageInfo {
    pub fn new(length: f64, width: f64, height: f64, weight: f64) -> Self {
        PackageInfo { length, width, height, weight }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShipmentRequest {
    pub order_id: String,
    pub customer_id: String,
    pub to_address: Address,
    pub from_address: Address,
    pub carrier: Carrier,
    pub service_level: ServiceLevel,
    pub package_info: PackageInfo,
    pub idempotency_key: String,
}

impl ShipmentRequest {
    pub fn new(
        order_id: String,
        customer_id: String,
        to_address: Address,
        from_address: Address,
        carrier: Carrier,
        service_level: ServiceLevel,
        package_info: PackageInfo,
    ) -> Self {
        ShipmentRequest {
            order_id,
            customer_id,
            to_address,
            from_address,
            carrier,
            service_level,
            package_info,
            idempotency_key: ulid::Ulid::new().to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShippingLabel {
    pub label_id: String,
    pub order_id: String,
    pub tracking_number: String,
    pub carrier: Carrier,
    pub service_level: ServiceLevel,
    pub label_url: String,
    pub estimated_delivery: DateTime<Utc>,
    pub cost: i64,
}

impl ShippingLabel {
    pub fn new(
        order_id: String,
        tracking_number: String,
        carrier: Carrier,
        service_level: ServiceLevel,
        label_url: String,
        estimated_delivery: DateTime<Utc>,
        cost: i64,
    ) -> Self {
        ShippingLabel {
            label_id: ulid::Ulid::new().to_string(),
            order_id,
            tracking_number,
            carrier,
            service_level,
            label_url,
            estimated_delivery,
            cost,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CarrierInfo {
    pub carrier: Carrier,
    pub api_endpoint: String,
}

#[derive(Debug, thiserror::Error, Clone)]
pub enum ShippingError {
    #[error("Circuit breaker is open")]
    CircuitBreakerOpen,
    #[error("Carrier API error: {0}")]
    CarrierError(String),
}
