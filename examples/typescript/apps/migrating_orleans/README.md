# Orleans vs PlexSpaces Comparison - Batch Prediction (TypeScript)

**Real-world use case**: ML inference pipeline with virtual actors, model caching, and batch processing.

This comparison demonstrates how to implement batch prediction with virtual actors (grains) in both Microsoft Orleans (C#) and PlexSpaces (TypeScript WASM), showcasing model caching and parallel processing.

## Use Case: Batch Prediction with Model Caching

A virtual actor system that:
- Loads and caches ML models in actors (efficient reuse)
- Processes large batches of data points (10K+)
- Uses timers for periodic batch processing
- Uses reminders for scheduled batch jobs
- Demonstrates virtual actor lifecycle with model caching

**Native Orleans**: C# (Microsoft Orleans framework)  
**PlexSpaces**: TypeScript WASM actor using `@plexspaces/sdk`

---

## Quick Start

### Prerequisites

- **Node.js** (v18+) and **npm**
- **PlexSpaces node** running (start with `./scripts/server.sh` from repo root)

### Build and Test

```bash
# Build TypeScript → WASM
./build.sh

# Verify actor logic (no server)
npm test

# Full E2E test (requires running node)
# Terminal 1: Start node (from repo root)
cd ../../../../ && ./scripts/server.sh

# Terminal 2: Run test (from example directory)
cd examples/typescript/apps/migrating_orleans
./test.sh 8092  # HTTP port (default: 8092)
```

---

## Orleans Implementation

### Native C# Code

**See**: `native/BatchPredictorGrain.cs` for complete Orleans implementation.

**Key Features:**
- **Virtual Grains**: Automatic activation/deactivation (always addressable, activated on-demand)
- **Model Caching**: Model loaded once per grain, cached in memory
- **Timers**: Built-in `RegisterTimer` for periodic operations
- **Reminders**: Built-in `RegisterReminder` for durable scheduled jobs
- **State Persistence**: Grain state persisted automatically (if configured)

```csharp
using Orleans;

public interface IBatchPredictorGrain : IGrainWithStringKey
{
    Task LoadModel(string modelId);
    Task<List<Prediction>> PredictBatch(string shardPath, List<DataPoint> data);
    Task StartPeriodicBatch(int intervalSecs);
    Task ScheduleBatchJob(string jobId, DateTime scheduledTime);
    Task<Stats> GetStats();
}

public class BatchPredictorGrain : Grain, IBatchPredictorGrain
{
    private MLModel _model;
    private long _processedCount = 0;
    private string _modelId = null;
    private bool _modelLoaded = false;

    // Orleans automatically activates grain on first message
    public override Task OnActivateAsync()
    {
        // Grain activated - initialize state
        return base.OnActivateAsync();
    }

    public Task LoadModel(string modelId)
    {
        // Load model once, cache in grain (persists until grain deactivates)
        _model = MLModel.Load(modelId); // Load from storage (S3, HDFS, etc.)
        _modelId = modelId;
        _modelLoaded = true;
        return Task.CompletedTask;
    }

    public Task<List<Prediction>> PredictBatch(string shardPath, List<DataPoint> data)
    {
        // Model already cached - no reload needed
        if (!_modelLoaded)
        {
            // Auto-load default model if not loaded
            _model = MLModel.Load("default-model");
            _modelLoaded = true;
        }
        
        var predictions = _model.Predict(data); // Use cached model
        _processedCount += predictions.Count;
        return Task.FromResult(predictions);
    }

    public Task<Stats> GetStats()
    {
        return Task.FromResult(new Stats
        {
            ProcessedCount = _processedCount,
            ModelLoaded = _modelLoaded,
            ModelId = _modelId,
        });
    }

    public Task StartPeriodicBatch(int intervalSecs)
    {
        // Orleans built-in timer registration
        RegisterTimer(async _ => 
        {
            await ProcessPeriodicBatch();
        }, null, 
        TimeSpan.FromSeconds(1), // Initial delay
        TimeSpan.FromSeconds(intervalSecs)); // Periodic interval
        return Task.CompletedTask;
    }

    public async Task ScheduleBatchJob(string jobId, DateTime scheduledTime)
    {
        // Orleans built-in reminder registration (durable)
        await RegisterOrUpdateReminder(
            jobId, 
            TimeSpan.FromSeconds(10), // Due time
            TimeSpan.FromSeconds(30)); // Period
    }

    private async Task ProcessPeriodicBatch()
    {
        // Periodic batch processing logic
        var data = await FetchBatchData();
        await PredictBatch("periodic-batch", data);
    }
}
```

**Orleans Characteristics:**
- ✅ **Virtual Grains**: Always addressable, activated on first message
- ✅ **Automatic Deactivation**: Grains deactivate after idle timeout
- ✅ **Built-in Timers**: `RegisterTimer` for periodic operations
- ✅ **Built-in Reminders**: `RegisterReminder` for durable scheduled jobs
- ✅ **State Persistence**: Grain state persisted automatically (if configured)
- ✅ **Model Caching**: Model loaded once per grain, reused for all predictions

---

## PlexSpaces Implementation

### TypeScript WASM Actor

**Key Features:**
- **Virtual Actors**: Via `VirtualActorFacet` (configured in `app-config.toml`)
- **Model Caching**: Model loaded once per actor, cached in actor state
- **Timers**: Via `TimerFacet` (configured in `app-config.toml`)
- **Reminders**: Via `ReminderFacet` (configured in `app-config.toml`)
- **State Persistence**: Via `DurabilityFacet` (optional, configured in `app-config.toml`)

```typescript
import { PlexSpacesActor } from "@plexspaces/sdk";

interface BatchPredictorState {
  model_id: string | null;
  model_loaded: boolean;
  processed_count: number;
  model_payload_size_mb: number;
}

export class BatchPredictorActor extends PlexSpacesActor<BatchPredictorState> {
  getDefaultState(): BatchPredictorState {
    return {
      model_id: null,
      model_loaded: false,
      processed_count: 0,
      model_payload_size_mb: 0,
    };
  }

  protected override onInit(config: Record<string, unknown>): void {
    // Actor initialized - setup initial state
    this.state.model_id = String(config.model_id ?? null);
    this.state.model_loaded = false;
    this.state.processed_count = 0;
    this.state.model_payload_size_mb = 0;
  }

  onLoad_model(payload: Record<string, unknown>): Record<string, unknown> {
    const modelId = String(payload.model_id ?? "default-model");
    // Load model once, cache in actor state (persists until actor deactivates)
    // In production, load from storage (S3, HDFS, etc.)
    this.state.model_id = modelId;
    this.state.model_loaded = true;
    this.state.model_payload_size_mb = 10; // Simulated model size
    return {
      status: "ok",
      model_id: modelId,
      model_loaded: true,
      model_size_mb: this.state.model_payload_size_mb,
    };
  }

  onPredict_batch(payload: Record<string, unknown>): Record<string, unknown> {
    const computeStart = performance.now();
    const shardPath = String(payload.shard_path ?? "");
    const dataPoints = (payload.data ?? []) as DataPoint[];
    
    // Model already cached - no reload needed
    if (!this.state.model_loaded) {
      // Auto-load default model if not loaded
      this.state.model_loaded = true;
      this.state.model_id = "default-model";
      this.state.model_payload_size_mb = 10;
    }
    
    // Process predictions using cached model
    const predictions = this.processPredictions(dataPoints);
    this.state.processed_count += predictions.length;
    
    const computeTime = performance.now() - computeStart;
    
    return {
      status: "ok",
      shard_path: shardPath,
      predictions,
      count: predictions.length,
      total_processed: this.state.processed_count,
      compute_time_ms: Math.round(computeTime * 100) / 100,
    };
  }

  onGet_stats(): Record<string, unknown> {
    return {
      processed_count: this.state.processed_count,
      model_loaded: this.state.model_loaded,
      model_id: this.state.model_id,
      model_size_mb: this.state.model_payload_size_mb,
    };
  }

  onStart_periodic_batch(payload: Record<string, unknown>): Record<string, unknown> {
    // Timer registration handled by framework (TimerFacet via app-config.toml)
    return {
      status: "ok",
      message: "Timer registration handled by framework (TimerFacet)",
    };
  }

  onSchedule_batch_job(payload: Record<string, unknown>): Record<string, unknown> {
    // Reminder registration handled by framework (ReminderFacet via app-config.toml)
    return {
      status: "ok",
      message: "Reminder registration handled by framework (ReminderFacet)",
    };
  }

  private processPredictions(dataPoints: DataPoint[]): Prediction[] {
    // Simulated ML inference (sum features, compute score)
    // Matches Orleans pattern: model.Predict(data) → predictions
    const predictions: Prediction[] = [];
    const dataPointsCount = Math.min(dataPoints.length, 10); // Limit for WASM safety
    
    for (let i = 0; i < dataPointsCount; i++) {
      const point = dataPoints[i];
      if (!point || typeof point !== 'object') continue;
      
      const features = Array.isArray(point.features) ? point.features : [];
      let sum = 0;
      const featuresCount = Math.min(features.length, 100);
      
      for (let j = 0; j < featuresCount; j++) {
        const val = features[j];
        if (typeof val === 'number') {
          sum += val;
        }
      }
      
      predictions.push({
        data_id: String(point.id ?? `data-${i}`),
        score: sum % 2, // Binary classification (0 or 1)
        timestamp: 1771041911, // Fixed timestamp
      });
    }
    return predictions;
  }
}
```

**PlexSpaces Characteristics:**
- ✅ **Virtual Actors**: Via `VirtualActorFacet` (configured in `app-config.toml`)
- ✅ **Automatic Activation**: Actor activated on first message via VirtualActorFacet
- ✅ **Automatic Deactivation**: Actor deactivated after idle timeout (5m default)
- ✅ **Timers**: Via `TimerFacet` (configured in `app-config.toml`, not SDK)
- ✅ **Reminders**: Via `ReminderFacet` (configured in `app-config.toml`, not SDK)
- ✅ **State Persistence**: Via `DurabilityFacet` (optional, configured in `app-config.toml`)
- ✅ **Model Caching**: Model loaded once per actor, cached in actor state
- ✅ **WASM Deployment**: TypeScript compiled to WASM, deployed as component

---

## SDK Gaps: TypeScript vs Rust

**⚠️ Important**: TypeScript SDK (and Python/Go SDKs) currently lack parity with Rust SDK.

### Missing Features in TypeScript SDK

| Feature | Rust SDK | TypeScript SDK | Workaround |
|---------|----------|----------------|------------|
| **Facet annotations** | `#[gen_server_actor(facets = ["virtual_actor", "timer", "reminder"])]` | ❌ Not supported | Configure via `app-config.toml` facets array |
| **Spawn helpers** | `spawn_with_facets()`, `spawn_with_storage()` | ❌ Not supported | Handled by framework deployment |
| **CoordinationComputeTracker** | ✅ Available | ❌ Not available | Metrics provided by framework runtime |
| **Message helpers** | `call_message()`, `cast_message()` | ❌ Not supported | Use JSON payload directly |
| **Timer registration** | `TimerFacet::register_periodic()` | ❌ Not supported | Configure via framework (TimerFacet) |
| **Reminder registration** | `ReminderFacet::register_reminder()` | ❌ Not supported | Configure via framework (ReminderFacet) |

### How Facets Work in TypeScript

**Facets are configured at deployment time**, not in the SDK:

1. **VirtualActorFacet**: Configured via framework deployment (automatic activation/deactivation)
2. **TimerFacet**: Configured via framework deployment (periodic timers)
3. **ReminderFacet**: Configured via framework deployment (durable reminders)
4. **DurabilityFacet**: Configured via framework deployment (state persistence)

The TypeScript SDK focuses on **actor logic** (handlers, state management), while **facets** are framework capabilities configured during deployment.

**Planned Improvements**: Facet annotations, spawn helpers, message helpers, and metrics support are planned for future SDK releases. See [PlexSpaces SDK Documentation](../../../../docs/sdk.md) for roadmap.

---

## Key Differences: Orleans vs PlexSpaces

### Architecture Differences

| Aspect | Orleans (C#) | PlexSpaces (TypeScript WASM) |
|--------|--------------|------------------------------|
| **Language** | C# (.NET) | TypeScript (compiled to WASM) |
| **Deployment** | .NET assemblies | WASM components |
| **Virtual Actors** | Built-in `Grain` base class | `VirtualActorFacet` (configured in `app-config.toml`) |
| **Activation** | Automatic via `OnActivateAsync()` | Automatic via `VirtualActorFacet` (lazy activation) |
| **Deactivation** | Automatic after idle timeout | Automatic via `VirtualActorFacet` (5m idle timeout) |
| **Model Caching** | Instance variable in grain | State property in actor |
| **Timers** | `RegisterTimer()` method | `TimerFacet` (configured in `app-config.toml`) |
| **Reminders** | `RegisterReminder()` method | `ReminderFacet` (configured in `app-config.toml`) |
| **State Persistence** | Grain state (automatic if configured) | `DurabilityFacet` (optional, configured in `app-config.toml`) |
| **Message Handling** | Interface methods (`Task<T>`) | Handler methods (`on<Op>()`) |
| **Metrics** | Built-in Orleans metrics | Framework metrics + `compute_time_ms` in response |

### Code Pattern Differences

#### Orleans Pattern (C#)
```csharp
// Interface defines grain contract
public interface IBatchPredictorGrain : IGrainWithStringKey
{
    Task LoadModel(string modelId);
    Task<List<Prediction>> PredictBatch(string shardPath, List<DataPoint> data);
}

// Implementation inherits from Grain
public class BatchPredictorGrain : Grain, IBatchPredictorGrain
{
    private MLModel _model; // Instance variable (cached)
    
    public Task LoadModel(string modelId)
    {
        _model = MLModel.Load(modelId); // Direct method call
        return Task.CompletedTask;
    }
    
    public Task StartPeriodicBatch(int intervalSecs)
    {
        RegisterTimer(...); // Built-in timer registration
        return Task.CompletedTask;
    }
}
```

#### PlexSpaces Pattern (TypeScript WASM)
```typescript
// Actor extends PlexSpacesActor base class
export class BatchPredictorActor extends PlexSpacesActor<BatchPredictorState> {
  private state: BatchPredictorState; // State property (cached)
  
  onLoad_model(payload: Record<string, unknown>) {
    // Handler method (not interface method)
    this.state.model_id = String(payload.model_id);
    this.state.model_loaded = true;
    return { status: "ok", model_id: this.state.model_id };
  }
  
  onStart_periodic_batch(payload: Record<string, unknown>) {
    // Timer registration handled by framework (TimerFacet)
    // Configured in app-config.toml, not SDK
    return { status: "ok", message: "Timer handled by framework" };
  }
}
```

### Configuration Differences

#### Orleans Configuration
```csharp
// Timers and reminders registered in code
RegisterTimer(async _ => await ProcessBatch(), null, 
    TimeSpan.FromSeconds(1), TimeSpan.FromSeconds(intervalSecs));

await RegisterOrUpdateReminder(jobId, 
    TimeSpan.FromSeconds(10), TimeSpan.FromSeconds(30));
```

#### PlexSpaces Configuration
```toml
# app-config.toml - Facets configured at deployment time
[[supervisor.children]]
id = "batch-predictor-model-1"
facets = [
  { type = "virtual_actor", priority = 100, 
    config = { idle_timeout = "5m", activation_strategy = "lazy" } },
  { type = "timer", priority = 50,
    config = { interval_secs = 5, handler = "onPeriodicBatch" } },
  { type = "reminder", priority = 50,
    config = { due_time_secs = 10, period_secs = 30 } }
]
```

### Metrics Differences

#### Orleans Metrics
- Built-in Orleans metrics (grain activation, message count, latency)
- Custom metrics via `GetGrainMetrics()`
- No explicit coordination vs computation breakdown

#### PlexSpaces Metrics
- Framework-provided metrics (coordination vs computation)
- Actor returns `compute_time_ms` in response
- Test script tracks coordination (HTTP overhead) vs computation (actor work)
- Granularity ratio (compute/coordinate) displayed in test output

### Deployment Differences

#### Orleans Deployment
```bash
# Deploy .NET assemblies to Orleans silo
dotnet publish
# Orleans silo loads assemblies and registers grains
```

#### PlexSpaces Deployment
```bash
# Build TypeScript → WASM
./build.sh  # tsc → esbuild bundle → jco componentize

# Deploy WASM component via HTTP API
curl -X POST http://localhost:8092/api/v1/applications/deploy \
  -F "application_id=orleans-batch-predictor" \
  -F "name=orleans-batch-predictor" \
  -F "version=1.0.0" \
  -F "wasm_file=@batch_predictor_actor.wasm" \
  -F "config=@app-config.toml"
```

**SDK Simplification**:
- **WIT Types**: Automatically generated by SDK during build (`npm run build` in SDK) - no need to run `jco types` or import generated files
- **Iterative Serializer**: SDK uses iterative JSON serialization to avoid WASM recursion issues in StarlingMonkey (the JavaScript engine used by jco componentize)
- **Simple API**: Just extend `PlexSpacesActor` and implement handlers - SDK handles all WIT interface details, serialization, and boilerplate

---

## Side-by-Side Comparison

| Feature | Orleans (C#) | PlexSpaces (TypeScript WASM) |
|---------|--------------|------------------------------|
| **Virtual Actors** | Built-in `Grain` base class | `VirtualActorFacet` (framework config) |
| **Model Caching** | Instance variable in grain | State property in actor |
| **Activation** | `OnActivateAsync()` override | Automatic via `VirtualActorFacet` |
| **Deactivation** | Automatic after idle timeout | Automatic via `VirtualActorFacet` (5m) |
| **Timers** | `RegisterTimer()` method | `TimerFacet` (framework config) |
| **Reminders** | `RegisterReminder()` method | `ReminderFacet` (framework config) |
| **State Persistence** | Grain state (automatic) | `DurabilityFacet` (optional) |
| **Message Handling** | Interface methods | Handler methods (`on<Op>()`) |
| **Metrics** | Orleans built-in metrics | Framework metrics + `compute_time_ms` |
| **Deployment** | .NET assemblies | WASM components |
| **Language** | C# | TypeScript |
| **Polyglot Support** | C# only | Rust, Python, TypeScript, Go |

---

## Operations

| Operation | Payload | Response |
|-----------|---------|----------|
| **Load Model** | `{"op":"load_model","model_id":"ml-model-v1"}` | `{"status":"ok","model_loaded":true,"model_size_mb":100}` |
| **Predict Batch** | `{"op":"predict_batch","shard_path":"s3://...","data":[...]}` | `{"status":"ok","predictions":[...],"count":N}` |
| **Get Stats** | `{"op":"get_stats"}` | `{"processed_count":N,"model_loaded":true}` |
| **Start Periodic** | `{"op":"start_periodic_batch","interval_secs":5}` | `{"status":"ok","message":"Timer registration handled by framework"}` |
| **Schedule Job** | `{"op":"schedule_batch_job","job_id":"...","scheduled_time":...}` | `{"status":"ok","message":"Reminder registration handled by framework"}` |

---

## Metrics & Benchmarks

### Test Output Example

The `test.sh` script displays comprehensive metrics matching the `heat_diffusion` example pattern:

```
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Step 5: Performance Metrics
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

Execution Summary:
  Total execution time: XXXms (X.XXs)
  Operations completed: X
  Batch predictions: X
  Data points processed: X

Coordination vs Computation Breakdown:
  Coordination time: XX.XXms (X.X%)
  Computation time: XX.XXms (X.X%)
  Efficiency (compute/total): X.X%

Message Metrics:
  Total messages sent: X
  Average latency per message: XX.XXms
  Message throughput: X.X msg/s

Granularity Analysis:
  Granularity ratio (compute/coordinate): X.XX
  ✅ Excellent/Good/Moderate/Poor granularity
```

### Metrics Tracking

**Coordination Time**: HTTP request/response overhead, message passing  
**Computation Time**: Actual prediction work (extracted from actor `compute_time_ms` response)  
**Granularity Ratio**: `compute_time / coordinate_time` (target: >= 10×, ideally >= 100×)

**Note**: TypeScript SDK doesn't have `CoordinationComputeTracker` (Rust-only). Metrics are tracked in the test script and extracted from actor responses (`compute_time_ms` field).

---

## Key Benefits of Model Caching

### Efficiency
- **Model loaded once**: Loaded when actor is activated, cached for reuse
- **No repeated loading**: Subsequent predictions reuse cached model
- **Memory efficient**: Model shared across all predictions in same actor

### Performance
- **Fast predictions**: No model loading overhead for each batch
- **Parallel processing**: Multiple actors can process batches in parallel
- **Resource optimization**: Model cached per actor, not per prediction

### Lifecycle
- **Automatic activation**: Actor activated on first message
- **Automatic deactivation**: Actor deactivated after idle timeout
- **Model reload**: Model reloaded on reactivation (or persisted via DurabilityFacet)

---

## Files

| File | Description |
|------|-------------|
| `batch_predictor_actor.ts` | TypeScript actor using `@plexspaces/sdk` |
| `native/BatchPredictorGrain.cs` | Native Orleans C# implementation (reference) |
| `app-config.toml` | Supervisor configuration (3 batch predictor actors) |
| `build.sh` | Build TypeScript → WASM (tsc + jco componentize) |
| `test.sh` | E2E test: deploy WASM and run HTTP operations with metrics |
| `verify.mjs` | In-process verification (no server, no WASM) |
| `build-bundle.mjs` | Bundle script for esbuild (creates bundle for jco) |

---

## Summary: Key Takeaways

### Orleans Advantages
- ✅ **Built-in Virtual Actors**: Native grain support with automatic activation/deactivation
- ✅ **Code-Based Configuration**: Timers and reminders registered in code (familiar pattern)
- ✅ **Strong Typing**: C# interface contracts provide compile-time safety
- ✅ **Mature Ecosystem**: Well-established framework with extensive documentation

### PlexSpaces Advantages
- ✅ **Polyglot Support**: TypeScript, Python, Rust, Go (not just C#)
- ✅ **WASM Deployment**: Deploy actors as WASM components (portable, sandboxed)
- ✅ **Framework Configuration**: Facets configured declaratively (separation of concerns)
- ✅ **Coordination Metrics**: Built-in coordination vs computation tracking
- ✅ **Multi-Tenancy**: Built-in tenant/namespace isolation
- ✅ **TupleSpace Coordination**: Extended Linda model for coordination

### Migration Path
1. **Replace Grain Interface** → Handler methods (`on<Op>()`)
2. **Replace Instance Variables** → State properties (`this.state`)
3. **Replace `RegisterTimer`** → Configure `TimerFacet` in `app-config.toml`
4. **Replace `RegisterReminder`** → Configure `ReminderFacet` in `app-config.toml`
5. **Replace Grain State** → Actor state (with optional `DurabilityFacet`)

### When to Use PlexSpaces
- ✅ Need polyglot actors (TypeScript, Python, Rust, Go)
- ✅ Need WASM deployment (portable, sandboxed)
- ✅ Need coordination metrics (coordination vs computation)
- ✅ Need multi-tenancy (tenant/namespace isolation)
- ✅ Need TupleSpace coordination (extended Linda model)

### When to Use Orleans
- ✅ C#/.NET ecosystem only
- ✅ Prefer code-based configuration
- ✅ Need mature ecosystem with extensive tooling
- ✅ Prefer interface-based contracts

---

## See Also

- [TypeScript SDK](../../../../sdks/typescript/README.md) – Inheritance-based actor base class
  - **WIT Types**: Automatically generated by SDK during build - no need to run `jco types` or import generated files
  - **Serialization**: SDK uses iterative JSON serializer to avoid WASM recursion issues
  - **Simple API**: Just extend `PlexSpacesActor` and implement handlers - SDK handles all WIT details
- [Orleans Documentation](https://dotnet.github.io/orleans/)
- [PlexSpaces SDK Documentation](../../../../docs/sdk.md) – SDK reference and roadmap
- [Heat Diffusion Example](../../../rust/embedded/heat_diffusion/README.md) – Similar metrics pattern

---

## License

LGPL-2.1-or-later
