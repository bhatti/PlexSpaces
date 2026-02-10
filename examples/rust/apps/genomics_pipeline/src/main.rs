// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Genomics Pipeline - DNA Sequencing Workflow
//
// Real-world use case: DNA sequencing analysis pipeline inspired by GATK/Illumina.
// Processes raw sequence reads through QC → Alignment → Variant Calling.
//
// ## SDK Features Demonstrated
// - `#[workflow_actor]` - Durable workflow execution with crash recovery
// - `#[run_handler]` - Main pipeline execution
// - `#[signal_handler("name")]` - External events (pause, resume, cancel)
// - `#[query_handler("name")]` - Status queries (status, progress, metrics)
// - `DurabilityFacet` - Workflow state persistence for recovery
//
// ## Pipeline Steps
// 1. Quality Control (QC): Filter low-quality reads, check adapters
// 2. Genome Alignment: Map reads to reference genome (hg38)
// 3. Variant Calling: Identify SNPs and indels
//
// ## Workflow Signals
// - pause: Pause pipeline execution
// - resume: Resume paused pipeline
// - cancel: Cancel pipeline execution
//
// ## Workflow Queries
// - status: Get current pipeline status
// - progress: Get detailed progress metrics
// - metrics: Get performance metrics (compute/coordinate ratio)

use plexspaces_sdk::{
    workflow_actor, plexspaces_handlers, spawn_workflow_actor, WorkflowRef,
    ActorContext, BehaviorError, RequestContext, Message, NodeBuilder,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn, Level};
use chrono::{DateTime, Utc};

// Required for macro-generated code
extern crate plexspaces_core;
extern crate plexspaces_behavior;

// =============================================================================
// Domain Types - Genomics Pipeline
// =============================================================================

/// Quality control result
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QCResult {
    pub total_reads: usize,
    pub passed_reads: usize,
    pub failed_reads: usize,
    pub avg_quality: f64,
    pub gc_content: f64,
    pub adapter_contamination: f64,
}

/// Alignment result
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AlignmentResult {
    pub aligned_reads: usize,
    pub unaligned_reads: usize,
    pub alignment_rate: f64,
    pub avg_mapping_quality: f64,
    pub reference_genome: String,
}

/// Variant calling result
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VariantResult {
    pub total_variants: usize,
    pub snps: usize,
    pub indels: usize,
    pub novel_variants: usize,
    pub known_variants: usize,
}

/// Pipeline input configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineInput {
    pub sample_id: String,
    pub num_reads: usize,
    pub reference_genome: String,
    pub min_quality_score: u8,
}

/// Pipeline execution state
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PipelineState {
    pub sample_id: String,
    pub status: String,
    pub current_step: String,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub qc_result: Option<QCResult>,
    pub alignment_result: Option<AlignmentResult>,
    pub variant_result: Option<VariantResult>,
    pub error: Option<String>,
    pub is_paused: bool,
    // Performance metrics
    pub compute_time_ms: u64,
    pub coordinate_time_ms: u64,
}

// =============================================================================
// Genomics Pipeline Workflow Actor
// =============================================================================

/// DNA sequencing pipeline workflow.
///
/// Implements a 3-step bioinformatics pipeline:
/// 1. Quality Control - Filter low-quality reads
/// 2. Alignment - Map reads to reference genome
/// 3. Variant Calling - Identify genetic variations
///
/// ## Features
/// - Durable execution (survives crashes)
/// - Pause/resume capability
/// - Progress tracking
/// - Performance metrics (compute vs coordinate time)
#[workflow_actor]
struct GenomicsPipelineWorkflow {
    state: PipelineState,
    input: Option<PipelineInput>,
}

impl GenomicsPipelineWorkflow {
    fn new() -> Self {
        Self {
            state: PipelineState::default(),
            input: None,
        }
    }

