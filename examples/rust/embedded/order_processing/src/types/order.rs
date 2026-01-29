// SPDX-License-Identifier: LGPL-2.1-or-later  
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum OrderState {
    Pending,
    PaymentProcessing,
    Reserved,
    Shipped,
    Completed,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderItem {
    pub product_id: String,
    pub name: String,
    pub quantity: i64,
    pub price: i64,
}

impl OrderItem {
    pub fn new(product_id: String, name: String, quantity: i64, price: i64) -> Self {
        OrderItem { product_id, name, quantity, price }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
    pub order_id: String,
    pub customer_id: String,
    pub items: Vec<OrderItem>,
    pub total_amount: i64,
    pub state: OrderState,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Order {
    pub fn new(customer_id: String, items: Vec<OrderItem>) -> Self {
        let total_amount = items.iter().map(|i| i.price * i.quantity).sum();
        let now = Utc::now();
        Order {
            order_id: ulid::Ulid::new().to_string(),
            customer_id,
            items,
            total_amount,
            state: OrderState::Pending,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn start_payment_processing(&mut self) -> Result<(), OrderError> {
        match self.state {
            OrderState::Pending => {
                self.state = OrderState::PaymentProcessing;
                self.updated_at = Utc::now();
                Ok(())
            }
            _ => Err(OrderError::InvalidStateTransition {
                from: format!("{:?}", self.state),
                to: "PaymentProcessing".to_string(),
            }),
        }
    }

    pub fn cancel(&mut self) -> Result<(), OrderError> {
        match self.state {
            OrderState::Pending | OrderState::PaymentProcessing => {
                self.state = OrderState::Cancelled;
                self.updated_at = Utc::now();
                Ok(())
            }
            _ => Err(OrderError::InvalidStateTransition {
                from: format!("{:?}", self.state),
                to: "Cancelled".to_string(),
            }),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum OrderError {
    #[error("Order not found: {0}")]
    NotFound(String),
    #[error("Invalid state transition from {from} to {to}")]
    InvalidStateTransition { from: String, to: String },
}
