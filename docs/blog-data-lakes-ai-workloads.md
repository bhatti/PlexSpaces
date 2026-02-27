# From Data Lakes to AI Inference: Building Scalable Data Pipelines with PlexSpaces

*By Shahzad Bhatti*

---

Every data team I have worked with over three decades hits the same inflection point. The prototype runs on one machine. It works. Then someone asks: "Can we run this on a thousand images?" or "Can we process the entire S3 bucket?" and suddenly you need Ray, Spark, Airflow, Kubernetes, message queues, and a team of infrastructure engineers just to scale what was a fifty-line Python script.

The problem is not complexity — data pipelines *are* complex. The problem is accidental complexity: the orchestration scaffolding, the serialization layers, the queue configurations, the container images, the cluster managers. You spend more time wiring infrastructure together than writing the actual data processing logic.

[PlexSpaces](https://github.com/bhatti/PlexSpaces) takes a different approach. It combines the actor model with data-parallel primitives — shard groups, worker pools, resource-based routing, process groups, workflow orchestration, and message channels — into a single framework that runs on a unified WASM runtime. You write actors in Python. You deploy them to a cluster. The framework handles partitioning, routing, fault tolerance, and scaling. No Kubernetes YAML. No message queue configuration. No container builds.

In my [earlier posts](https://shahbhat.medium.com/building-plexspaces-decades-of-distributed-systems-distilled-into-one-framework-a63132040dd8), I described the five foundational pillars: TupleSpace coordination, Erlang/OTP supervision, durable execution, WASM runtime, and Firecracker isolation. In the [polyglot WebAssembly post](https://shahbhat.medium.com/building-polyglot-applications-with-webassembly), I showed how four languages compile to the same runtime. This post focuses on something different: how PlexSpaces handles the workloads that teams currently build on Ray, Spark, and AWS Lambda+SQS — batch AI inference, data lake ingestion, and scientific computing — using actors as the universal compute primitive.

---

## The Data-Parallel Actor Model

Before diving into examples, let me explain the theoretical foundation. PlexSpaces' data-parallel support builds on the [Data-Parallel Actors](https://www.usenix.org/conference/nsdi22/presentation/kraft) programming model from NSDI'22 (Kraft, Kazhamiaka, Bailis, Zaharia — Stanford). The paper's key insight: every distributed query-serving system — ElasticSearch, Druid, InfluxDB — shares the same architecture:

1. **Partition data into shards** (horizontal scaling)
2. **Route requests to the correct shard** (hash, range, or consistent-hash partitioning)
3. **Execute queries in parallel across shards** (scatter-gather)
4. **Aggregate results** (concat, merge, majority vote)

PlexSpaces encapsulates this pattern in first-class primitives that work with any actor:

### Shard Groups: Horizontal Data Parallelism

A **shard group** is a set of actors that together hold a partitioned dataset. Each actor is a shard. The framework routes messages to the correct shard based on a partition key:

```
┌─────────────────────────────────────────────────────┐
│                  Shard Group                         │
│                                                     │
│  Partition Strategy: hash(key) % shard_count         │
│                                                     │
│  ┌──────┐  ┌──────┐  ┌──────┐  ┌──────┐           │
│  │Shard │  │Shard │  │Shard │  │Shard │           │
│  │  0   │  │  1   │  │  2   │  │  3   │           │
│  └──────┘  └──────┘  └──────┘  └──────┘           │
│                                                     │
│  Operations:                                         │
│  - parallel_map:    query all shards, collect results │
│  - parallel_update: write to correct shard by key     │
│  - scatter_gather:  query all, aggregate responses    │
│  - map_reduce:      map + aggregate in one call       │
└─────────────────────────────────────────────────────┘
```

Three partition strategies:

| Strategy | Routing | Best For | Scaling Behavior |
|----------|---------|----------|-----------------|
| **Hash** | `hash(key) % N` | Uniform access | ~50% reshuffle on scale |
| **Consistent Hash** | Virtual-node ring | Frequent scaling | ~1/N keys move |
| **Range** | Binary search on boundaries | Ordered keys, range queries | Manual split |

PlexSpaces implements this in the `ParallelClient` SDK:

```rust
// Create a 20-worker pool with hash partitioning
let pool_id = client.create_worker_pool(
    "inference-pool",
    "image-classifier",
    20,
    PartitionStrategy::Hash,
    labels! { "accelerator" => "gpu" },
).await?;

// Scatter: send 10,000 images to workers (routed by hash)
client.parallel_update(&pool_id, images, Consistency::Eventual, false).await?;

// Gather: query all workers for results
let results = client.parallel_map(&pool_id, json!({"action": "get_results"})).await?;

// Reduce: aggregate with concat strategy
let aggregate = client.parallel_reduce(
    &pool_id, json!({"action": "stats"}),
    Aggregation::Concat, 20,
).await?;
```

### Resource-Based Routing: Right Work, Right Hardware

Not all actors need the same hardware. Preprocessing actors need CPU. Inference actors need GPU. Alignment workers need high memory. PlexSpaces routes actors to nodes based on **resource requirements and labels** — the same scheduling model Kubernetes uses, but at the actor level instead of the pod level:

```protobuf
message ActorResourceRequirements {
    ResourceSpec resources = 1;          // CPU cores, memory, disk, GPU count
    map<string, string> required_labels = 2;  // Node labels that must match
    PlacementPreferences placement = 3;       // Affinity, anti-affinity
}

message ResourceSpec {
    double cpu_cores = 1;        // Fractional: 0.5 = half core
    uint64 memory_bytes = 2;     // 4GB = 4294967296
    int32 gpu_count = 4;         // Number of GPUs required
    string gpu_type = 5;         // "nvidia-tesla-v100", "nvidia-a100"
}
```

The node selector uses a **bin-packing algorithm** that scores candidate nodes by resource utilization and label matching. Nodes that satisfy the required labels (hard constraints) are scored by available capacity, with preference for balanced utilization across the cluster. Placement preferences (affinity, anti-affinity) provide soft hints — co-locate related actors for locality, spread replicas across failure domains.

This means inference workers automatically land on GPU nodes, preprocessing workers land on CPU nodes, and the framework balances load across the cluster without manual placement rules.

### Worker Pools and Process Groups

**Worker pools** are collections of stateless actors that process tasks in parallel. They combine two mechanisms:

- **Stateless worker config**: Auto-scaling between min/max instances with load-balancing (round-robin, least-loaded, random)
- **Process groups**: Erlang pg/pg2-style pub/sub groups where actors register dynamically

```python
# Worker joins a process group on init
host.process_groups.join("inference-workers")

# Coordinator discovers workers dynamically
workers = host.process_groups.members("inference-workers")

# Broadcast to all workers
host.process_groups.broadcast("inference-workers", "flush_metrics", {})
```

Process groups provide **location-transparent pub/sub**: workers join from any node in the cluster, coordinators discover them without hardcoded addresses, and the framework handles network routing.

### Workflow Actors: Multi-Step Pipeline Orchestration

For complex multi-step pipelines — data lake ingestion, genomics analysis, ML training — PlexSpaces provides **workflow actors** with durable state:

```python
@workflow_actor
class DataPipeline:
    pipeline_state: str = state(default="idle")
    checkpoint: dict = state(default_factory=dict)

    @handler("run_pipeline")
    def run(self, documents: list) -> dict:
        # Step 1: Parse (survives crashes via journaling)
        parsed = self._scatter_parse(documents)

        # Step 2: Chunk
        chunks = self._scatter_chunk(parsed)

        # Step 3: Embed (GPU workers)
        embeddings = self._scatter_embed(chunks)

        # Step 4: Store
        self._store_vectors(embeddings)

        return {"status": "complete", "docs": len(documents)}
```

Workflow actors support:
- **Durable execution**: State checkpointed via journaling; crash recovery replays to exact state
- **Signals**: External control events (pause, resume, cancel) delivered to running workflows
- **Queries**: Status inspection while pipeline is running (e.g., "how many documents processed?")
- **Retry policies**: Exponential backoff with jitter per step
- **Saga compensation**: Rollback steps on failure
- **Conditional steps**: Choice/branch based on data properties (e.g., route PDFs vs HTMLs differently)

### Durable Execution: Journaling, Snapshots, and Replay

PlexSpaces durability goes beyond simple state persistence. Every message an actor processes is **journaled** — creating an append-only log of the actor's complete execution history. If the actor crashes, the framework replays the journal from the last snapshot to restore exact state.

```
Timeline:  msg1 → msg2 → msg3 → [SNAPSHOT] → msg4 → msg5 → [CRASH]
Recovery:  Load snapshot → replay msg4 → replay msg5 → resume from msg6
```

Three journal entry types:
- **MessageReceived**: Incoming message logged before processing
- **MessageProcessed**: Handler result logged after processing
- **StateCheckpoint**: Full state snapshot at configurable intervals

For data pipelines, this means: if an ingestion coordinator crashes after processing 500 of 1000 documents, it recovers from the last checkpoint and resumes at document 501 — not document 1. No duplicate processing, no lost progress.

Durability is enabled via the **facets** system (see below):

```python
@workflow_actor(facets=["durability"])
class IngestionCoordinator:
    pipeline_state: str = state(default="idle")
    docs_processed: int = state(default=0)
    # State is automatically checkpointed and restored on crash
```

### Facets: Composable Actor Capabilities

**Facets** are PlexSpaces' extensibility mechanism — pluggable capabilities that attach to actors at deployment time. Think of them as middleware for actors. Instead of baking durability, timers, or HTTP capabilities into every actor, you compose them declaratively:

| Facet | What It Adds | Use Case |
|-------|-------------|----------|
| **durability** | Journaling + checkpoint + replay | Long-running pipelines |
| **event_sourcing** | Full audit trail + time-travel | Compliance, debugging |
| **timer** | Durable delayed messages (reminders) | Scheduled batch ingestion |
| **virtual_actor** | Grain-style lifecycle (activate on demand) | Elastic scaling |
| **registry** | Service discovery registration | Microservices |

Facets are declared in `app-config.toml`:

```toml
[[supervisor.children]]
id = "ingestion-coordinator"
type = "worker"
restart = "permanent"
facets = [
  { type = "durability", priority = 10, config = { checkpoint_interval = 100 } },
  { type = "timer", priority = 20, config = {} }
]
```

Or in Python decorators:

```python
@workflow_actor(facets=["durability"])
class DurablePipeline:
    ...
```

This composition model means you pay only for the capabilities you use. A stateless preprocessing worker needs no durability facet. A long-running ingestion coordinator needs durability + timers. A service registry actor needs the registry facet.

### Channels: Queue-Based Task Distribution

For AWS Lambda+SQS-style patterns, PlexSpaces provides **channels** — message queues with multiple backend options:

| Backend | Use Case | Delivery Guarantee |
|---------|----------|-------------------|
| **In-Memory** | Same-node, testing | At-most-once |
| **Redis Streams** | Low-latency cross-node | At-least-once |
| **Kafka** | High-throughput, ordered | Exactly-once |
| **SQS** | AWS-native, managed | At-least-once |
| **Process Group** | No external deps | At-most-once |
| **PostgreSQL** | LISTEN/NOTIFY, transactional | At-least-once |

Channels support consumer groups, dead-letter queues, backpressure strategies (block, drop, overflow), and ack/nack for reliable processing. Backpressure is critical for AI pipelines: if GPU inference is slower than CPU preprocessing, the channel automatically throttles the producer instead of overwhelming the consumer.

### Distributed Locks: Coordinating Critical Sections

When multiple actors need exclusive access to a shared resource — preventing duplicate document ingestion, coordinating shard rebalancing, acquiring a leader role — PlexSpaces provides **distributed locks** with lease-based expiration:

```python
# Acquire lock before writing to shared index (prevents duplicate ingestion)
lock_id = host.lock_acquire("ingestion-lock", lease_ms=30000, timeout_ms=5000)
try:
    # Only one actor executes this block at a time
    host.kv_put(f"index:{doc_id}", embedding_data)
finally:
    host.lock_release("ingestion-lock", lock_id)
```

Locks automatically expire if the holder crashes (lease-based), preventing deadlocks. Holders can `host.lock_renew()` to extend the lease for long operations. This is the same pattern as Redis Redlock or ZooKeeper ephemeral nodes, but built into the framework.

### Blob Storage: Large Data Transfer

For data too large for the KV store — image batches, model weights, embedding vectors, intermediate pipeline results — PlexSpaces provides **blob storage** with S3-compatible semantics:

```python
# Store a batch of embeddings as a binary blob
embedding_bytes = serialize_embeddings(embeddings)
host.blob_upload(f"embeddings/{batch_id}", embedding_bytes, "application/octet-stream")

# Download on another actor (e.g., vector index builder)
data = host.blob_download(f"embeddings/{batch_id}")
embeddings = deserialize_embeddings(data)

# List all embedding blobs
blob_keys = host.blob_list("embeddings/")
```

The KV store handles metadata (document IDs, index mappings). Blob storage handles payloads (raw images, serialized tensors, model checkpoints). This separation mirrors the S3 + DynamoDB pattern used in production data lakes.

### Timers and Scheduled Tasks

Actors can schedule delayed messages using `host.send_after()` — useful for retry logic, periodic health checks, and scheduled batch ingestion:

```python
@actor
class BatchScheduler:
    @handler("schedule_ingestion")
    def schedule(self, interval_ms: int = 3600000) -> dict:
        # Schedule next ingestion run in 1 hour
        host.send_after(interval_ms, "run_ingestion", {"source": "s3://data-lake/"})
        return {"status": "scheduled", "next_run_ms": interval_ms}

    @handler("run_ingestion")
    def run_ingestion(self, source: str = "") -> dict:
        # Process data, then reschedule
        result = self._ingest_from(source)
        host.send_after(3600000, "run_ingestion", {"source": source})
        return result
```

For durable scheduling that survives crashes, combine timers with the **timer facet** — which persists scheduled messages and re-fires them after recovery.

### Actor Lifecycle: Dynamic Spawning and Linking

Beyond static supervisor configurations, actors can dynamically spawn, stop, link, and monitor other actors at runtime:

```python
# Dynamically spawn extra workers when load increases
worker_id = host.spawn("extra-preprocessor-5", "preprocessor-module")

# Link: if worker crashes, coordinator crashes too (fail-fast)
host.link(worker_id)

# Monitor: get notified if worker crashes (without crashing ourselves)
monitor_ref = host.monitor(worker_id)

# Stop a worker gracefully
host.stop(worker_id)
```

**Linking** implements the Erlang "let it crash" philosophy: linked actors fail together, and the supervisor restarts the entire group. **Monitoring** provides one-directional failure notification without cascading crashes — useful for coordinators that need to know when workers die but should not die themselves.

### Supervision Strategies

The `app-config.toml` examples show `one_for_one`, but PlexSpaces supports three Erlang/OTP supervision strategies:

| Strategy | Behavior | When to Use |
|----------|----------|-------------|
| **one_for_one** | Restart only the crashed actor | Independent workers (default) |
| **rest_for_one** | Restart crashed actor + all actors started after it | Ordered dependencies (pipeline stages) |
| **one_for_all** | Restart entire group if any actor crashes | Tightly coupled actors (shared state) |

For data pipelines with stage dependencies (preprocessor → inference), `rest_for_one` ensures downstream actors restart when an upstream actor crashes:

```toml
[supervisor]
strategy = "rest_for_one"    # Restart downstream stages on upstream failure
max_restarts = 10
max_restart_window_seconds = 60

[[supervisor.children]]
id = "preprocessor"          # If this crashes...
restart = "permanent"

[[supervisor.children]]
id = "inference"             # ...this also restarts (started after preprocessor)
restart = "permanent"
```

For coordinators with shared state, `one_for_all` ensures consistency by restarting the entire group:

```toml
[supervisor]
strategy = "one_for_all"     # Any crash restarts everything
```

### Behavior Types: GenServer, GenEvent, FSM, Workflow

PlexSpaces actors support four Erlang-inspired behavior types, each optimized for different communication patterns:

```python
@actor                  # GenServer: request-reply (default)
class BankAccount:      # Client sends request, gets response
    ...

@event_actor            # GenEvent: fire-and-forget event handling
class TelemetryLogger:  # Receives events, no response needed
    ...                 # Ideal for logging, metrics, audit trails

@fsm_actor              # GenStateMachine: state transitions
class DocumentFSM:      # Models explicit state machine with guards
    current_state: str = state(default="pending")

    @handler("process")
    def process(self, doc_id: str = "") -> dict:
        if self.current_state != "pending":
            return {"error": "invalid_transition"}
        self.current_state = "processing"
        return {"state": self.current_state}

@workflow_actor          # Workflow: multi-step durable orchestration
class IngestionPipeline: # Long-running with checkpoints
    ...
```

For AI pipelines, the pattern is typically: **GenServer** coordinators and workers (request-reply for scatter-gather), **event actors** for telemetry (fire-and-forget metrics), **FSM actors** for document state tracking, and **workflow actors** for pipeline orchestration.

### Multi-Actor Modules: ACTOR_ROLES

A single WASM module can contain multiple actor classes. The framework uses the `ACTOR_ROLES` dictionary to route actor IDs to the correct class via prefix matching:

```python
# One module, three actor types
ACTOR_ROLES = {
    "coordinator":  PipelineCoordinator,   # "coordinator" → PipelineCoordinator
    "preprocessor": ImagePreprocessor,     # "preprocessor-0" → ImagePreprocessor
    "inference":    ModelInferenceWorker,   # "inference-0" → ModelInferenceWorker
}
```

This means you deploy a single `.wasm` binary that contains the entire pipeline — coordinator, preprocessors, and inference workers. The `app-config.toml` supervisor configuration maps child IDs to actor classes via prefix matching. No separate deployments per actor type.

---

## Example 1: Batch Image Classification Pipeline

This example demonstrates the core data-parallel pattern: a multi-stage AI inference pipeline that classifies thousands of images using shard groups, resource-based routing, and worker pools. It mirrors what you would build with [Ray Data's ViT batch prediction](https://docs.ray.io/en/latest/data/examples/huggingface_vit_batch_prediction.html) or [PyTorch ResNet batch inference](https://docs.ray.io/en/latest/data/examples/pytorch_resnet_batch_prediction.html).

### Architecture

```
                        ┌─────────────────────┐
  HTTP POST             │  PipelineCoordinator │
  /classify_images ───▶ │  (orchestration)     │
                        └──────────┬──────────┘
                                   │
              ┌────────────────────┼────────────────────┐
              │                    │                    │
     ┌────────▼─────┐    ┌────────▼─────┐     ┌───────▼──────┐
     │Preprocessor-0│    │Preprocessor-1│     │Preprocessor-3│
     │ {cpu: 2}     │    │ {cpu: 2}     │     │ {cpu: 2}     │
     └────────┬─────┘    └────────┬─────┘     └───────┬──────┘
              │                    │                    │
              └────────────────────┼────────────────────┘
                                   │
              ┌────────────────────┼────────────────────┐
              │                                         │
     ┌────────▼──────┐                        ┌────────▼──────┐
     │ Inference-0   │                        │ Inference-1   │
     │ {gpu: 1,      │                        │ {gpu: 1,      │
     │  nvidia-a100}  │                        │  nvidia-a100}  │
     └────────┬──────┘                        └────────┬──────┘
              │                                         │
              └─────────────────┬───────────────────────┘
                                │
                        ┌───────▼──────┐
                        │  Predictions │
                        │  (aggregated)│
                        └──────────────┘
```

### The Pipeline Coordinator

The coordinator implements the scatter-gather pattern: partition images across shard groups, fan out work, fan in results:

```python
@actor
class PipelineCoordinator:
    """Orchestrates batch inference across preprocessing and inference stages."""

    num_preprocessors: int = state(default=4)
    num_inference_workers: int = state(default=2)
    preprocessor_ids: list = state(default_factory=list)
    inference_worker_ids: list = state(default_factory=list)

    @handler("classify_images")
    def classify_images(self, images: list = None, batch_size: int = 16) -> dict:
        """End-to-end classification — equivalent to Ray's ds.map_batches()."""

        # ---- Stage 1: Scatter images to preprocessor shard group ----
        # Partition by hash(image_id) % num_preprocessors
        partitions = [[] for _ in range(self.num_preprocessors)]
        for img in images:
            shard = hash(img["id"]) % self.num_preprocessors
            partitions[shard].append(img)

        # Fan-out: ask each preprocessor to process its partition
        preprocess_results = []
        for shard_idx, partition in enumerate(partitions):
            if not partition:
                continue
            resp = host.ask(self.preprocessor_ids[shard_idx],
                           "preprocess_batch",
                           {"images": partition, "batch_id": f"batch-{shard_idx}"},
                           timeout_ms=30000)
            preprocess_results.append(resp)

        # Collect all preprocessed tensors
        all_preprocessed = []
        for result in preprocess_results:
            all_preprocessed.extend(result.get("preprocessed", []))

        # ---- Stage 2: Scatter to inference shard group (GPU workers) ----
        inf_partitions = [[] for _ in range(self.num_inference_workers)]
        for prep in all_preprocessed:
            shard = hash(prep["id"]) % self.num_inference_workers
            inf_partitions[shard].append(prep)

        # Fan-out to GPU workers
        all_predictions = []
        for shard_idx, partition in enumerate(inf_partitions):
            if not partition:
                continue
            resp = host.ask(self.inference_worker_ids[shard_idx],
                           "classify_batch",
                           {"preprocessed": partition},
                           timeout_ms=60000)
            all_predictions.extend(resp.get("predictions", []))

        return {"predictions": all_predictions, "count": len(all_predictions)}
```

### Preprocessing Workers (CPU Nodes)

Each preprocessor is a shard in the preprocessing shard group. It runs on CPU-labeled nodes and handles image resize + normalization — the same operations you would put in a `torchvision.transforms` pipeline:

```python
@actor
class ImagePreprocessor:
    """Preprocesses image batches on CPU nodes.

    Resource requirements: {accelerator: "cpu", role: "preprocessor"}
    """

    shard_id: int = state(default=0)
    images_processed: int = state(default=0)
    target_size: list = state(default_factory=lambda: [224, 224])
    # ImageNet normalization constants
    mean: list = state(default_factory=lambda: [0.485, 0.456, 0.406])
    std: list = state(default_factory=lambda: [0.229, 0.224, 0.225])

    @init_handler
    def on_init(self, config: dict):
        self.shard_id = int(config.get("args", {}).get("shard_id", 0))
        host.process_groups.join("pipeline-preprocessors")

    @handler("preprocess_batch")
    def preprocess_batch(self, images: list = None, batch_id: str = "") -> dict:
        """Resize + normalize a batch of images to model input tensors."""
        preprocessed = []
        for img in images:
            # Simulate resize to 224x224 + ImageNet normalization
            tensor = self._resize_and_normalize(img)
            preprocessed.append({
                "id": img["id"],
                "tensor_shape": [3, 224, 224],
                "tensor_summary": {"elements": 3 * 224 * 224},
            })
            self.images_processed += 1

        return {"status": "ok", "preprocessed": preprocessed, "count": len(preprocessed)}
```

### Inference Workers (GPU Nodes)

Inference workers run on GPU-labeled nodes. The model loads once in `__init__` (expensive), then runs inference per batch — exactly the pattern Ray Data uses with `map_batches()`:

```python
@actor
class ModelInferenceWorker:
    """Runs ViT/ResNet classification on GPU.

    Resource requirements: {accelerator: "gpu", gpu_type: "nvidia", gpu_count: 1}

    This mirrors Ray Data's class-based inference:
        class ImageClassifier:
            def __init__(self):
                self.model = load_model()  # Once per worker
            def __call__(self, batch):
                return self.model(batch)   # Per batch
    """

    model_name: str = state(default="vit-base-patch16-224")
    model_loaded: bool = state(default=False)

    @init_handler
    def on_init(self, config: dict):
        self._load_model()  # Load once, reuse across batches
        host.process_groups.join("pipeline-gpu-workers")

    def _load_model(self):
        """Load model weights (in production: transformers.ViTForImageClassification)."""
        # from transformers import ViTForImageClassification
        # self.model = ViTForImageClassification.from_pretrained("google/vit-base-patch16-224")
        # self.model.to("cuda").eval()
        self.model_loaded = True

    @handler("classify_batch")
    def classify_batch(self, preprocessed: list = None, batch_id: str = "") -> dict:
        """Run inference on preprocessed images. Returns top-5 predictions."""
        predictions = []
        for img in preprocessed:
            # In production: logits = self.model(tensor.unsqueeze(0).cuda())
            # probs = torch.softmax(logits, dim=-1)
            top5 = self._simulate_inference(img["id"])
            predictions.append({
                "image_id": img["id"],
                "top_prediction": top5[0],
                "top5": top5,
            })
        return {"status": "ok", "predictions": predictions}
```

### Supervisor Configuration

The `app-config.toml` deploys the full pipeline under an Erlang-style supervision tree:

```toml
[supervisor]
strategy = "one_for_one"
max_restarts = 10
max_restart_window_seconds = 60

# Pipeline coordinator
[[supervisor.children]]
id = "coordinator"
type = "worker"
restart = "permanent"

[supervisor.children.args]
num_preprocessors = "4"
num_inference_workers = "2"

# Preprocessor shard group (CPU nodes)
[[supervisor.children]]
id = "preprocessor-0"
type = "worker"
restart = "permanent"
[supervisor.children.args]
shard_id = "0"

# ... preprocessor-1, preprocessor-2, preprocessor-3 ...

# Inference shard group (GPU nodes)
[[supervisor.children]]
id = "inference-0"
type = "worker"
restart = "permanent"
[supervisor.children.args]
shard_id = "0"
model_name = "vit-base-patch16-224"
batch_size = "64"

[[supervisor.children]]
id = "inference-1"
type = "worker"
restart = "permanent"
[supervisor.children.args]
shard_id = "1"
model_name = "vit-base-patch16-224"
```

### Invoke the Pipeline

```bash
# Deploy the WASM module
curl -X POST http://localhost:8001/api/v1/applications/deploy \
    -F "application_id=batch-inference" \
    -F "name=image-classifier" \
    -F "version=1.0.0" \
    -F "wasm_file=@batch_inference_actors.wasm"

# Classify a batch of images
curl -X POST http://localhost:8001/api/v1/actors/coordinator/ask \
    -H "Content-Type: application/json" \
    -d '{
      "message_type": "classify_images",
      "payload": {
        "images": [
          {"id": "img-001", "width": 640, "height": 480, "channels": 3},
          {"id": "img-002", "width": 1920, "height": 1080, "channels": 3},
          {"id": "img-003", "width": 800, "height": 600, "channels": 3}
        ],
        "batch_size": 16
      }
    }'

# Response:
# {
#   "status": "ok",
#   "predictions": [
#     {"image_id": "img-001", "top_prediction": {"label": "tench", "confidence": 0.87}},
#     {"image_id": "img-002", "top_prediction": {"label": "goldfish", "confidence": 0.92}},
#     ...
#   ],
#   "summary": {
#     "total_images": 3,
#     "pipeline_ms": 45,
#     "throughput_images_per_sec": 66.7,
#     "preprocessors_used": 3,
#     "inference_workers_used": 2
#   }
# }
```

### How This Compares to Ray Data

| Aspect | Ray Data | PlexSpaces |
|--------|----------|------------|
| **Pipeline** | `ds.map_batches(Class, num_gpus=1)` | Shard group + `host.ask()` scatter-gather |
| **GPU routing** | `num_gpus=1` parameter | `required_labels: {accelerator: "gpu"}` |
| **Model loading** | `__init__` (once per worker) | `@init_handler` (once per actor) |
| **Batch inference** | `__call__(batch)` | `@handler("classify_batch")` |
| **Fault tolerance** | Task retry | Supervision tree + durable state |
| **State** | Stateless (external) | Built-in persistent state |
| **Scaling** | Autoscaler + resource requests | Worker pool + node selector |
| **Cluster setup** | `ray start`, KubeRay | `plexspaces start`, Docker |

The fundamental difference: Ray treats functions as the compute unit and manages state externally. PlexSpaces treats actors as the compute unit with built-in state, supervision, and coordination. For stateless batch inference, both work well. For stateful pipelines — incremental ingestion, accumulating metrics, maintaining model state — the actor model eliminates the external state store entirely.

### Why Actors for Batch Inference?

The actor model adds three capabilities that Ray's stateless `map_batches()` lacks:

1. **Persistent per-worker state**: Each inference worker maintains `model_loaded`, `batches_processed`, and `total_inference_ms` across requests. In Ray, you get this with class-based actors, but state is lost on worker failure. In PlexSpaces, state survives crashes via durability facets.

2. **Supervision-tree recovery**: If an inference worker crashes mid-batch (OOM, GPU fault), the supervisor restarts it automatically and the coordinator retries the failed partition. No external monitoring, no manual intervention.

3. **Resource-based routing at the actor level**: Each actor specifies its hardware requirements declaratively. The framework places preprocessors on CPU nodes and inference workers on GPU nodes without Kubernetes affinity rules or KubeRay operator configuration.

---

## Example 2: Data Lake Ingestion Pipeline

This example demonstrates a production-grade document ingestion pipeline — the kind you build for RAG (Retrieval-Augmented Generation) systems. It mirrors [Ray Data's unstructured data ingestion](https://docs.ray.io/en/master/data/examples/unstructured_data_ingestion/content/unstructured_data_ingestion.html) and [scalable RAG data ingestion](https://docs.ray.io/en/latest/ray-overview/examples/e2e-rag/notebooks/02_Scalable_RAG_Data_Ingestion_with_Ray_Data.html) examples.

### Architecture

```
  Documents (PDF, DOCX, HTML, TXT)
         │
         ▼
  ┌──────────────────────┐
  │ IngestionCoordinator │  Workflow actor (durable state)
  │ @workflow_actor      │  Orchestrates 4-stage pipeline
  └──────────┬───────────┘
             │
    ┌────────┼────────┐
    ▼        ▼        ▼
  ┌─────┐ ┌─────┐
  │Parse│ │Parse│   Process Group: "pipeline-parsers"
  │  -0 │ │  -1 │   Extract text from any document format
  └──┬──┘ └──┬──┘
     │       │
     ▼       ▼
  ┌──────┐ ┌──────┐
  │Chunk │ │Chunk │   Process Group: "pipeline-chunkers"
  │  -0  │ │  -1  │   Split text into overlapping chunks
  └──┬───┘ └──┬───┘   (1500 chars, 150 overlap)
     │        │
     ▼        ▼
  ┌───────┐ ┌───────┐
  │Embed  │ │Embed  │   Shard Group (GPU nodes)
  │  -0   │ │  -1   │   Generate 768-dim vector embeddings
  └───┬───┘ └───┬───┘
      │         │
      ▼         ▼
  ┌────────────────┐
  │  Vector Index   │   KV Store + Blob Storage
  │  (searchable)   │
  └────────────────┘
```

### The Workflow Coordinator

The ingestion coordinator is a `@workflow_actor` — it orchestrates the multi-step pipeline with durable state that survives crashes:

```python
@workflow_actor
class IngestionCoordinator:
    """Orchestrates the full data lake ingestion pipeline.

    Four stages:
    1. Parse: extract text from documents (PDF, DOCX, HTML, TXT)
    2. Chunk: split into overlapping segments for embedding
    3. Embed: generate vector embeddings (GPU workers)
    4. Store: write to vector index (KV store)
    """

    @handler("ingest")
    def ingest_documents(self, documents: list = None) -> dict:
        # ---- Stage 1: Parse documents ----
        parsed_docs = []
        for i, doc in enumerate(documents):
            worker_id = self.parser_ids[i % self.num_parsers]
            resp = host.ask(worker_id, "parse", {
                "doc_id": doc["id"],
                "doc_type": doc["type"],
                "content": doc["content"],
            }, timeout_ms=30000)
            parsed_docs.append(resp)

        # ---- Stage 2: Chunk parsed text ----
        all_chunks = []
        for i, parsed in enumerate(parsed_docs):
            worker_id = self.chunker_ids[i % self.num_chunkers]
            resp = host.ask(worker_id, "chunk", {
                "doc_id": parsed["doc_id"],
                "text": parsed["text"],
                "metadata": parsed["metadata"],
            }, timeout_ms=30000)
            all_chunks.extend(resp.get("chunks", []))

        # ---- Stage 3: Embed chunks (GPU shard group) ----
        embed_partitions = [[] for _ in range(self.num_embedders)]
        for chunk in all_chunks:
            shard = hash(chunk["chunk_id"]) % self.num_embedders
            embed_partitions[shard].append(chunk)

        all_embeddings = []
        for shard_idx, partition in enumerate(embed_partitions):
            resp = host.ask(self.embedder_ids[shard_idx], "embed_batch",
                           {"chunks": partition}, timeout_ms=60000)
            all_embeddings.extend(resp.get("embeddings", []))

        # ---- Stage 4: Store in vector index ----
        for emb in all_embeddings:
            host.kv_put(f"index:{emb['chunk_id']}", json.dumps({
                "doc_id": emb["doc_id"],
                "embedding_dim": emb["embedding_dim"],
            }))

        return {
            "documents_parsed": len(parsed_docs),
            "chunks_created": len(all_chunks),
            "embeddings_generated": len(all_embeddings),
        }
```

### Document Parser (Process Group Workers)

Parsers join a process group dynamically. The coordinator discovers them at runtime — no hardcoded addresses:

```python
@actor
class DocumentParser:
    """Parses PDF, DOCX, HTML, TXT into plain text.

    In production: PyPDF2, python-docx, BeautifulSoup, Unstructured.io
    """

    @init_handler
    def on_init(self, config: dict):
        host.process_groups.join("pipeline-parsers")

    @handler("parse")
    def parse_document(self, doc_id: str = "", doc_type: str = "txt",
                       content: str = "") -> dict:
        if doc_type == "pdf":
            text = self._parse_pdf(content)
        elif doc_type == "html":
            text = self._parse_html(content)
        else:
            text = content
        return {"status": "ok", "doc_id": doc_id, "text": text}
```

### Text Chunker (RAG-Optimized)

Following RAG best practices: 1500-character chunks with 150-character overlap, breaking at sentence boundaries:

```python
@actor
class TextChunker:
    chunk_size: int = state(default=1500)
    overlap: int = state(default=150)

    @handler("chunk")
    def chunk_document(self, doc_id: str = "", text: str = "") -> dict:
        chunks = []
        start = 0
        while start < len(text):
            end = min(start + self.chunk_size, len(text))
            # Break at sentence boundary
            if end < len(text):
                for boundary in ['. ', '.\n', '\n\n']:
                    last = text.rfind(boundary, start + self.chunk_size // 2, end)
                    if last > start:
                        end = last + len(boundary)
                        break
            chunks.append({"chunk_id": f"{doc_id}-chunk-{len(chunks)}",
                           "text": text[start:end].strip()})
            start = end - self.overlap
        return {"status": "ok", "chunks": chunks}
```

### Embedding Worker (GPU Shard Group)

Embedding workers form a shard group on GPU nodes. Each processes a partition of chunks:

```python
@actor
class EmbeddingWorker:
    """Generate vector embeddings on GPU.

    In production:
        from sentence_transformers import SentenceTransformer
        model = SentenceTransformer("all-mpnet-base-v2")
        embeddings = model.encode(texts, batch_size=32)
    """

    model_name: str = state(default="all-mpnet-base-v2")
    embedding_dim: int = state(default=768)

    @init_handler
    def on_init(self, config: dict):
        host.process_groups.join("pipeline-embedders")

    @handler("embed_batch")
    def embed_batch(self, chunks: list = None) -> dict:
        embeddings = []
        for chunk in chunks:
            vector = self._encode(chunk["text"])  # 768-dim embedding
            # Store in blob storage for vector search
            host.kv_put(f"embedding:{chunk['chunk_id']}", json.dumps({
                "vector": vector, "doc_id": chunk["doc_id"],
            }))
            embeddings.append({"chunk_id": chunk["chunk_id"],
                               "embedding_dim": self.embedding_dim})
        return {"status": "ok", "embeddings": embeddings}
```

### Channel-Based Alternative (SQS Pattern)

For even looser coupling, you can connect pipeline stages with channels instead of direct `host.ask()` calls. Channels are configured at the infrastructure level (node config) and actors interact with them through message routing — the framework handles the queue mechanics. This decouples producers from consumers, the same pattern as AWS Lambda + SQS but without external infrastructure.

When to use channels vs direct `host.ask()`:
- **`host.ask()` (scatter-gather)**: When the coordinator needs results immediately and controls parallelism. Best for batch pipelines where all stages complete before returning.
- **Channels (queue-based)**: When stages run at different speeds and you need backpressure. Best for streaming ingestion where documents arrive continuously.

### Why Actors for Data Lake Ingestion?

Three properties of the actor model make it superior to stateless function pipelines for data lake workloads:

1. **Durable state eliminates external stores**: The ingestion coordinator tracks progress (docs processed, chunks created, embeddings stored) in its own state — no Redis, no DynamoDB, no checkpoint files. If it crashes at document 500, it resumes from the last checkpoint, not from zero.

2. **Process groups replace service registries**: Workers discover each other via `host.process_groups.join()` — no Consul, no Eureka, no Kubernetes service objects. Add a new parser node and it joins the group automatically.

3. **Supervision trees replace retry logic**: Instead of wrapping every function call in try/except with exponential backoff, the supervision tree automatically restarts crashed workers. The coordinator does not handle worker failures — the supervisor does.

---

## Example 3: Heat Diffusion (Scientific Computing)

This example demonstrates the **TupleSpace coordination pattern** for stencil computations — the foundation of weather modeling, thermal simulation, and image processing. It is ported from the Rust example at `examples/rust/embedded/heat_diffusion/`.

### The Physics

2D heat equation solved via Jacobi iteration (5-point stencil):

```
        N
      W C E    →    C_new = (W + E + N + S) / 4.0
        S
```

The grid is partitioned into horizontal strips. Each strip is an actor. Neighbors exchange boundary values ("ghost cells") via TupleSpace.

### TupleSpace Ghost Cell Exchange

```python
@actor
class GridRegionActor:
    region_id: int = state(default=0)
    data: list = state(default_factory=list)       # Temperature values
    width: int = state(default=100)

    @handler("compute")
    def compute_iteration(self, iteration: int = 0) -> dict:
        # Step 1: Write our boundary to TupleSpace for neighbors
        host.ts_write(json.dumps(
            ["boundary", iteration, self.region_id, "south", self.data[:]]
        ))

        # Step 2: Read neighbor's boundary from TupleSpace
        pattern = json.dumps(
            ["boundary", iteration, self.region_id - 1, "south", None]
        )
        result = host.ts_read(pattern)
        north = json.loads(result)[4]  # Neighbor's south = our north

        # Step 3: Compute 5-point stencil
        new_data = self.data[:]
        max_diff = 0.0
        for i in range(1, self.width - 1):
            new_val = (north[i] + south[i] + self.data[i-1] + self.data[i+1]) / 4.0
            max_diff = max(max_diff, abs(new_val - self.data[i]))
            new_data[i] = new_val
        self.data = new_data

        # Step 4: Barrier synchronization
        host.ts_write(json.dumps(["barrier", iteration, self.region_id]))

        return {"max_diff": max_diff, "avg_temp": sum(self.data) / len(self.data)}
```

### Barrier Synchronization

The coordinator waits for all regions to complete before starting the next iteration:

```python
@actor
class SimulationCoordinator:
    @handler("run_simulation")
    def run_simulation(self, iterations: int = 100) -> dict:
        for it in range(iterations):
            # Fan-out: all regions compute in parallel
            results = []
            for region_id in self.region_ids:
                resp = host.ask(region_id, "compute", {"iteration": it},
                               timeout_ms=30000)
                results.append(resp)

            # Convergence check
            max_diff = max(r["max_diff"] for r in results)
            if max_diff < self.tolerance:
                return {"converged": True, "iteration": it}

        return {"converged": False, "iterations": iterations}
```

The TupleSpace coordination pattern replaces MPI ghost cell exchange with a higher-level abstraction: actors write tuples, neighbors read matching patterns. No explicit send/receive, no rank management, no communicator setup.

### Why Actors for Scientific Computing?

Three advantages over MPI:

1. **Dynamic membership via process groups**: MPI requires static rank allocation at launch time — you cannot add workers mid-simulation. PlexSpaces actors join process groups dynamically. Scale from 4 to 16 grid regions mid-simulation by spawning new actors that join the "heat-regions" group.

2. **Fault tolerance for long-running simulations**: MPI has no built-in fault tolerance — one crashed rank kills the entire job. PlexSpaces supervision trees restart crashed regions, and durable state preserves temperature data across restarts. A 48-hour simulation survives node failures.

3. **Heterogeneous hardware**: MPI assumes uniform ranks. PlexSpaces resource-based routing places compute-intensive regions on high-CPU nodes and I/O-intensive regions on high-memory nodes — within the same simulation.

---

## Example 4: Genomics Pipeline (Workflow Actor)

This example demonstrates a multi-step bioinformatics pipeline — ported from `examples/rust/embedded/genomics_pipeline/` — using workflow actors for durable orchestration.

### Pipeline Stages

```
  Raw Reads (FASTQ)
       │
       ▼
  ┌─────────────┐
  │ Quality     │  Filter reads with Phred Q < 30
  │ Control     │  95% pass rate (Illumina standards)
  └──────┬──────┘
         │
         ▼
  ┌─────────────┐
  │ Genome      │  Map reads to hg38 reference
  │ Alignment   │  98% alignment rate (BWA-MEM2)
  └──────┬──────┘
         │
         ▼
  ┌─────────────┐
  │ Variant     │  Call SNPs and indels
  │ Calling     │  ~4-5M variants per genome (GATK)
  └─────────────┘
```

### Workflow Orchestration

```python
@workflow_actor
class GenomicsPipelineCoordinator:
    """Durable genomics pipeline with pause/resume/cancel support."""

    pipeline_state: str = state(default="idle")

    @handler("run_pipeline")
    def run_pipeline(self, reads: list = None, sample_id: str = "") -> dict:
        self.pipeline_state = "running"

        # Stage 1: Quality Control (scatter across QC workers)
        passed_reads = self._scatter_qc(reads, sample_id)

        # Stage 2: Alignment (scatter across alignment workers)
        aligned_reads = self._scatter_align(passed_reads, sample_id)

        # Stage 3: Variant Calling
        variants = self._scatter_variants(aligned_reads, sample_id)

        self.pipeline_state = "idle"
        return {
            "sample_id": sample_id,
            "qc_passed": len(passed_reads),
            "aligned": len(aligned_reads),
            "variants_found": len(variants),
        }

    @handler("pause")
    def pause(self) -> dict:
        self.pipeline_state = "paused"
        return {"status": "paused"}

    @handler("resume")
    def resume(self) -> dict:
        self.pipeline_state = "idle"
        return {"status": "resumed"}
```

Each stage uses specialized workers joined via process groups. The coordinator distributes work across workers using round-robin partitioning, and each stage's output feeds directly into the next.

### Why Actors for Genomics Pipelines?

Genomics pipelines are the canonical use case for workflow actors:

1. **Pause/resume for operational control**: A running pipeline can be paused via `@handler("pause")` when downstream storage is full, then resumed when capacity is available. In Airflow/Nextflow, you would have to kill the job and restart from scratch.

2. **Durable progress tracking**: The coordinator's `total_reads_passed_qc`, `total_reads_aligned`, and `total_variants` state fields are checkpointed automatically. If the coordinator crashes after QC completes but before alignment finishes, it recovers and resumes alignment — it does not re-run QC.

3. **Heterogeneous resource allocation**: QC workers need CPU only. Alignment workers need high memory (hg38 reference genome is ~3GB in RAM). Variant callers need CPU + moderate memory. Resource-based routing places each stage on appropriate hardware automatically.

---

## Example 5: Matrix Multiplication (Scatter-Gather)

This example demonstrates the purest form of the data-parallel actor pattern: scatter rows to workers, compute in parallel, gather results — ported from `examples/rust/embedded/matrix_multiply/`.

### Data-Parallel Pattern

```python
@actor
class MatrixMaster:
    """Partitions, scatters, and gathers matrix multiplication."""

    @handler("multiply")
    def multiply(self, matrix_a: list = None, matrix_b: list = None) -> dict:
        n_rows = len(matrix_a)
        rows_per_worker = n_rows // self.num_workers

        # Scatter: partition A rows across workers
        worker_results = []
        for w in range(self.num_workers):
            row_start = w * rows_per_worker
            row_end = min(row_start + rows_per_worker, n_rows)
            a_partition = matrix_a[row_start:row_end]

            resp = host.ask(self.worker_ids[w], "compute_rows", {
                "matrix_a_rows": a_partition,
                "matrix_b": matrix_b,  # Each worker needs all of B
                "start_row": row_start,
            }, timeout_ms=60000)
            worker_results.append(resp)

        # Gather: assemble result matrix (sorted by start_row)
        worker_results.sort(key=lambda r: r["start_row"])
        result = []
        for wr in worker_results:
            result.extend(wr["rows"])

        return {"result": result, "gflops": total_flops / elapsed}
```

Each `MatrixWorker` computes its assigned rows independently — no inter-worker communication:

```python
@actor
class MatrixWorker:
    @handler("compute_rows")
    def compute_rows(self, matrix_a_rows: list, matrix_b: list,
                     start_row: int = 0) -> dict:
        result_rows = []
        for a_row in matrix_a_rows:
            c_row = []
            for j in range(len(matrix_b[0])):
                total = sum(a_row[k] * matrix_b[k][j]
                           for k in range(len(a_row)))
                c_row.append(total)
            result_rows.append(c_row)
        return {"rows": result_rows, "start_row": start_row}
```

This is coordination-free execution: each worker is a pure function from (A_rows, B) to C_rows. The scatter-gather pattern handles all distribution.

---

## Putting It All Together: Multi-Node Cluster Deployment

Here is how you deploy these examples on a production cluster with heterogeneous hardware:

### Node Configuration

```bash
# Node 1: CPU node (preprocessing, parsing, chunking, QC)
plexspaces start \
    --node-id cpu-node-1 \
    --labels "accelerator=cpu,role=worker" \
    --resources "cpu_cores=32,memory_bytes=68719476736"

# Node 2: GPU node (inference, embedding, variant calling)
plexspaces start \
    --node-id gpu-node-1 \
    --labels "accelerator=gpu,gpu_type=nvidia-a100" \
    --resources "cpu_cores=16,memory_bytes=137438953472,gpu_count=4"

# Node 3: High-memory node (alignment, scientific computing)
plexspaces start \
    --node-id mem-node-1 \
    --labels "accelerator=cpu,role=alignment,memory=high" \
    --resources "cpu_cores=64,memory_bytes=549755813888"
```

### Resource-Based Placement

The framework's node selector automatically places actors on appropriate nodes:

```
Actor                    Required Labels               Node Placed
─────────────────────────────────────────────────────────────────
ImagePreprocessor        {accelerator: cpu}             cpu-node-1
ModelInferenceWorker     {accelerator: gpu}             gpu-node-1
EmbeddingWorker          {accelerator: gpu}             gpu-node-1
AlignmentWorker          {role: alignment}              mem-node-1
QCWorker                 {accelerator: cpu}             cpu-node-1
GridRegionActor          {accelerator: cpu}             cpu-node-1
MatrixWorker             {accelerator: cpu}             cpu-node-1
```

### HTTP Gateway: Serverless Actor Invocation

Every deployed actor is automatically accessible via PlexSpaces' HTTP gateway — no additional configuration needed. This turns actors into serverless functions callable with plain `curl`:

```bash
# Request-reply (ask): POST returns the handler's response
curl -X POST http://localhost:8001/api/v1/actors/coordinator/ask \
    -H "Content-Type: application/json" \
    -d '{"message_type": "classify_images", "payload": {...}}'

# Fire-and-forget (tell): POST returns immediately, processing is async
curl -X POST http://localhost:8001/api/v1/actors/coordinator/tell \
    -d '{"message_type": "log_event", "payload": {...}}'

# GET for read-only queries (routed via ask)
curl http://localhost:8001/api/v1/actors/coordinator/ask?msg_type=pipeline_stats
```

This means external systems — data loaders, monitoring dashboards, CI/CD pipelines — can invoke actors via HTTP without client libraries or SDKs. Webhooks trigger ingestion pipelines. Monitoring tools query pipeline stats. The gateway handles routing, serialization, and timeout management.

### Scaling the Pipeline

```bash
# Scale inference workers from 2 to 8 (add more GPU shards)
curl -X POST http://localhost:8001/api/v1/shard-groups/inference-pool/scale \
    -d '{"target_shard_count": 8}'

# The framework:
# 1. Creates new inference actor instances (shards 2-7)
# 2. Places them on GPU nodes via resource-based routing
# 3. Rebalances partition assignments (consistent hashing → minimal key movement)
# 4. New shards join the "pipeline-gpu-workers" process group
# 5. Coordinator discovers them via host.process_groups.members()
```

---

## Framework Comparison: PlexSpaces vs Ray vs Spark vs Lambda+SQS

| Capability | PlexSpaces | Ray | Spark | Lambda+SQS |
|---|---|---|---|---|
| **Compute unit** | Actor (stateful) | Task/Actor (mixed) | RDD/DataFrame | Function (stateless) |
| **State** | Built-in (durable) | External (Redis/S3) | External (HDFS) | External (DynamoDB) |
| **GPU routing** | Resource labels | `num_gpus=1` | RAPIDS plugin | Lambda layers |
| **Fault tolerance** | Supervision trees | Task retry | Stage retry | DLQ + retry |
| **Coordination** | TupleSpace, process groups | Object store | Shuffle | SQS, Step Functions |
| **Scaling** | Shard groups, worker pools | Autoscaler | Executors | Concurrency limits |
| **Multi-step pipelines** | Workflow actors | Ray DAG | Spark SQL | Step Functions |
| **Cold start** | Microseconds (WASM AOT) | ~100ms (Python) | ~10s (JVM) | 100ms-10s |
| **Message queues** | Built-in channels (6+ backends) | External (Kafka/SQS) | External | SQS |
| **Polyglot** | Rust, Python, Go, TS (same runtime) | Python primary | JVM + PySpark | Per-runtime images |
| **Deployment** | `plexspaces start` or Docker | `ray start` + KubeRay | Spark submit + YARN | CloudFormation |

### When to Use What

**Use PlexSpaces when:**
- You need stateful actors with built-in persistence
- Your pipeline has heterogeneous stages (CPU + GPU + high-memory)
- You want Erlang-style fault tolerance (supervision trees, not just retries)
- You need coordination primitives (TupleSpace, process groups) without external infrastructure
- You are building a polyglot system (Rust + Python + Go + TypeScript)

**Use Ray when:**
- You are primarily a Python shop doing ML training and inference
- You need tight integration with PyTorch/TensorFlow/JAX
- Your workloads are mostly stateless batch transformations
- You want Ray Serve for online model serving

**Use Spark when:**
- You are doing SQL-heavy analytics on structured data
- You need ACID transactions (Delta Lake)
- Your data is in Parquet/ORC/Iceberg format
- You are already in the Hadoop/Databricks ecosystem

**Use Lambda+SQS when:**
- Your workloads are event-driven and bursty
- You want zero infrastructure management
- Each function invocation is independent
- You are fully committed to AWS

---

## The MPI Connection: From HPC to Actors

Traditional scientific computing uses MPI (Message Passing Interface) for parallel computation. PlexSpaces maps MPI concepts to actor primitives:

| MPI Concept | PlexSpaces Equivalent | Example |
|---|---|---|
| `MPI_Send` / `MPI_Recv` | `host.send()` / `host.ask()` | Point-to-point messaging |
| `MPI_Bcast` | `host.process_groups.broadcast()` | Broadcast to all workers |
| `MPI_Scatter` / `MPI_Gather` | Shard group `parallel_map` / `parallel_reduce` | Data distribution |
| `MPI_Allreduce` | Scatter-gather with aggregation | Global reduction |
| `MPI_Barrier` | TupleSpace barrier tuples | Synchronization |
| Communicator | Process group | Named group of participants |
| Rank | Actor ID / shard_id | Worker identity |

The PlexSpaces `mpi_collectives` example (`examples/rust/embedded/mpi_collectives/`) implements broadcast, scatter, gather, all-reduce, and barrier using these actor primitives, demonstrating that the full MPI programming model maps naturally onto the actor framework.

The advantage over raw MPI: actors provide fault tolerance (supervision trees restart crashed workers), dynamic membership (process groups vs static rank allocation), and heterogeneous hardware support (resource-based routing vs uniform MPI ranks).

---

## Running the Examples

All five examples are available under `examples/python/apps/`:

```bash
# Clone and setup
git clone https://github.com/bhatti/PlexSpaces.git
cd PlexSpaces
./scripts/setup.sh

# Build all Python WASM actors
for app in batch_image_classification data_lake_ingestion heat_diffusion \
           genomics_pipeline matrix_multiply; do
    cd examples/python/apps/$app
    ./build.sh
    cd ../../../..
done

# Start a PlexSpaces node
docker run -d -p 8000:8000 -p 8001:8001 \
    -e PLEXSPACES_NODE_ID=dev-node \
    plexobject/plexspaces:latest

# Deploy and test each example
curl -X POST http://localhost:8001/api/v1/applications/deploy \
    -F "application_id=batch-inference" \
    -F "name=image-classifier" \
    -F "version=1.0.0" \
    -F "wasm_file=@examples/python/apps/batch_image_classification/batch_inference_actors.wasm"
```

Each example directory contains:
- `*_actor.py` or `*_actors.py`: Actor source code
- `app-config.toml`: Supervisor configuration
- `build.sh`: WASM build script

The examples also exist in Rust (`examples/rust/embedded/`) with full benchmarking and metrics.

---

## What Comes Next

The convergence of data processing, AI inference, and scientific computing into a single actor-based framework opens several directions:

**GPU-native WASM**: As WASM gains GPU compute capabilities, PlexSpaces actors will execute model inference directly inside the WASM sandbox — no host-side Python or CUDA driver needed.

**Streaming execution**: PlexSpaces channels already support Kafka and Redis Streams backends. The next step is streaming execution within actor pipelines — continuous ingestion, not just batch processing — bridging the gap between batch and stream processing.

**Federated data processing**: PlexSpaces actors running at the edge (CDN nodes, IoT devices) process data locally and synchronize through tuple spaces. A genomics pipeline runs on hospital hardware; embeddings sync to the cloud.

**Auto-scaling based on pipeline pressure**: The framework already tracks actor message queue depth and processing latency. Combining this with shard group scaling provides automatic backpressure-driven scaling — scale up inference workers when the preprocess-to-inference queue grows.

The examples in this post are intentionally practical — real pipelines you would build for production AI and data workloads. The framework handles the distributed systems engineering. You focus on the data processing logic.

---

*PlexSpaces is available at [github.com/bhatti/PlexSpaces](https://github.com/bhatti/PlexSpaces). The examples from this post are in `examples/python/apps/`. Star the repo, try the examples, and join the conversation.*
