# Kueue native reference: GPU job scheduler

This file describes how the same use case is implemented with [Kueue](https://k8s.io/docs/concepts/scheduling/kueue/) (Kubernetes-native job queue).

## Kueue model

- **Queue / ClusterQueue / LocalQueue**: Kubernetes resources that define quotas (e.g. GPU), priority, and preemption.
- **Job**: A Kubernetes Job (or other workload) that gets admitted by Kueue when quota is available.
- **Priority**: Jobs have priority; higher-priority jobs can preempt lower-priority ones when quota is scarce.
- **Resource quotas**: ClusterQueue defines total resources (e.g. 8 GPUs); Kueue admits jobs up to quota.
- **Preemption**: Configurable preemption policies (e.g. lower priority jobs are preempted to make room).

## Native pattern (conceptual)

```yaml
# ClusterQueue: total GPU quota
apiVersion: kueue.x-k8s.io/v1beta1
kind: ClusterQueue
metadata:
  name: gpu-pool
spec:
  resourceGroups:
  - name: gpu
    resources:
    - name: nvidia.com/gpu
      nominalQuota: 8
  queueingStrategy: BestEffort
---
# LocalQueue (namespace-scoped)
apiVersion: kueue.x-k8s.io/v1beta1
kind: LocalQueue
metadata:
  namespace: default
  name: training
spec:
  clusterQueue: gpu-pool
---
# Job with priority and resource request
apiVersion: batch/v1
kind: Job
metadata:
  name: train-1
  annotations:
    kueue.x-k8s.io/queue-name: training
spec:
  priority: 10
  template:
    spec:
      containers:
      - name: train
        resources:
          limits:
            nvidia.com/gpu: 2
```

- Kueue controller admits jobs when quota allows; priority and preemption are handled by the scheduler.

## PlexSpaces equivalent

- **GenServer actor**: Single scheduler actor; state = queue (priority-sorted), used_gpus, max_gpus.
- **submit**: Add job to queue; optionally persist to KV (job metadata).
- **allocate**: Acquire distributed lock (critical section), pick highest-priority pending job that fits quota, mark allocated, release lock.
- **complete / preempt**: Free GPUs; preempt re-queues job as pending.
- **Lock**: host.lock_acquire/release around allocate so only one allocator at a time (single scheduler replica semantics).

See README comparison table (Kueue vs PlexSpaces).
