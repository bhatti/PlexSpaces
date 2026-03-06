# SkyPilot native reference: spot jobs and checkpointing

This file describes how the same use case is implemented with [SkyPilot](https://github.com/skypilot-org/skypilot).

## SkyPilot model

- **Spot instances**: Launch VMs on spot/preemptible capacity; lower cost but can be preempted.
- **Checkpointing**: Save job state (e.g. model weights, step index) to cloud storage (S3, GCS) so that on preemption the job can resume.
- **Failover**: On preemption, SkyPilot can relaunch the job (same or different cloud/region); the task restores from the last checkpoint and continues.
- **YAML task file**: Define resources, run commands, and optional checkpoint paths.

## Native pattern (conceptual)

```yaml
# task.yaml (SkyPilot)
resources:
  cloud: aws
  use_spot: true
  cpus: 4
  memory: 16

run: |
  # Training script that checkpoints to S3
  python train.py --checkpoint-dir s3://my-bucket/run-1
  # On preemption, same command restores from s3://my-bucket/run-1 and continues
```

```bash
# Launch (spot); job checkpoints to storage
sky launch task.yaml

# If preempted, relaunch resumes from checkpoint
sky launch task.yaml  # or sky spot launch ...
```

- The user's script (e.g. `train.py`) writes checkpoints to object storage; on relaunch it detects existing checkpoint and resumes.

## PlexSpaces equivalent

- **Workflow actor**: One actor per job (virtual_actor); `run()` executes steps, writes checkpoint to **KV** after each step (`skypilot:checkpoint:{job_id}`).
- **Preemption**: Simulated via payload `simulate_preemption_at_step`; actor returns "preempted" and exits; checkpoint is already in KV.
- **Failover**: Next `run()` with same `job_id` loads checkpoint from KV, restores `current_step` and `total_steps`, and continues the loop until completion.
- **Durability**: Actor state (durability facet) plus KV checkpoints give resume semantics similar to SkyPilot.

See README comparison table (SkyPilot vs PlexSpaces).
