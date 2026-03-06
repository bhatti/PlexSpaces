// SPDX-License-Identifier: LGPL-2.1-or-later
// Native Dapr reference: background job processing with State Store and Workflow.
//
// This file is for comparison only — it is not built. It illustrates how the
// same use case is implemented with Dapr's native building blocks.
//
// Dapr: https://docs.dapr.io/
// State: https://docs.dapr.io/developing-applications/building-blocks/state-management/
// Workflow: https://docs.dapr.io/developing-applications/building-blocks/workflow/

package main

// Dapr State Store (e.g. components/statestore.yaml with Redis):
//   - Store queue as key "job-queue" (JSON array of jobs)
//   - Store DLQ as key "job-dlq" (JSON array of failed jobs)
//
// Enqueue (native Dapr client):
//   client.SaveState(ctx, "statestore", "job-queue", marshal(append(queue, newJob)), nil)
//
// Process one (get queue, pop first, process, on failure retry or move to DLQ):
//   client.GetState(ctx, "statestore", "job-queue", nil)
//   client.SaveState(ctx, "statestore", "job-queue", marshal(remainingQueue), nil)
//   if processErr != nil && retries >= maxRetry {
//       client.GetState(ctx, "statestore", "job-dlq", nil)
//       client.SaveState(ctx, "statestore", "job-dlq", marshal(append(dlq, job)), nil)
//   }
//
// Dapr Workflow (alternative): define a workflow that runs "ProcessJob" activity
// with retry policy; on final failure run "MoveToDLQ" activity. Workflow engine
// handles durability and replay.
//
// Comparison: In PlexSpaces we use a single WorkflowActor (job_processor.go) with
// queue/DLQ in actor state, checkpointed by the durability facet; no separate
// state store component. Run() handles enqueue and process; Signal(cancel) and
// Query(status) complete the API.
