// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! Integration tests for WASM calculator

use wasm_calculator::*;

#[tokio::test]
async fn test_calculator_addition() {
    let req = CalculationRequest::new(
        Operation::Add,
        vec![10.0, 5.0],
        "test".to_string()
    );

    let result = req.execute().expect("Calculation failed");
    assert_eq!(result, 15.0);
}

#[tokio::test]
async fn test_calculator_all_operations() {
    let operations = vec![
        (Operation::Add, vec![10.0, 5.0], 15.0),
        (Operation::Subtract, vec![10.0, 5.0], 5.0),
        (Operation::Multiply, vec![10.0, 5.0], 50.0),
        (Operation::Divide, vec![10.0, 5.0], 2.0),
        (Operation::Power, vec![2.0, 3.0], 8.0),
        (Operation::SquareRoot, vec![16.0], 4.0),
    ];

    for (op, operands, expected) in operations {
        let req = CalculationRequest::new(op.clone(), operands, "test".to_string());
        let result = req.execute().expect(&format!("Failed: {:?}", op));
        assert_eq!(result, expected, "Operation {:?} failed", op);
    }
}
