// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryItem {
    pub product_id: String,
    pub name: String,
    pub available_quantity: i64,
    pub reserved_quantity: i64,
    pub version: u64,
}

impl InventoryItem {
    pub fn new(product_id: String, name: String, initial_quantity: i64) -> Self {
        InventoryItem {
            product_id,
            name,
            available_quantity: initial_quantity,
            reserved_quantity: 0,
            version: 0,
        }
    }

    pub fn reserve(&mut self, quantity: i64) -> Result<(), InventoryError> {
        if self.available_quantity < quantity {
            return Err(InventoryError::InsufficientStock {
                product_id: self.product_id.clone(),
                requested: quantity,
                available: self.available_quantity,
            });
        }
        self.available_quantity -= quantity;
        self.reserved_quantity += quantity;
        self.version += 1;
        Ok(())
    }

    pub fn release(&mut self, quantity: i64) -> Result<(), InventoryError> {
        self.reserved_quantity -= quantity;
        self.available_quantity += quantity;
        self.version += 1;
        Ok(())
    }

    pub fn fulfill(&mut self, quantity: i64) -> Result<(), InventoryError> {
        self.reserved_quantity -= quantity;
        self.version += 1;
        Ok(())
    }

    pub fn add_stock(&mut self, quantity: i64) {
        self.available_quantity += quantity;
        self.version += 1;
    }

    pub fn stock_level(&self) -> StockLevel {
        StockLevel {
            product_id: self.product_id.clone(),
            available: self.available_quantity,
            reserved: self.reserved_quantity,
            total: self.available_quantity + self.reserved_quantity,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReservationRequest {
    pub reservation_id: String,
    pub order_id: String,
    pub items: Vec<ReservationItem>,
}

impl ReservationRequest {
    pub fn new(order_id: String, items: Vec<ReservationItem>) -> Self {
        ReservationRequest {
            reservation_id: ulid::Ulid::new().to_string(),
            order_id,
            items,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReservationItem {
    pub product_id: String,
    pub quantity: i64,
}

impl ReservationItem {
    pub fn new(product_id: String, quantity: i64) -> Self {
        ReservationItem { product_id, quantity }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StockLevel {
    pub product_id: String,
    pub available: i64,
    pub reserved: i64,
    pub total: i64,
}

#[derive(Debug, thiserror::Error)]
pub enum InventoryError {
    #[error("Product not found: {0}")]
    ProductNotFound(String),
    #[error("Insufficient stock for {product_id}: requested {requested}, available {available}")]
    InsufficientStock {
        product_id: String,
        requested: i64,
        available: i64,
    },
}
