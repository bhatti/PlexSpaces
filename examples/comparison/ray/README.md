# Ray vs PlexSpaces Comparison

This comparison demonstrates how to implement Ray's Parameter Server pattern for distributed ML training in both Ray and PlexSpaces.

**Based on**: [Ray Parameter Server Example](https://docs.ray.io/en/latest/ray-core/examples/plot_parameter_server.html)

## Use Case: Parameter Server for Distributed ML Training

A distributed ML training system that:
- Uses a centralized parameter server to manage model weights
- Distributes gradient computation across elastic worker pools
- Aggregates gradients from multiple workers
- Demonstrates synchronous and asynchronous training patterns
- Showcases horizontal scaling and resource-aware scheduling

## PlexSpaces Abstractions Showcased

This example demonstrates:
- ✅ **Parameter Server Pattern** - Centralized model weights management
- ✅ **Elastic Worker Pools** - Data workers scale horizontally
- ✅ **Gradient Aggregation** - Multiple workers compute gradients in parallel
- ✅ **Synchronous Training** - All workers compute gradients before update
- ✅ **Asynchronous Training** - Workers update as gradients arrive
- ✅ **Resource-Aware Scheduling** - Tasks distributed based on worker availability
- ✅ **Distributed ML Training** - Parallel gradient computation across worker pool

## Design Decisions

**Why Parameter Server Pattern?**
- Centralized weights: Single source of truth for model parameters
- Gradient aggregation: Combine gradients from multiple workers
- Synchronous/Asynchronous: Support both training patterns
- Industry standard: Used by TensorFlow, PyTorch, Ray

**Why Elastic Worker Pools?**
- Horizontal scaling: Add/remove workers based on dataset size
- Data parallelism: Each worker processes a shard of the dataset
- Resource-aware: Distribute computation based on worker availability
- Fault tolerance: Failed workers don't block entire training

**Why Actor Model for Parameter Server?**
- Stateful server: Parameter server maintains model weights
- Message passing: Workers send gradients, server sends weights
- Location transparency: Works across multiple nodes
- Natural fit: Matches Ray's actor-based parameter server pattern

---

## Ray Implementation

### Native Python Code

```python
# parameter_server.py
import ray
import torch
import torch.nn as nn

@ray.remote
class ParameterServer:
    def __init__(self, lr):
        self.model = ConvNet()
        self.optimizer = torch.optim.SGD(self.model.parameters(), lr=lr)

    def apply_gradients(self, *gradients):
        # Aggregate gradients from multiple workers
        summed_gradients = [
            np.stack(gradient_zip).sum(axis=0) 
            for gradient_zip in zip(*gradients)
        ]
        self.optimizer.zero_grad()
        self.model.set_gradients(summed_gradients)
        self.optimizer.step()
        return self.model.get_weights()

    def get_weights(self):
        return self.model.get_weights()

@ray.remote
class DataWorker:
    def __init__(self):
        self.model = ConvNet()
        self.data_iterator = iter(get_data_loader()[0])

    def compute_gradients(self, weights):
        self.model.set_weights(weights)
        data, target = next(self.data_iterator)
        self.model.zero_grad()
        output = self.model(data)
        loss = F.nll_loss(output, target)
        loss.backward()
        return self.model.get_gradients()

# Synchronous training
ps = ParameterServer.remote(1e-2)
workers = [DataWorker.remote() for i in range(4)]

current_weights = ps.get_weights.remote()
for i in range(iterations):
    # All workers compute gradients in parallel
    gradients = [worker.compute_gradients.remote(current_weights) 
                 for worker in workers]
    # Parameter server aggregates and applies
    current_weights = ps.apply_gradients.remote(*gradients)
        self.client = openai.OpenAI(api_key=api_key)
    
    def extract_entities(self, document: str) -> Dict[str, List[str]]:
        """Extract entities from document using LLM."""
        response = self.client.chat.completions.create(
            model="gpt-4",
            messages=[
                {"role": "system", "content": "Extract entities (person, organization, location) from the text."},
                {"role": "user", "content": document}
            ]
        )
        
        # Parse response and extract entities
        entities = self._parse_entities(response.choices[0].message.content)
        return entities
    
    def _parse_entities(self, text: str) -> Dict[str, List[str]]:
        # Simplified parsing
        return {
            "person": ["John Doe", "Jane Smith"],
            "organization": ["Acme Corp"],
            "location": ["New York", "San Francisco"]
        }

@ray.remote
class ResultAggregator:
    def __init__(self):
        self.results = []
    
    def add_result(self, result: Dict[str, List[str]]):
        self.results.append(result)
    
    def get_all_results(self) -> List[Dict[str, List[str]]]:
        return self.results

def main():
    ray.init()
    
    # Create document processor actors
    processors = [
        DocumentProcessor.remote(api_key="sk-...")
        for _ in range(4)  # 4 parallel processors
    ]
    
    # Create aggregator
    aggregator = ResultAggregator.remote()
    
    # Documents to process
    documents = [
        "John Doe works at Acme Corp in New York.",
        "Jane Smith visited San Francisco last week.",
        # ... more documents
    ]
    
    # Process documents in parallel
    futures = []
    for i, doc in enumerate(documents):
        processor = processors[i % len(processors)]
        future = processor.extract_entities.remote(doc)
        futures.append(future)
    
    # Wait for all results
    results = ray.get(futures)
    
    # Aggregate results
    for result in results:
        aggregator.add_result.remote(result)
    
    # Get final aggregated results
    all_results = ray.get(aggregator.get_all_results.remote())
    
    print(f"Processed {len(all_results)} documents")
    ray.shutdown()

if __name__ == "__main__":
    main()
```

### Running Ray Example

```bash
# Install Ray
pip install ray openai

# Run the example
python entity_recognition.py
```

---

## PlexSpaces Implementation

### Rust Implementation

See `src/main.rs` for the complete implementation.

### Key Differences

| Feature | Ray | PlexSpaces |
|---------|-----|------------|
| **Language** | Python | Rust |
| **Resource Scheduling** | @ray.remote decorator | Actor placement service |
| **Parallel Execution** | ray.get() | Actor coordination |
| **Result Aggregation** | Shared objects | TupleSpace |
| **Distribution** | Built-in | gRPC + ActorRef |

### Architecture Comparison

**Ray**:
```
DocumentProcessor (Python actor)
  └─ LLM API calls
      └─ ResultAggregator (shared object)
```

**PlexSpaces**:
```
DocumentProcessorActor (Rust actor)
  └─ LLM API calls
      └─ TupleSpace (distributed coordination)
          └─ AggregatorActor
```

---

## Side-by-Side Code Comparison

### Actor Definition

**Ray (Python)**:
```python
@ray.remote(num_cpus=2, num_gpus=0)
class DocumentProcessor:
    def extract_entities(self, document: str) -> Dict[str, List[str]]:
        # Process document
        return entities
```

**PlexSpaces (Rust)**:
```rust
pub struct DocumentProcessorActor {
    api_key: String,
}

impl ActorBehavior for DocumentProcessorActor {
    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        let document: String = serde_json::from_slice(msg.payload())?;
        let entities = self.extract_entities(&document).await?;
        
        // Write result to tuplespace
        ctx.tuplespace()
            .write(tuple!["entity_result", entities])
            .await?;
        
        Ok(())
    }
}
```

### Parallel Processing

**Ray (Python)**:
```python
futures = [processor.extract_entities.remote(doc) for doc in documents]
results = ray.get(futures)
```

**PlexSpaces (Rust)**:
```rust
// Spawn multiple processor actors using ActorFactory
use plexspaces_actor::{ActorFactory, actor_factory_impl::ActorFactoryImpl, Actor};
use plexspaces_mailbox::{mailbox_config_default, Mailbox};
use std::sync::Arc;

let actor_factory: Arc<ActorFactoryImpl> = node.service_locator().get_service().await
    .ok_or_else(|| "ActorFactory not found")?;
let mut processors = Vec::new();
for i in 0..4 {
    let behavior = Box::new(DocumentProcessorActor::new());
    let actor_id = format!("processor-{}@{}", i, node.id());
    let mut mailbox_config = mailbox_config_default();
    mailbox_config.storage_strategy = plexspaces_mailbox::StorageStrategy::Memory as i32;
    let mailbox = Mailbox::new(mailbox_config, format!("{}:mailbox", actor_id)).await?;
    let actor = Actor::new(actor_id.clone(), behavior, mailbox, "default".to_string(), None);
    let _message_sender = actor_factory.spawn_built_actor(Arc::new(actor), None, None, None).await?;
    processors.push(plexspaces_core::ActorRef::new(actor_id)?);
}

// Send documents to processors
for (i, doc) in documents.iter().enumerate() {
    let processor = &processors[i % processors.len()];
    processor.tell(Message::new(doc.as_bytes().to_vec())).await?;
}

// Read results from tuplespace
let results = tuplespace.read_all(pattern!["entity_result", _]).await?;
```

---

## Running the Comparison

### Prerequisites

```bash
# For Ray example (optional)
pip install ray openai

# For PlexSpaces example
cargo build --release
```

### Run PlexSpaces Example

```bash
# Run the example
cargo run --release

# Run tests
cargo test

# Run benchmarks
./scripts/benchmark.sh
```

---

## Performance Comparison

### Benchmarks

| Metric | Ray | PlexSpaces | Notes |
|--------|-----|------------|-------|
| **Task Latency** | <100ms | <50ms | Local execution |
| **Throughput** | 100K+ tasks/s | 50K+ tasks/s | Single node |
| **Resource Scheduling** | Built-in | PlacementService | Similar capabilities |
| **Memory per Actor** | ~50MB | ~5MB | Python vs Rust |
| **Cold Start** | <1s | <50ms | Actor activation |

*Note: Benchmarks are approximate. See `metrics/benchmark_results.json` for detailed results.*

---

## Feature Comparison

| Feature | Ray | PlexSpaces | Notes |
|---------|-----|------------|-------|
| **Resource Scheduling** | ✅ Built-in | ✅ PlacementService | Similar |
| **Parallel Execution** | ✅ Built-in | ✅ Actor coordination | Different model |
| **Distributed Execution** | ✅ Built-in | ✅ gRPC + ActorRef | Similar |
| **Result Aggregation** | ✅ Shared objects | ✅ TupleSpace | Different approach |
| **Python Support** | ✅ Native | ✅ WASM actors | Different model |
| **ML Libraries** | ✅ Rich ecosystem | ⚠️ Via WASM | Different approach |

---

## When to Use Each

### Use Ray When:
- ✅ Building Python ML applications
- ✅ Need rich ML library ecosystem
- ✅ Want Ray Tune, Ray Serve, etc.
- ✅ Prefer Python-native development

### Use PlexSpaces When:
- ✅ Need Rust performance
- ✅ Want unified actor + workflow model
- ✅ Need proto-first contracts
- ✅ Want WASM support (polyglot)
- ✅ Need Firecracker isolation
- ✅ Want TupleSpace coordination

---

## Design Decisions Explained

### Why Actor Model for ML?

**Ray Approach**: Python actors with @ray.remote decorator, automatic resource scheduling.

**PlexSpaces Approach**: Rust actors with PlacementService for resource-aware scheduling.

**Rationale**:
- **Performance**: Rust actors have lower overhead than Python
- **Resource Control**: Fine-grained control over actor placement
- **Composition**: Can combine ML workloads with workflows, coordination, etc.

### Why TupleSpace for Aggregation?

**Ray Approach**: Shared objects or return values.

**PlexSpaces Approach**: TupleSpace for decoupled coordination.

**Rationale**:
- **Decoupling**: Producers and consumers don't need to know each other
- **Flexibility**: Pattern matching enables flexible queries
- **Distribution**: Works seamlessly across nodes
- **Coordination**: Can coordinate multiple types of results

---

## References

- [Ray Documentation](https://docs.ray.io/)
- [PlexSpaces Actor Model](../../../../crates/actor)
- [PlexSpaces TupleSpace](../../../../crates/tuplespace)
- [PlexSpaces Placement Service](../../../../crates/node)
