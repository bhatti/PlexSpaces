# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 PlexSpaces Contributors
"""
Job Processing Actor - Distributed Task Processing with TupleSpace

Demonstrates scatter/gather pattern for distributed job processing:
- Submit jobs → scatter tasks to TupleSpace
- Workers take tasks → process → write results to TupleSpace
- Coordinator gathers results → aggregates

Real-world use cases:
- Image processing pipeline (resize, thumbnail generation)
- Data transformation (ETL jobs)
- Batch report generation
- Machine learning inference jobs

## TupleSpace API Used

- ts_write: Write job tasks and results
- ts_read: Check for results (non-destructive)
- ts_take: Claim a task (destructive - prevents duplicate processing)
- ts_read_all: Gather all results

## Tuple Patterns

Tasks: ["job", job_id, "task", task_id, task_data]
Results: ["job", job_id, "result", task_id, result_data]
Status: ["job", job_id, "status", status_value]
"""

import json
from plexspaces import actor, state, handler, init_handler, host


@actor
class JobProcessor:
    """Job processing coordinator using TupleSpace for task distribution."""
    
    # Persistent state
    job_counter: int = state(default=0)
    active_jobs: dict = state(default_factory=dict)
    
    @init_handler
    def on_init(self, config: dict) -> None:
        """Initialize job processor."""
        self.job_counter = 0
        self.active_jobs = {}
        host.log("info", "JobProcessor initialized")
    
    @handler("submit", "call")
    def submit_job(self, job_type: str = "default", tasks: list = None, data: dict = None) -> dict:
        """
        Submit a new job with multiple tasks.
        
        Scatters tasks to TupleSpace for workers to process.
        
        Args:
            job_type: Type of job (e.g., "image_resize", "data_transform")
            tasks: List of task specifications
            data: Optional shared data for all tasks
        
        Returns:
            Job ID and task count
        """
        self.job_counter += 1
        job_id = f"job-{self.job_counter}"
        
        task_list = tasks or [{"id": 0, "data": data or {}}]
        task_count = len(task_list)
        
        # Track job
        self.active_jobs[job_id] = {
            "type": job_type,
            "task_count": task_count,
            "completed": 0,
            "status": "submitted"
        }
        
        # Write job status to TupleSpace
        status_tuple = json.dumps(["job", job_id, "status", "submitted"])
        result = host.ts_write(status_tuple) or ""
        if result.startswith("ERROR"):
            return {"error": result, "job_id": job_id}
        
        # Scatter: Write each task to TupleSpace
        for i, task in enumerate(task_list):
            task_id = f"task-{i}"
            task_data = json.dumps(task) if isinstance(task, dict) else str(task)
            task_tuple = json.dumps(["job", job_id, "task", task_id, task_data])
            result = host.ts_write(task_tuple) or ""
            if result.startswith("ERROR"):
                host.log("warn", f"Failed to write task {task_id}: {result}")
        
        host.log("info", f"Job {job_id} submitted with {task_count} tasks")
        return {"job_id": job_id, "task_count": task_count, "status": "submitted"}
    
    @handler("claim_task", "call")
    def claim_task(self, job_id: str = "") -> dict:
        """
        Claim a task from a job (for workers).
        
        Uses ts_take for atomic task claiming (prevents duplicate processing).
        
        Args:
            job_id: Job ID to claim task from (empty = any job)
        
        Returns:
            Task details or empty if no tasks available
        """
        # Pattern: ["job", job_id, "task", *, *] where * is wildcard
        if job_id:
            pattern = json.dumps(["job", job_id, "task", None, None])
        else:
            pattern = json.dumps(["job", None, "task", None, None])
        
        result = host.ts_take(pattern) or ""
        
        if not result or result.startswith("ERROR"):
            return {"task": None, "message": "No tasks available"}
        
        try:
            task_tuple = json.loads(result)
            if len(task_tuple) >= 5:
                return {
                    "job_id": task_tuple[1],
                    "task_id": task_tuple[3],
                    "task_data": task_tuple[4],
                    "claimed": True
                }
        except json.JSONDecodeError:
            pass
        
        return {"task": None, "message": "Invalid task format"}
    
    @handler("submit_result", "call")
    def submit_result(self, job_id: str = "", task_id: str = "", result_data: str = "") -> dict:
        """
        Submit result for a completed task (for workers).
        
        Writes result to TupleSpace for gathering.
        
        Args:
            job_id: Job ID
            task_id: Task ID
            result_data: Result data (JSON string)
        
        Returns:
            Confirmation
        """
        if not job_id or not task_id:
            return {"error": "job_id and task_id required"}
        
        result_tuple = json.dumps(["job", job_id, "result", task_id, result_data])
        result = host.ts_write(result_tuple) or ""
        
        if result.startswith("ERROR"):
            return {"error": result}
        
        # Update job status
        if job_id in self.active_jobs:
            self.active_jobs[job_id]["completed"] += 1
        
        host.log("info", f"Result submitted for {job_id}/{task_id}")
        return {"status": "ok", "job_id": job_id, "task_id": task_id}
    
    @handler("gather_results", "call")
    def gather_results(self, job_id: str = "") -> dict:
        """
        Gather all results for a job.
        
        Uses ts_read_all to collect all results non-destructively.
        
        Args:
            job_id: Job ID to gather results for
        
        Returns:
            All results for the job
        """
        if not job_id:
            return {"error": "job_id required"}
        
        # Pattern: ["job", job_id, "result", *, *]
        pattern = json.dumps(["job", job_id, "result", None, None])
        result = host.ts_read_all(pattern) or ""
        
        if not result or result.startswith("ERROR"):
            return {"job_id": job_id, "results": [], "count": 0}
        
        try:
            result_tuples = json.loads(result)
            results = []
            for tuple_data in result_tuples:
                if len(tuple_data) >= 5:
                    results.append({
                        "task_id": tuple_data[3],
                        "result_data": tuple_data[4]
                    })
            
            return {"job_id": job_id, "results": results, "count": len(results)}
        except json.JSONDecodeError:
            return {"job_id": job_id, "results": [], "count": 0, "error": "Parse error"}
    
    @handler("job_status", "call")
    def job_status(self, job_id: str = "") -> dict:
        """
        Get job status.
        
        Args:
            job_id: Job ID
        
        Returns:
            Job status including task count and completion
        """
        if not job_id:
            return {"error": "job_id required"}
        
        if job_id in self.active_jobs:
            job = self.active_jobs[job_id]
            return {
                "job_id": job_id,
                "type": job["type"],
                "task_count": job["task_count"],
                "completed": job["completed"],
                "status": "completed" if job["completed"] >= job["task_count"] else "in_progress"
            }
        
        return {"job_id": job_id, "status": "unknown"}
    
    @handler("list_jobs", "call")
    def list_jobs(self) -> dict:
        """List all active jobs."""
        jobs = []
        for job_id, job in self.active_jobs.items():
            jobs.append({
                "job_id": job_id,
                "type": job["type"],
                "task_count": job["task_count"],
                "completed": job["completed"],
                "status": "completed" if job["completed"] >= job["task_count"] else "in_progress"
            })
        return {"jobs": jobs, "count": len(jobs)}
