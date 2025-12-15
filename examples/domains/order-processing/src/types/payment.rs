// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PaymentMethod {
    CreditCard { token: String, last_four: String, brand: String },
    PayPal { email: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentRequest {
    pub order_id: String,
    pub customer_id: String,
    pub amount: i64,
    pub payment_method: PaymentMethod,
    pub idempotency_key: String,
}

impl PaymentRequest {
    pub fn new(order_id: String, customer_id: String, amount: i64, payment_method: PaymentMethod) -> Self {
        PaymentRequest {
            order_id,
            customer_id,
            amount,
            payment_method,
            idempotency_key: ulid::Ulid::new().to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PaymentResult {
    Success {
        transaction_id: String,
        charged_amount: i64,
        processed_at: DateTime<Utc>,
        metadata: HashMap<String, String>,
    },
    Declined {
        reason: String,
    },
}

impl PaymentResult {
    pub fn is_success(&self) -> bool {
        matches!(self, PaymentResult::Success { .. })
    }

    pub fn transaction_id(&self) -> Option<&str> {
        match self {
            PaymentResult::Success { transaction_id, .. } => Some(transaction_id),
            _ => None,
        }
    }

    pub fn charged_amount(&self) -> Option<i64> {
        match self {
            PaymentResult::Success { charged_amount, .. } => Some(*charged_amount),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChargeRequest {
    pub amount: i64,
}

#[derive(Debug, thiserror::Error, Clone)]
pub enum PaymentError {
    #[error("Circuit breaker is open")]
    CircuitBreakerOpen,
    #[error("Payment declined: {0}")]
    Declined(String),
    #[error("Gateway error: {0}")]
    GatewayError(String),
}
