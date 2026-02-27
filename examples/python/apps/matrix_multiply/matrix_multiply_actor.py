# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 PlexSpaces Contributors
"""
Matrix Multiplication - Scatter-Gather Data Parallel Computing

Demonstrates parallel matrix multiplication using PlexSpaces actors,
ported from the Rust embedded example (examples/rust/embedded/matrix_multiply/).

## Architecture

    +------------------+
    |  MatrixMaster    |  Partitions rows, scatters to workers
    |  (Coordinator)   |  Gathers results, assembles output matrix
    +--------+---------+
             |  scatter: rows → workers (hash partition)
    +--------v---------+     Shard Group (data-parallel workers)
    |  MatrixWorker    |     Each worker computes C[i,j] for assigned rows
    |  Pool            |     C[i,j] = Σ_k A[i,k] * B[k,j]
    |  [0] [1] .. [N]  |
    +------------------+
             |  gather: partial results → master
    +--------v---------+
    |  Result Matrix   |
    +------------------+

## Data Parallel Actor Pattern (NSDI'22)

This implements the scatter-gather pattern from the Data-Parallel Actors paper:
- Each worker is a shard holding a partition of rows
- Scatter: master distributes row ranges to workers via hash partitioning
- Gather: workers return computed rows; master assembles the result
- Coordination-free: workers compute independently (no inter-worker communication)

## Framework Features Demonstrated

- **Shard Groups**: Hash-partitioned worker pool for row distribution
- **Scatter-Gather**: Fan-out computation, fan-in results
- **Data Parallel Actors**: Coordination-free parallel execution
- **Process Groups**: Worker pool membership
- **Benchmarking**: GFLOPS, throughput, coordination overhead
"""

from plexspaces import actor, state, handler, init_handler, host


@actor
class MatrixWorker:
    """Computes a partition of matrix multiplication rows.

    Each worker handles rows [start_row, end_row) of the result matrix C.
    C[i,j] = Σ_k A[i,k] * B[k,j] for i in [start_row, end_row)

    This is a stateless data-parallel worker: it receives input data
    (matrix partitions) and returns computed results. No inter-worker
    coordination is needed.
    """

    worker_id: str = state(default="")
    shard_id: int = state(default=0)
    rows_computed: int = state(default=0)
    total_compute_ms: float = state(default=0.0)
    total_flops: int = state(default=0)

    @init_handler
    def on_init(self, config: dict):
        self.worker_id = config.get("actor_id", "")
        args = config.get("args", {})
        self.shard_id = int(args.get("shard_id", 0))
        host.process_groups.join("matrix-workers")
        host.info(f"MatrixWorker shard {self.shard_id} ready")

    @handler("compute_rows")
    def compute_rows(self, matrix_a_rows: list = None, matrix_b: list = None,
                     start_row: int = 0, n_cols_b: int = 0,
                     batch_id: str = "", from_actor: str = "") -> dict:
        """Compute C rows for assigned partition.

        Args:
            matrix_a_rows: Rows of matrix A assigned to this worker [[a00, a01, ...], ...]
            matrix_b: Full matrix B (each worker needs all of B)
            start_row: Starting row index in the full result matrix
            n_cols_b: Number of columns in B (and result C)
            batch_id: Computation batch identifier
        """
        start = host.now_ms()
        if not matrix_a_rows or not matrix_b:
            return {"error": "missing matrix data", "batch_id": batch_id}

        n_rows = len(matrix_a_rows)
        k_dim = len(matrix_a_rows[0]) if matrix_a_rows else 0
        cols_b = n_cols_b or (len(matrix_b[0]) if matrix_b and matrix_b[0] else 0)

        # Compute C[i,j] = Σ_k A[i,k] * B[k,j]
        result_rows = []
        for i, a_row in enumerate(matrix_a_rows):
            c_row = []
            for j in range(cols_b):
                total = 0.0
                for k in range(min(k_dim, len(matrix_b))):
                    b_row = matrix_b[k]
                    if j < len(b_row):
                        total += a_row[k] * b_row[j]
                c_row.append(round(total, 6))
            result_rows.append(c_row)

        elapsed = host.now_ms() - start
        flops = n_rows * cols_b * k_dim * 2  # multiply + add per element
        self.rows_computed += n_rows
        self.total_compute_ms += elapsed
        self.total_flops += flops

        return {
            "status": "ok",
            "batch_id": batch_id,
            "shard_id": self.shard_id,
            "start_row": start_row,
            "rows": result_rows,
            "n_rows": n_rows,
            "compute_ms": elapsed,
            "flops": flops,
        }

    @handler("stats")
    def stats(self) -> dict:
        gflops = (self.total_flops / (self.total_compute_ms / 1000) / 1e9
                  if self.total_compute_ms > 0 else 0)
        return {
            "worker_id": self.worker_id,
            "shard_id": self.shard_id,
            "rows_computed": self.rows_computed,
            "total_compute_ms": self.total_compute_ms,
            "total_flops": self.total_flops,
            "gflops": round(gflops, 3),
        }


