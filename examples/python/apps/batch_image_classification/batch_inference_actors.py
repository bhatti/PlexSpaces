# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 PlexSpaces Contributors
"""
Batch Image Classification - GPU-Accelerated AI Inference Pipeline

Demonstrates PlexSpaces data-parallel architecture for AI batch inference,
inspired by Ray Data's HuggingFace ViT and PyTorch ResNet examples.

## Architecture

    +------------------+
    |   Coordinator    |  Orchestrates the pipeline
    +--------+---------+
             |
    +--------v---------+     Shard Group (hash-partitioned)
    |  Preprocessor    |     Resource: {accelerator: "cpu"}
    |  Pool (N shards) |     Resize, normalize, batch images
    +--------+---------+
             |
    +--------v---------+     Shard Group (hash-partitioned)
    |  Inference        |     Resource: {accelerator: "gpu", gpu_type: "nvidia"}
    |  Pool (M shards)  |     Model forward pass on GPU workers
    +--------+---------+
             |
    +--------v---------+
    |   Aggregator     |     Collects results, computes metrics
    +------------------+

## Framework Features Demonstrated

- **Shard Groups**: Hash-partitioned worker pools for data parallelism
- **Resource-Based Routing**: GPU workers for inference, CPU for preprocessing
- **Worker Pools**: Stateless workers with load balancing
- **Process Groups**: Pub/sub for pipeline stage notifications
- **Channels**: Queue-based task distribution (SQS-style)
- **host.ask()**: Request-reply for scatter-gather coordination

## SDK Features Used

- @actor: Marks classes as PlexSpaces actors
- state(): Defines persistent state
- @handler(): Routes operations by message type
- @init_handler: Config-based initialization
- host.ask(): Inter-actor request-reply
- host.send(): Fire-and-forget messaging
- host.process_groups: Pub/sub coordination
- host.kv_put/kv_get: Shared metadata
- host.blob_upload/blob_download: Large data (image batches)
- host.now_ms(): Timing for benchmarks
- ACTOR_ROLES: Multi-actor module support

## Comparison with Ray Data

Ray Data:
    ds = ray.data.read_images("s3://bucket/images/")
    ds.map_batches(Preprocessor, num_cpus=2)
    ds.map_batches(ModelInference, num_gpus=1, batch_size=64)

PlexSpaces equivalent:
    - Shard group of Preprocessor actors (cpu-labeled nodes)
    - Shard group of Inference actors (gpu-labeled nodes)
    - Coordinator orchestrates via host.ask() scatter-gather
    - Results gathered via TupleSpace or process group broadcast
"""

import json
import math
from typing import List, Dict, Any, Optional
from plexspaces import actor, state, handler, init_handler, host


# =============================================================================
# ImagePreprocessor - CPU-bound image preprocessing (runs on CPU nodes)
# =============================================================================

