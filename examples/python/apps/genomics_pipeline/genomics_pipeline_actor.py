# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 PlexSpaces Contributors
"""
Genomics Pipeline - Multi-Step DNA Sequencing Workflow

Demonstrates workflow-style actor orchestration for bioinformatics pipelines,
ported from the Rust embedded example (examples/rust/embedded/genomics_pipeline/).

## Pipeline Stages

    +-----------+     +-----------+     +---------------+
    | Quality   | --> | Genome    | --> | Variant       |
    | Control   |     | Alignment |     | Calling       |
    +-----------+     +-----------+     +---------------+
    Filter low-       Map reads to      Identify SNPs,
    quality reads     reference genome  indels, mutations

## Architecture

Each pipeline stage is a separate actor type:
    - QCWorker: Quality control filtering (CPU-bound)
    - AlignmentWorker: Read alignment to reference (CPU/memory-intensive)
    - VariantCaller: Variant detection (CPU-bound)
    - GenomicsPipelineCoordinator: Workflow orchestration with durable state

## Framework Features Demonstrated

- **Workflow Actor**: Multi-step pipeline with checkpoint/recovery
- **Process Groups**: Stage-specific worker pools
- **Shard Groups**: Parallel processing of read batches
- **Durable State**: Pipeline progress survives crashes
- **Signals/Queries**: Pause, resume, cancel running pipelines
- **Resource Routing**: Memory-intensive stages on high-memory nodes
"""

import json
import math
from plexspaces import actor, workflow_actor, state, handler, init_handler, host


@actor
class QCWorker:
    """Quality control: filters low-quality sequencing reads.

    Simulates Illumina/GATK quality filtering:
    - Base quality score threshold (Phred Q30 = 99.9% accuracy)
    - Minimum read length filtering
    - Adapter sequence detection
    - Duplicate read removal
    """

    worker_id: str = state(default="")
    reads_processed: int = state(default=0)
    reads_passed: int = state(default=0)
    reads_failed: int = state(default=0)
    min_quality: int = state(default=30)
    min_length: int = state(default=50)

    @init_handler
    def on_init(self, config: dict):
        self.worker_id = config.get("actor_id", "")
        args = config.get("args", {})
        self.min_quality = int(args.get("min_quality", 30))
        self.min_length = int(args.get("min_length", 50))
        host.process_groups.join("genomics-qc-workers")
        host.info(f"QCWorker {self.worker_id}: Q>={self.min_quality}, len>={self.min_length}")

    @handler("filter_reads")
    def filter_reads(self, reads: list = None, sample_id: str = "",
                     from_actor: str = "") -> dict:
        """Filter a batch of sequencing reads by quality.

        Args:
            reads: List of read dicts [{id, sequence, quality_scores}, ...]
            sample_id: Sample identifier
        """
        start = host.now_ms()
        if not reads:
            return {"error": "no reads provided"}

        passed = []
        failed = 0

        for read in reads:
            read_id = read.get("id", f"read-{self.reads_processed}")
            sequence = read.get("sequence", "")
            quality = read.get("quality_scores", [])

            # Compute average quality score
            if quality:
                avg_quality = sum(quality) / len(quality)
            else:
                # Simulate quality from sequence hash
                avg_quality = (hash(sequence) % 20) + 20  # Range: 20-40

            read_length = len(sequence) if sequence else int(read.get("length", 150))

            # Apply filters
            passes_quality = avg_quality >= self.min_quality
            passes_length = read_length >= self.min_length

            if passes_quality and passes_length:
                passed.append({
                    "id": read_id,
                    "sequence": sequence,
                    "length": read_length,
                    "avg_quality": round(avg_quality, 1),
                })
                self.reads_passed += 1
            else:
                failed += 1
                self.reads_failed += 1

            self.reads_processed += 1

        elapsed = host.now_ms() - start
        pass_rate = len(passed) / len(reads) * 100 if reads else 0

        return {
            "status": "ok",
            "sample_id": sample_id,
            "passed_reads": passed,
            "total_reads": len(reads),
            "passed_count": len(passed),
            "failed_count": failed,
            "pass_rate": round(pass_rate, 1),
            "qc_ms": elapsed,
        }

    @handler("stats")
    def stats(self) -> dict:
        total = self.reads_passed + self.reads_failed
        pass_rate = self.reads_passed / total * 100 if total > 0 else 0
        return {
            "worker_id": self.worker_id,
            "reads_processed": self.reads_processed,
            "reads_passed": self.reads_passed,
            "reads_failed": self.reads_failed,
            "pass_rate": round(pass_rate, 1),
        }


