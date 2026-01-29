#!/usr/bin/env python3
"""
Task Queue Coordinator Actor - Distributed Task Queue (Python WASM)

Demonstrates using distributed locks via LockFacet for task queue coordination.
The LockFacet intercepts lock operation messages and uses the real LockManager
backend (configured via node-config/runtimeconfig, not hardcoded).

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
- "try_acquire_lock": Non-blocking lock attempt (used for claiming jobs)
- "get_lock": Get current lock state

## Actor Operations (Not Intercepted by Facet)

- "submit": Submit a new job to the queue
- "list": List all jobs in the queue
- "status": Get queue status
"""

import json
from wit_world import exports

# Task queue state - tracks submitted jobs
# Format: job_id -> {task_type, payload, submitted_at, status}
_job_queue = {}


class Actor(exports.Actor):
    """Task queue coordinator actor using LockFacet for distributed job coordination.
    
    Note: Lock operations (acquire_lock, release_lock, etc.) are intercepted
    by LockFacet and never reach this class's handle() method.
    """
    
    def init(self, config_json: str) -> str:
        """Initialize task queue coordinator actor."""
        global _job_queue
        _job_queue = {}
        if config_json:
            try:
                config = json.loads(config_json)
                _job_queue = config.get("jobs", {})
            except Exception as e:
                return f"ERROR: Failed to parse config: {e}"
        return ""
    
    def handle(self, from_actor: str, msg_type: str, payload_json: str) -> str:
        """
        Handle task queue operations.
        
        Note: Lock operations (acquire_lock, release_lock, etc.) are intercepted
        by LockFacet and never reach this method. This method only handles
        queue-specific operations.
        
        Message types handled here:
        - "submit": Submit a new job to the queue
        - "list": List all jobs in the queue
        - "status": Get queue status
        - "get_state": Get actor state (for persistence)
        """
        global _job_queue
        
        try:
            data = json.loads(payload_json) if payload_json else {}
            
            if msg_type == "submit":
                # Submit a new job to the queue
                job_id = data.get("job_id", "")
                task_type = data.get("task_type", "")
                task_payload = data.get("payload", {})
                
                if not job_id:
                    return json.dumps({
                        "status": "error",
                        "error": "job_id is required"
                    })
                
                if job_id in _job_queue:
                    return json.dumps({
                        "status": "error",
                        "error": f"Job {job_id} already exists"
                    })
                
                import time
                _job_queue[job_id] = {
                    "task_type": task_type,
                    "payload": task_payload,
                    "submitted_at": time.time(),
                    "status": "pending"  # pending, processing, completed, failed
                }
                
                # Log job submission (will appear in server logs)
                print(f"[TASK-QUEUE] Job submitted: {job_id} (type: {task_type})", flush=True)
                
                return json.dumps({
                    "status": "ok",
                    "job_id": job_id,
                    "message": f"Job {job_id} submitted to queue"
                })
            
            elif msg_type == "list":
                # List all jobs in the queue
                job_list = [
                    {
                        "job_id": job_id,
                        "task_type": info.get("task_type", ""),
                        "status": info.get("status", "pending"),
                        "submitted_at": info.get("submitted_at", 0)
                    }
                    for job_id, info in _job_queue.items()
                ]
                print(f"[TASK-QUEUE] Listing {len(job_list)} jobs in queue", flush=True)
                return json.dumps({
                    "status": "ok",
                    "jobs": job_list,
                    "count": len(job_list)
                })
            
            elif msg_type == "status":
                # Get queue status
                pending_count = sum(1 for info in _job_queue.values() if info.get("status") == "pending")
                processing_count = sum(1 for info in _job_queue.values() if info.get("status") == "processing")
                completed_count = sum(1 for info in _job_queue.values() if info.get("status") == "completed")
                
                print(f"[TASK-QUEUE] Status: {len(_job_queue)} total, {pending_count} pending, {processing_count} processing, {completed_count} completed", flush=True)
                
                return json.dumps({
                    "status": "ok",
                    "queue": "task-queue",
                    "total_jobs": len(_job_queue),
                    "pending": pending_count,
                    "processing": processing_count,
                    "completed": completed_count
                })
            
            elif msg_type in ("get_state", "call"):
                # Get state for persistence
                return json.dumps({"jobs": _job_queue})
            
            else:
                # Unknown message type - return error
                # Note: Lock operations should be intercepted by LockFacet
                return json.dumps({
                    "status": "error",
                    "error": f"Unknown message type: {msg_type}. Lock operations (acquire_lock, release_lock, etc.) should be intercepted by LockFacet."
                })
                
        except Exception as e:
            return json.dumps({
                "status": "error",
                "error": str(e)
            })
    
    def get_state(self) -> str:
        """Get task queue state as JSON."""
        global _job_queue
        return json.dumps({"jobs": _job_queue})
    
    def set_state(self, state_json: str) -> str:
        """Restore task queue state from JSON."""
        global _job_queue
        try:
            state = json.loads(state_json)
            _job_queue = state.get("jobs", {})
            return ""
        except Exception as e:
            return f"ERROR: Failed to restore state: {e}"