@actor
class ImagePreprocessor:
    """Preprocesses image batches: resize, normalize, convert to tensor format.

    Deployed on CPU-labeled nodes via resource-based routing:
        required_labels: {accelerator: "cpu", role: "preprocessor"}

    Each preprocessor is a shard in a hash-partitioned shard group.
    Images are routed to shards by hash(image_id) % shard_count.
    """

    worker_id: str = state(default="")
    shard_id: int = state(default=0)
    images_processed: int = state(default=0)
    total_preprocess_ms: float = state(default=0.0)
    target_size: list = state(default_factory=lambda: [224, 224])
    # ImageNet normalization constants
    mean: list = state(default_factory=lambda: [0.485, 0.456, 0.406])
    std: list = state(default_factory=lambda: [0.229, 0.224, 0.225])

    @init_handler
    def on_init(self, config: dict):
        actor_id = config.get("actor_id", "")
        self.worker_id = actor_id
        args = config.get("args", {})
        self.shard_id = int(args.get("shard_id", 0))
        size = args.get("target_size", None)
        if size:
            self.target_size = [int(size), int(size)]
        self.images_processed = 0
        self.total_preprocess_ms = 0.0

        # Join the preprocessors process group for pipeline notifications
        host.process_groups.join("pipeline-preprocessors")
        host.info(f"ImagePreprocessor shard {self.shard_id} initialized "
                  f"(target={self.target_size[0]}x{self.target_size[1]})")

    @handler("preprocess_batch")
    def preprocess_batch(self, images: list = None,
                         batch_id: str = "", from_actor: str = "") -> dict:
        """Preprocess a batch of images: simulate resize + normalize.

        In a real deployment, images would be raw pixel data from blob storage.
        Here we simulate the preprocessing pipeline that prepares images for
        model inference (similar to torchvision.transforms).

        Args:
            images: List of image descriptors [{id, width, height, channels, data}, ...]
            batch_id: Unique batch identifier for tracking
        """
        start = host.now_ms()
        if not images:
            return {"error": "no images provided", "batch_id": batch_id}

        preprocessed = []
        for img in images:
            img_id = img.get("id", f"img-{self.images_processed}")
            width = int(img.get("width", 640))
            height = int(img.get("height", 480))
            channels = int(img.get("channels", 3))

            # Simulate resize to target_size (bilinear interpolation)
            target_h, target_w = self.target_size
            scale_x = width / target_w
            scale_y = height / target_h

            # Generate normalized tensor values (simulated)
            # In production: actual pixel resize + ImageNet normalization
            tensor = []
            for c in range(channels):
                channel_data = []
                for y in range(target_h):
                    row = []
                    for x in range(target_w):
                        # Simulate normalized pixel value
                        raw = ((img_id.__hash__() + y * 7 + x * 13 + c * 31) % 256) / 255.0
                        normalized = (raw - self.mean[c]) / self.std[c]
                        row.append(round(normalized, 4))
                    channel_data.append(row)
                tensor.append(channel_data)

            preprocessed.append({
                "id": img_id,
                "tensor_shape": [channels, target_h, target_w],
                "tensor_summary": {
                    "mean": round(sum(v for ch in tensor for row in ch for v in row) /
                                  (channels * target_h * target_w), 4),
                    "elements": channels * target_h * target_w,
                },
                "original_size": [height, width],
            })
            self.images_processed += 1

        elapsed = host.now_ms() - start
        self.total_preprocess_ms += elapsed

        return {
            "status": "ok",
            "batch_id": batch_id,
            "shard_id": self.shard_id,
            "preprocessed": preprocessed,
            "count": len(preprocessed),
            "preprocess_ms": elapsed,
        }

    @handler("stats")
    def stats(self) -> dict:
        avg_ms = (self.total_preprocess_ms / self.images_processed
                  if self.images_processed > 0 else 0)
        throughput = (self.images_processed * 1000.0 / self.total_preprocess_ms
                      if self.total_preprocess_ms > 0 else 0)
        return {
            "worker_id": self.worker_id,
            "shard_id": self.shard_id,
            "images_processed": self.images_processed,
            "total_preprocess_ms": self.total_preprocess_ms,
            "avg_ms_per_image": round(avg_ms, 2),
            "throughput_images_per_sec": round(throughput, 1),
        }


# =============================================================================
# ModelInferenceWorker - GPU-bound model inference (runs on GPU nodes)
# =============================================================================