    /// Simulate QC processing with realistic compute time.
    /// In production, this would analyze FASTQ files for quality metrics.
    async fn run_quality_control(&self, num_reads: usize, min_quality: u8) -> QCResult {
        // Simulate compute time proportional to reads (50ms base + 1ms per 1000 reads)
        let compute_ms = 50 + (num_reads / 1000) as u64;
        tokio::time::sleep(Duration::from_millis(compute_ms)).await;
        
        // Simulate QC analysis
        let passed = (num_reads as f64 * 0.95) as usize; // 95% pass rate
        let failed = num_reads - passed;
        
        QCResult {
            total_reads: num_reads,
            passed_reads: passed,
            failed_reads: failed,
            avg_quality: 35.0 + (min_quality as f64 * 0.1),
            gc_content: 0.42, // Typical human genome GC content
            adapter_contamination: 0.02, // 2% adapter contamination
        }
    }

    /// Simulate alignment processing with realistic compute time.
    /// In production, this would use BWA-MEM2 or similar aligner.
    async fn run_alignment(&self, passed_reads: usize, reference: &str) -> AlignmentResult {
        // Alignment is compute-intensive (100ms base + 2ms per 1000 reads)
        let compute_ms = 100 + (passed_reads / 500) as u64;
        tokio::time::sleep(Duration::from_millis(compute_ms)).await;
        
        // Simulate alignment to reference genome
        let aligned = (passed_reads as f64 * 0.98) as usize; // 98% alignment rate
        let unaligned = passed_reads - aligned;
        
        AlignmentResult {
            aligned_reads: aligned,
            unaligned_reads: unaligned,
            alignment_rate: aligned as f64 / passed_reads as f64,
            avg_mapping_quality: 45.0,
            reference_genome: reference.to_string(),
        }
    }

    /// Simulate variant calling with realistic compute time.
    /// In production, this would use GATK HaplotypeCaller or DeepVariant.
    async fn run_variant_calling(&self, aligned_reads: usize) -> VariantResult {
        // Variant calling is most compute-intensive (150ms base + 3ms per 1000 reads)
        let compute_ms = 150 + (aligned_reads / 333) as u64;
        tokio::time::sleep(Duration::from_millis(compute_ms)).await;
        
        // Simulate variant calling (typical: ~4-5 million variants per genome)
        let variants_per_read = 0.005; // Simplified ratio
        let total = (aligned_reads as f64 * variants_per_read) as usize;
        let snps = (total as f64 * 0.85) as usize; // 85% SNPs
        let indels = total - snps;
        
        VariantResult {
            total_variants: total,
            snps,
            indels,
            novel_variants: (total as f64 * 0.1) as usize, // 10% novel
            known_variants: (total as f64 * 0.9) as usize, // 90% known
        }
    }
}

#[plexspaces_handlers(workflow)]
impl GenomicsPipelineWorkflow {
    /// Main workflow execution - runs the genomics pipeline
    #[run_handler]
    async fn run(
        &mut self,
        _ctx: &ActorContext,
        input: Message,
    ) -> Result<Message, BehaviorError> {
        // Parse pipeline input
        let pipeline_input: PipelineInput = serde_json::from_slice(&input.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("Invalid input: {}", e)))?;

        self.input = Some(pipeline_input.clone());
        self.state.sample_id = pipeline_input.sample_id.clone();
        self.state.status = "running".to_string();
        self.state.started_at = Some(Utc::now());

        info!("Starting genomics pipeline for sample: {}", pipeline_input.sample_id);

        // =====================================================================
        // Step 1: Quality Control
        // =====================================================================
        self.state.current_step = "quality_control".to_string();
        print!("  [1/3] Quality Control: Analyzing {} reads... ", pipeline_input.num_reads);
        std::io::Write::flush(&mut std::io::stdout()).ok();

        // Check if paused
        if self.state.is_paused {
            self.state.status = "paused".to_string();
            return self.create_result_message();
        }

        let qc_start = std::time::Instant::now();
        let qc_result = self.run_quality_control(
            pipeline_input.num_reads,
            pipeline_input.min_quality_score,
        ).await;
        let qc_elapsed = qc_start.elapsed().as_millis() as u64;
        self.state.compute_time_ms += qc_elapsed;
        self.state.qc_result = Some(qc_result.clone());

        println!("{} passed ({:.0}%) in {}ms ✓", 
            qc_result.passed_reads,
            qc_result.passed_reads as f64 / qc_result.total_reads as f64 * 100.0,
            qc_elapsed
        );

