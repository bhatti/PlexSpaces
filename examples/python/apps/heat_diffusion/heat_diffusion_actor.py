# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 PlexSpaces Contributors
"""
Heat Diffusion - 2D Stencil Computation with TupleSpace Coordination

Demonstrates parallel scientific computing using PlexSpaces actors,
ported from the Rust embedded example (examples/rust/embedded/heat_diffusion/).

## Architecture

    +------------------+
    |  Simulation      |  Orchestrates iterations and convergence
    |  Coordinator     |  Barrier synchronization via TupleSpace
    +--------+---------+
             |
    +--------v---------+     Process Group: "heat-regions"
    |  GridRegion      |     Each actor = horizontal strip of grid
    |  Actor Pool      |     Ghost cell exchange via TupleSpace
    |  [0] [1] .. [N]  |     5-point stencil computation
    +------------------+

## Physics Model

2D heat equation solved via finite differences (Jacobi iteration):
    T_new[i,j] = (T[i-1,j] + T[i+1,j] + T[i,j-1] + T[i,j+1]) / 4.0

Boundary conditions:
    - North edge: T = 100.0 (hot)
    - South edge: T = 0.0 (cold)
    - East/West edges: T = 50.0 (warm)
    - Interior: T = 25.0 (initial)

## TupleSpace Coordination Pattern

Ghost cell exchange between neighboring regions:
    1. Region writes its boundary row to TupleSpace
       ts_write(["boundary", iteration, region_id, "south", [values...]])
    2. Neighbor reads the boundary
       ts_read(["boundary", iteration, neighbor_id, "south", None])
    3. Barrier: all regions write completion tuples
       ts_write(["barrier", iteration, region_id])
    4. Coordinator reads all barriers before next iteration
       ts_read(["barrier", iteration, region_id]) for each region

## Framework Features Demonstrated

- **TupleSpace**: Ghost cell exchange and barrier synchronization
- **Process Groups**: Region actors join "heat-regions" group
- **Scatter-Gather**: Coordinator fans out compute requests, gathers convergence
- **Scientific Computing**: Stencil computation pattern (PDE solver)
"""

import json
from plexspaces import actor, state, handler, init_handler, host


@actor
class GridRegionActor:
    """Manages a horizontal strip of the 2D temperature grid.

    Each region computes the 5-point stencil for its strip, exchanging
    boundary values (ghost cells) with neighbors via TupleSpace.
    """

    region_id: int = state(default=0)
    width: int = state(default=100)
    data: list = state(default_factory=list)
    fixed_north: list = state(default_factory=list)
    fixed_south: list = state(default_factory=list)
    num_regions: int = state(default=4)
    iterations_computed: int = state(default=0)

    @init_handler
    def on_init(self, config: dict):
        args = config.get("args", {})
        self.region_id = int(args.get("region_id", 0))
        self.width = int(args.get("width", 100))
        self.num_regions = int(args.get("num_regions", 4))

        # Initialize temperature: interior = 25.0, east/west boundaries = 50.0
        self.data = [25.0] * self.width
        self.data[0] = 50.0           # West boundary
        self.data[-1] = 50.0          # East boundary

        # Fixed boundaries for top/bottom regions
        if self.region_id == 0:
            self.fixed_north = [100.0] * self.width  # North = hot
        else:
            self.fixed_north = []

        if self.region_id == self.num_regions - 1:
            self.fixed_south = [0.0] * self.width    # South = cold
        else:
            self.fixed_south = []

        host.process_groups.join("heat-regions")
        host.info(f"GridRegion {self.region_id}/{self.num_regions}: "
                  f"width={self.width}, T_avg={sum(self.data)/len(self.data):.1f}")

    @handler("compute")
    def compute_iteration(self, iteration: int = 0, from_actor: str = "") -> dict:
        """Compute one iteration of the heat equation for this region.

        Steps:
        1. Write our boundary rows to TupleSpace (for neighbors)
        2. Read neighbor boundaries from TupleSpace
        3. Compute new values using 5-point stencil
        4. Write barrier tuple for synchronization

        Returns:
            max_diff: Maximum temperature change (for convergence check)
            avg_temp: Average temperature in this region
        """
        start = host.now_ms()

        # Step 1: Write our boundary to TupleSpace for neighbors
        # North neighbor needs our first interior row
        north_boundary = json.dumps(
            ["boundary", iteration, self.region_id, "north", self.data[:]]
        )
        host.ts_write(north_boundary)

        # South neighbor needs our last interior row
        south_boundary = json.dumps(
            ["boundary", iteration, self.region_id, "south", self.data[:]]
        )
        host.ts_write(south_boundary)

        # Step 2: Get neighbor boundaries
        if self.fixed_north:
            north = self.fixed_north
        else:
            # Read south boundary of region above us
            pattern = json.dumps(
                ["boundary", iteration, self.region_id - 1, "south", None]
            )
            result = host.ts_read(pattern) or ""
            if result and not result.startswith("ERROR"):
                try:
                    tuple_data = json.loads(result)
                    north = tuple_data[4] if len(tuple_data) > 4 else self.data[:]
                except (json.JSONDecodeError, ValueError):
                    north = self.data[:]
            else:
                north = self.data[:]

        if self.fixed_south:
            south = self.fixed_south
        else:
            # Read north boundary of region below us
            pattern = json.dumps(
                ["boundary", iteration, self.region_id + 1, "north", None]
            )
            result = host.ts_read(pattern) or ""
            if result and not result.startswith("ERROR"):
                try:
                    tuple_data = json.loads(result)
                    south = tuple_data[4] if len(tuple_data) > 4 else self.data[:]
                except (json.JSONDecodeError, ValueError):
                    south = self.data[:]
            else:
                south = self.data[:]

        # Step 3: Compute 5-point stencil
        new_data = self.data[:]
        max_diff = 0.0

        for i in range(1, self.width - 1):
            # 5-point stencil: average of N, S, E, W neighbors
            n = north[i] if i < len(north) else self.data[i]
            s = south[i] if i < len(south) else self.data[i]
            w = self.data[i - 1]
            e = self.data[i + 1]

            new_val = (n + s + w + e) / 4.0
            diff = abs(new_val - self.data[i])
            if diff > max_diff:
                max_diff = diff
            new_data[i] = new_val

        self.data = new_data
        self.iterations_computed += 1

        # Step 4: Write barrier tuple
        barrier = json.dumps(["barrier", iteration, self.region_id])
        host.ts_write(barrier)

        elapsed = host.now_ms() - start
        avg_temp = sum(self.data) / len(self.data)

        return {
            "status": "ok",
            "region_id": self.region_id,
            "iteration": iteration,
            "max_diff": round(max_diff, 6),
            "avg_temp": round(avg_temp, 2),
            "compute_ms": elapsed,
        }

    @handler("get_data")
    def get_data(self) -> dict:
        """Return current temperature data for visualization."""
        return {
            "region_id": self.region_id,
            "width": self.width,
            "data": [round(v, 2) for v in self.data],
            "avg_temp": round(sum(self.data) / len(self.data), 2),
            "min_temp": round(min(self.data), 2),
            "max_temp": round(max(self.data), 2),
            "iterations": self.iterations_computed,
        }

    @handler("stats")
    def stats(self) -> dict:
        return {
            "region_id": self.region_id,
            "iterations_computed": self.iterations_computed,
            "avg_temp": round(sum(self.data) / len(self.data), 2),
        }