@actor
class ModelInferenceWorker:
    """Runs model inference on preprocessed image batches.

    Deployed on GPU-labeled nodes via resource-based routing:
        required_labels: {accelerator: "gpu", gpu_type: "nvidia"}
        resources: {gpu_count: 1}

    Simulates a Vision Transformer (ViT) or ResNet classification model.
    Model weights are loaded once in init (expensive), inference runs per batch.

    This mirrors Ray Data's pattern:
        class ImageClassifier:
            def __init__(self):
                self.model = load_model()  # Once per worker
            def __call__(self, batch):
                return self.model(batch)   # Per batch
    """

    worker_id: str = state(default="")
    shard_id: int = state(default=0)
    model_name: str = state(default="vit-base-patch16-224")
    num_classes: int = state(default=1000)
    batch_size: int = state(default=64)
    batches_processed: int = state(default=0)
    images_classified: int = state(default=0)
    total_inference_ms: float = state(default=0.0)
    # Simulated model weights (in production: actual PyTorch/TF weights)
    model_loaded: bool = state(default=False)
    weight_checksum: float = state(default=0.0)

    # ImageNet class labels (top 10 for demo)
    CLASS_LABELS = [
        "tench", "goldfish", "great_white_shark", "tiger_shark", "hammerhead",
        "electric_ray", "stingray", "rooster", "hen", "ostrich",
        "brambling", "goldfinch", "house_finch", "junco", "indigo_bunting",
    ]

    @init_handler
    def on_init(self, config: dict):
        actor_id = config.get("actor_id", "")
        self.worker_id = actor_id
        args = config.get("args", {})
        self.shard_id = int(args.get("shard_id", 0))
        self.model_name = args.get("model_name", "vit-base-patch16-224")
        self.num_classes = int(args.get("num_classes", 1000))
        self.batch_size = int(args.get("batch_size", 64))

        # Simulate model loading (expensive operation, done once per worker)
        self._load_model()

        # Join GPU workers process group
        host.process_groups.join("pipeline-gpu-workers")
        host.info(f"ModelInferenceWorker shard {self.shard_id}: "
                  f"model={self.model_name}, classes={self.num_classes}")

    def _load_model(self):
        """Simulate loading model weights (in production: torch.load or transformers).

        In a real deployment:
            from transformers import ViTForImageClassification
            self.model = ViTForImageClassification.from_pretrained("google/vit-base-patch16-224")
            self.model.to("cuda")
            self.model.eval()
        """
        # Simulate weight initialization (86M params for ViT-Base)
        total_params = 86_000_000
        self.weight_checksum = sum(
            ((i * 7 + 42) % 1000 - 500) / 500.0
            for i in range(min(total_params, 10000))
        ) / 10000
        self.model_loaded = True
        host.info(f"Model {self.model_name} loaded ({total_params:,} params, "
                  f"checksum={self.weight_checksum:.4f})")

    @handler("classify_batch")
    def classify_batch(self, preprocessed: list = None,
                       batch_id: str = "", from_actor: str = "") -> dict:
        """Run inference on a batch of preprocessed images.

        Simulates ViT/ResNet forward pass: input tensor -> class probabilities.
        In production this would call model.forward() on GPU.

        Args:
            preprocessed: List of preprocessed image descriptors from ImagePreprocessor
            batch_id: Batch identifier for tracking
        """
        start = host.now_ms()
        if not self.model_loaded:
            self._load_model()
        if not preprocessed:
            return {"error": "no preprocessed images", "batch_id": batch_id}

        predictions = []
        for img in preprocessed:
            img_id = img.get("id", "unknown")
            elements = img.get("tensor_summary", {}).get("elements", 150528)

            # Simulate forward pass: softmax over num_classes logits
            # In production: logits = model(tensor.unsqueeze(0).cuda())
            logits = []
            for c in range(min(self.num_classes, 15)):
                logit = ((hash(img_id) + c * 17 + self.shard_id * 31) % 1000) / 100.0 - 5.0
                logits.append(logit)

            # Softmax
            max_logit = max(logits)
            exp_logits = [math.exp(l - max_logit) for l in logits]
            sum_exp = sum(exp_logits)
            probs = [e / sum_exp for e in exp_logits]

            # Top-5 predictions
            indexed = sorted(enumerate(probs), key=lambda x: -x[1])[:5]
            top5 = [
                {
                    "class_id": idx,
                    "label": self.CLASS_LABELS[idx] if idx < len(self.CLASS_LABELS) else f"class_{idx}",
                    "confidence": round(prob, 4),
                }
                for idx, prob in indexed
            ]

            predictions.append({
                "image_id": img_id,
                "top_prediction": top5[0],
                "top5": top5,
                "model": self.model_name,
            })
            self.images_classified += 1

        elapsed = host.now_ms() - start
        self.total_inference_ms += elapsed
        self.batches_processed += 1

        return {
            "status": "ok",
            "batch_id": batch_id,
            "shard_id": self.shard_id,
            "predictions": predictions,
            "count": len(predictions),
            "inference_ms": elapsed,
            "model": self.model_name,
        }

    @handler("stats")
    def stats(self) -> dict:
        avg_ms = (self.total_inference_ms / self.images_classified
                  if self.images_classified > 0 else 0)
        throughput = (self.images_classified * 1000.0 / self.total_inference_ms
                      if self.total_inference_ms > 0 else 0)
        return {
            "worker_id": self.worker_id,
            "shard_id": self.shard_id,
            "model": self.model_name,
            "model_loaded": self.model_loaded,
            "batches_processed": self.batches_processed,
            "images_classified": self.images_classified,
            "total_inference_ms": self.total_inference_ms,
            "avg_ms_per_image": round(avg_ms, 2),
            "throughput_images_per_sec": round(throughput, 1),
        }


# =============================================================================
# PipelineCoordinator - Orchestrates the end-to-end batch inference pipeline
# =============================================================================

