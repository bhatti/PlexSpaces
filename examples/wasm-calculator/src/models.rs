// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! Data models for distributed calculator

use serde::{Deserialize, Serialize};

/// Calculator operation types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Operation {
    Add,
    Subtract,
    Multiply,
    Divide,
    Power,
    SquareRoot,
}

/// Calculator request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalculationRequest {
    /// Operation ID (ULID for sortability)
    pub operation_id: String,

    /// Operation type
    pub operation: Operation,

    /// Operands (2 for binary ops, 1 for unary)
    pub operands: Vec<f64>,

    /// Requester actor ID
    pub requester_id: String,
}

/// Calculator response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalculationResponse {
    /// Operation ID (matches request)
    pub operation_id: String,

    /// Result of calculation
    pub result: f64,

    /// Error message if calculation failed
    pub error: Option<String>,
}

/// Multi-step calculation (e.g., (a + b) * (c - d))
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplexCalculation {
    /// Calculation ID
    pub calculation_id: String,

    /// Steps in calculation
    pub steps: Vec<CalculationStep>,

    /// Final result
    pub result: Option<f64>,
}

/// Single step in complex calculation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalculationStep {
    /// Step ID
    pub step_id: String,

    /// Operation
    pub operation: Operation,

    /// Operands (can reference previous step results)
    pub operands: Vec<Operand>,

    /// Result (filled after execution)
    pub result: Option<f64>,
}

/// Operand can be constant or reference to previous result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Operand {
    Constant(f64),
    StepResult(String), // References step_id
}

impl CalculationRequest {
    /// Create new calculation request
    pub fn new(operation: Operation, operands: Vec<f64>, requester_id: String) -> Self {
        Self {
            operation_id: ulid::Ulid::new().to_string(),
            operation,
            operands,
            requester_id,
        }
    }

    /// Validate request
    pub fn validate(&self) -> Result<(), String> {
        match self.operation {
            Operation::Add | Operation::Subtract | Operation::Multiply | Operation::Divide | Operation::Power => {
                if self.operands.len() != 2 {
                    return Err(format!("{:?} requires exactly 2 operands", self.operation));
                }
            }
            Operation::SquareRoot => {
                if self.operands.len() != 1 {
                    return Err("SquareRoot requires exactly 1 operand".to_string());
                }
            }
        }
        Ok(())
    }

    /// Execute calculation
    pub fn execute(&self) -> Result<f64, String> {
        self.validate()?;

        match self.operation {
            Operation::Add => Ok(self.operands[0] + self.operands[1]),
            Operation::Subtract => Ok(self.operands[0] - self.operands[1]),
            Operation::Multiply => Ok(self.operands[0] * self.operands[1]),
            Operation::Divide => {
                if self.operands[1] == 0.0 {
                    Err("Division by zero".to_string())
                } else {
                    Ok(self.operands[0] / self.operands[1])
                }
            }
            Operation::Power => Ok(self.operands[0].powf(self.operands[1])),
            Operation::SquareRoot => {
                if self.operands[0] < 0.0 {
                    Err("Square root of negative number".to_string())
                } else {
                    Ok(self.operands[0].sqrt())
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_addition() {
        let req = CalculationRequest::new(Operation::Add, vec![2.0, 3.0], "test".to_string());
        assert_eq!(req.execute().unwrap(), 5.0);
    }

    #[test]
    fn test_subtraction() {
        let req = CalculationRequest::new(Operation::Subtract, vec![5.0, 3.0], "test".to_string());
        assert_eq!(req.execute().unwrap(), 2.0);
    }

    #[test]
    fn test_multiplication() {
        let req = CalculationRequest::new(Operation::Multiply, vec![4.0, 3.0], "test".to_string());
        assert_eq!(req.execute().unwrap(), 12.0);
    }

    #[test]
    fn test_division() {
        let req = CalculationRequest::new(Operation::Divide, vec![10.0, 2.0], "test".to_string());
        assert_eq!(req.execute().unwrap(), 5.0);
    }

    #[test]
    fn test_division_by_zero() {
        let req = CalculationRequest::new(Operation::Divide, vec![10.0, 0.0], "test".to_string());
        assert!(req.execute().is_err());
    }

    #[test]
    fn test_power() {
        let req = CalculationRequest::new(Operation::Power, vec![2.0, 3.0], "test".to_string());
        assert_eq!(req.execute().unwrap(), 8.0);
    }

    #[test]
    fn test_square_root() {
        let req = CalculationRequest::new(Operation::SquareRoot, vec![9.0], "test".to_string());
        assert_eq!(req.execute().unwrap(), 3.0);
    }

    #[test]
    fn test_square_root_negative() {
        let req = CalculationRequest::new(Operation::SquareRoot, vec![-1.0], "test".to_string());
        assert!(req.execute().is_err());
    }

    #[test]
    fn test_validation_wrong_operand_count() {
        let req = CalculationRequest::new(Operation::Add, vec![2.0], "test".to_string());
        assert!(req.validate().is_err());
    }
}