        // Simulate coordination overhead
        tokio::time::sleep(Duration::from_millis(10)).await;
        self.state.coordinate_time_ms += 10;

        // =====================================================================
        // Step 2: Genome Alignment
        // =====================================================================
        self.state.current_step = "alignment".to_string();
        print!("  [2/3] Genome Alignment: Mapping to {}... ", pipeline_input.reference_genome);
        std::io::Write::flush(&mut std::io::stdout()).ok();

        // Check if paused
        if self.state.is_paused {
            self.state.status = "paused".to_string();
            return self.create_result_message();
        }

        let align_start = std::time::Instant::now();
        let alignment_result = self.run_alignment(
            qc_result.passed_reads,
            &pipeline_input.reference_genome,
        ).await;
        let align_elapsed = align_start.elapsed().as_millis() as u64;
        self.state.compute_time_ms += align_elapsed;
        self.state.alignment_result = Some(alignment_result.clone());

        println!("{} aligned ({:.0}%) in {}ms ✓", 
            alignment_result.aligned_reads,
            alignment_result.alignment_rate * 100.0,
            align_elapsed
        );

        // Simulate coordination overhead
        tokio::time::sleep(Duration::from_millis(10)).await;
        self.state.coordinate_time_ms += 10;

        // =====================================================================
        // Step 3: Variant Calling
        // =====================================================================
        self.state.current_step = "variant_calling".to_string();
        print!("  [3/3] Variant Calling: Identifying SNPs/Indels... ");
        std::io::Write::flush(&mut std::io::stdout()).ok();

        // Check if paused
        if self.state.is_paused {
            self.state.status = "paused".to_string();
            return self.create_result_message();
        }

        let variant_start = std::time::Instant::now();
        let variant_result = self.run_variant_calling(alignment_result.aligned_reads).await;
        let variant_elapsed = variant_start.elapsed().as_millis() as u64;
        self.state.compute_time_ms += variant_elapsed;
        self.state.variant_result = Some(variant_result.clone());

        println!("{} variants ({} SNPs, {} indels) in {}ms ✓", 
            variant_result.total_variants,
            variant_result.snps,
            variant_result.indels,
            variant_elapsed
        );

        // =====================================================================
        // Complete
        // =====================================================================
        self.state.status = "completed".to_string();
        self.state.current_step = "done".to_string();
        self.state.completed_at = Some(Utc::now());

        info!("Pipeline completed for sample: {}", pipeline_input.sample_id);