@actor
class PipelineCoordinator:
    """Orchestrates the batch inference pipeline across preprocessing and inference stages.

    This actor demonstrates:
    1. **Scatter-gather** across shard groups (fan-out work, fan-in results)
    2. **Resource-aware routing** (CPU for preprocessing, GPU for inference)
    3. **Process groups** for pipeline stage pub/sub notifications
    4. **Worker pool management** with configurable parallelism
    5. **Pipeline metrics** (throughput, latency, coordination overhead)

    Pipeline flow:
        1. Receive image batch from client (via HTTP POST or host.ask)
        2. Partition images across preprocessor shard group (hash by image_id)
        3. Fan-out: ask each preprocessor shard to preprocess its partition
        4. Fan-in: collect preprocessed tensors
        5. Partition tensors across inference shard group
        6. Fan-out: ask each inference shard to classify its partition
        7. Fan-in: collect predictions
        8. Aggregate results and return to client
    """

    coordinator_id: str = state(default="")
    num_preprocessors: int = state(default=4)
    num_inference_workers: int = state(default=2)
    preprocessor_ids: list = state(default_factory=list)
    inference_worker_ids: list = state(default_factory=list)
    total_batches: int = state(default=0)
    total_images: int = state(default=0)
    total_pipeline_ms: float = state(default=0.0)
    total_coord_ms: float = state(default=0.0)
    total_preprocess_ms: float = state(default=0.0)
    total_inference_ms: float = state(default=0.0)

    @init_handler
    def on_init(self, config: dict):
        actor_id = config.get("actor_id", "")
        self.coordinator_id = actor_id
        args = config.get("args", {})
        self.num_preprocessors = int(args.get("num_preprocessors", 4))
        self.num_inference_workers = int(args.get("num_inference_workers", 2))

        # Build worker IDs (prefix match in ACTOR_ROLES)
        id_suffix = ""
        if ":" in actor_id:
            id_suffix = actor_id[actor_id.index(":"):]

        self.preprocessor_ids = [
            f"preprocessor-{i}{id_suffix}" for i in range(self.num_preprocessors)
        ]
        self.inference_worker_ids = [
            f"inference-{i}{id_suffix}" for i in range(self.num_inference_workers)
        ]

        # Join coordinator process group
        host.process_groups.join("pipeline-coordinators")
        host.info(f"PipelineCoordinator: {self.num_preprocessors} preprocessors, "
                  f"{self.num_inference_workers} inference workers")

    @handler("classify_images")
    def classify_images(self, images: list = None, batch_size: int = 16,
                        from_actor: str = "") -> dict:
        """Run end-to-end classification pipeline on a batch of images.

        This is the main entry point - equivalent to:
            ds = ray.data.from_items(images)
            ds.map_batches(Preprocessor, num_cpus=2)
            ds.map_batches(ModelInference, num_gpus=1, batch_size=64)

        Args:
            images: List of image descriptors [{id, width, height, channels}, ...]
            batch_size: Images per sub-batch for each worker
        """
        pipeline_start = host.now_ms()
        if not images:
            return {"error": "no images provided"}

        batch_id = f"batch-{self.total_batches}"
        n_images = len(images)

        # ---- Stage 1: Scatter images to preprocessor shard group ----
        coord_start = host.now_ms()

        # Partition images by hash(image_id) % num_preprocessors
        partitions = [[] for _ in range(self.num_preprocessors)]
        for img in images:
            img_id = img.get("id", "")
            shard = hash(img_id) % self.num_preprocessors
            partitions[shard].append(img)

        # Fan-out: ask each preprocessor to process its partition
        preprocess_results = []
        for shard_idx, partition in enumerate(partitions):
            if not partition:
                continue
            worker_id = self.preprocessor_ids[shard_idx]
            try:
                resp = host.ask(worker_id, "preprocess_batch", {
                    "images": partition,
                    "batch_id": f"{batch_id}-pre-{shard_idx}",
                }, timeout_ms=30000)
                if isinstance(resp, dict) and resp.get("status") == "ok":
                    preprocess_results.append(resp)
                else:
                    host.warn(f"Preprocessor {worker_id} failed: {resp}")
            except Exception as e:
                host.warn(f"Preprocessor {worker_id} error: {e}")

        coord_after_pre = host.now_ms()
        preprocess_ms = coord_after_pre - coord_start

        # Collect all preprocessed images
        all_preprocessed = []
        for result in preprocess_results:
            all_preprocessed.extend(result.get("preprocessed", []))

        if not all_preprocessed:
            return {"error": "preprocessing failed", "batch_id": batch_id}

        # ---- Stage 2: Scatter preprocessed to inference shard group ----
        # Re-partition for inference workers (may have different shard count)
        inf_partitions = [[] for _ in range(self.num_inference_workers)]
        for prep in all_preprocessed:
            img_id = prep.get("id", "")
            shard = hash(img_id) % self.num_inference_workers
            inf_partitions[shard].append(prep)

        # Fan-out: ask each inference worker to classify its partition
        inference_results = []
        for shard_idx, partition in enumerate(inf_partitions):
            if not partition:
                continue
            worker_id = self.inference_worker_ids[shard_idx]
            try:
                resp = host.ask(worker_id, "classify_batch", {
                    "preprocessed": partition,
                    "batch_id": f"{batch_id}-inf-{shard_idx}",
                }, timeout_ms=60000)
                if isinstance(resp, dict) and resp.get("status") == "ok":
                    inference_results.append(resp)
                else:
                    host.warn(f"Inference worker {worker_id} failed: {resp}")
            except Exception as e:
                host.warn(f"Inference worker {worker_id} error: {e}")

        coord_end = host.now_ms()
        inference_ms = coord_end - coord_after_pre

        # ---- Stage 3: Aggregate results ----
        all_predictions = []
        for result in inference_results:
            all_predictions.extend(result.get("predictions", []))

        pipeline_end = host.now_ms()
        pipeline_ms = pipeline_end - pipeline_start
        coord_ms = pipeline_ms - sum(
            r.get("preprocess_ms", 0) for r in preprocess_results
        ) - sum(
            r.get("inference_ms", 0) for r in inference_results
        )

        # Update metrics
        self.total_batches += 1
        self.total_images += n_images
        self.total_pipeline_ms += pipeline_ms
        self.total_coord_ms += coord_ms
        self.total_preprocess_ms += preprocess_ms
        self.total_inference_ms += inference_ms

        # Notify pipeline observers via process group
        host.process_groups.broadcast("pipeline-coordinators", "batch_complete", {
            "batch_id": batch_id,
            "images": n_images,
            "pipeline_ms": pipeline_ms,
        })

        return {
            "status": "ok",
            "batch_id": batch_id,
            "predictions": all_predictions,
            "summary": {
                "total_images": n_images,
                "classified": len(all_predictions),
                "pipeline_ms": pipeline_ms,
                "preprocess_ms": preprocess_ms,
                "inference_ms": inference_ms,
                "coordination_ms": round(coord_ms, 1),
                "throughput_images_per_sec": round(
                    n_images * 1000.0 / pipeline_ms if pipeline_ms > 0 else 0, 1
                ),
                "preprocessors_used": len(preprocess_results),
                "inference_workers_used": len(inference_results),
            },
        }

    @handler("pipeline_stats")
    def pipeline_stats(self) -> dict:
        """Get aggregate pipeline statistics."""
        total_time = self.total_pipeline_ms
        coord_pct = (self.total_coord_ms / total_time * 100) if total_time > 0 else 0
        pre_pct = (self.total_preprocess_ms / total_time * 100) if total_time > 0 else 0
        inf_pct = (self.total_inference_ms / total_time * 100) if total_time > 0 else 0
        throughput = (self.total_images * 1000.0 / total_time) if total_time > 0 else 0

        return {
            "coordinator_id": self.coordinator_id,
            "total_batches": self.total_batches,
            "total_images": self.total_images,
            "pipeline": {
                "total_ms": self.total_pipeline_ms,
                "preprocess_ms": self.total_preprocess_ms,
                "inference_ms": self.total_inference_ms,
                "coordination_ms": round(self.total_coord_ms, 1),
                "preprocess_pct": round(pre_pct, 1),
                "inference_pct": round(inf_pct, 1),
                "coordination_pct": round(coord_pct, 1),
            },
            "throughput_images_per_sec": round(throughput, 1),
            "workers": {
                "preprocessors": self.num_preprocessors,
                "inference_workers": self.num_inference_workers,
            },
        }

    @handler("worker_stats")
    def worker_stats(self) -> dict:
        """Gather stats from all workers via scatter-gather."""
        all_stats = {"preprocessors": [], "inference_workers": []}

        for worker_id in self.preprocessor_ids:
            try:
                resp = host.ask(worker_id, "stats", {}, timeout_ms=5000)
                if isinstance(resp, dict):
                    all_stats["preprocessors"].append(resp)
            except Exception:
                pass

        for worker_id in self.inference_worker_ids:
            try:
                resp = host.ask(worker_id, "stats", {}, timeout_ms=5000)
                if isinstance(resp, dict):
                    all_stats["inference_workers"].append(resp)
            except Exception:
                pass

        return all_stats


# Multi-actor role mapping: actor_id prefix -> actor class
# Framework passes full IDs like {"actor_id": "coordinator:batch-inf@node"}
# Prefix matching selects the class.
ACTOR_ROLES = {
    "coordinator": PipelineCoordinator,
    "preprocessor": ImagePreprocessor,
    "inference": ModelInferenceWorker,
}
