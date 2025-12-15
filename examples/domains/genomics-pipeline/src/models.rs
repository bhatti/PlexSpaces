// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Data models for genomics DNA sequencing pipeline
//!
//! Defines the data structures passed between workflow steps:
//! - Sample metadata
//! - QC results
//! - Alignment results
//! - Variant calling results
//! - Annotation results
//! - Final clinical report

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Sample information submitted for processing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sample {
    /// Unique sample identifier
    pub sample_id: String,

    /// Path to FASTQ file
    pub fastq_path: String,

    /// Sample metadata (patient info, etc.)
    pub metadata: HashMap<String, String>,
}

/// Quality control analysis results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QCResult {
    /// Sample ID
    pub sample_id: String,

    /// Quality score (0-40, higher is better)
    pub quality_score: f64,

    /// GC content percentage
    pub gc_content: f64,

    /// Total number of reads
    pub total_reads: u64,

    /// Adapter contamination detected
    pub adapter_contamination: bool,

    /// QC passed threshold
    pub passed: bool,
}

/// Genome alignment results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlignmentResult {
    /// Sample ID
    pub sample_id: String,

    /// Path to generated BAM file
    pub bam_path: String,

    /// Percentage of reads aligned
    pub alignment_rate: f64,

    /// Coverage depth
    pub coverage_depth: f64,
}

/// Variant calling result for a single chromosome
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChromosomeVariants {
    /// Sample ID
    pub sample_id: String,

    /// Chromosome name (chr1, chr2, ..., chrX, chrY)
    pub chromosome: String,

    /// Number of SNPs detected
    pub snp_count: u32,

    /// Number of indels detected
    pub indel_count: u32,

    /// Path to VCF file with variants
    pub vcf_path: String,
}

/// Aggregated variant calling results from all chromosomes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariantCallingResult {
    /// Sample ID
    pub sample_id: String,

    /// Per-chromosome results
    pub chromosome_results: Vec<ChromosomeVariants>,

    /// Total SNPs across all chromosomes
    pub total_snps: u32,

    /// Total indels across all chromosomes
    pub total_indels: u32,
}

/// Variant annotation with clinical significance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnnotatedVariant {
    /// Chromosome location
    pub chromosome: String,

    /// Position on chromosome
    pub position: u64,

    /// Reference allele
    pub reference: String,

    /// Alternate allele
    pub alternate: String,

    /// Gene name
    pub gene: String,

    /// Clinical significance (Pathogenic, Benign, VUS)
    pub clinical_significance: String,

    /// ClinVar ID if available
    pub clinvar_id: Option<String>,

    /// Population frequency (gnomAD)
    pub population_frequency: Option<f64>,
}

/// Annotation results with clinical interpretation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnnotationResult {
    /// Sample ID
    pub sample_id: String,

    /// Annotated variants
    pub variants: Vec<AnnotatedVariant>,

    /// Number of pathogenic variants
    pub pathogenic_count: u32,

    /// Number of benign variants
    pub benign_count: u32,

    /// Number of variants of unknown significance
    pub vus_count: u32,
}

/// Final clinical report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClinicalReport {
    /// Sample ID
    pub sample_id: String,

    /// QC metrics
    pub qc_metrics: QCResult,

    /// Alignment metrics
    pub alignment_metrics: AlignmentResult,

    /// Variant summary
    pub variant_summary: VariantCallingResult,

    /// Annotated variants
    pub annotations: AnnotationResult,

    /// Clinical interpretation
    pub interpretation: String,

    /// Recommendations
    pub recommendations: Vec<String>,

    /// Report generation timestamp
    pub generated_at: String,
}

/// Request messages for worker actors

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QCRequest {
    pub sample_id: String,
    pub fastq_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlignmentRequest {
    pub sample_id: String,
    pub qc_data: QCResult,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariantCallingRequest {
    pub sample_id: String,
    pub chromosome: String,
    pub alignment_bam: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnnotationRequest {
    pub sample_id: String,
    pub variants: VariantCallingResult,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportRequest {
    pub sample_id: String,
    pub qc_metrics: QCResult,
    pub alignment_metrics: AlignmentResult,
    pub variant_summary: VariantCallingResult,
    pub annotations: AnnotationResult,
}