        self.create_result_message()
    }

    /// Pause the pipeline execution
    #[signal_handler("pause")]
    async fn on_pause(
        &mut self,
        _ctx: &ActorContext,
        _data: Message,
    ) -> Result<(), BehaviorError> {
        if self.state.status == "running" {
            self.state.is_paused = true;
            self.state.status = "paused".to_string();
            warn!("Pipeline paused at step: {}", self.state.current_step);
            println!("  ⏸ Pipeline paused at: {}", self.state.current_step);
        }
        Ok(())
    }

    /// Resume the pipeline execution
    #[signal_handler("resume")]
    async fn on_resume(
        &mut self,
        _ctx: &ActorContext,
        _data: Message,
    ) -> Result<(), BehaviorError> {
        if self.state.is_paused {
            self.state.is_paused = false;
            self.state.status = "running".to_string();
            info!("Pipeline resumed from step: {}", self.state.current_step);
            println!("  ▶ Pipeline resumed from: {}", self.state.current_step);
        }
        Ok(())
    }

    /// Cancel the pipeline execution
    #[signal_handler("cancel")]
    async fn on_cancel(
        &mut self,
        _ctx: &ActorContext,
        data: Message,
    ) -> Result<(), BehaviorError> {
        #[derive(Deserialize)]
        struct CancelPayload {
            reason: String,
        }

        let payload: CancelPayload = serde_json::from_slice(&data.payload)
            .unwrap_or(CancelPayload { reason: "User requested".to_string() });

        self.state.status = "cancelled".to_string();
        self.state.error = Some(payload.reason.clone());
        self.state.completed_at = Some(Utc::now());

        warn!("Pipeline cancelled: {}", payload.reason);
        println!("  ✗ Pipeline cancelled: {}", payload.reason);

        Ok(())
    }

    /// Query current pipeline status
    #[query_handler("status")]
    async fn get_status(
        &self,
        _ctx: &ActorContext,
        _params: Message,
    ) -> Result<Message, BehaviorError> {
        #[derive(Serialize)]
        struct StatusResponse {
            sample_id: String,
            status: String,
            current_step: String,
            is_paused: bool,
        }

        let response = StatusResponse {
            sample_id: self.state.sample_id.clone(),
            status: self.state.status.clone(),
            current_step: self.state.current_step.clone(),
            is_paused: self.state.is_paused,
        };

        let payload = serde_json::to_vec(&response)
            .map_err(|e| BehaviorError::ProcessingError(e.to_string()))?;

        Ok(Message {
            id: ulid::Ulid::new().to_string(),
            payload,
            ..Default::default()
        })
    }

    /// Query detailed progress
    #[query_handler("progress")]
    async fn get_progress(
        &self,
        _ctx: &ActorContext,
        _params: Message,
    ) -> Result<Message, BehaviorError> {
        #[derive(Serialize)]
        struct ProgressResponse {
            sample_id: String,
            status: String,
            current_step: String,
            steps_completed: Vec<String>,
            qc_passed: Option<usize>,
            aligned_reads: Option<usize>,
            variants_found: Option<usize>,
        }

        let mut steps_completed = Vec::new();
        if self.state.qc_result.is_some() {
            steps_completed.push("quality_control".to_string());
        }
        if self.state.alignment_result.is_some() {
            steps_completed.push("alignment".to_string());
        }
        if self.state.variant_result.is_some() {
            steps_completed.push("variant_calling".to_string());
        }

        let response = ProgressResponse {
            sample_id: self.state.sample_id.clone(),
            status: self.state.status.clone(),
            current_step: self.state.current_step.clone(),
            steps_completed,
            qc_passed: self.state.qc_result.as_ref().map(|r| r.passed_reads),
            aligned_reads: self.state.alignment_result.as_ref().map(|r| r.aligned_reads),
            variants_found: self.state.variant_result.as_ref().map(|r| r.total_variants),
        };

        let payload = serde_json::to_vec(&response)
            .map_err(|e| BehaviorError::ProcessingError(e.to_string()))?;

        Ok(Message {
            id: ulid::Ulid::new().to_string(),
            payload,
            ..Default::default()
        })
    }

    /// Query performance metrics
    #[query_handler("metrics")]
    async fn get_metrics(
        &self,
        _ctx: &ActorContext,
        _params: Message,
    ) -> Result<Message, BehaviorError> {
        #[derive(Serialize)]
        struct MetricsResponse {
            compute_time_ms: u64,
            coordinate_time_ms: u64,
            total_time_ms: u64,
            granularity_ratio: f64,
            efficiency: f64,
        }

        let total = self.state.compute_time_ms + self.state.coordinate_time_ms;
        let ratio = if self.state.coordinate_time_ms > 0 {
            self.state.compute_time_ms as f64 / self.state.coordinate_time_ms as f64
        } else {
            0.0
        };
        let efficiency = if total > 0 {
            self.state.compute_time_ms as f64 / total as f64
        } else {
            0.0
        };

        let response = MetricsResponse {
            compute_time_ms: self.state.compute_time_ms,
            coordinate_time_ms: self.state.coordinate_time_ms,
            total_time_ms: total,
            granularity_ratio: ratio,
            efficiency,
        };

        let payload = serde_json::to_vec(&response)
            .map_err(|e| BehaviorError::ProcessingError(e.to_string()))?;

        Ok(Message {
            id: ulid::Ulid::new().to_string(),
            payload,
            ..Default::default()
        })
    }

    fn create_result_message(&self) -> Result<Message, BehaviorError> {
        let payload = serde_json::to_vec(&self.state)
            .map_err(|e| BehaviorError::ProcessingError(e.to_string()))?;

        Ok(Message {
            id: ulid::Ulid::new().to_string(),
            payload,
            ..Default::default()
        })
    }
}

