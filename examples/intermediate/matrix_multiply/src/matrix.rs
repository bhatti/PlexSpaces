// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! Matrix utilities for block-based parallel multiplication

use serde::{Deserialize, Serialize};

/// Dense matrix stored as row-major Vec
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Matrix {
    pub rows: usize,
    pub cols: usize,
    pub data: Vec<f64>,
}

impl Matrix {
    /// Create new matrix filled with zeros
    pub fn zeros(rows: usize, cols: usize) -> Self {
        Matrix {
            rows,
            cols,
            data: vec![0.0; rows * cols],
        }
    }

    /// Create matrix with random values [0.0, 1.0)
    pub fn random(rows: usize, cols: usize) -> Self {
        use std::collections::hash_map::RandomState;
        use std::hash::{BuildHasher, Hash, Hasher};

        let mut matrix = Self::zeros(rows, cols);
        let hasher = RandomState::new();

        for i in 0..rows {
            for j in 0..cols {
                // Simple deterministic "random" for testing
                let mut h = hasher.build_hasher();
                (i, j).hash(&mut h);
                let val = (h.finish() % 1000) as f64 / 1000.0;
                matrix.set(i, j, val);
            }
        }

        matrix
    }

    /// Get value at (row, col)
    pub fn get(&self, row: usize, col: usize) -> f64 {
        assert!(row < self.rows && col < self.cols);
        self.data[row * self.cols + col]
    }

    /// Set value at (row, col)
    pub fn set(&mut self, row: usize, col: usize, value: f64) {
        assert!(row < self.rows && col < self.cols);
        self.data[row * self.cols + col] = value;
    }

    /// Extract a block (submatrix)
    pub fn get_block(&self, start_row: usize, start_col: usize, block_rows: usize, block_cols: usize) -> Matrix {
        let mut block = Matrix::zeros(block_rows, block_cols);

        for i in 0..block_rows {
            for j in 0..block_cols {
                let val = self.get(start_row + i, start_col + j);
                block.set(i, j, val);
            }
        }

        block
    }

    /// Set a block (submatrix) into this matrix
    pub fn set_block(&mut self, start_row: usize, start_col: usize, block: &Matrix) {
        for i in 0..block.rows {
            for j in 0..block.cols {
                let val = block.get(i, j);
                self.set(start_row + i, start_col + j, val);
            }
        }
    }

    /// Multiply this matrix by another (C = A × B)
    pub fn multiply(&self, other: &Matrix) -> Matrix {
        assert_eq!(self.cols, other.rows, "Matrix dimensions incompatible for multiplication");

        let mut result = Matrix::zeros(self.rows, other.cols);

        for i in 0..self.rows {
            for j in 0..other.cols {
                let mut sum = 0.0;
                for k in 0..self.cols {
                    sum += self.get(i, k) * other.get(k, j);
                }
                result.set(i, j, sum);
            }
        }

        result
    }

    /// Add another matrix to this one
    pub fn add(&mut self, other: &Matrix) {
        assert_eq!(self.rows, other.rows);
        assert_eq!(self.cols, other.cols);

        for i in 0..self.data.len() {
            self.data[i] += other.data[i];
        }
    }

    /// Check if matrices are approximately equal (for verification)
    pub fn approx_equal(&self, other: &Matrix, epsilon: f64) -> bool {
        if self.rows != other.rows || self.cols != other.cols {
            return false;
        }

        for i in 0..self.data.len() {
            if (self.data[i] - other.data[i]).abs() > epsilon {
                return false;
            }
        }

        true
    }

    /// Print matrix (for small matrices)
    #[allow(dead_code)]
    pub fn print(&self) {
        println!("Matrix {}×{}:", self.rows, self.cols);
        for i in 0..self.rows {
            print!("  [");
            for j in 0..self.cols {
                print!("{:8.4}", self.get(i, j));
                if j < self.cols - 1 {
                    print!(", ");
                }
            }
            println!("]");
        }
    }
}

/// Multiply blocks: C_block = Σ(A_blocks[k] × B_blocks[k])
///
/// For block matrix multiplication C = A × B:
/// C[i][j] = Σ(k) A[i][k] × B[k][j]
///
/// Each block multiplication requires K block multiplications and additions
pub fn multiply_blocks(a_blocks: &[Matrix], b_blocks: &[Matrix]) -> Matrix {
    assert!(!a_blocks.is_empty());
    assert_eq!(a_blocks.len(), b_blocks.len());

    // First multiplication
    let mut result = a_blocks[0].multiply(&b_blocks[0]);

    // Add remaining products
    for k in 1..a_blocks.len() {
        let product = a_blocks[k].multiply(&b_blocks[k]);
        result.add(&product);
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_matrix_creation() {
        let m = Matrix::zeros(3, 4);
        assert_eq!(m.rows, 3);
        assert_eq!(m.cols, 4);
        assert_eq!(m.data.len(), 12);
    }

    #[test]
    fn test_matrix_get_set() {
        let mut m = Matrix::zeros(2, 2);
        m.set(0, 0, 1.0);
        m.set(0, 1, 2.0);
        m.set(1, 0, 3.0);
        m.set(1, 1, 4.0);

        assert_eq!(m.get(0, 0), 1.0);
        assert_eq!(m.get(0, 1), 2.0);
        assert_eq!(m.get(1, 0), 3.0);
        assert_eq!(m.get(1, 1), 4.0);
    }

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
    fn test_get_block() {
        // 4×4 matrix
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
    }

    #[test]
    fn test_set_block() {
        let mut m = Matrix::zeros(4, 4);

        // Create 2×2 block
        let mut block = Matrix::zeros(2, 2);
        block.set(0, 0, 99.0);
        block.set(0, 1, 98.0);
        block.set(1, 0, 97.0);
        block.set(1, 1, 96.0);

        // Set block at (1, 1)
        m.set_block(1, 1, &block);

        assert_eq!(m.get(1, 1), 99.0);
        assert_eq!(m.get(1, 2), 98.0);
        assert_eq!(m.get(2, 1), 97.0);
        assert_eq!(m.get(2, 2), 96.0);

        // Check other elements still zero
        assert_eq!(m.get(0, 0), 0.0);
        assert_eq!(m.get(3, 3), 0.0);
    }

    #[test]
    fn test_multiply_blocks() {
        // Test block multiplication for 4×4 matrices with 2×2 blocks
        let mut a = Matrix::zeros(2, 2);
        a.set(0, 0, 1.0);
        a.set(0, 1, 2.0);
        a.set(1, 0, 3.0);
        a.set(1, 1, 4.0);

        let mut b = Matrix::zeros(2, 2);
        b.set(0, 0, 5.0);
        b.set(0, 1, 6.0);
        b.set(1, 0, 7.0);
        b.set(1, 1, 8.0);

        let result = multiply_blocks(&[a], &[b]);

        assert_eq!(result.get(0, 0), 19.0);
        assert_eq!(result.get(0, 1), 22.0);
        assert_eq!(result.get(1, 0), 43.0);
        assert_eq!(result.get(1, 1), 50.0);
    }
}
