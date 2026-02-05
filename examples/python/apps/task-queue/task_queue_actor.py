"""
Task Queue Coordinator Actor - Distributed Task Queue (Python WASM with SDK)

Demonstrates using distributed locks via LockFacet for task queue coordination.
The LockFacet intercepts lock operation messages and uses the real LockManager backend.

Real-world use case: Distributed job processing, task scheduling, preventing
duplicate work across a cluster (similar to Celery, Sidekiq, Bull).

## How It Works

1. Jobs are submitted to the queue
2. Workers claim jobs by acquiring locks on job IDs
3. Workers renew locks periodically (heartbeat) during processing
4. Workers release locks when jobs complete
5. If a worker crashes, the lock expires and another worker can claim the job

## LockFacet Operations (Intercepted)

- "acquire_lock": Acquire lock with lease duration
- "release_lock": Release lock (requires version)
- "renew_lock": Renew lock lease (heartbeat)
- "try_acquire_lock": Non-blocking lock attempt
- "get_lock": Get current lock state

## SDK Features Used

- @actor: Marks class as PlexSpaces actor
- state(): Defines persistent job queue
- @handler(): Routes queue operations
"""

import time
from plexspaces import actor, state, handler, init_handler, host


@actor
class TaskQueueCoordinator:
    """Task queue coordinator using LockFacet for distributed job coordination.
    
    Lock operations are intercepted by LockFacet and never reach handlers.
    """
    
    # Job queue: job_id -> {task_type, payload, submitted_at, status}
    job_queue: dict = state(default_factory=dict)
    
    @init_handler
    def on_init(self, config: dict):
        """Initialize task queue from config."""
        self.job_queue = config.get("jobs", {})
    
    @handler("submit")
    def submit_job(self, job_id: str = "", task_type: str = "", payload: dict = None) -> dict:
        """Submit a new job to the queue."""
        if not job_id:
            return {"status": "error", "error": "job_id is required"}
        
        if job_id in self.job_queue:
            return {"status": "error", "error": f"Job {job_id} already exists"}
        
        self.job_queue[job_id] = {
            "task_type": task_type,
            "payload": payload or {},
            "submitted_at": time.time(),
            "status": "pending"
        }
        
        host.info(f"Job submitted: {job_id} (type: {task_type})")
        
        return {
            "status": "ok",
            "job_id": job_id,
            "message": f"Job {job_id} submitted to queue"
        }
    
    @handler("list")
    def list_jobs(self) -> dict:
        """List all jobs in the queue."""
        job_list = [
            {
                "job_id": job_id,
                "task_type": info.get("task_type", ""),
                "status": info.get("status", "pending"),
                "submitted_at": info.get("submitted_at", 0)
            }
            for job_id, info in self.job_queue.items()
        ]
        
        host.info(f"Listing {len(job_list)} jobs in queue")
        
        return {
            "status": "ok",
            "jobs": job_list,
            "count": len(job_list)
        }
    
    @handler("status")
    def get_status(self) -> dict:
        """Get queue status."""
        pending = sum(1 for info in self.job_queue.values() if info.get("status") == "pending")
        processing = sum(1 for info in self.job_queue.values() if info.get("status") == "processing")
        completed = sum(1 for info in self.job_queue.values() if info.get("status") == "completed")
        
        host.info(f"Status: {len(self.job_queue)} total, {pending} pending, {processing} processing")
        
        return {
            "status": "ok",
            "queue": "task-queue",
            "total_jobs": len(self.job_queue),
            "pending": pending,
            "processing": processing,
            "completed": completed
        }
    
    @handler("get_state", "call")
    def get_state_handler(self) -> dict:
        """Get state for persistence."""
        return {"jobs": self.job_queue}