// =============================================================================
// Main - Demonstrates the workflow
// =============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Initialize tracing - suppress all framework logs to show clean example output
    // Use "off" for framework crates to completely silence them
    tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .with_env_filter("genomics_pipeline=info,plexspaces=off,plexspaces_node=off,plexspaces_core=off,plexspaces_actor=off,plexspaces_behavior=off")
        .init();

    println!();
    println!("╔════════════════════════════════════════════════════════════════════════╗");
    println!("║                                                                        ║");
    println!("║     🧬 GENOMICS PIPELINE - DNA Sequencing Workflow                     ║");
    println!("║                                                                        ║");
    println!("║     Real-world use case: GATK/Illumina-inspired analysis               ║");
    println!("║     Demonstrates: #[workflow_actor] with signals and queries           ║");
    println!("║                                                                        ║");
    println!("╚════════════════════════════════════════════════════════════════════════╝");
    println!();

    // =========================================================================
    // Infrastructure Setup (hidden from user - just shows result)
    // =========================================================================
    print!("🔧 Initializing PlexSpaces node... ");
    std::io::Write::flush(&mut std::io::stdout()).unwrap();

    let node = NodeBuilder::new("genomics-node")
        .with_clustering_enabled(false)
        .build().await;
    let node = Arc::new(node);

    let node_for_start = node.clone();
    tokio::spawn(async move {
        let _ = node_for_start.start().await;
    });
    tokio::time::sleep(Duration::from_millis(200)).await;
    println!("✓");

    let ctx = RequestContext::new_without_auth(
        "biotech-lab".to_string(),
        "sequencing".to_string(),
    );

    // Spawn workflow actor
    print!("🧬 Spawning workflow actor... ");
    std::io::Write::flush(&mut std::io::stdout()).unwrap();
    
    let workflow: WorkflowRef = spawn_workflow_actor(
        &ctx,
        node.service_locator(),
        "sample-001-pipeline@genomics-node",
        GenomicsPipelineWorkflow::new(),
        vec![], // Could add DurabilityFacet for crash recovery
    ).await?;
    println!("✓");
    println!();

    // =========================================================================
    // Run Pipeline - This is the interesting part!
    // =========================================================================
    println!("┌────────────────────────────────────────────────────────────────────────┐");
    println!("│  PIPELINE INPUT                                                        │");
    println!("├────────────────────────────────────────────────────────────────────────┤");

    let pipeline_input = PipelineInput {
        sample_id: "SAMPLE-2025-001".to_string(),
        num_reads: 100_000,  // More reads for realistic demo
        reference_genome: "hg38".to_string(),
        min_quality_score: 20,
    };

    println!("│  Sample ID:        {}                                    │", pipeline_input.sample_id);
    println!("│  Input Reads:      {:>10} (simulated FASTQ)                      │", pipeline_input.num_reads);
    println!("│  Reference Genome: {} (Human Genome Assembly)                      │", pipeline_input.reference_genome);
    println!("│  Min Quality:      {} (Phred score threshold)                        │", pipeline_input.min_quality_score);
    println!("└────────────────────────────────────────────────────────────────────────┘");
    println!();

    println!("▶ Starting pipeline via workflow.run(&input)...");
    println!();

    // Run the workflow with typed API
    let start_time = std::time::Instant::now();
    let final_state: PipelineState = workflow.run(&pipeline_input).await
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
    let wall_time = start_time.elapsed();

    // =========================================================================
    // Query Results using WorkflowRef API
    // =========================================================================
    println!();
    println!("▶ Querying workflow state via workflow.query(\"metrics\")...");
    println!();

    // Query metrics
    let metrics: serde_json::Value = workflow.query("metrics").await
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;

    // Query progress
    let progress: serde_json::Value = workflow.query("progress").await
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;

    println!("┌────────────────────────────────────────────────────────────────────────┐");
    println!("│  PIPELINE RESULTS                                                      │");
    println!("├────────────────────────────────────────────────────────────────────────┤");
    println!("│  Sample:           {}                                    │", final_state.sample_id);
    println!("│  Status:           {} ✓                                          │", final_state.status);
    // Format steps as comma-separated list
    let steps: Vec<String> = progress["steps_completed"]
        .as_array()
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    println!("│  Steps Completed:  {}                 │", steps.join(" → "));
    println!("├────────────────────────────────────────────────────────────────────────┤");
    println!("│  QC Results:                                                           │");
    if let Some(qc) = &final_state.qc_result {
        println!("│    • Reads passed:     {:>10} / {:>10} ({:.1}%)                   │", 
            qc.passed_reads, qc.total_reads, 
            qc.passed_reads as f64 / qc.total_reads as f64 * 100.0);
        println!("│    • Avg quality:      {:>10.1} Phred                                │", qc.avg_quality);
        println!("│    • GC content:       {:>10.1}%                                     │", qc.gc_content * 100.0);
    }
    println!("├────────────────────────────────────────────────────────────────────────┤");
    println!("│  Alignment Results:                                                    │");
    if let Some(align) = &final_state.alignment_result {
        println!("│    • Aligned reads:    {:>10} ({:.1}% rate)                         │", 
            align.aligned_reads, align.alignment_rate * 100.0);
        println!("│    • Mapping quality:  {:>10.1}                                      │", align.avg_mapping_quality);
        println!("│    • Reference:        {}                                          │", align.reference_genome);
    }
    println!("├────────────────────────────────────────────────────────────────────────┤");
    println!("│  Variant Calling Results:                                              │");
    if let Some(var) = &final_state.variant_result {
        println!("│    • Total variants:   {:>10}                                       │", var.total_variants);
        println!("│    • SNPs:             {:>10} ({:.0}%)                               │", 
            var.snps, var.snps as f64 / var.total_variants as f64 * 100.0);
        println!("│    • Indels:           {:>10} ({:.0}%)                               │", 
            var.indels, var.indels as f64 / var.total_variants as f64 * 100.0);
        println!("│    • Novel variants:   {:>10}                                       │", var.novel_variants);
        println!("│    • Known (dbSNP):    {:>10}                                       │", var.known_variants);
    }
    println!("├────────────────────────────────────────────────────────────────────────┤");
    println!("│  Performance Metrics:                                                  │");
    println!("│    • Compute time:     {:>10} ms (actual processing)                 │", metrics["compute_time_ms"]);
    println!("│    • Coordinate time:  {:>10} ms (framework overhead)                │", metrics["coordinate_time_ms"]);
    println!("│    • Total time:       {:>10} ms                                     │", metrics["total_time_ms"]);
    println!("│    • Wall clock:       {:>10} ms                                     │", wall_time.as_millis());
    println!("│    • Efficiency:       {:>10.1}% (compute / total)                   │", metrics["efficiency"].as_f64().unwrap_or(0.0) * 100.0);
    println!("│    • Granularity:      {:>10.1}× (compute / coordinate)              │", metrics["granularity_ratio"].as_f64().unwrap_or(0.0));
    println!("└────────────────────────────────────────────────────────────────────────┘");
    println!();

    // =========================================================================
    // Summary
    // =========================================================================
    println!("┌────────────────────────────────────────────────────────────────────────┐");
    println!("│  SDK PATTERNS DEMONSTRATED                                             │");
    println!("├────────────────────────────────────────────────────────────────────────┤");
    println!("│  Workflow Actor:     #[workflow_actor]                                 │");
    println!("│  Run Handler:        #[run_handler] - main pipeline execution          │");
    println!("│  Signal Handlers:    #[signal_handler(\"pause/resume/cancel\")]          │");
    println!("│  Query Handlers:     #[query_handler(\"status/progress/metrics\")]       │");
    println!("│  Spawn Helper:       spawn_workflow_actor(ctx, locator, id, behavior)  │");
    println!("│  Typed API:          WorkflowRef::run(), signal(), query()             │");
    println!("└────────────────────────────────────────────────────────────────────────┘");
    println!();

    // Graceful shutdown (silent)
    node.shutdown(Duration::from_secs(2)).await?;

    Ok(())
}
