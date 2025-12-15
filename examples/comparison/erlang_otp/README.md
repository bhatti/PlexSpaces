# Erlang/OTP vs PlexSpaces Comparison

This comparison demonstrates how to implement a GenServer with supervision in both Erlang/OTP and PlexSpaces.

## Use Case: Counter GenServer with Supervision

A simple counter that:
- Increments/decrements on command
- Handles crashes gracefully via supervision
- Demonstrates OTP behaviors and supervision trees

## PlexSpaces Abstractions Showcased

This example demonstrates:
- ✅ **GenServerBehavior** - Request-reply pattern (Erlang gen_server:call)
- ✅ **Supervision Trees** - Fault tolerance with automatic restart
- ✅ **Actor Model** - Core actor abstraction
- ✅ **Message Passing** - tell() and ask() patterns

## Design Decisions

**Why GenServerBehavior?**
- Matches Erlang/OTP's proven request-reply pattern
- Separates synchronous (call) from asynchronous (cast) communication
- Zero overhead - compile-time trait, not runtime facet

**Why Supervision Trees?**
- "Let it crash" philosophy - actors fail fast, supervisors restart
- Hierarchical fault tolerance - failures isolated to subtrees
- Proven pattern from Erlang/OTP (30+ years of production use)

---

## Erlang/OTP Implementation

### Native Erlang Code

See `native/counter.erl` for the complete Erlang/OTP implementation.

Key features:
- **GenServer Behavior**: Request-reply pattern (gen_server:call)
- **Supervision Trees**: Fault tolerance with automatic restart
- **Message Passing**: Asynchronous (cast) and synchronous (call) communication
- **State Management**: Stateful processes with isolated state

```erlang
% Usage:
{ok, Pid} = counter:start_link(),
Count = counter:increment(Pid),
Count = counter:get(Pid).
```

code_change(_OldVsn, State, _Extra) ->
    {ok, State}.

%% supervisor.erl
-module(counter_supervisor).
-behaviour(supervisor).

-export([start_link/0, init/1]).

start_link() ->
    supervisor:start_link({local, ?MODULE}, ?MODULE, []).

init([]) ->
    SupFlags = #{strategy => one_for_one,
                 intensity => 5,
                 period => 10},
    ChildSpecs = [#{id => counter,
                    start => {counter, start_link, []},
                    restart => permanent,
                    shutdown => 5000,
                    type => worker,
                    modules => [counter]}],
    {ok, {SupFlags, ChildSpecs}}.
```

### Running Erlang Example

```bash
# Start Erlang shell
erl

# Compile and run
1> c(counter).
2> c(counter_supervisor).
3> counter_supervisor:start_link().
4> counter:increment(counter).
5> counter:get(counter).
```

---

## PlexSpaces Implementation

### Rust Implementation

See `src/main.rs` for the complete implementation.

### Key Differences

| Feature | Erlang/OTP | PlexSpaces |
|---------|------------|------------|
| **Language** | Erlang | Rust |
| **Process Model** | BEAM processes | Tokio tasks |
| **Supervision** | Built-in OTP | Supervisor actor |
| **Message Passing** | `!` operator | `tell()` / `ask()` |
| **State** | Process dictionary | Struct fields |
| **Fault Tolerance** | "Let it crash" | Supervision trees |

### Architecture Comparison

**Erlang/OTP**:
```
Supervisor
  └─ GenServer (counter)
      └─ State (#state{count = 0})
```

**PlexSpaces**:
```
SupervisorActor
  └─ CounterActor (GenServer behavior)
      └─ State (CounterState { count: 0 })
```

---

## Running the Comparison

### Prerequisites

```bash
# For Erlang example (optional)
# Install Erlang/OTP: https://www.erlang.org/downloads

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

| Metric | Erlang/OTP | PlexSpaces | Notes |
|--------|------------|------------|-------|
| **Message Latency (P50)** | <1ms | <2ms | Local messages |
| **Message Latency (P99)** | <5ms | <10ms | Local messages |
| **Throughput** | 100K+ msg/s | 50K+ msg/s | Single actor |
| **Memory per Actor** | ~2KB | ~1KB | Minimal state |
| **Supervision Overhead** | <1% | <2% | Supervision tree |

*Note: Benchmarks are approximate and depend on hardware. See `metrics/benchmark_results.json` for detailed results.*

---

## Feature Comparison

| Feature | Erlang/OTP | PlexSpaces | Notes |
|---------|------------|------------|-------|
| **GenServer Behavior** | ✅ Built-in | ✅ GenServerBehavior trait | Similar API |
| **Supervision Trees** | ✅ OTP | ✅ SupervisorActor | Same patterns |
| **Hot Code Reload** | ✅ Built-in | ⚠️ Planned | Not yet implemented |
| **Process Isolation** | ✅ BEAM | ✅ Tokio tasks | Different model |
| **Fault Tolerance** | ✅ "Let it crash" | ✅ Supervision | Same philosophy |
| **Location Transparency** | ✅ Built-in | ✅ ActorRef | Similar |
| **Distribution** | ✅ Built-in | ✅ gRPC | Different protocol |

---

## Code Comparison

### Message Handling

**Erlang/OTP**:
```erlang
handle_call(increment, _From, State) ->
    NewCount = State#state.count + 1,
    {reply, NewCount, State#state{count = NewCount}}.
```

**PlexSpaces**:
```rust
async fn handle_call(&mut self, msg: CounterMessage, ctx: &ActorContext) -> Result<CounterMessage> {
    match msg {
        CounterMessage::Increment => {
            self.count += 1;
            Ok(CounterMessage::Count(self.count))
        }
        _ => Err(ActorError::UnknownMessage)
    }
}
```

### Supervision

**Erlang/OTP**:
```erlang
SupFlags = #{strategy => one_for_one,
             intensity => 5,
             period => 10}.
```

**PlexSpaces**:
```rust
let supervisor = SupervisorBuilder::new()
    .with_strategy(SupervisionStrategy::OneForOne)
    .with_max_restarts(5)
    .with_window(Duration::from_secs(10))
    .build();
```

---

## When to Use Each

### Use Erlang/OTP When:
- ✅ Need hot code reload
- ✅ Want mature ecosystem (OTP libraries)
- ✅ Prefer functional programming
- ✅ Need BEAM VM features (schedulers, distribution)

### Use PlexSpaces When:
- ✅ Need Rust performance
- ✅ Want proto-first contracts
- ✅ Need WASM support
- ✅ Want unified actor + workflow model
- ✅ Need Firecracker isolation

---

## Next Steps

- [ ] Add more complex OTP behaviors (gen_fsm, gen_event)
- [ ] Compare distribution patterns
- [ ] Benchmark large supervision trees
- [ ] Compare hot code reload (when implemented)

---

## References

- [Erlang/OTP Documentation](https://www.erlang.org/doc/)
- [PlexSpaces GenServer Behavior](../../../../crates/behavior)
- [PlexSpaces Supervision](../../../../crates/supervisor)