@actor
class MatrixMaster:
    """Orchestrates parallel matrix multiplication.

    Partitions matrix A rows across workers, scatters computation,
    gathers results, and assembles the final result matrix.

    Demonstrates the full scatter-gather data-parallel pattern
    from the NSDI'22 paper.
    """

    coordinator_id: str = state(default="")
    num_workers: int = state(default=4)
    worker_ids: list = state(default_factory=list)
    total_multiplications: int = state(default=0)
    total_ms: float = state(default=0.0)
    total_coord_ms: float = state(default=0.0)
    total_compute_ms: float = state(default=0.0)

    @init_handler
    def on_init(self, config: dict):
        actor_id = config.get("actor_id", "")
        self.coordinator_id = actor_id
        args = config.get("args", {})
        self.num_workers = int(args.get("num_workers", 4))

        id_suffix = ""
        if ":" in actor_id:
            id_suffix = actor_id[actor_id.index(":"):]
        self.worker_ids = [f"matworker-{i}{id_suffix}" for i in range(self.num_workers)]

        host.process_groups.join("matrix-coordinators")
        host.info(f"MatrixMaster: {self.num_workers} workers")

    @handler("multiply")
    def multiply(self, matrix_a: list = None, matrix_b: list = None,
                 from_actor: str = "") -> dict:
        """Multiply two matrices using parallel scatter-gather.

        Args:
            matrix_a: Matrix A as list of rows [[a00, a01, ...], ...]
            matrix_b: Matrix B as list of rows [[b00, b01, ...], ...]
        """
        start = host.now_ms()
        if not matrix_a or not matrix_b:
            return {"error": "missing matrices"}

        n_rows_a = len(matrix_a)
        k_dim = len(matrix_a[0]) if matrix_a else 0
        n_cols_b = len(matrix_b[0]) if matrix_b and matrix_b[0] else 0
        batch_id = f"mul-{self.total_multiplications}"

        # ---- Scatter: partition rows across workers ----
        rows_per_worker = max(1, n_rows_a // self.num_workers)
        worker_results = []
        compute_ms_total = 0

        for w in range(self.num_workers):
            row_start = w * rows_per_worker
            row_end = min(row_start + rows_per_worker, n_rows_a)
            if w == self.num_workers - 1:
                row_end = n_rows_a  # Last worker gets remainder

            if row_start >= n_rows_a:
                break

            a_partition = matrix_a[row_start:row_end]
            worker_id = self.worker_ids[w]

            try:
                resp = host.ask(worker_id, "compute_rows", {
                    "matrix_a_rows": a_partition,
                    "matrix_b": matrix_b,
                    "start_row": row_start,
                    "n_cols_b": n_cols_b,
                    "batch_id": f"{batch_id}-w{w}",
                }, timeout_ms=60000)

                if isinstance(resp, dict) and resp.get("status") == "ok":
                    worker_results.append(resp)
                    compute_ms_total += resp.get("compute_ms", 0)
                else:
                    host.warn(f"Worker {worker_id} failed: {resp}")
            except Exception as e:
                host.warn(f"Worker {worker_id} error: {e}")

        # ---- Gather: assemble result matrix ----
        # Sort by start_row to reconstruct correct order
        worker_results.sort(key=lambda r: r.get("start_row", 0))

        result_matrix = []
        for wr in worker_results:
            result_matrix.extend(wr.get("rows", []))

        elapsed = host.now_ms() - start
        coord_ms = elapsed - compute_ms_total
        total_flops = sum(wr.get("flops", 0) for wr in worker_results)
        gflops = total_flops / (elapsed / 1000) / 1e9 if elapsed > 0 else 0

        self.total_multiplications += 1
        self.total_ms += elapsed
        self.total_coord_ms += coord_ms
        self.total_compute_ms += compute_ms_total

        return {
            "status": "ok",
            "batch_id": batch_id,
            "result": result_matrix,
            "dimensions": {
                "A": [n_rows_a, k_dim],
                "B": [k_dim, n_cols_b],
                "C": [n_rows_a, n_cols_b],
            },
            "performance": {
                "total_ms": elapsed,
                "compute_ms": compute_ms_total,
                "coordination_ms": round(coord_ms, 1),
                "total_flops": total_flops,
                "gflops": round(gflops, 3),
                "workers_used": len(worker_results),
            },
        }

    @handler("benchmark")
    def benchmark(self, size: int = 100, from_actor: str = "") -> dict:
        """Run a benchmark with a square matrix of given size.

        Generates deterministic test matrices and multiplies them.
        """
        # Generate test matrices
        matrix_a = []
        matrix_b = []
        for i in range(size):
            row_a = []
            row_b = []
            for j in range(size):
                row_a.append(round(((i * 7 + j * 13 + 42) % 100) / 10.0, 2))
                row_b.append(round(((i * 11 + j * 3 + 17) % 100) / 10.0, 2))
            matrix_a.append(row_a)
            matrix_b.append(row_b)

        result = self.multiply(matrix_a=matrix_a, matrix_b=matrix_b)

        # Add benchmark metadata
        if isinstance(result, dict) and result.get("status") == "ok":
            result["benchmark"] = {
                "matrix_size": size,
                "total_elements": size * size,
                "total_ops": 2 * size * size * size,
            }
            # Verify: check a few elements of C
            c = result.get("result", [])
            if c and len(c) > 0:
                result["verification"] = {
                    "C[0][0]": c[0][0] if c[0] else None,
                    "C_rows": len(c),
                    "C_cols": len(c[0]) if c[0] else 0,
                }

        return result

    @handler("stats")
    def stats(self) -> dict:
        total = self.total_coord_ms + self.total_compute_ms
        return {
            "total_multiplications": self.total_multiplications,
            "total_ms": self.total_ms,
            "total_compute_ms": self.total_compute_ms,
            "total_coord_ms": round(self.total_coord_ms, 1),
            "granularity": round(
                self.total_compute_ms / self.total_coord_ms
                if self.total_coord_ms > 0 else 0, 2
            ),
            "compute_pct": round(
                self.total_compute_ms / total * 100 if total > 0 else 0, 1
            ),
        }


ACTOR_ROLES = {
    "matrix-master": MatrixMaster,
    "matworker": MatrixWorker,
}
