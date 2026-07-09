# SPDX-License-Identifier: AGPL-3.0-or-later
"""TrajectoryStoreActor — persists and indexes agent trajectory records.

Demonstrates: BlobStorage for large payloads, TupleSpace-based discovery index,
read-all pattern for eval collection, and trajectory lifecycle management.
"""

import json
from plexspaces import actor, state, init_handler, handler, host


@actor
class TrajectoryStoreActor:
    """
    Trajectory storage and index: persists full AgentTrajectory records and
    exposes query patterns for eval collection.

    Two storage tiers:
    - KV: trajectory metadata + full record (for fast retrieval)
    - TupleSpace index: discovery entries (eval_run_id, outcome) for batch collection

    The ExecutionTraceFacet writes directly to KV when attached to an AgentActor.
    This actor provides the query and management layer on top.

    Key patterns:
    - EvalRunnerActor calls list_for_eval_run() to collect all trajectories for a run
    - RegressionDetectorActor calls get() to load specific trajectories for diff
    - DashboardActor calls get_stats() for overview metrics
    """

    actor_id: str = state(default="")
    stored_count: int = state(default=0)
    failed_count: int = state(default=0)

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        try:
            host.kv_put("svc:trajectory_store", host.self_id())
        except Exception:
            pass
        try:
            host.registry.register(None, self.actor_id or host.self_id(), "actor", "",
                                   object_category="trajectory_store")
        except Exception:
            pass
        host.info(f"TrajectoryStoreActor init actor_id={self.actor_id}")

    @handler("put")
    def put(self, trajectory: dict = None) -> dict:
        """Store a trajectory record."""
        if not trajectory:
            return {"error": "trajectory is required"}

        traj_id = trajectory.get("trajectory_id", "")
        if not traj_id:
            traj_id = host.new_id()
            trajectory["trajectory_id"] = traj_id

        eval_run_id = trajectory.get("eval_run_id", "")
        outcome = trajectory.get("outcome", "unknown")
        agent_actor_id = trajectory.get("agent_actor_id", "")

        # Store full trajectory in KV
        try:
            host.kv_put(f"trajectory:{traj_id}", json.dumps(trajectory))
        except Exception as e:
            self.failed_count += 1
            host.warn(f"Failed to store trajectory {traj_id}: {e}")
            return {"error": f"kv_put failed: {e}"}

        # Store metadata for listing
        meta = {
            "trajectory_id": traj_id,
            "eval_run_id": eval_run_id,
            "agent_actor_id": agent_actor_id,
            "outcome": outcome,
            "score": trajectory.get("score", 0.0),
            "total_input_tokens": trajectory.get("total_input_tokens", 0),
            "total_output_tokens": trajectory.get("total_output_tokens", 0),
            "step_count": len(trajectory.get("steps", [])),
            "stored_at_ms": host.now_ms(),
        }
        try:
            host.kv_put(f"traj_meta:{traj_id}", json.dumps(meta))
        except Exception as e:
            host.warn(f"Failed to store trajectory metadata {traj_id}: {e}")

        # Index by eval_run_id for batch collection
        if eval_run_id:
            try:
                index_key = f"traj_index:{eval_run_id}"
                existing_raw = host.kv_get(index_key)
                index = json.loads(existing_raw) if existing_raw else []
                if traj_id not in index:
                    index.append(traj_id)
                    host.kv_put(index_key, json.dumps(index))
            except Exception as e:
                host.warn(f"Failed to update trajectory index for {eval_run_id}: {e}")

        self.stored_count += 1
        host.incr_counter("trajectories_stored_total", 1)
        host.info(f"TrajectoryStore: stored traj_id={traj_id} eval_run={eval_run_id} outcome={outcome}")

        return {"status": "ok", "trajectory_id": traj_id}

    @handler("get")
    def get(self, trajectory_id: str = "") -> dict:
        """Get a full trajectory by ID."""
        if not trajectory_id:
            return {"error": "trajectory_id is required"}
        raw = host.kv_get(f"trajectory:{trajectory_id}")
        if not raw:
            return {"error": f"trajectory {trajectory_id} not found"}
        try:
            return {"status": "ok", "trajectory": json.loads(raw)}
        except Exception:
            return {"error": "failed to parse trajectory"}

    @handler("list_for_eval_run")
    def list_for_eval_run(self, eval_run_id: str = "", include_full: bool = False) -> dict:
        """
        List all trajectories for an eval run.

        Set include_full=True to return full trajectory records.
        Default (False) returns only metadata — cheaper for scoring.
        """
        if not eval_run_id:
            return {"error": "eval_run_id is required"}

        # Check TupleSpace index first (written by ExecutionTraceFacet)
        try:
            ts_entries = host.ts_read_all({"type": "trajectory", "eval_run_id": eval_run_id})
            traj_ids_from_ts = [e.get("trajectory_id") for e in (ts_entries or []) if e.get("trajectory_id")]
        except Exception:
            traj_ids_from_ts = []

        # Fall back to KV index
        try:
            index_raw = host.kv_get(f"traj_index:{eval_run_id}")
            traj_ids_from_kv = json.loads(index_raw) if index_raw else []
        except Exception:
            traj_ids_from_kv = []

        # Merge, deduplicate
        all_ids = list({t for t in traj_ids_from_ts + traj_ids_from_kv if t})

        trajectories = []
        for traj_id in all_ids:
            if include_full:
                raw = host.kv_get(f"trajectory:{traj_id}")
                if raw:
                    try:
                        trajectories.append(json.loads(raw))
                    except Exception:
                        pass
            else:
                raw = host.kv_get(f"traj_meta:{traj_id}")
                if raw:
                    try:
                        trajectories.append(json.loads(raw))
                    except Exception:
                        pass

        return {
            "status": "ok",
            "eval_run_id": eval_run_id,
            "trajectories": trajectories,
            "count": len(trajectories),
        }

    @handler("delete")
    def delete(self, trajectory_id: str = "") -> dict:
        """Delete a trajectory and its metadata."""
        if not trajectory_id:
            return {"error": "trajectory_id is required"}
        try:
            host.kv_delete(f"trajectory:{trajectory_id}")
            host.kv_delete(f"traj_meta:{trajectory_id}")
            host.incr_counter("trajectories_deleted_total", 1)
            return {"status": "ok", "trajectory_id": trajectory_id}
        except Exception as e:
            return {"error": str(e)}

    @handler("delete_eval_run")
    def delete_eval_run(self, eval_run_id: str = "") -> dict:
        """Delete all trajectories for an eval run (cleanup after scoring)."""
        if not eval_run_id:
            return {"error": "eval_run_id is required"}
        try:
            index_raw = host.kv_get(f"traj_index:{eval_run_id}")
            traj_ids = json.loads(index_raw) if index_raw else []
            deleted = 0
            for traj_id in traj_ids:
                host.kv_delete(f"trajectory:{traj_id}")
                host.kv_delete(f"traj_meta:{traj_id}")
                deleted += 1
            host.kv_delete(f"traj_index:{eval_run_id}")
            return {"status": "ok", "eval_run_id": eval_run_id, "deleted": deleted}
        except Exception as e:
            return {"error": str(e)}

    @handler("get_stats")
    def get_stats(self) -> dict:
        return {
            "status": "ok",
            "actor_id": self.actor_id,
            "stored_count": self.stored_count,
            "failed_count": self.failed_count,
        }
