// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! Basic Integration Tests
//!
//! Tests core matrix multiplication functionality without distribution

use matrix_multiply::matrix::{Matrix, multiply_blocks};

#[test]
fn test_matrix_multiply_2x2() {
    // A = [[1, 2],
    //      [3, 4]]
    let mut a = Matrix::zeros(2, 2);
    a.set(0, 0, 1.0);
    a.set(0, 1, 2.0);
    a.set(1, 0, 3.0);
    a.set(1, 1, 4.0);

    // B = [[5, 6],
    //      [7, 8]]
    let mut b = Matrix::zeros(2, 2);
    b.set(0, 0, 5.0);
    b.set(0, 1, 6.0);
    b.set(1, 0, 7.0);
    b.set(1, 1, 8.0);

    // C = A × B = [[19, 22],
    //              [43, 50]]
    let c = a.multiply(&b);

    assert_eq!(c.get(0, 0), 19.0);
    assert_eq!(c.get(0, 1), 22.0);
    assert_eq!(c.get(1, 0), 43.0);
    assert_eq!(c.get(1, 1), 50.0);
}

#[test]
fn test_block_extraction_and_insertion() {
    // Create 4×4 matrix
    let mut m = Matrix::zeros(4, 4);
    for i in 0..4 {
        for j in 0..4 {
            m.set(i, j, (i * 4 + j) as f64);
        }
    }

    // Extract 2×2 block at (1, 1)
    let block = m.get_block(1, 1, 2, 2);

    assert_eq!(block.rows, 2);
    assert_eq!(block.cols, 2);
    assert_eq!(block.get(0, 0), 5.0);  // m[1][1]
    assert_eq!(block.get(0, 1), 6.0);  // m[1][2]
    assert_eq!(block.get(1, 0), 9.0);  // m[2][1]
    assert_eq!(block.get(1, 1), 10.0); // m[2][2]

    // Create new matrix and insert block
    let mut m2 = Matrix::zeros(4, 4);
    m2.set_block(1, 1, &block);

    assert_eq!(m2.get(1, 1), 5.0);
    assert_eq!(m2.get(1, 2), 6.0);
    assert_eq!(m2.get(2, 1), 9.0);
    assert_eq!(m2.get(2, 2), 10.0);

    // Other elements should still be zero
    assert_eq!(m2.get(0, 0), 0.0);
    assert_eq!(m2.get(3, 3), 0.0);
}

#[test]
fn test_block_multiplication() {
    // Test that block multiplication works correctly
    // For C[i][j] = Σ(k) A[i][k] × B[k][j]

    // Create simple 2×2 blocks
    let mut a1 = Matrix::zeros(2, 2);
    a1.set(0, 0, 1.0);
    a1.set(0, 1, 2.0);
    a1.set(1, 0, 3.0);
    a1.set(1, 1, 4.0);

    let mut b1 = Matrix::zeros(2, 2);
    b1.set(0, 0, 5.0);
    b1.set(0, 1, 6.0);
    b1.set(1, 0, 7.0);
    b1.set(1, 1, 8.0);

    let result = multiply_blocks(&[a1], &[b1]);

    assert_eq!(result.get(0, 0), 19.0);
    assert_eq!(result.get(0, 1), 22.0);
    assert_eq!(result.get(1, 0), 43.0);
    assert_eq!(result.get(1, 1), 50.0);
}

#[test]
fn test_approx_equal() {
    let mut m1 = Matrix::zeros(2, 2);
    m1.set(0, 0, 1.0);
    m1.set(0, 1, 2.0);
    m1.set(1, 0, 3.0);
    m1.set(1, 1, 4.0);

    let mut m2 = Matrix::zeros(2, 2);
    m2.set(0, 0, 1.0000001);
    m2.set(0, 1, 2.0000001);
    m2.set(1, 0, 3.0000001);
    m2.set(1, 1, 4.0000001);

    assert!(m1.approx_equal(&m2, 1e-6));
    assert!(!m1.approx_equal(&m2, 1e-8));
}
