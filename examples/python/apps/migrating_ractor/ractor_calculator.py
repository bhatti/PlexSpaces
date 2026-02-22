"""
Ractor Calculator - Type-Safe Actor Message Passing (Python WASM)

Demonstrates Ractor-style Rust-native actor patterns for a calculator:
- CalculatorActor handles typed arithmetic messages (Add, Subtract, Multiply, Divide)
- Operation counting and history tracking
- Batch operations for benchmarking throughput

Real-world use case: Computation service with type-safe RPC (like Ractor, Actix, Bastion).

## SDK Features Used

- @actor: Marks class as PlexSpaces actor
- state(): Persistent state fields (operation_count, history)
- @handler(): Routes messages by operation type
- @init_handler: Actor initialization from framework config
- host.now_ms(): Timing for benchmarks
"""

from plexspaces import actor, state, handler, init_handler, host


@actor
class CalculatorActor:
    """Calculator actor implementing Ractor-style typed message passing.

    In Ractor (Rust), actors receive typed enum messages:
        CalculatorMessage::Add { a: 10.0, b: 5.0, reply: port }

    In PlexSpaces, the @handler decorator routes by msg_type:
        @handler("add") -> def add(self, a, b)
    """

    # Persistent state
    operation_count: int = state(default=0)
    history: list = state(default_factory=list)
    actor_id: str = state(default="")

    # Benchmark tracking
    total_compute_ms: float = state(default=0.0)
    total_ops: int = state(default=0)

    @init_handler
    def on_init(self, config: dict):
        """Initialize calculator from framework config."""
        self.actor_id = config.get("actor_id", "calculator")
        self.operation_count = 0
        self.history = []
        self.total_compute_ms = 0.0
        self.total_ops = 0
        host.info(f"CalculatorActor {self.actor_id}: initialized")

    @handler("add")
    def add(self, a: float = 0, b: float = 0) -> dict:
        """Add two numbers. Ractor: CalculatorMessage::Add { a, b }"""
        start = host.now_ms()
        a, b = float(a), float(b)
        result = a + b
        self._record("add", a, b, result)
        elapsed = host.now_ms() - start
        self.total_compute_ms += elapsed
        return {"status": "ok", "operation": "add", "a": a, "b": b,
                "result": result, "op_number": self.operation_count}

    @handler("subtract")
    def subtract(self, a: float = 0, b: float = 0) -> dict:
        """Subtract b from a. Ractor: CalculatorMessage::Subtract { a, b }"""
        start = host.now_ms()
        a, b = float(a), float(b)
        result = a - b
        self._record("subtract", a, b, result)
        elapsed = host.now_ms() - start
        self.total_compute_ms += elapsed
        return {"status": "ok", "operation": "subtract", "a": a, "b": b,
                "result": result, "op_number": self.operation_count}

    @handler("multiply")
    def multiply(self, a: float = 0, b: float = 0) -> dict:
        """Multiply two numbers. Ractor: CalculatorMessage::Multiply { a, b }"""
        start = host.now_ms()
        a, b = float(a), float(b)
        result = a * b
        self._record("multiply", a, b, result)
        elapsed = host.now_ms() - start
        self.total_compute_ms += elapsed
        return {"status": "ok", "operation": "multiply", "a": a, "b": b,
                "result": result, "op_number": self.operation_count}

    @handler("divide")
    def divide(self, a: float = 0, b: float = 0) -> dict:
        """Divide a by b. Ractor: CalculatorMessage::Divide { a, b }"""
        start = host.now_ms()
        a, b = float(a), float(b)
        if b == 0:
            return {"status": "error", "error": "Division by zero"}
        result = a / b
        self._record("divide", a, b, result)
        elapsed = host.now_ms() - start
        self.total_compute_ms += elapsed
        return {"status": "ok", "operation": "divide", "a": a, "b": b,
                "result": result, "op_number": self.operation_count}

    @handler("batch")
    def batch(self, count: int = 1000) -> dict:
        """Run batch arithmetic operations for benchmarking throughput."""
        if not isinstance(count, int):
            count = int(count)
        start = host.now_ms()
        allowed = 0
        for i in range(count):
            a = float(i)
            b = float(i + 1)
            op = i % 4
            if op == 0:
                _ = a + b
            elif op == 1:
                _ = a - b
            elif op == 2:
                _ = a * b
            else:
                _ = a / b if b != 0 else 0
            allowed += 1
        elapsed = host.now_ms() - start
        self.total_compute_ms += elapsed
        self.total_ops += allowed
        ops_per_sec = (allowed / (elapsed / 1000.0)) if elapsed > 0 else 0
        return {
            "status": "ok",
            "total_operations": allowed,
            "duration_ms": elapsed,
            "ops_per_sec": ops_per_sec,
        }

    @handler("get_history")
    def get_history(self) -> dict:
        """Get operation history."""
        return {"status": "ok", "history": self.history,
                "operation_count": self.operation_count}

    @handler("stats")
    def get_stats(self) -> dict:
        """Get comprehensive statistics and benchmarks."""
        total_time = self.total_compute_ms
        ops_per_sec = (self.operation_count / (total_time / 1000.0)) if total_time > 0 else 0

        return {
            "status": "ok",
            "actor_id": self.actor_id,
            "counters": {
                "operation_count": self.operation_count,
                "batch_ops": self.total_ops,
            },
            "benchmarks": {
                "total_compute_ms": round(total_time, 2),
                "ops_per_sec": round(ops_per_sec, 2),
            },
            "history_size": len(self.history),
        }

    def _record(self, operation: str, a: float, b: float, result: float):
        """Record operation in history and increment counter."""
        self.operation_count += 1
        self.history.append({
            "op": operation, "a": a, "b": b,
            "result": result, "op_number": self.operation_count
        })
        # Keep history bounded
        if len(self.history) > 100:
            self.history = self.history[-100:]
