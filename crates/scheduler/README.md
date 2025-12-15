# Scheduler - Resource-Aware Task Scheduling

**Purpose**: Implements two-layer resource-aware scheduling:
- **Layer 1 (Actor-Level)**: Place actors on nodes that match resource requirements and labels
- **Layer 2 (Task-Level)**: Route tasks to appropriate actors based on groups and load

## Overview

This crate provides the scheduling service that runs on every node:
- **gRPC Service**: SchedulingService for actor placement requests
- **Background Scheduler**: Processes requests asynchronously with lease-based coordination
- **State Store**: Tracks scheduling requests and status
- **Node Selector**: Selects best node based on resources and labels
- **Capacity Tracker**: Tracks node capacity via ObjectRegistry

## Key Components

### SchedulingService

gRPC service for scheduling requests:

```rust
pub struct SchedulingServiceImpl {
    state_store: Arc<dyn SchedulingStateStore>,
    node_selector: NodeSelector,
    capacity_tracker: CapacityTracker,
}
```

### TaskRouter

Routes tasks to appropriate actors:

```rust
pub struct TaskRouter {
    routing_strategy: RoutingStrategy,
    state_store: Arc<dyn SchedulingStateStore>,
}

pub enum RoutingStrategy {
    RoundRobin,
    LeastLoaded,
    Random,
    ConsistentHashing,
}
```

### Background Scheduler

Processes scheduling requests asynchronously:

```rust
pub struct BackgroundScheduler {
    state_store: Arc<dyn SchedulingStateStore>,
    lock_manager: Arc<dyn LockManager>,
}
```

## Usage Examples

### Schedule Actor Placement

```rust
use plexspaces_scheduler::SchedulingServiceImpl;
use plexspaces_proto::scheduler::v1::ScheduleActorRequest;

let scheduler = SchedulingServiceImpl::new(state_store, node_selector, capacity_tracker);

let request = ScheduleActorRequest {
    actor_id: "counter@node1".to_string(),
    resource_requirements: Some(ResourceRequirements {
        cpu_millicores: 100,
        memory_bytes: 64 * 1024 * 1024,
        ..Default::default()
    }),
    node_labels: vec!["zone=us-west-1".to_string()],
    ..Default::default()
};

let response = scheduler.schedule_actor(request).await?;
```

### Route Task to Actor

```rust
use plexspaces_scheduler::TaskRouter;

let router = TaskRouter::new(RoutingStrategy::LeastLoaded, state_store);

let task = Task {
    task_id: "task-001".to_string(),
    actor_group: "workers".to_string(),
    payload: vec![1, 2, 3],
};

let actor_id = router.route_task(task).await?;
```

## Dependencies

This crate depends on:
- `plexspaces_proto`: Protocol buffer definitions
- `plexspaces_locks`: Distributed locks for coordination
- `plexspaces_object_registry`: Node capacity tracking
- `sqlx`: SQL backends (optional)

## Dependents

This crate is used by:
- `plexspaces_node`: Node includes scheduler service
- Applications: Applications use scheduler for actor placement

## References

- Implementation: `crates/scheduler/src/`
- Tests: `crates/scheduler/tests/`
- Proto definitions: `proto/plexspaces/v1/scheduler.proto`