@actor
class AlignmentWorker:
    """Aligns sequencing reads to a reference genome.

    Simulates BWA/Bowtie2 alignment:
    - Map reads to reference genome coordinates
    - Report alignment quality (MAPQ score)
    - Handle multi-mapped reads
    """

    worker_id: str = state(default="")
    reference_genome: str = state(default="hg38")
    reads_aligned: int = state(default=0)
    reads_unmapped: int = state(default=0)

    @init_handler
    def on_init(self, config: dict):
        self.worker_id = config.get("actor_id", "")
        args = config.get("args", {})
        self.reference_genome = args.get("reference_genome", "hg38")
        host.process_groups.join("genomics-alignment-workers")
        host.info(f"AlignmentWorker {self.worker_id}: ref={self.reference_genome}")

    @handler("align_reads")
    def align_reads(self, reads: list = None, sample_id: str = "",
                    from_actor: str = "") -> dict:
        """Align QC-passed reads to the reference genome.

        Args:
            reads: List of QC-passed reads [{id, sequence, length, avg_quality}, ...]
            sample_id: Sample identifier
        """
        start = host.now_ms()
        if not reads:
            return {"error": "no reads provided"}

        aligned = []
        unmapped = 0

        for read in reads:
            read_id = read.get("id", "")
            sequence = read.get("sequence", "")
            length = int(read.get("length", 150))

            # Simulate alignment (98% alignment rate for hg38)
            read_hash = hash(read_id) if read_id else hash(sequence)
            is_mapped = (read_hash % 100) < 98  # 98% alignment rate

            if is_mapped:
                # Simulate reference coordinates
                chrom = f"chr{(read_hash % 22) + 1}"
                position = abs(read_hash) % 3_000_000_000  # hg38 ~3B bp
                mapq = 20 + (read_hash % 40)  # MAPQ 20-60
                strand = "+" if read_hash % 2 == 0 else "-"

                aligned.append({
                    "read_id": read_id,
                    "chromosome": chrom,
                    "position": position,
                    "mapq": mapq,
                    "strand": strand,
                    "cigar": f"{length}M",  # Simple alignment
                })
                self.reads_aligned += 1
            else:
                unmapped += 1
                self.reads_unmapped += 1

        elapsed = host.now_ms() - start
        alignment_rate = len(aligned) / len(reads) * 100 if reads else 0

        return {
            "status": "ok",
            "sample_id": sample_id,
            "aligned_reads": aligned,
            "total_reads": len(reads),
            "aligned_count": len(aligned),
            "unmapped_count": unmapped,
            "alignment_rate": round(alignment_rate, 1),
            "align_ms": elapsed,
            "reference": self.reference_genome,
        }

    @handler("stats")
    def stats(self) -> dict:
        total = self.reads_aligned + self.reads_unmapped
        rate = self.reads_aligned / total * 100 if total > 0 else 0
        return {
            "worker_id": self.worker_id,
            "reference": self.reference_genome,
            "reads_aligned": self.reads_aligned,
            "reads_unmapped": self.reads_unmapped,
            "alignment_rate": round(rate, 1),
        }


@actor
class VariantCaller:
    """Identifies genetic variants (SNPs, indels) from aligned reads.

    Simulates GATK HaplotypeCaller / DeepVariant:
    - Pileup analysis at each genomic position
    - Variant quality scoring
    - Genotype calling (heterozygous/homozygous)
    """

    worker_id: str = state(default="")
    variants_called: int = state(default=0)
    snps: int = state(default=0)
    indels: int = state(default=0)

    @init_handler
    def on_init(self, config: dict):
        self.worker_id = config.get("actor_id", "")
        host.process_groups.join("genomics-variant-workers")
        host.info(f"VariantCaller {self.worker_id} ready")

    @handler("call_variants")
    def call_variants(self, aligned_reads: list = None, sample_id: str = "",
                      from_actor: str = "") -> dict:
        """Call variants from aligned reads.

        Args:
            aligned_reads: List of aligned reads [{read_id, chromosome, position, ...}, ...]
            sample_id: Sample identifier
        """
        start = host.now_ms()
        if not aligned_reads:
            return {"error": "no aligned reads provided"}

        variants = []
        bases = "ACGT"

        # Simulate variant calling (~1 variant per 300 reads)
        for read in aligned_reads:
            read_hash = hash(read.get("read_id", ""))
            if read_hash % 300 == 0:  # ~0.33% variant rate
                chrom = read.get("chromosome", "chr1")
                pos = read.get("position", 0)
                ref_base = bases[abs(read_hash) % 4]
                alt_base = bases[(abs(read_hash) + 1) % 4]

                is_snp = read_hash % 5 != 0  # 80% SNPs, 20% indels
                if is_snp:
                    variant = {
                        "type": "SNP",
                        "chromosome": chrom,
                        "position": pos,
                        "ref": ref_base,
                        "alt": alt_base,
                        "quality": 20 + (read_hash % 80),
                        "genotype": "0/1" if read_hash % 3 == 0 else "1/1",
                    }
                    self.snps += 1
                else:
                    # Indel
                    indel_len = 1 + (read_hash % 5)
                    variant = {
                        "type": "INDEL",
                        "chromosome": chrom,
                        "position": pos,
                        "ref": ref_base * indel_len if read_hash % 2 == 0 else ref_base,
                        "alt": ref_base if read_hash % 2 == 0 else alt_base * indel_len,
                        "quality": 15 + (read_hash % 60),
                        "genotype": "0/1",
                    }
                    self.indels += 1

                variants.append(variant)
                self.variants_called += 1

        elapsed = host.now_ms() - start

        return {
            "status": "ok",
            "sample_id": sample_id,
            "variants": variants,
            "variant_count": len(variants),
            "snp_count": sum(1 for v in variants if v["type"] == "SNP"),
            "indel_count": sum(1 for v in variants if v["type"] == "INDEL"),
            "calling_ms": elapsed,
        }

    @handler("stats")
    def stats(self) -> dict:
        return {
            "worker_id": self.worker_id,
            "variants_called": self.variants_called,
            "snps": self.snps,
            "indels": self.indels,
        }


