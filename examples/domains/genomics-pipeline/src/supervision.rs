// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Supervision Trees for Genomics Pipeline
//!
//! Creates hierarchical supervision trees for the genomics pipeline:
//!
//! ```text
//! RootSupervisor (OneForAll)
//!     ├─> CoordinatorSupervisor (OneForOne)
//!     │       └─> GenomicsCoordinator (Permanent)
//!     ├─> QCPoolSupervisor (OneForOne)
//!     │       ├─> QCWorker-1 (Permanent)
//!     │       ├─> QCWorker-2 (Permanent)
//!     │       └─> QCWorker-3 (Permanent)
//!     ├─> AlignmentPoolSupervisor (OneForOne)
//!     │       ├─> AlignmentWorker-1 (Permanent)
//!     │       ├─> AlignmentWorker-2 (Permanent)
//!     │       ├─> AlignmentWorker-3 (Permanent)
//!     │       └─> AlignmentWorker-4 (Permanent)
//!     └─> ChromosomePoolSupervisor (OneForOne)
//!             ├─> ChromosomeWorker-chr1 (Permanent)
//!             ├─> ChromosomeWorker-chr2 (Permanent)
//!             ├─> ... (chr3-chr22)
//!             ├─> ChromosomeWorker-chrX (Permanent)
//!             └─> ChromosomeWorker-chrY (Permanent)
//! ```
//!
//! ## Supervision Strategies
//!
//! - **RootSupervisor**: OneForAll (if coordinator dies, restart everything)
//! - **Worker Pools**: OneForOne (individual worker restart doesn't affect others)
//!
//! ## Restart Policies
//!
//! - **Coordinator**: Permanent (always restart)
//! - **Workers**: Permanent (always restart)
//!
//! ## Why This Design?
//!
//! 1. **OneForAll at Root**: Coordinator crash = restart all workers (clean slate)
//! 2. **OneForOne for Pools**: Worker crash doesn't affect siblings
//! 3. **Permanent Restart**: All components are critical, always restart
//! 4. **Hierarchical**: Clear separation of concerns, easy to reason about

use crate::coordinator::GenomicsCoordinator;
use crate::workers::*;
use std::sync::Arc;

/// Configuration for genomics supervision tree
pub struct GenomicsSupervisionConfig {
    pub qc_pool_size: usize,
    pub alignment_pool_size: usize,
    pub chromosome_pool_size: usize,
    pub annotation_pool_size: usize,
    pub report_pool_size: usize,
}

impl Default for GenomicsSupervisionConfig {
    fn default() -> Self {
        Self {
            qc_pool_size: 3,
            alignment_pool_size: 4,
            chromosome_pool_size: 24, // chr1-22, X, Y
            annotation_pool_size: 2,
            report_pool_size: 2,
        }
    }
}

/// Create the full genomics supervision tree
///
/// ## Returns
/// Root supervisor managing the entire pipeline
pub fn create_supervision_tree(
    _config: GenomicsSupervisionConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    // TODO: Implement using plexspaces_supervisor::Supervisor
    // This is a placeholder for the actual implementation

    // Example structure (to be implemented):
    // 1. Create root supervisor (OneForAll)
    // 2. Add coordinator supervisor (OneForOne)
    //    - Add coordinator actor
    // 3. Add QC pool supervisor (OneForOne)
    //    - Add QC workers (3)
    // 4. Add alignment pool supervisor (OneForOne)
    //    - Add alignment workers (4)
    // 5. Add chromosome pool supervisor (OneForOne)
    //    - Add chromosome workers (24)

    Ok(())
}

/// Helper to create chromosome list
fn chromosome_list() -> Vec<String> {
    let mut chromosomes = Vec::new();

    // Autosomes: chr1 to chr22
    for i in 1..=22 {
        chromosomes.push(format!("chr{}", i));
    }

    // Sex chromosomes
    chromosomes.push("chrX".to_string());
    chromosomes.push("chrY".to_string());

    chromosomes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = GenomicsSupervisionConfig::default();
        assert_eq!(config.qc_pool_size, 3);
        assert_eq!(config.alignment_pool_size, 4);
        assert_eq!(config.chromosome_pool_size, 24);
    }

    #[test]
    fn test_chromosome_list() {
        let chromosomes = chromosome_list();
        assert_eq!(chromosomes.len(), 24);
        assert_eq!(chromosomes[0], "chr1");
        assert_eq!(chromosomes[21], "chr22");
        assert_eq!(chromosomes[22], "chrX");
        assert_eq!(chromosomes[23], "chrY");
    }

    #[test]
    fn test_create_supervision_tree() {
        let config = GenomicsSupervisionConfig::default();
        let result = create_supervision_tree(config);
        assert!(result.is_ok());
    }
}
