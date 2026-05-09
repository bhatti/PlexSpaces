// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Matrix Multiplication Library
//
// Exposes MatrixWorker actor and compute_rows function for testing

use plexspaces_sdk::{gen_server_actor, plexspaces_handlers, handler, json};
use plexspaces_actor::{ActorId, BehaviorError, ActorContext, Message};
use std::time::Instant;

// =============================================================================
// MatrixWorker Actor - Computes assigned rows of matrix multiplication
// =============================================================================

/// Worker actor for parallel matrix multiplication
///
/// ## Purpose
/// Each worker computes a partition of rows for C = A × B.
/// Master distributes work via scatter (cast) and collects results via gather (call).
///
/// ## Architecture
/// - Actor receives compute request with row range and matrices
/// - Performs matrix multiplication for assigned rows
/// - Returns result rows for master to assemble
#[gen_server_actor]
pub struct MatrixWorker {
    /// Worker ID for identification
    pub worker_id: usize,
    /// Computed result rows (stored after compute, returned on get_result)
    result: Option<WorkerResult>,
}

impl MatrixWorker {
    /// Create a new matrix worker actor
    ///
    /// ## Arguments
    /// - `worker_id`: Unique identifier for this worker
    pub fn new(worker_id: usize) -> Self {
        Self {
            worker_id,
            result: None,
        }
    }

    /// Compute matrix multiplication rows using standard algorithm
    ///
    /// ## Algorithm
    /// For each row i in [start, end):
    ///   For each column j in B:
    ///     C[i][j] = Σ(k=0 to n-1) A[i][k] * B[k][j]
    ///
    /// ## Arguments
    /// - `a`: Matrix A (m×n)
    /// - `b`: Matrix B (n×p)
    /// - `start`: Start row index (inclusive)
    /// - `end`: End row index (exclusive)
    ///
    /// ## Returns
    /// Vector of result rows for rows [start, end)
    pub fn compute_rows(a: &[Vec<f64>], b: &[Vec<f64>], start: usize, end: usize) -> Vec<Vec<f64>> {
        let b_cols = b[0].len();
        let mut result = Vec::new();

        for row_idx in start..end {
            let row_a = &a[row_idx];
            let mut result_row = vec![0.0; b_cols];
            
            for j in 0..b_cols {
                let mut sum = 0.0;
                for k in 0..row_a.len() {
                    sum += row_a[k] * b[k][j];
                }
                result_row[j] = sum;
            }
            result.push(result_row);
        }
        result
    }
}

#[plexspaces_handlers(gen_server)]
impl MatrixWorker {
    /// Handle compute request for assigned row range
    ///
    /// Supports both call (request-reply) and cast (fire-and-forget) patterns:
    /// - **call**: Returns result immediately
    /// - **cast**: Stores result for later retrieval via get_result
    ///
    /// ## Request Format
    /// ```json
    /// {
    ///   "start_row": 0,
    ///   "end_row": 100,
    ///   "matrix_a": [[...], [...]],
    ///   "matrix_b": [[...], [...]]
    /// }
    /// ```
    ///
    /// ## Response Format (for call pattern)
    /// ```json
    /// {
    ///   "start_row": 0,
    ///   "rows": [[...], [...]],
    ///   "compute_time_ms": 123
    /// }
    /// ```
    #[handler("compute_rows")]
    pub async fn handle_compute_rows(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<serde_json::Value, BehaviorError> {
        let request: serde_json::Value = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("Invalid compute request: {}", e)))?;

        let start_row = request["start_row"]
            .as_u64()
            .ok_or_else(|| BehaviorError::ProcessingError("Missing start_row".to_string()))? as usize;
        let end_row = request["end_row"]
            .as_u64()
            .ok_or_else(|| BehaviorError::ProcessingError("Missing end_row".to_string()))? as usize;
        
        let matrix_a: Vec<Vec<f64>> = serde_json::from_value(
            request["matrix_a"].clone()
        ).map_err(|e| BehaviorError::ProcessingError(format!("Invalid matrix_a: {}", e)))?;
        let matrix_b: Vec<Vec<f64>> = serde_json::from_value(
            request["matrix_b"].clone()
        ).map_err(|e| BehaviorError::ProcessingError(format!("Invalid matrix_b: {}", e)))?;

        // Compute phase: perform matrix multiplication for assigned rows
        let compute_start = Instant::now();
        let rows = Self::compute_rows(&matrix_a, &matrix_b, start_row, end_row);
        let compute_time = compute_start.elapsed();

        // Store result for later retrieval (for get_result handler)
        self.result = Some(WorkerResult {
            start_row,
            rows: rows.clone(),
        });

        // Return result immediately (for call pattern) or store for get_result (for cast pattern)
        Ok(json!({
            "start_row": start_row,
            "rows": rows,
            "compute_time_ms": compute_time.as_millis() as u64,
        }))
    }

    /// Handle get_result request to retrieve computed rows
    ///
    /// Used after cast("compute_rows") to retrieve stored results.
    /// Returns the result that was computed and stored by compute_rows handler.
    ///
    /// ## Request Format
    /// ```json
    /// {}
    /// ```
    ///
    /// ## Response Format
    /// ```json
    /// {
    ///   "start_row": 0,
    ///   "rows": [[...], [...]]
    /// }
    /// ```
    #[handler("get_result")]
    pub async fn handle_get_result(
        &mut self,
        _ctx: &ActorContext,
        _msg: &Message,
    ) -> Result<serde_json::Value, BehaviorError> {
        if let Some(ref result) = self.result {
            Ok(json!({
                "start_row": result.start_row,
                "rows": result.rows,
            }))
        } else {
            Err(BehaviorError::ProcessingError("No result computed yet".to_string()))
        }
    }
}

/// Worker result structure
struct WorkerResult {
    start_row: usize,
    rows: Vec<Vec<f64>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_rows_2x2() {
        let matrix_a = vec![
            vec![1.0, 2.0],
            vec![3.0, 4.0],
        ];
        let matrix_b = vec![
            vec![5.0, 6.0],
            vec![7.0, 8.0],
        ];
        
        // Expected: C = A × B
        // C[0][0] = 1*5 + 2*7 = 19
        // C[0][1] = 1*6 + 2*8 = 22
        // C[1][0] = 3*5 + 4*7 = 43
        // C[1][1] = 3*6 + 4*8 = 50
        
        let result = MatrixWorker::compute_rows(&matrix_a, &matrix_b, 0, 2);
        
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].len(), 2);
        assert_eq!(result[0][0], 19.0);
        assert_eq!(result[0][1], 22.0);
        assert_eq!(result[1][0], 43.0);
        assert_eq!(result[1][1], 50.0);
    }

    #[test]
    fn test_compute_rows_partial() {
        let matrix_a = vec![
            vec![1.0, 2.0],
            vec![3.0, 4.0],
            vec![5.0, 6.0],
        ];
        let matrix_b = vec![
            vec![1.0, 0.0],
            vec![0.0, 1.0],
        ];
        
        // Compute only rows 1-2 (index 1 to 2, exclusive)
        let result = MatrixWorker::compute_rows(&matrix_a, &matrix_b, 1, 2);
        
        assert_eq!(result.len(), 1);
        assert_eq!(result[0][0], 3.0);
        assert_eq!(result[0][1], 4.0);
    }

    #[test]
    fn test_matrix_worker_new() {
        let worker = MatrixWorker::new(42);
        assert_eq!(worker.worker_id, 42);
    }
}