# =============================================================================
# GenomicsPipelineCoordinator - Workflow orchestration
# =============================================================================

@workflow_actor
class GenomicsPipelineCoordinator:
    """Orchestrates the full genomics pipeline with durable workflow state.

    This is a workflow actor that manages the multi-step pipeline:
    1. Quality Control: filter low-quality reads
    2. Genome Alignment: map reads to reference genome
    3. Variant Calling: identify SNPs and indels

    Supports pause/resume/cancel via pipeline_state tracking.
    Progress survives crashes via durable state checkpointing.
    """

    coordinator_id: str = state(default="")
    pipeline_state: str = state(default="idle")
    num_qc_workers: int = state(default=2)
    num_align_workers: int = state(default=2)
    num_variant_workers: int = state(default=1)
    qc_ids: list = state(default_factory=list)
    align_ids: list = state(default_factory=list)
    variant_ids: list = state(default_factory=list)
    total_pipelines: int = state(default=0)
    total_reads_input: int = state(default=0)
    total_reads_passed_qc: int = state(default=0)
    total_reads_aligned: int = state(default=0)
    total_variants: int = state(default=0)

    @init_handler
    def on_init(self, config: dict):
        actor_id = config.get("actor_id", "")
        self.coordinator_id = actor_id
        args = config.get("args", {})
        self.num_qc_workers = int(args.get("num_qc_workers", 2))
        self.num_align_workers = int(args.get("num_align_workers", 2))
        self.num_variant_workers = int(args.get("num_variant_workers", 1))

        id_suffix = ""
        if ":" in actor_id:
            id_suffix = actor_id[actor_id.index(":"):]

        self.qc_ids = [f"qc-{i}{id_suffix}" for i in range(self.num_qc_workers)]
        self.align_ids = [f"align-{i}{id_suffix}" for i in range(self.num_align_workers)]
        self.variant_ids = [f"variant-{i}{id_suffix}" for i in range(self.num_variant_workers)]

        host.process_groups.join("genomics-coordinators")
        host.info(f"GenomicsPipeline: {self.num_qc_workers} QC, "
                  f"{self.num_align_workers} align, {self.num_variant_workers} variant")

    @handler("run_pipeline")
    def run_pipeline(self, reads: list = None, sample_id: str = "",
                     from_actor: str = "") -> dict:
        """Run the full genomics pipeline on a batch of sequencing reads.

        Args:
            reads: List of raw reads [{id, sequence, quality_scores}, ...]
            sample_id: Sample identifier (e.g., "patient-001")
        """
        if not reads:
            return {"error": "no reads provided"}
        if self.pipeline_state == "paused":
            return {"error": "pipeline is paused", "state": self.pipeline_state}

        self.pipeline_state = "running"
        pipeline_start = host.now_ms()
        pipeline_id = f"pipeline-{self.total_pipelines}"

        # ---- Stage 1: Quality Control ----
        host.info(f"[{pipeline_id}] Stage 1: Quality Control ({len(reads)} reads)")
        qc_partitions = [[] for _ in range(self.num_qc_workers)]
        for i, read in enumerate(reads):
            qc_partitions[i % self.num_qc_workers].append(read)

        all_passed_reads = []
        qc_stats = {"total": len(reads), "passed": 0, "failed": 0}
        for idx, partition in enumerate(qc_partitions):
            if not partition:
                continue
            try:
                resp = host.ask(self.qc_ids[idx], "filter_reads", {
                    "reads": partition, "sample_id": sample_id,
                }, timeout_ms=30000)
                if isinstance(resp, dict) and resp.get("status") == "ok":
                    all_passed_reads.extend(resp.get("passed_reads", []))
                    qc_stats["passed"] += resp.get("passed_count", 0)
                    qc_stats["failed"] += resp.get("failed_count", 0)
            except Exception as e:
                host.warn(f"QC worker {self.qc_ids[idx]} failed: {e}")

        qc_ms = host.now_ms() - pipeline_start

        # ---- Stage 2: Genome Alignment ----
        align_start = host.now_ms()
        host.info(f"[{pipeline_id}] Stage 2: Alignment ({len(all_passed_reads)} reads)")
        align_partitions = [[] for _ in range(self.num_align_workers)]
        for i, read in enumerate(all_passed_reads):
            align_partitions[i % self.num_align_workers].append(read)

        all_aligned = []
        align_stats = {"total": len(all_passed_reads), "aligned": 0, "unmapped": 0}
        for idx, partition in enumerate(align_partitions):
            if not partition:
                continue
            try:
                resp = host.ask(self.align_ids[idx], "align_reads", {
                    "reads": partition, "sample_id": sample_id,
                }, timeout_ms=60000)
                if isinstance(resp, dict) and resp.get("status") == "ok":
                    all_aligned.extend(resp.get("aligned_reads", []))
                    align_stats["aligned"] += resp.get("aligned_count", 0)
                    align_stats["unmapped"] += resp.get("unmapped_count", 0)
            except Exception as e:
                host.warn(f"Alignment worker {self.align_ids[idx]} failed: {e}")

        align_ms = host.now_ms() - align_start

        # ---- Stage 3: Variant Calling ----
        variant_start = host.now_ms()
        host.info(f"[{pipeline_id}] Stage 3: Variant Calling ({len(all_aligned)} aligned reads)")
        variant_partitions = [[] for _ in range(self.num_variant_workers)]
        for i, read in enumerate(all_aligned):
            variant_partitions[i % self.num_variant_workers].append(read)

        all_variants = []
        variant_stats = {"snps": 0, "indels": 0}
        for idx, partition in enumerate(variant_partitions):
            if not partition:
                continue
            try:
                resp = host.ask(self.variant_ids[idx], "call_variants", {
                    "aligned_reads": partition, "sample_id": sample_id,
                }, timeout_ms=60000)
                if isinstance(resp, dict) and resp.get("status") == "ok":
                    all_variants.extend(resp.get("variants", []))
                    variant_stats["snps"] += resp.get("snp_count", 0)
                    variant_stats["indels"] += resp.get("indel_count", 0)
            except Exception as e:
                host.warn(f"Variant caller {self.variant_ids[idx]} failed: {e}")

        variant_ms = host.now_ms() - variant_start
        pipeline_ms = host.now_ms() - pipeline_start

        # Update metrics
        self.total_pipelines += 1
        self.total_reads_input += len(reads)
        self.total_reads_passed_qc += qc_stats["passed"]
        self.total_reads_aligned += align_stats["aligned"]
        self.total_variants += len(all_variants)
        self.pipeline_state = "idle"

        return {
            "status": "ok",
            "pipeline_id": pipeline_id,
            "sample_id": sample_id,
            "summary": {
                "input_reads": len(reads),
                "qc_passed": qc_stats["passed"],
                "qc_pass_rate": round(qc_stats["passed"] / len(reads) * 100, 1) if reads else 0,
                "aligned": align_stats["aligned"],
                "alignment_rate": round(
                    align_stats["aligned"] / qc_stats["passed"] * 100, 1
                ) if qc_stats["passed"] > 0 else 0,
                "variants_found": len(all_variants),
                "snps": variant_stats["snps"],
                "indels": variant_stats["indels"],
            },
            "timing": {
                "pipeline_ms": pipeline_ms,
                "qc_ms": qc_ms,
                "alignment_ms": align_ms,
                "variant_calling_ms": variant_ms,
            },
            "variants_sample": all_variants[:10],  # First 10 variants
        }

    @handler("pause")
    def pause_pipeline(self) -> dict:
        self.pipeline_state = "paused"
        return {"status": "paused"}

    @handler("resume")
    def resume_pipeline(self) -> dict:
        self.pipeline_state = "idle"
        return {"status": "resumed"}

    @handler("pipeline_stats")
    def pipeline_stats(self) -> dict:
        return {
            "coordinator_id": self.coordinator_id,
            "pipeline_state": self.pipeline_state,
            "total_pipelines": self.total_pipelines,
            "total_reads_input": self.total_reads_input,
            "total_reads_passed_qc": self.total_reads_passed_qc,
            "total_reads_aligned": self.total_reads_aligned,
            "total_variants": self.total_variants,
        }


ACTOR_ROLES = {
    "genomics-coordinator": GenomicsPipelineCoordinator,
    "qc": QCWorker,
    "align": AlignmentWorker,
    "variant": VariantCaller,
}