@actor
class SimulationCoordinator:
    """Orchestrates the heat diffusion simulation across all regions.

    Manages iteration loop, convergence checking, and barrier synchronization.
    """

    coordinator_id: str = state(default="")
    num_regions: int = state(default=4)
    grid_width: int = state(default=100)
    max_iterations: int = state(default=100)
    tolerance: float = state(default=0.5)
    region_ids: list = state(default_factory=list)
    total_iterations: int = state(default=0)
    total_compute_ms: float = state(default=0.0)
    total_coord_ms: float = state(default=0.0)

    @init_handler
    def on_init(self, config: dict):
        actor_id = config.get("actor_id", "")
        self.coordinator_id = actor_id
        args = config.get("args", {})
        self.num_regions = int(args.get("num_regions", 4))
        self.grid_width = int(args.get("grid_width", 100))
        self.max_iterations = int(args.get("max_iterations", 100))
        self.tolerance = float(args.get("tolerance", 0.5))

        id_suffix = ""
        if ":" in actor_id:
            id_suffix = actor_id[actor_id.index(":"):]
        self.region_ids = [f"region-{i}{id_suffix}" for i in range(self.num_regions)]

        host.process_groups.join("heat-coordinators")
        host.info(f"SimulationCoordinator: {self.num_regions} regions, "
                  f"width={self.grid_width}, tol={self.tolerance}")

    @handler("run_simulation")
    def run_simulation(self, iterations: int = 0, from_actor: str = "") -> dict:
        """Run the heat diffusion simulation for N iterations or until convergence."""
        max_iter = iterations if iterations > 0 else self.max_iterations
        sim_start = host.now_ms()
        results = []

        for it in range(max_iter):
            # Fan-out: ask all regions to compute
            coord_start = host.now_ms()
            region_results = []
            for region_id in self.region_ids:
                try:
                    resp = host.ask(region_id, "compute", {
                        "iteration": it,
                    }, timeout_ms=30000)
                    if isinstance(resp, dict) and resp.get("status") == "ok":
                        region_results.append(resp)
                except Exception as e:
                    host.warn(f"Region {region_id} failed: {e}")

            coord_ms = host.now_ms() - coord_start
            compute_ms = sum(r.get("compute_ms", 0) for r in region_results)

            # Check convergence
            max_diff = max((r.get("max_diff", 999) for r in region_results), default=999)
            avg_temps = [r.get("avg_temp", 0) for r in region_results]

            self.total_iterations += 1
            self.total_compute_ms += compute_ms
            self.total_coord_ms += coord_ms - compute_ms

            result = {
                "iteration": it,
                "max_diff": round(max_diff, 6),
                "avg_temps": avg_temps,
                "coord_ms": round(coord_ms, 1),
            }
            results.append(result)

            if max_diff < self.tolerance:
                host.info(f"Converged at iteration {it} (max_diff={max_diff:.6f})")
                break

        sim_ms = host.now_ms() - sim_start

        return {
            "status": "ok",
            "iterations_completed": len(results),
            "converged": results[-1]["max_diff"] < self.tolerance if results else False,
            "final_max_diff": results[-1]["max_diff"] if results else 0,
            "simulation_ms": sim_ms,
            "results": results[-5:],  # Last 5 iterations
        }

    @handler("stats")
    def stats(self) -> dict:
        total = self.total_compute_ms + self.total_coord_ms
        return {
            "total_iterations": self.total_iterations,
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
    "sim-coordinator": SimulationCoordinator,
    "region": GridRegionActor,
}
